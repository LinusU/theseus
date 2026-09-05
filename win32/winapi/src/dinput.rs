//! DirectInput keyboard, mouse, and a generic joystick.
//!
//! Device state comes from the shared input state in user32, which the host
//! message pump keeps up to date; see user32::input.

use std::{collections::HashMap, sync::Mutex};

use runtime::Context;

use crate::{ddraw::GUID, heap::Heap, kernel32, locked_state::LockedState, user32};

const GUID_SysMouse: GUID = GUID::new(
    0x6F1D2B60,
    0xD5A0,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

const GUID_SysKeyboard: GUID = GUID::new(
    0x6F1D2B61,
    0xD5A0,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

const GUID_Joystick: GUID = GUID::new(
    0x6F1D2B70,
    0xD5A0,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

/// Product GUID the MM2 executable passes when creating a joystick.
/// The debug string `00013b28-66c0-0057-bc20-ab0501000000` is produced by
/// GUID's little-endian display of the first two data4 bytes, so the actual
/// byte order is [0x20, 0xbc, ...].
const GUID_Mm2Joystick: GUID = GUID::new(
    0x0001_3B28,
    0x66C0,
    0x0057,
    [0x20, 0xBC, 0xAB, 0x05, 0x01, 0x00, 0x00, 0x00],
);

const IID_IUnknown: GUID = GUID::new(
    0x00000000,
    0x0000,
    0x0000,
    [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);
const IID_IDirectInputA: GUID = GUID::new(
    0x89521360,
    0xAA8A,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);
const IID_IDirectInput2A: GUID = GUID::new(
    0x5944E662,
    0xAA8A,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);
const IID_IDirectInputDeviceA: GUID = GUID::new(
    0x5944E680,
    0xC92E,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);
const IID_IDirectInputDevice2A: GUID = GUID::new(
    0x5944E682,
    0xC92E,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
);

const DI_OK: u32 = 0;
/// More events were buffered than the app's buffer could hold.
const DI_BUFFEROVERFLOW: u32 = 1;
/// DirectInput reports plain win32 error codes as HRESULTs, which is what
/// MAKE_HRESULT with FACILITY_WIN32 comes out as.
const fn make_dierror(win32_code: u32) -> u32 {
    (1 << 31) | (7 << 16) | win32_code
}

/// REGDB_E_CLASSNOTREG, which is in a different facility to the rest.
const DIERR_DEVICENOTREG: u32 = 0x80040154;
const DIERR_NOTACQUIRED: u32 = make_dierror(0x0c); // ERROR_INVALID_ACCESS
const DIERR_INVALIDPARAM: u32 = make_dierror(0x57); // ERROR_INVALID_PARAMETER
const E_POINTER: u32 = 0x80004003;
const E_NOINTERFACE: u32 = 0x80004002;
const E_NOTIMPL: u32 = 0x80004001;
/// DIERR_UNSUPPORTED aliases E_NOINTERFACE in the DirectInput headers.
const DIERR_UNSUPPORTED: u32 = E_NOINTERFACE;

/// Shared COM identity check: the object answers for `IID_IUnknown` and any
/// interface GUIDs in `accepted`.
fn query_interface(ctx: &mut Context, this: u32, riid: u32, ppv: u32, accepted: &[GUID]) -> u32 {
    if ppv == 0 {
        return E_POINTER;
    }
    if riid == 0 {
        ctx.memory.write::<u32>(ppv, 0);
        return E_NOINTERFACE;
    }
    let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
    if iid == IID_IUnknown || accepted.contains(&iid) {
        ctx.memory.write::<u32>(ppv, this);
        DI_OK
    } else {
        ctx.memory.write::<u32>(ppv, 0);
        E_NOINTERFACE
    }
}

/// One buffered event, as GetDeviceData reports it.
#[repr(C)]
#[derive(Debug, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct DIDEVICEOBJECTDATA {
    pub dwOfs: u32,
    pub dwData: u32,
    pub dwTimeStamp: u32,
    pub dwSequence: u32,
}

/// sizeof(DIMOUSESTATE): three i32 axes then four button bytes.
const DIMOUSESTATE_SIZE: usize = 16;

/// DIGDD_PEEK: leave the returned events in the buffer.
const DIGDD_PEEK: u32 = 0x00000001;

/// DirectInput property GUIDs are really small integers cast to a GUID pointer
/// (see MAKEDIPROP), so a property is identified by the pointer value itself.
const DIPROP_BUFFERSIZE: u32 = 1;
const DIPROP_AXISMODE: u32 = 2;
const DIPROP_GRANULARITY: u32 = 3;
const DIPROP_RANGE: u32 = 4;
const DIPROP_DEADZONE: u32 = 5;
const DIPROP_SATURATION: u32 = 6;
const DIPROP_FFGAIN: u32 = 7;
const DIPROP_FFLOAD: u32 = 8;
const DIPROP_AUTOCENTER: u32 = 9;
const DIPROP_CALIBRATIONMODE: u32 = 10;
/// Offset of DIPROPDWORD::dwData, past the DIPROPHEADER.
const DIPROPDWORD_DWDATA: u32 = 16;

/// DIDEVCAPS layout sizes: the original header ends after dwPOVs (24
/// bytes); DX5 added five force-feedback fields, which we report as zero.
const DIDEVCAPS_MIN_SIZE: usize = 24;
const DIDEVCAPS_SIZE: usize = 44;

/// DIDEVICEINSTANCEA layout sizes: the original header ends after
/// tszProductName (560 bytes); DX5 added the FF-driver GUID and the HID
/// usage page/usage pair, which we report as zero.
const DIDEVICEINSTANCE_MIN_SIZE: usize = 560;
const DIDEVICEINSTANCE_SIZE: usize = 580;
const MAX_PATH: usize = 260;

/// DIDEVTYPE_* device type codes used by the DX5-era headers.
const DIDEVTYPE_MOUSE: u32 = 2;
const DIDEVTYPE_KEYBOARD: u32 = 3;
const DIDEVTYPE_JOYSTICK: u32 = 4;
/// DIDEVTYPEMOUSE_/DIDEVTYPEKEYBOARD_ subtype codes.
const DIDEVTYPEMOUSE_TRADITIONAL: u32 = 1;
const DIDEVTYPEKEYBOARD_PCENH: u32 = 4;
const DIDEVTYPEJOYSTICK_TRADITIONAL: u32 = 1;

/// DIDC_* capability flags.
const DIDC_ATTACHED: u32 = 0x00000001;
const DIDC_POLLEDDEVICE: u32 = 0x00000002;
const DIDC_EMULATED: u32 = 0x00000004;
const DIDC_POLLEDDATAFORMAT: u32 = 0x00000008;

/// HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND); the requested object does
/// not exist on this device.
const DIERR_OBJECTNOTFOUND: u32 = make_dierror(0x02);

fn write_guid(ctx: &mut Context, addr: u32, guid: &GUID) {
    let mut bytes = [0u8; 16];
    bytes[..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..].copy_from_slice(&guid.data4);
    ctx.memory[addr..][..16].copy_from_slice(&bytes);
}

fn write_cstr(ctx: &mut Context, addr: u32, s: &[u8]) {
    ctx.memory[addr..][..s.len()].copy_from_slice(s);
    ctx.memory[addr + s.len() as u32] = 0;
}

/// Which physical device a created IDirectInputDevice stands for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Joystick,
}

pub struct Device {
    pub kind: DeviceKind,
    pub acquired: bool,
    /// The GUID the device was created with; used to answer GetDeviceInfo.
    pub guid: GUID,
    /// Properties most recently set through SetProperty, keyed by MAKEDIPROP id.
    pub properties: HashMap<u32, Vec<u8>>,
    /// COM reference count for this device object.
    pub refcount: u32,
}

#[derive(Default)]
pub struct State {
    /// Maps an IDirectInputDevice interface pointer to the device it represents.
    pub devices: HashMap<u32, Device>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
type Lock = LockedState<State>;

fn lock() -> Lock {
    LockedState::from_or_init(&STATE, Default::default)
}

pub const VTABLES: [(&'static str, &[&str]); 2] = [
    ("IDirectInput", IDirectInput::VTABLE_ENTRIES.as_slice()),
    (
        "IDirectInputDevice",
        IDirectInputDevice::VTABLE_ENTRIES.as_slice(),
    ),
];

#[win32_derive::dllexport]
pub fn DirectInputCreateA(
    ctx: &mut Context,
    _hinst: u32,
    _dwVersion: u32,
    ppDI: u32,
    _punkOuter: u32,
) -> u32 {
    let mut kernel32 = kernel32::lock();
    let ptr = IDirectInput::new(ctx, &mut kernel32.process_heap);
    drop(kernel32);
    ctx.memory.write::<u32>(ppDI, ptr);
    DI_OK
}

pub mod IDirectInput {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 8] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "CreateDevice",
        "EnumDevices",
        "GetDeviceStatus",
        "RunControlPanel",
        "Initialize",
    ];

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> u32 {
        query_interface(
            ctx,
            this,
            riid,
            ppv,
            &[IID_IDirectInputA, IID_IDirectInput2A],
        )
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn CreateDevice(
        ctx: &mut Context,
        _this: u32,
        lpGUID: u32,
        lplpDirectInputDevice: u32,
        _pUnkOuter: u32,
    ) -> u32 {
        let guid = crate::Ptr::<GUID>::new(lpGUID).read(&ctx.memory).unwrap();
        let kind = if guid == GUID_SysKeyboard {
            DeviceKind::Keyboard
        } else if guid == GUID_SysMouse {
            DeviceKind::Mouse
        } else if guid == GUID_Joystick || guid == GUID_Mm2Joystick {
            DeviceKind::Joystick
        } else {
            log::warn!("CreateDevice: unknown GUID {guid:?}");
            return DIERR_DEVICENOTREG;
        };
        let mut kernel32 = kernel32::lock();
        let device = IDirectInputDevice::new(ctx, &mut kernel32.process_heap);
        drop(kernel32);
        lock().devices.insert(
            device,
            Device {
                kind,
                acquired: false,
                guid,
                properties: HashMap::new(),
                refcount: 1,
            },
        );
        ctx.memory.write::<u32>(lplpDirectInputDevice, device);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn EnumDevices(
        _ctx: &mut Context,
        _this: u32,
        _dwDevType: u32,
        _callback: u32,
        _pvRef: u32,
        _dwFlags: u32,
    ) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceStatus(ctx: &mut Context, _this: u32, rguid: u32) -> u32 {
        if rguid == 0 {
            return DIERR_INVALIDPARAM;
        }
        let guid = crate::Ptr::<GUID>::new(rguid).read(&ctx.memory).unwrap();
        if guid == GUID_SysKeyboard
            || guid == GUID_SysMouse
            || guid == GUID_Joystick
            || guid == GUID_Mm2Joystick
        {
            DI_OK
        } else {
            DIERR_DEVICENOTREG
        }
    }

    #[win32_derive::dllexport]
    pub fn RunControlPanel(_ctx: &mut Context, _this: u32, _hwnd: u32, _dwFlags: u32) -> u32 {
        // No host control panel exists to run.
        E_NOTIMPL
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _hinst: u32, _dwVersion: u32) -> u32 {
        DI_OK
    }
}

pub mod IDirectInputDevice {
    use super::*;

    // IDirectInputDevice2 layout; the game may call Poll() before reading state.
    pub const VTABLE_ENTRIES: [&str; 27] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "GetCapabilities",
        "EnumObjects",
        "GetProperty",
        "SetProperty",
        "Acquire",
        "Unacquire",
        "GetDeviceState",
        "GetDeviceData",
        "SetDataFormat",
        "SetEventNotification",
        "SetCooperativeLevel",
        "GetObjectInfo",
        "GetDeviceInfo",
        "RunControlPanel",
        "Initialize",
        "CreateEffect",
        "EnumEffects",
        "GetEffectInfo",
        "GetForceFeedbackState",
        "SendForceFeedbackCommand",
        "EnumCreatedEffectObjects",
        "Escape",
        "Poll",
        "SendDeviceData",
    ];

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }

    /// The device behind an interface pointer, and whether it's acquired.
    /// A device we never created reads as an unacquired keyboard.
    pub fn device(this: u32) -> (DeviceKind, bool) {
        lock()
            .devices
            .get(&this)
            .map(|device| (device.kind, device.acquired))
            .unwrap_or((DeviceKind::Keyboard, false))
    }

    pub fn device_guid(this: u32) -> GUID {
        lock()
            .devices
            .get(&this)
            .map(|device| device.guid)
            .unwrap_or(GUID_SysKeyboard)
    }

    fn set_acquired(this: u32, acquired: bool) {
        if let Some(device) = lock().devices.get_mut(&this) {
            device.acquired = acquired;
        }
    }

    fn add_ref(this: u32) -> u32 {
        if let Some(device) = lock().devices.get_mut(&this) {
            device.refcount += 1;
            device.refcount
        } else {
            1
        }
    }

    fn release(this: u32) -> u32 {
        let mut state = lock();
        let Some(device) = state.devices.get_mut(&this) else {
            return 0;
        };
        device.refcount -= 1;
        if device.refcount == 0 {
            state.devices.remove(&this);
            0
        } else {
            device.refcount
        }
    }

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> u32 {
        let hr = query_interface(
            ctx,
            this,
            riid,
            ppv,
            &[IID_IDirectInputDeviceA, IID_IDirectInputDevice2A],
        );
        if hr == DI_OK {
            add_ref(this);
        }
        hr
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, this: u32) -> u32 {
        add_ref(this)
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        release(this)
    }

    #[win32_derive::dllexport]
    pub fn GetCapabilities(ctx: &mut Context, this: u32, lpCaps: u32) -> u32 {
        if lpCaps == 0 {
            return DIERR_INVALIDPARAM;
        }
        let size = ctx.memory.read::<u32>(lpCaps) as usize;
        if !(DIDEVCAPS_MIN_SIZE..=DIDEVCAPS_SIZE).contains(&size)
            || lpCaps as usize + size > ctx.memory.bytes.len()
        {
            return DIERR_INVALIDPARAM;
        }
        let (kind, _) = device(this);
        let (flags, devtype, axes, buttons) = match kind {
            DeviceKind::Keyboard => (
                DIDC_ATTACHED | DIDC_EMULATED | DIDC_POLLEDDEVICE | DIDC_POLLEDDATAFORMAT,
                DIDEVTYPE_KEYBOARD | (DIDEVTYPEKEYBOARD_PCENH << 8),
                0,
                0,
            ),
            DeviceKind::Mouse => (
                DIDC_ATTACHED | DIDC_EMULATED,
                DIDEVTYPE_MOUSE | (DIDEVTYPEMOUSE_TRADITIONAL << 8),
                3,
                DIMOUSESTATE_SIZE as u32 - 12,
            ),
            DeviceKind::Joystick => (
                DIDC_ATTACHED | DIDC_EMULATED,
                DIDEVTYPE_JOYSTICK | (DIDEVTYPEJOYSTICK_TRADITIONAL << 8),
                6,
                32,
            ),
        };
        for (i, field) in [size as u32, flags, devtype, axes, buttons, 0]
            .into_iter()
            .enumerate()
        {
            ctx.memory.write::<u32>(lpCaps + i as u32 * 4, field);
        }
        // Any DX5 force-feedback tail fields report zero.
        ctx.memory[lpCaps + 24..][..size - 24].fill(0);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn EnumObjects(
        _ctx: &mut Context,
        _this: u32,
        _lpCallback: u32,
        _pvRef: u32,
        _dwFlags: u32,
    ) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn GetProperty(ctx: &mut Context, this: u32, rguidProp: u32, pdiph: u32) -> u32 {
        if pdiph == 0 {
            return E_POINTER;
        }
        if rguidProp == DIPROP_BUFFERSIZE {
            let (kind, _) = device(this);
            let size = user32::state()
                .input
                .borrow()
                .buffer_size(kind == DeviceKind::Keyboard);
            ctx.memory
                .write::<u32>(pdiph + DIPROPDWORD_DWDATA, size as u32);
            return DI_OK;
        }
        let state = lock();
        let Some(device) = state.devices.get(&this) else {
            return DIERR_INVALIDPARAM;
        };
        let Some(stored) = device.properties.get(&rguidProp) else {
            return DIERR_INVALIDPARAM;
        };
        let size = ctx.memory.read::<u32>(pdiph) as usize;
        let len = stored.len().min(size);
        if len == 0 || pdiph as usize + len > ctx.memory.bytes.len() {
            return DIERR_INVALIDPARAM;
        }
        ctx.memory[pdiph..][..len].copy_from_slice(&stored[..len]);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SetProperty(ctx: &mut Context, this: u32, rguidProp: u32, pdiph: u32) -> u32 {
        if pdiph == 0 {
            return DIERR_INVALIDPARAM;
        }
        if rguidProp == DIPROP_BUFFERSIZE {
            let (kind, _) = device(this);
            let size = ctx.memory.read::<u32>(pdiph + DIPROPDWORD_DWDATA);
            user32::state()
                .input
                .borrow_mut()
                .set_buffer_size(kind == DeviceKind::Keyboard, size as usize);
            return DI_OK;
        }
        if rguidProp != DIPROP_AXISMODE
            && rguidProp != DIPROP_GRANULARITY
            && rguidProp != DIPROP_RANGE
            && rguidProp != DIPROP_DEADZONE
            && rguidProp != DIPROP_SATURATION
            && rguidProp != DIPROP_FFGAIN
            && rguidProp != DIPROP_FFLOAD
            && rguidProp != DIPROP_AUTOCENTER
            && rguidProp != DIPROP_CALIBRATIONMODE
        {
            log::warn!("dinput SetProperty: unhandled property {rguidProp:#x}");
            return DI_OK;
        }
        let size = ctx.memory.read::<u32>(pdiph) as usize;
        if size == 0 || pdiph as usize + size > ctx.memory.bytes.len() {
            return DIERR_INVALIDPARAM;
        }
        let mut state = lock();
        let Some(device) = state.devices.get_mut(&this) else {
            return DIERR_INVALIDPARAM;
        };
        let bytes = ctx.memory[pdiph..][..size].to_vec();
        device.properties.insert(rguidProp, bytes);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn Acquire(_ctx: &mut Context, this: u32) -> u32 {
        set_acquired(this, true);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn Unacquire(_ctx: &mut Context, this: u32) -> u32 {
        set_acquired(this, false);
        DI_OK
    }

    /// Read immediate device state into the caller's buffer.
    ///
    /// Keyboard: a byte array indexed by DIK scan code (0x80 = pressed).
    /// Mouse: a DIMOUSESTATE — lX/lY/lZ relative to the last read, then one
    /// byte per button.
    /// Joystick: a DIJOYSTATE (or DIJOYSTATE2) — the host has no real stick,
    /// so the entire buffer is reported as centered/neutral with no buttons.
    #[win32_derive::dllexport]
    pub fn GetDeviceState(ctx: &mut Context, this: u32, cbData: u32, lpvData: u32) -> u32 {
        let (kind, acquired) = device(this);
        if !acquired {
            return DIERR_NOTACQUIRED;
        }
        user32::pump_host_input();

        // cbData comes straight from the app; a garbage value would otherwise
        // ask for a multi-gigabyte allocation.
        let len = match kind {
            DeviceKind::Keyboard => 256,
            // DIMOUSESTATE: lX, lY, lZ, then four buttons.
            DeviceKind::Mouse => DIMOUSESTATE_SIZE,
            // Accept DIJOYSTATE (44 bytes) or the larger DIJOYSTATE2 layout.
            DeviceKind::Joystick if (44..=256).contains(&cbData) => cbData as usize,
            DeviceKind::Joystick => {
                log::warn!("GetDeviceState: cbData {cbData} does not match Joystick");
                return DIERR_INVALIDPARAM;
            }
        };
        if lpvData as usize + len > ctx.memory.bytes.len() {
            return DIERR_INVALIDPARAM;
        }
        let mut buf = vec![0u8; len];
        let mut input = user32::state().input.borrow_mut();
        match kind {
            DeviceKind::Keyboard => {
                let keys = len.min(256);
                buf[..keys].copy_from_slice(&input.dik_state()[..keys]);
            }
            DeviceKind::Mouse => {
                let (dx, dy) = input.mouse.take_motion();
                // DIMOUSESTATE: lX, lY, lZ, then rgbButtons.
                for (ofs, value) in [(0, dx), (4, dy), (8, 0)] {
                    if ofs + 4 <= len {
                        buf[ofs..ofs + 4].copy_from_slice(&value.to_le_bytes());
                    }
                }
                for (index, &button) in input.mouse.buttons.iter().enumerate() {
                    if 12 + index < len {
                        buf[12 + index] = button;
                    }
                }
            }
            DeviceKind::Joystick => {
                // Leave the whole buffer zero: centered axes and no pressed
                // buttons.  A real host joystick is not wired in yet.
            }
        }
        drop(input);
        ctx.memory[lpvData..][..len].copy_from_slice(&buf);
        DI_OK
    }

    /// Read buffered events into the caller's DIDEVICEOBJECTDATA array.
    #[win32_derive::dllexport]
    pub fn GetDeviceData(
        ctx: &mut Context,
        this: u32,
        cbObjectData: u32,
        rgdod: u32,
        pdwInOut: u32,
        dwFlags: u32,
    ) -> u32 {
        if pdwInOut == 0 {
            return DIERR_INVALIDPARAM;
        }
        let (kind, acquired) = device(this);
        if !acquired {
            return DIERR_NOTACQUIRED;
        }
        user32::pump_host_input();

        // A null array means the caller wants the pending events discarded,
        // or, with DIGDD_PEEK, just counted.
        let capacity = if rgdod == 0 {
            usize::MAX
        } else {
            ctx.memory.read::<u32>(pdwInOut) as usize
        };
        let peek = dwFlags & DIGDD_PEEK != 0;
        let (events, overflowed) = if kind == DeviceKind::Joystick {
            // The emulated joystick does not buffer events; it is read with
            // GetDeviceState and is currently reported as neutral.
            (Vec::new(), false)
        } else {
            user32::state().input.borrow_mut().take_events(
                kind == DeviceKind::Keyboard,
                capacity,
                peek,
            )
        };

        if rgdod != 0 {
            for (i, event) in events.iter().enumerate() {
                // The stride comes from the caller rather than from the struct,
                // in case it passes the larger DirectInput 8 version.
                let addr = rgdod + i as u32 * cbObjectData;
                ctx.memory.write(
                    addr,
                    DIDEVICEOBJECTDATA {
                        dwOfs: event.ofs,
                        dwData: event.data,
                        dwTimeStamp: event.time,
                        dwSequence: event.sequence,
                    },
                );
            }
        }
        ctx.memory.write::<u32>(pdwInOut, events.len() as u32);

        if overflowed { DI_BUFFEROVERFLOW } else { DI_OK }
    }

    #[win32_derive::dllexport]
    pub fn SetDataFormat(_ctx: &mut Context, _this: u32, _lpdf: u32) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SetEventNotification(_ctx: &mut Context, _this: u32, _hEvent: u32) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SetCooperativeLevel(_ctx: &mut Context, _this: u32, _hwnd: u32, _dwFlags: u32) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn GetObjectInfo(
        _ctx: &mut Context,
        _this: u32,
        _pdidoi: u32,
        _dwObj: u32,
        _dwHow: u32,
    ) -> u32 {
        // The emulated devices expose their state buffers but do not model
        // individually named objects, so any lookup misses.
        DIERR_OBJECTNOTFOUND
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceInfo(ctx: &mut Context, this: u32, pdidi: u32) -> u32 {
        if pdidi == 0 {
            return DIERR_INVALIDPARAM;
        }
        let size = ctx.memory.read::<u32>(pdidi) as usize;
        if !(DIDEVICEINSTANCE_MIN_SIZE..=DIDEVICEINSTANCE_SIZE).contains(&size)
            || pdidi as usize + size > ctx.memory.bytes.len()
        {
            return DIERR_INVALIDPARAM;
        }
        let (kind, _) = device(this);
        let guid = device_guid(this);
        let (product, devtype, instance, product_name): (&GUID, u32, &[u8], &[u8]) = match kind {
            DeviceKind::Keyboard => (
                &GUID_SysKeyboard,
                DIDEVTYPE_KEYBOARD | (DIDEVTYPEKEYBOARD_PCENH << 8),
                b"Keyboard",
                b"System Keyboard",
            ),
            DeviceKind::Mouse => (
                &GUID_SysMouse,
                DIDEVTYPE_MOUSE | (DIDEVTYPEMOUSE_TRADITIONAL << 8),
                b"Mouse",
                b"System Mouse",
            ),
            DeviceKind::Joystick => (
                &GUID_Joystick,
                DIDEVTYPE_JOYSTICK | (DIDEVTYPEJOYSTICK_TRADITIONAL << 8),
                b"Joystick",
                b"Theseus Joystick",
            ),
        };
        ctx.memory[pdidi..][..size].fill(0);
        ctx.memory.write::<u32>(pdidi, size as u32);
        // The first GUID is the device instance, the second is the product.
        write_guid(ctx, pdidi + 4, &guid);
        write_guid(ctx, pdidi + 20, product);
        ctx.memory.write::<u32>(pdidi + 36, devtype);
        write_cstr(ctx, pdidi + 40, instance);
        write_cstr(ctx, pdidi + 40 + MAX_PATH as u32, product_name);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn RunControlPanel(_ctx: &mut Context, _this: u32, _hwnd: u32, _dwFlags: u32) -> u32 {
        // No host control panel exists to run.
        E_NOTIMPL
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _hinst: u32,
        _dwVersion: u32,
        _rguid: u32,
    ) -> u32 {
        DI_OK
    }

    /// The emulated keyboard and mouse have no force-feedback actuators, so
    /// every effect-management call reports an explicit failure or an empty
    /// enumeration rather than fabricating an effect object.
    #[win32_derive::dllexport]
    pub fn CreateEffect(
        ctx: &mut Context,
        _this: u32,
        _rguid: u32,
        _lpeff: u32,
        lplpde: u32,
        _punkOuter: u32,
    ) -> u32 {
        if lplpde != 0 {
            ctx.memory.write::<u32>(lplpde, 0);
        }
        DIERR_UNSUPPORTED
    }

    #[win32_derive::dllexport]
    pub fn EnumEffects(
        _ctx: &mut Context,
        _this: u32,
        _lpCallback: u32,
        _pvRef: u32,
        _dwEffType: u32,
    ) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn GetEffectInfo(_ctx: &mut Context, _this: u32, _pdei: u32, _rguid: u32) -> u32 {
        DIERR_OBJECTNOTFOUND
    }

    #[win32_derive::dllexport]
    pub fn GetForceFeedbackState(_ctx: &mut Context, _this: u32, _pdwOut: u32) -> u32 {
        DIERR_UNSUPPORTED
    }

    #[win32_derive::dllexport]
    pub fn SendForceFeedbackCommand(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> u32 {
        DIERR_UNSUPPORTED
    }

    #[win32_derive::dllexport]
    pub fn EnumCreatedEffectObjects(
        _ctx: &mut Context,
        _this: u32,
        _lpenum: u32,
        _pv: u32,
        _fl: u32,
    ) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn Escape(_ctx: &mut Context, _this: u32, _pesc: u32) -> u32 {
        // Hardware-specific escapes have no backing device.
        E_NOTIMPL
    }

    #[win32_derive::dllexport]
    pub fn Poll(_ctx: &mut Context, _this: u32) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SendDeviceData(
        _ctx: &mut Context,
        _this: u32,
        _cbObjectData: u32,
        _rgdod: u32,
        _pdwInOut: u32,
        _dwFlags: u32,
    ) -> u32 {
        // There is no hardware to accept device data.
        DIERR_UNSUPPORTED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    /// Write a GUID in its little-endian memory layout (FromBytes reads it back).
    fn write_guid(ctx: &mut Context, addr: u32, guid: &GUID) {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&guid.data1.to_le_bytes());
        bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
        bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
        bytes[8..].copy_from_slice(&guid.data4);
        ctx.memory[addr..][..16].copy_from_slice(&bytes);
    }

    #[test]
    fn query_interface_answers_for_iunknown_and_accepted_iids() {
        let mut ctx = context();
        write_guid(&mut ctx, 0x1000, &IID_IUnknown);
        write_guid(&mut ctx, 0x1020, &GUID_SysMouse);
        write_guid(&mut ctx, 0x1030, &IID_IDirectInput2A);
        write_guid(&mut ctx, 0x1040, &IID_IDirectInputDevice2A);

        assert_eq!(
            IDirectInput::QueryInterface(&mut ctx, 0x2000, 0x1000, 0x1100),
            DI_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0x2000);

        assert_eq!(
            IDirectInput::QueryInterface(&mut ctx, 0x2000, 0x1030, 0x1100),
            DI_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0x2000);

        assert_eq!(
            IDirectInputDevice::QueryInterface(&mut ctx, 0x2100, 0x1040, 0x1200),
            DI_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1200), 0x2100);

        assert_eq!(
            IDirectInputDevice::QueryInterface(&mut ctx, 0x2100, 0x1020, 0x1200),
            E_NOINTERFACE
        );
        assert_eq!(ctx.memory.read::<u32>(0x1200), 0);

        assert_eq!(
            IDirectInput::QueryInterface(&mut ctx, 0x2000, 0x1000, 0),
            E_POINTER
        );
    }

    #[test]
    fn set_and_get_property_stores_dword_properties() {
        let mut ctx = context();
        lock().devices.insert(
            0x2100,
            Device {
                kind: DeviceKind::Joystick,
                acquired: false,
                guid: GUID_Joystick,
                properties: HashMap::new(),
                refcount: 1,
            },
        );

        // Write a DIPROPDWORD for DIPROP_DEADZONE at 0x1000.
        ctx.memory.write::<u32>(0x1000, 20); // dwSize
        ctx.memory.write::<u32>(0x1004, 16); // dwHeaderSize
        ctx.memory.write::<u32>(0x1008, 0); // dwObj
        ctx.memory.write::<u32>(0x100c, 0); // dwHow
        ctx.memory.write::<u32>(0x1010, 5000); // dwData

        assert_eq!(
            IDirectInputDevice::SetProperty(&mut ctx, 0x2100, DIPROP_DEADZONE, 0x1000),
            DI_OK
        );

        // GetProperty should return the stored DIPROPDWORD into 0x1100.
        ctx.memory.write::<u32>(0x1100, 20);
        ctx.memory.write::<u32>(0x1104, 16);
        ctx.memory.write::<u32>(0x1108, 0);
        ctx.memory.write::<u32>(0x110c, 0);
        assert_eq!(
            IDirectInputDevice::GetProperty(&mut ctx, 0x2100, DIPROP_DEADZONE, 0x1100),
            DI_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1110), 5000);

        assert_eq!(
            IDirectInputDevice::GetProperty(&mut ctx, 0x2100, DIPROP_SATURATION, 0x1100),
            DIERR_INVALIDPARAM
        );
    }

    #[test]
    fn get_device_status_reports_system_devices() {
        let mut ctx = context();
        write_guid(&mut ctx, 0x1000, &GUID_SysKeyboard);
        write_guid(&mut ctx, 0x1020, &IID_IUnknown);

        assert_eq!(
            IDirectInput::GetDeviceStatus(&mut ctx, 0x2000, 0x1000),
            DI_OK
        );
        assert_eq!(
            IDirectInput::GetDeviceStatus(&mut ctx, 0x2000, 0x1020),
            DIERR_DEVICENOTREG
        );
        assert_eq!(
            IDirectInput::GetDeviceStatus(&mut ctx, 0x2000, 0),
            DIERR_INVALIDPARAM
        );
    }

    #[test]
    fn get_capabilities_reports_keyboard_layout() {
        let mut ctx = context();
        // An unknown device pointer reads as an unacquired keyboard.
        ctx.memory.write::<u32>(0x1000, DIDEVCAPS_SIZE as u32);

        assert_eq!(
            IDirectInputDevice::GetCapabilities(&mut ctx, 0x2000, 0x1000),
            DI_OK
        );
        assert_eq!(
            ctx.memory.read::<u32>(0x1000 + 4),
            DIDC_ATTACHED | DIDC_EMULATED | DIDC_POLLEDDEVICE | DIDC_POLLEDDATAFORMAT
        );
        assert_eq!(
            ctx.memory.read::<u32>(0x1000 + 8),
            DIDEVTYPE_KEYBOARD | (DIDEVTYPEKEYBOARD_PCENH << 8)
        );
        // The DX5 force-feedback tail is zeroed.
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 40), 0);

        ctx.memory.write::<u32>(0x1000, 12);
        assert_eq!(
            IDirectInputDevice::GetCapabilities(&mut ctx, 0x2000, 0x1000),
            DIERR_INVALIDPARAM
        );
    }

    #[test]
    fn get_device_info_writes_device_instance() {
        let mut ctx = context();
        ctx.memory
            .write::<u32>(0x1000, DIDEVICEINSTANCE_SIZE as u32);

        assert_eq!(
            IDirectInputDevice::GetDeviceInfo(&mut ctx, 0x2000, 0x1000),
            DI_OK
        );
        assert_eq!(
            ctx.memory.read::<u32>(0x1000 + 36),
            DIDEVTYPE_KEYBOARD | (DIDEVTYPEKEYBOARD_PCENH << 8)
        );
        assert_eq!(&ctx.memory[0x1000 + 40..][..9], b"Keyboard\0");
        assert_eq!(&ctx.memory[0x1000 + 300..][..16], b"System Keyboard\0");
        // guidFFDriver and the usage fields remain zero.
        assert_eq!(&ctx.memory[0x1000 + 560..][..20], &[0u8; 20]);

        ctx.memory.write::<u32>(0x1000, 100);
        assert_eq!(
            IDirectInputDevice::GetDeviceInfo(&mut ctx, 0x2000, 0x1000),
            DIERR_INVALIDPARAM
        );
    }
}
