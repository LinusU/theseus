//! DirectInput keyboard and mouse.
//!
//! Device state comes from the shared input state in user32, which the host
//! message pump keeps up to date; see user32::input.

use std::{collections::HashMap, sync::Mutex};

use runtime::Context;

use crate::{ddraw::GUID, heap::Heap, kernel32, locked_state::LockedState, user32};

const GUID_SysMouse: GUID = GUID((
    0x6F1D2B60,
    0xD5A0,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
));

const GUID_SysKeyboard: GUID = GUID((
    0x6F1D2B61,
    0xD5A0,
    0x11CF,
    [0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
));

const IID_IUnknown: GUID = GUID((
    0x00000000,
    0x0000,
    0x0000,
    [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
));

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

/// Shared COM identity check: the object answers for IID_IUnknown only.
fn query_interface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> u32 {
    if ppv == 0 {
        return E_POINTER;
    }
    if riid == 0 {
        ctx.memory.write::<u32>(ppv, 0);
        return E_NOINTERFACE;
    }
    let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
    if iid == IID_IUnknown {
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
/// Offset of DIPROPDWORD::dwData, past the DIPROPHEADER.
const DIPROPDWORD_DWDATA: u32 = 16;

/// Which physical device a created IDirectInputDevice stands for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
}

pub struct Device {
    pub kind: DeviceKind,
    pub acquired: bool,
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
        query_interface(ctx, this, riid, ppv)
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
        if guid == GUID_SysKeyboard || guid == GUID_SysMouse {
            DI_OK
        } else {
            DIERR_DEVICENOTREG
        }
    }

    #[win32_derive::dllexport]
    pub fn RunControlPanel(_ctx: &mut Context, _this: u32, _hwnd: u32, _dwFlags: u32) -> u32 {
        todo!()
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

    fn set_acquired(this: u32, acquired: bool) {
        if let Some(device) = lock().devices.get_mut(&this) {
            device.acquired = acquired;
        }
    }

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> u32 {
        query_interface(ctx, this, riid, ppv)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        lock().devices.remove(&this);
        0
    }

    #[win32_derive::dllexport]
    pub fn GetCapabilities(_ctx: &mut Context, _this: u32, _lpCaps: u32) -> u32 {
        todo!()
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
        if rguidProp != DIPROP_BUFFERSIZE {
            log::warn!("dinput GetProperty: unhandled property {rguidProp:#x}");
            return DIERR_INVALIDPARAM;
        }
        let (kind, _) = device(this);
        let size = user32::state()
            .input
            .borrow()
            .buffer_size(kind == DeviceKind::Keyboard);
        ctx.memory
            .write::<u32>(pdiph + DIPROPDWORD_DWDATA, size as u32);
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SetProperty(ctx: &mut Context, this: u32, rguidProp: u32, pdiph: u32) -> u32 {
        if rguidProp != DIPROP_BUFFERSIZE {
            // Axis mode, dead zone and the force feedback properties don't
            // apply to the plain keyboard/mouse we emulate.
            log::warn!("dinput SetProperty: ignoring property {rguidProp:#x}");
            return DI_OK;
        }
        let (kind, _) = device(this);
        let size = ctx.memory.read::<u32>(pdiph + DIPROPDWORD_DWDATA);
        user32::state()
            .input
            .borrow_mut()
            .set_buffer_size(kind == DeviceKind::Keyboard, size as usize);
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
        };
        if cbData as usize != len {
            log::warn!("GetDeviceState: cbData {cbData} does not match {kind:?}");
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
        let (events, overflowed) = user32::state().input.borrow_mut().take_events(
            kind == DeviceKind::Keyboard,
            capacity,
            peek,
        );

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
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceInfo(_ctx: &mut Context, _this: u32, _pdidi: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn RunControlPanel(_ctx: &mut Context, _this: u32, _hwnd: u32, _dwFlags: u32) -> u32 {
        todo!()
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

    #[win32_derive::dllexport]
    pub fn CreateEffect(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumEffects(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetEffectInfo(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetForceFeedbackState(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SendForceFeedbackCommand(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumCreatedEffectObjects(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Escape(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Poll(_ctx: &mut Context, _this: u32) -> u32 {
        DI_OK
    }

    #[win32_derive::dllexport]
    pub fn SendDeviceData(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
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
        bytes[..4].copy_from_slice(&guid.0.0.to_le_bytes());
        bytes[4..6].copy_from_slice(&guid.0.1.to_le_bytes());
        bytes[6..8].copy_from_slice(&guid.0.2.to_le_bytes());
        bytes[8..].copy_from_slice(&guid.0.3);
        ctx.memory[addr..][..16].copy_from_slice(&bytes);
    }

    #[test]
    fn query_interface_answers_for_iunknown_only() {
        let mut ctx = context();
        write_guid(&mut ctx, 0x1000, &IID_IUnknown);
        write_guid(&mut ctx, 0x1020, &GUID_SysMouse);

        assert_eq!(
            IDirectInput::QueryInterface(&mut ctx, 0x2000, 0x1000, 0x1100),
            DI_OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x1100), 0x2000);

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
}
