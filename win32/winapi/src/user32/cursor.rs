//! Mouse cursors. Cursors carry no images yet; programs only need their
//! handles to be non-null and stable.

use runtime::Context;

use super::{HCURSOR, HINSTANCE, state};
use crate::{HANDLE, Ptr, handle::Handles, kernel32};

/// The IDC_* ids that LoadCursor(NULL, ...) accepts.
const SYSTEM_CURSORS: &[u32] = &[
    32512, // IDC_ARROW
    32513, // IDC_IBEAM
    32514, // IDC_WAIT
    32515, // IDC_CROSS
    32516, // IDC_UPARROW
    32640, // IDC_SIZE
    32641, // IDC_ICON
    32642, // IDC_SIZENWSE
    32643, // IDC_SIZENESW
    32644, // IDC_SIZEWE
    32645, // IDC_SIZENS
    32646, // IDC_SIZEALL
    32648, // IDC_NO
    32649, // IDC_HAND
    32650, // IDC_APPSTARTING
    32651, // IDC_HELP
];

/// A resource name as passed to the Load* functions.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceId {
    Id(u32),
    /// Upper-cased, as resource names compare case-insensitively.
    Name(String),
}

impl ResourceId {
    fn new(addr: u32, read_name: impl FnOnce() -> String) -> Self {
        if addr >> 16 == 0 {
            return ResourceId::Id(addr);
        }
        let name = read_name();
        // "#123" names resource 123.
        if let Some(id) = name.strip_prefix('#').and_then(|id| id.parse().ok()) {
            return ResourceId::Id(id);
        }
        ResourceId::Name(name.to_ascii_uppercase())
    }
}

#[derive(Debug, PartialEq)]
pub enum Cursor {
    /// A predefined cursor, by IDC_* id.
    System(u32),
    /// A cursor from the program's resources.
    Resource(ResourceId),
    /// A cursor built by CreateCursor.
    Created,
}

#[derive(Default)]
pub struct Cursors {
    handles: Handles<Cursor>,
    /// The cursor last passed to SetCursor.
    current: HCURSOR,
    /// ShowCursor's display counter; the cursor shows while it's at least 0.
    /// It starts at 0, as on a system with a mouse.
    show_count: i32,
}

impl Cursors {
    /// LoadCursor hands out one shared handle per cursor, however often it's
    /// asked for it.
    fn shared(&mut self, cursor: Cursor) -> HCURSOR {
        let existing = self.handles.iter().find(|(_, c)| **c == cursor);
        match existing {
            Some((handle, _)) => handle.to_raw(),
            None => self.handles.add(cursor).to_raw(),
        }
    }
}

fn load_cursor(ctx: &Context, hInstance: HINSTANCE, name: ResourceId) -> HCURSOR {
    let cursor = if hInstance == 0 {
        match name {
            ResourceId::Id(id) if SYSTEM_CURSORS.contains(&id) => Cursor::System(id),
            _ => {
                log::warn!("LoadCursor: no system cursor {name:?}");
                return 0;
            }
        }
    } else {
        // Only the program's own module has resources.
        let group_cursor = exe::ResourceName::Id(exe::RT::GROUP_CURSOR as u32);
        let wide;
        let resource_name = match &name {
            ResourceId::Id(id) => exe::ResourceName::Id(*id),
            ResourceId::Name(name) => {
                wide = widestring::U16String::from_str(name);
                exe::ResourceName::Name(&wide)
            }
        };
        if kernel32::lock()
            .find_resource(ctx, group_cursor, resource_name)
            .is_none()
        {
            log::warn!("LoadCursor: resource {name:?} not found");
            return 0;
        }
        Cursor::Resource(name)
    };
    state().cursors.borrow_mut().shared(cursor)
}

#[win32_derive::dllexport]
pub fn LoadCursorA(ctx: &mut Context, hInstance: HINSTANCE, lpCursorName: Ptr<u8>) -> HCURSOR {
    let name = ResourceId::new(lpCursorName.addr, || {
        ctx.memory.read_str(lpCursorName.addr).into_owned()
    });
    load_cursor(ctx, hInstance, name)
}

#[win32_derive::dllexport]
pub fn LoadCursorW(
    ctx: &mut Context,
    hInstance: HINSTANCE,
    lpCursorName: Ptr<u16>, /* WSTR */
) -> HCURSOR {
    let name = ResourceId::new(lpCursorName.addr, || {
        ctx.memory.read_wstr(lpCursorName.addr).to_string_lossy()
    });
    load_cursor(ctx, hInstance, name)
}

#[win32_derive::dllexport]
pub fn CreateCursor(
    _ctx: &mut Context,
    _hInst: HINSTANCE,
    _xHotSpot: i32,
    _yHotSpot: i32,
    _nWidth: i32,
    _nHeight: i32,
    _pvANDPlane: Ptr<u8>,
    _pvXORPlane: Ptr<u8>,
) -> HCURSOR {
    state()
        .cursors
        .borrow_mut()
        .handles
        .add(Cursor::Created)
        .to_raw()
}

#[win32_derive::dllexport]
pub fn SetCursor(_ctx: &mut Context, hCursor: HCURSOR) -> HCURSOR {
    let mut cursors = state().cursors.borrow_mut();
    if hCursor != 0 && cursors.handles.get(HANDLE::from_raw(hCursor)).is_none() {
        log::warn!("SetCursor({hCursor:#x}): not a cursor");
        return 0;
    }
    std::mem::replace(&mut cursors.current, hCursor)
}

#[win32_derive::dllexport]
pub fn GetCursor(_ctx: &mut Context) -> HCURSOR {
    state().cursors.borrow().current
}

#[win32_derive::dllexport]
pub fn ShowCursor(_ctx: &mut Context, bShow: bool) -> i32 {
    let mut cursors = state().cursors.borrow_mut();
    cursors.show_count += if bShow { 1 } else { -1 };
    cursors.show_count
}
