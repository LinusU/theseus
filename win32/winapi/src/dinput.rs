//! DirectInput keyboard, mouse, and a generic joystick.
//!
//! Device state comes from the shared input state in user32, which the host
//! message pump keeps up to date; see user32::input.

use std::{collections::HashMap, sync::Mutex};

use runtime::Context;

use crate::{Ptr, ddraw::GUID, heap::Heap, kernel32, locked_state::LockedState, user32};

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
const DIERR_OUTOFMEMORY: u32 = make_dierror(0x0e); // ERROR_OUTOFMEMORY
const E_POINTER: u32 = 0x80004003;
const E_NOINTERFACE: u32 = 0x80004002;
const E_NOTIMPL: u32 = 0x80004001;
/// DIERR_UNSUPPORTED aliases E_NOINTERFACE in the DirectInput headers.
const DIERR_UNSUPPORTED: u32 = E_NOINTERFACE;

/// Shared COM identity check: the object answers for `IID_IUnknown` and any
/// interface GUIDs in `accepted`.
fn query_interface(ctx: &mut Context, this: u32, riid: u32, ppv: u32, accepted: &[GUID]) -> u32 {
    if !crate::ddraw::guest_range(ctx, ppv, 4) {
        return E_POINTER;
    }
    if riid == 0 {
        ctx.memory.write::<u32>(ppv, 0);
        return E_NOINTERFACE;
    }
    let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
        ctx.memory.write::<u32>(ppv, 0);
        return E_POINTER;
    };
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
    if crate::ddraw::guest_range(ctx, addr, std::mem::size_of::<GUID>() as u32) {
        Ptr::<GUID>::new(addr).write(&mut ctx.memory, *guid);
    }
}

fn write_cstr(ctx: &mut Context, addr: u32, s: &[u8]) {
    if !crate::ddraw::guest_range(ctx, addr, s.len() as u32 + 1) {
        return;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(addr as usize..)
        .and_then(|b| b.get_mut(..s.len()))
    {
        dst.copy_from_slice(s);
    }
    Ptr::<u8>::new(addr + s.len() as u32).write(&mut ctx.memory, 0);
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
    /// `dwDataSize` from the most recent SetDataFormat; 0 when no format
    /// has been negotiated yet.
    pub data_size: u32,
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

pub const VTABLES: [(&str, &[&str]); 2] = [
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
    if !crate::ddraw::guest_range(ctx, ppDI, 4) {
        return DIERR_INVALIDPARAM;
    }
    let mut kernel32 = kernel32::lock();
    let Some(ptr) = IDirectInput::new(ctx, &mut kernel32.process_heap) else {
        return DIERR_OUTOFMEMORY;
    };
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

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> Option<u32> {
        let addr = heap.try_alloc(&mut ctx.memory, 4)?;
        ctx.memory.write(addr, unsafe { VTABLE });
        Some(addr)
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
        if !crate::ddraw::guest_range(ctx, lplpDirectInputDevice, 4) {
            return DIERR_INVALIDPARAM;
        }
        let Some(guid) = crate::Ptr::<GUID>::new(lpGUID).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
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
        let Some(device) = IDirectInputDevice::new(ctx, &mut kernel32.process_heap) else {
            return DIERR_OUTOFMEMORY;
        };
        drop(kernel32);
        lock().devices.insert(
            device,
            Device {
                kind,
                acquired: false,
                guid,
                properties: HashMap::new(),
                refcount: 1,
                data_size: 0,
            },
        );
        ctx.memory.write::<u32>(lplpDirectInputDevice, device);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn EnumDevices(
        ctx: &mut Context,
        _this: u32,
        dwDevType: u32,
        lpCallback: u32,
        pvRef: u32,
        _dwFlags: u32,
    ) -> u32 {
        // A null-page callback would dispatch to a missing block and halt.
        if lpCallback < 0x1000 {
            return DIERR_INVALIDPARAM;
        }
        // The callback returns DIENUM_STOP (0) to end enumeration.
        for (kind, guid) in [
            (DeviceKind::Keyboard, GUID_SysKeyboard),
            (DeviceKind::Mouse, GUID_SysMouse),
            (DeviceKind::Joystick, GUID_Joystick),
        ] {
            let devtype = match kind {
                DeviceKind::Keyboard => DIDEVTYPE_KEYBOARD,
                DeviceKind::Mouse => DIDEVTYPE_MOUSE,
                DeviceKind::Joystick => DIDEVTYPE_JOYSTICK,
            };
            // dwDevType filters on the primary device type; 0 means all.
            if dwDevType != 0 && dwDevType != devtype {
                continue;
            }
            let Some(inst) = kernel32::lock()
                .process_heap
                .try_alloc(&mut ctx.memory, DIDEVICEINSTANCE_SIZE as u32)
            else {
                return DIERR_OUTOFMEMORY;
            };
            ctx.memory[inst..][..DIDEVICEINSTANCE_SIZE].fill(0);
            IDirectInputDevice::write_device_instance(
                ctx,
                inst,
                DIDEVICEINSTANCE_SIZE as u32,
                kind,
                &guid,
            );
            let callback = ctx.indirect(lpCallback);
            ctx.call32_x86(callback, vec![inst, pvRef]);
            let stop = ctx.cpu.regs.eax == 0;
            kernel32::lock().process_heap.free(&mut ctx.memory, inst);
            if stop {
                break;
            }
        }
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceStatus(ctx: &mut Context, _this: u32, rguid: u32) -> u32 {
        if rguid == 0 {
            return DIERR_INVALIDPARAM;
        }
        let Some(guid) = crate::Ptr::<GUID>::new(rguid).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
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

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> Option<u32> {
        let addr = heap.try_alloc(&mut ctx.memory, 4)?;
        ctx.memory.write(addr, unsafe { VTABLE });
        Some(addr)
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
        if lpCaps < 0x1000 {
            return DIERR_INVALIDPARAM;
        }
        let Some(size) = crate::Ptr::<u32>::new(lpCaps).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
        let size = size as usize;
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
        if let Some(rest) = ctx
            .memory
            .bytes
            .get_mut(lpCaps as usize + 24..)
            .and_then(|b| b.get_mut(..size - 24))
        {
            rest.fill(0);
        }
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
        if pdiph < 0x1000 {
            return E_POINTER;
        }
        if rguidProp == DIPROP_BUFFERSIZE {
            let (kind, _) = device(this);
            let size = user32::state()
                .input
                .borrow()
                .buffer_size(kind == DeviceKind::Keyboard);
            let Some(field) = pdiph.checked_add(DIPROPDWORD_DWDATA) else {
                return DIERR_INVALIDPARAM;
            };
            if crate::Ptr::<u32>::new(field)
                .write(&mut ctx.memory, size as u32)
                .is_none()
            {
                return E_POINTER;
            }
            return DI_OK;
        }
        let state = lock();
        let Some(device) = state.devices.get(&this) else {
            return DIERR_INVALIDPARAM;
        };
        let Some(stored) = device.properties.get(&rguidProp) else {
            return DIERR_INVALIDPARAM;
        };
        let Some(size) = crate::Ptr::<u32>::new(pdiph).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
        let size = size as usize;
        let len = stored.len().min(size);
        if len == 0 || pdiph as usize + len > ctx.memory.bytes.len() {
            return DIERR_INVALIDPARAM;
        }
        if let Some(dst) = ctx
            .memory
            .bytes
            .get_mut(pdiph as usize..)
            .and_then(|b| b.get_mut(..len))
        {
            dst.copy_from_slice(&stored[..len]);
        }
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SetProperty(ctx: &mut Context, this: u32, rguidProp: u32, pdiph: u32) -> u32 {
        if pdiph < 0x1000 {
            return DIERR_INVALIDPARAM;
        }
        if rguidProp == DIPROP_BUFFERSIZE {
            let (kind, _) = device(this);
            let Some(field) = pdiph.checked_add(DIPROPDWORD_DWDATA) else {
                return DIERR_INVALIDPARAM;
            };
            let Some(size) = crate::Ptr::<u32>::new(field).read(&ctx.memory) else {
                return DIERR_INVALIDPARAM;
            };
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
        let Some(size) = crate::Ptr::<u32>::new(pdiph).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
        let size = size as usize;
        if size == 0 || pdiph as usize + size > ctx.memory.bytes.len() {
            return DIERR_INVALIDPARAM;
        }
        let mut state = lock();
        let Some(device) = state.devices.get_mut(&this) else {
            return DIERR_INVALIDPARAM;
        };
        let Some(bytes) = ctx
            .memory
            .bytes
            .get(pdiph as usize..)
            .and_then(|b| b.get(..size))
            .map(|b| b.to_vec())
        else {
            return DIERR_INVALIDPARAM;
        };
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
        let negotiated = lock()
            .devices
            .get(&this)
            .map(|device| device.data_size as usize)
            .unwrap_or(0);
        let len = match kind {
            DeviceKind::Keyboard => 256,
            // DIMOUSESTATE: lX, lY, lZ, then four buttons.
            DeviceKind::Mouse => DIMOUSESTATE_SIZE,
            // Accept DIJOYSTATE (44 bytes) up to the negotiated format size
            // (or DIJOYSTATE2's 256 when no format was set).
            DeviceKind::Joystick if (44..=negotiated.max(256)).contains(&(cbData as usize)) => {
                cbData as usize
            }
            DeviceKind::Joystick => {
                log::warn!("GetDeviceState: cbData {cbData} does not match Joystick");
                return DIERR_INVALIDPARAM;
            }
        };
        if lpvData < 0x1000 || lpvData as usize + len > ctx.memory.bytes.len() {
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
        if let Some(dst) = ctx
            .memory
            .bytes
            .get_mut(lpvData as usize..)
            .and_then(|b| b.get_mut(..len))
        {
            dst.copy_from_slice(&buf);
        }
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
        // The count out-param is required; a low non-null event array would
        // scribble into the null page.
        if pdwInOut < 0x1000 || (rgdod != 0 && rgdod < 0x1000) {
            return DIERR_INVALIDPARAM;
        }
        let (kind, acquired) = device(this);
        if !acquired {
            return DIERR_NOTACQUIRED;
        }
        user32::pump_host_input();

        // The in/out count pointer must be readable so we can write the count
        // back even when the caller just wants the number of pending events.
        let Some(capacity) = crate::Ptr::<u32>::new(pdwInOut).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
        // A null array means the caller wants the pending events discarded,
        // or, with DIGDD_PEEK, just counted.
        let capacity = if rgdod == 0 {
            usize::MAX
        } else {
            capacity as usize
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
                // in case it passes the larger DirectInput 8 version. An entry
                // address that does not fit is skipped rather than panicking.
                let Some(addr) = (i as u64)
                    .checked_mul(cbObjectData as u64)
                    .and_then(|ofs| (rgdod as u64).checked_add(ofs))
                    .and_then(|addr| u32::try_from(addr).ok())
                else {
                    continue;
                };
                if !crate::ddraw::guest_range(
                    ctx,
                    addr,
                    std::mem::size_of::<DIDEVICEOBJECTDATA>() as u32,
                ) {
                    continue;
                }
                if crate::Ptr::new(addr)
                    .write(
                        &mut ctx.memory,
                        DIDEVICEOBJECTDATA {
                            dwOfs: event.ofs,
                            dwData: event.data,
                            dwTimeStamp: event.time,
                            dwSequence: event.sequence,
                        },
                    )
                    .is_none()
                {
                    continue;
                }
            }
        }
        if crate::Ptr::<u32>::new(pdwInOut)
            .write(&mut ctx.memory, events.len() as u32)
            .is_none()
        {
            return DIERR_INVALIDPARAM;
        }

        if overflowed { DI_BUFFEROVERFLOW } else { DI_OK }
    }

    #[win32_derive::dllexport]
    pub fn SetDataFormat(ctx: &mut Context, this: u32, lpdf: u32) -> u32 {
        // DIDATAFORMAT is six dwords: dwSize, dwObjSize, dwFlags,
        // dwDataSize, dwNumObjs, rgodf.
        const DIDATAFORMAT_SIZE: u32 = 24;
        if !crate::ddraw::guest_range(ctx, lpdf, DIDATAFORMAT_SIZE) {
            return DIERR_INVALIDPARAM;
        }
        let read = |ofs: u32| crate::Ptr::<u32>::new(lpdf + ofs).read(&ctx.memory);
        let (Some(dw_size), Some(dw_obj_size), Some(dw_data_size), Some(dw_num_objs), Some(rgodf)) =
            (read(0), read(4), read(12), read(16), read(20))
        else {
            return DIERR_INVALIDPARAM;
        };
        if dw_size != DIDATAFORMAT_SIZE || dw_obj_size == 0 || dw_data_size == 0 {
            return DIERR_INVALIDPARAM;
        }
        // A non-empty object list has to point at a readable table.
        if dw_num_objs != 0 {
            let Some(table) = (dw_obj_size as u64)
                .checked_mul(dw_num_objs as u64)
                .and_then(|n| u32::try_from(n).ok())
            else {
                return DIERR_INVALIDPARAM;
            };
            if rgodf == 0 || !crate::ddraw::guest_range(ctx, rgodf, table) {
                return DIERR_INVALIDPARAM;
            }
        }
        if let Some(device) = lock().devices.get_mut(&this) {
            device.data_size = dw_data_size;
        }
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

    /// Serialize a DIDEVICEINSTANCE for `kind` at `addr`, with `guid` as the
    /// instance GUID. The caller has already zeroed and validated `size`
    /// bytes at `addr`.
    pub fn write_device_instance(
        ctx: &mut Context,
        addr: u32,
        size: u32,
        kind: DeviceKind,
        guid: &GUID,
    ) {
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
        ctx.memory.write::<u32>(addr, size);
        // The first GUID is the device instance, the second is the product.
        write_guid(ctx, addr + 4, guid);
        write_guid(ctx, addr + 20, product);
        ctx.memory.write::<u32>(addr + 36, devtype);
        write_cstr(ctx, addr + 40, instance);
        write_cstr(ctx, addr + 40 + MAX_PATH as u32, product_name);
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceInfo(ctx: &mut Context, this: u32, pdidi: u32) -> u32 {
        let Some(size) = crate::Ptr::<u32>::new(pdidi).read(&ctx.memory) else {
            return DIERR_INVALIDPARAM;
        };
        let size = size as usize;
        if !(DIDEVICEINSTANCE_MIN_SIZE..=DIDEVICEINSTANCE_SIZE).contains(&size)
            || pdidi as usize + size > ctx.memory.bytes.len()
        {
            return DIERR_INVALIDPARAM;
        }
        let (kind, _) = device(this);
        let guid = device_guid(this);
        if let Some(dst) = ctx
            .memory
            .bytes
            .get_mut(pdidi as usize..)
            .and_then(|b| b.get_mut(..size))
        {
            dst.fill(0);
        }
        write_device_instance(ctx, pdidi, size as u32, kind, &guid);
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
        if lplpde != 0 && crate::ddraw::guest_range(ctx, lplpde, 4) {
            crate::Ptr::<u32>::new(lplpde).write(&mut ctx.memory, 0);
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
    fn get_device_data_rejects_null_page_pointers() {
        let mut ctx = context();
        // The count out-param is required and must be out of the null page;
        // a low non-null event array is likewise rejected. Both checks run
        // before device lookup, so an unknown `this` does not matter.
        for (rgdod, pdw_in_out) in [(0x2000, 0), (0x2000, 0x500), (0x500, 0x2000)] {
            assert_eq!(
                IDirectInputDevice::GetDeviceData(&mut ctx, 0x2100, 16, rgdod, pdw_in_out, 0),
                DIERR_INVALIDPARAM
            );
        }
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
                data_size: 0,
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
    fn set_and_get_property_reject_null_pointers() {
        let mut ctx = context();
        lock().devices.insert(
            0x2200,
            Device {
                kind: DeviceKind::Joystick,
                acquired: false,
                guid: GUID_Joystick,
                properties: HashMap::new(),
                refcount: 1,
                data_size: 0,
            },
        );

        // SetProperty must not read or store from a null-page pdiph.
        ctx.memory[0x500..][..20].fill(0xAB);
        assert_eq!(
            IDirectInputDevice::SetProperty(&mut ctx, 0x2200, DIPROP_DEADZONE, 0x500),
            DIERR_INVALIDPARAM
        );
        assert!(
            !lock()
                .devices
                .get(&0x2200)
                .unwrap()
                .properties
                .contains_key(&DIPROP_DEADZONE)
        );

        // GetProperty (BUFFERSIZE branch) must not write through a null-page pdiph.
        ctx.memory[0x500..][..8].fill(0xCD);
        assert_eq!(
            IDirectInputDevice::GetProperty(&mut ctx, 0x2200, DIPROP_BUFFERSIZE, 0x500),
            E_POINTER
        );
        assert_eq!(&ctx.memory.bytes[0x500..0x508], &[0xCD; 8]);

        // GetProperty (BUFFERSIZE branch) must fail when the dwData field
        // is out of range instead of silently losing the result.
        assert_eq!(
            IDirectInputDevice::GetProperty(&mut ctx, 0x2200, DIPROP_BUFFERSIZE, 0x3FF0),
            E_POINTER
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
    fn get_capabilities_rejects_null_destination() {
        let mut ctx = context();

        assert_eq!(
            IDirectInputDevice::GetCapabilities(&mut ctx, 0x2000, 0),
            DIERR_INVALIDPARAM
        );
        assert_eq!(&ctx.memory.bytes[0..DIDEVCAPS_SIZE], &[0u8; DIDEVCAPS_SIZE]);

        assert_eq!(
            IDirectInputDevice::GetCapabilities(&mut ctx, 0x2000, 0x500),
            DIERR_INVALIDPARAM
        );
        assert_eq!(&ctx.memory.bytes[0..DIDEVCAPS_SIZE], &[0u8; DIDEVCAPS_SIZE]);
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

    #[test]
    fn set_data_format_validates_the_didataformat() {
        let mut ctx = context();
        lock().devices.insert(
            0x2300,
            Device {
                kind: DeviceKind::Joystick,
                acquired: true,
                guid: GUID_Joystick,
                properties: HashMap::new(),
                refcount: 1,
                data_size: 0,
            },
        );

        // Null, low, out-of-range, and wrong-size format pointers all fail.
        for lpdf in [0, 0x500, u32::MAX - 8] {
            assert_eq!(
                IDirectInputDevice::SetDataFormat(&mut ctx, 0x2300, lpdf),
                DIERR_INVALIDPARAM
            );
        }
        ctx.memory.write::<u32>(0x2000, 20); // dwSize too small
        assert_eq!(
            IDirectInputDevice::SetDataFormat(&mut ctx, 0x2300, 0x2000),
            DIERR_INVALIDPARAM
        );

        // A valid DIDATAFORMAT for a 272-byte DIJOYSTATE2 with no object
        // table is accepted and recorded.
        ctx.memory.write::<u32>(0x2000, 24); // dwSize
        ctx.memory.write::<u32>(0x2004, 24); // dwObjSize
        ctx.memory.write::<u32>(0x2008, 0); // dwFlags
        ctx.memory.write::<u32>(0x200c, 272); // dwDataSize
        ctx.memory.write::<u32>(0x2010, 0); // dwNumObjs
        ctx.memory.write::<u32>(0x2014, 0); // rgodf
        assert_eq!(
            IDirectInputDevice::SetDataFormat(&mut ctx, 0x2300, 0x2000),
            DI_OK
        );
        assert_eq!(lock().devices.get(&0x2300).unwrap().data_size, 272);

        // The negotiated size extends the accepted GetDeviceState range.
        assert_eq!(
            IDirectInputDevice::GetDeviceState(&mut ctx, 0x2300, 272, 0x3000),
            DI_OK
        );
    }

    #[test]
    fn write_guid_and_cstr_reject_null_page() {
        let mut ctx = context();
        let guid = GUID {
            data1: 0x12345678,
            data2: 0x9abc,
            data3: 0xdef0,
            data4: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        };

        ctx.memory.bytes[0..16].fill(0);
        super::write_guid(&mut ctx, 0, &guid);
        super::write_guid(&mut ctx, 0x500, &guid);
        assert_eq!(&ctx.memory.bytes[0..16], &[0u8; 16]);

        super::write_guid(&mut ctx, 0x1000, &guid);
        assert_eq!(ctx.memory.read::<GUID>(0x1000), guid);

        ctx.memory.bytes[0x1000..0x1008].fill(0);
        super::write_cstr(&mut ctx, 0x1000, b"foo");
        assert_eq!(&ctx.memory.bytes[0x1000..0x1004], b"foo\0");

        ctx.memory.bytes[0..4].fill(0);
        super::write_cstr(&mut ctx, 0, b"foo");
        super::write_cstr(&mut ctx, 0x500, b"foo");
        assert_eq!(&ctx.memory.bytes[0..4], &[0u8; 4]);
    }
}
