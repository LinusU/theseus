use runtime::*;

use super::*;
use crate::{Ptr, dllexport::win32flags, gdi32, handle::HANDLE, kernel32};

#[win32_derive::dllexport]
pub fn LoadCursorA(_ctx: &mut Context, hInstance: HINSTANCE, lpCursorName: Ptr<u8>) -> HCURSOR {
    // Cursor handles are opaque: the guest only ever hands them back to
    // SetCursor, so a stable handle per resource name is enough.
    state().load_cursor(hInstance, lpCursorName.addr)
}

#[win32_derive::dllexport]
pub fn LoadIconA(_ctx: &mut Context, hInstance: HINSTANCE, lpIconName: Ptr<u8>) -> HICON {
    state().load_icon(hInstance, lpIconName.addr)
}

#[derive(Debug, PartialEq, Eq, win32_derive::ABIEnum)]
pub enum IMAGE {
    BITMAP = 0,
    ICON = 1,
    CURSOR = 2,
}

win32flags! {
    pub struct LR {
        // TODO: add flags
    }
}

fn is_intresource(x: u32) -> bool {
    x >> 16 == 0
}

#[win32_derive::dllexport]
pub fn LoadImageA(
    ctx: &mut Context,
    hInst: HINSTANCE,
    name: Ptr<u8>,
    typ: u32,
    cx: u32,
    cy: u32,
    fuLoad: LR,
) -> HANDLE {
    if !is_intresource(name.addr) {
        log::warn!("LoadImage: string resource names are not supported");
        return HANDLE::null();
    }
    let name = exe::ResourceName::Id(name.addr);

    let Ok(typ) = IMAGE::try_from(typ) else {
        log::warn!("LoadImage: unknown image type {typ}");
        return HANDLE::null();
    };

    if typ != IMAGE::BITMAP {
        log::warn!("LoadImage: only bitmap resources are supported, got {typ:?}");
        return HANDLE::null();
    }
    let typ = exe::ResourceName::Id(match typ {
        IMAGE::CURSOR => exe::RT::CURSOR,
        IMAGE::BITMAP => exe::RT::BITMAP,
        IMAGE::ICON => exe::RT::ICON,
    } as u32);

    if !fuLoad.is_empty() {
        log::warn!("LoadImage: unsupported fuLoad flags {fuLoad:?}");
        return HANDLE::null();
    }

    let Some(buf) = kernel32::lock().find_resource(ctx, hInst, typ, name) else {
        log::warn!("LoadImage: resource not found");
        return HANDLE::null();
    };
    let (mut bitmap, pixels) = gdi32::Bitmap::parse(buf);
    // A zero cx/cy requests the image's natural size; nonzero requests that
    // differ mean scaling, which is not implemented.
    if (cx != 0 && bitmap.width != cx) || (cy != 0 && bitmap.height != cy) {
        log::warn!(
            "LoadImage: cannot scale {}x{} bitmap to {cx}x{cy}",
            bitmap.width,
            bitmap.height,
        );
        return HANDLE::null();
    }

    let pixels = unsafe { pixels.as_ptr().offset_from_unsigned(ctx.memory.as_ptr()) };
    bitmap.pixels = pixels as u32;

    gdi32::lock().new_bitmap_handle(bitmap).0
}

pub type HCURSOR = u32;
pub type HICON = u32;
pub type HMENU = u32;

/// Resolve an int-resource or string name and look it up in the module's
/// resource section. The resource's guest address is the opaque load handle:
/// it is stable, unique per resource, and valid for the module's lifetime.
fn load_named_resource(
    ctx: &Context,
    hInstance: HINSTANCE,
    addr: u32,
    typ: u32,
    wide: bool,
) -> u32 {
    let wide_name;
    let name = if is_intresource(addr) {
        exe::ResourceName::Id(addr)
    } else {
        // Resource names are Unicode; the ANSI form widens each byte.
        wide_name = if wide {
            ctx.memory.read_wstr(addr)
        } else {
            widestring::U16String::from_str(ctx.memory.read_str(addr))
        };
        exe::ResourceName::Name(&wide_name)
    };
    let Some(buf) =
        kernel32::lock().find_resource(ctx, hInstance, exe::ResourceName::Id(typ), name)
    else {
        return 0;
    };
    unsafe { buf.as_ptr().offset_from_unsigned(ctx.memory.as_ptr()) as u32 }
}

#[win32_derive::dllexport]
pub fn LoadAcceleratorsA(ctx: &mut Context, hInstance: HINSTANCE, lpTableName: Ptr<u8>) -> HACCEL {
    load_named_resource(
        ctx,
        hInstance,
        lpTableName.addr,
        9, /* RT_ACCELERATOR */
        false,
    )
}

#[win32_derive::dllexport]
pub fn LoadAcceleratorsW(
    ctx: &mut Context,
    hInstance: HINSTANCE,
    lpTableName: Ptr<u16>, /* WSTR */
) -> HACCEL {
    load_named_resource(
        ctx,
        hInstance,
        lpTableName.addr,
        9, /* RT_ACCELERATOR */
        true,
    )
}

#[win32_derive::dllexport]
pub fn LoadCursorW(
    _ctx: &mut Context,
    hInstance: HINSTANCE,
    lpCursorName: Ptr<u16>, /* WSTR */
) -> HCURSOR {
    state().load_cursor(hInstance, lpCursorName.addr)
}

#[win32_derive::dllexport]
pub fn LoadIconW(
    _ctx: &mut Context,
    hInstance: HINSTANCE,
    lpIconName: Ptr<u16>, /* WSTR */
) -> HICON {
    state().load_icon(hInstance, lpIconName.addr)
}

#[win32_derive::dllexport]
pub fn LoadMenuA(ctx: &mut Context, hInstance: HINSTANCE, lpMenuName: Ptr<u8>) -> HMENU {
    load_named_resource(ctx, hInstance, lpMenuName.addr, 4 /* RT_MENU */, false)
}

#[win32_derive::dllexport]
pub fn LoadMenuW(
    ctx: &mut Context,
    hInstance: HINSTANCE,
    lpMenuName: Ptr<u16>, /* WSTR */
) -> HMENU {
    load_named_resource(ctx, hInstance, lpMenuName.addr, 4 /* RT_MENU */, true)
}

fn find_string(ctx: &Context, hInstance: HINSTANCE, uID: u32) -> Option<&[u8]> {
    // Strings are stored as blocks of 16 consecutive strings.
    let (resource_id, index) = ((uID >> 4) + 1, uID & 0xF);

    let mut block = kernel32::lock().find_resource(
        ctx,
        hInstance,
        exe::ResourceName::Id(exe::RT::STRING as u32),
        exe::ResourceName::Id(resource_id),
    )?;

    use zerocopy::FromBytes;
    // Each block is a sequence of two byte length-prefixed strings.
    // Iterate through them to find the requested index.
    for i in 0.. {
        let (len, rest) = <u16>::read_from_prefix(block).ok()?;
        let (cur, next) = rest.split_at_checked(len as usize * 2)?;
        if i == index {
            return Some(cur);
        }
        block = next;
    }
    // The block is either exhausted (split_at_checked returns None) or the
    // index is found; this is a defensive fallback rather than panic.
    None
}

fn ansi_string(bytes: &[u8]) -> Vec<u8> {
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&utf16)
        .chars()
        .map(|ch| {
            if ch as u32 <= u8::MAX as u32 {
                ch as u8
            } else {
                b'?'
            }
        })
        .collect()
}

#[win32_derive::dllexport]
pub fn LoadStringA(
    ctx: &mut Context,
    hInstance: HINSTANCE,
    uID: u32,
    lpBuffer: Ptr<u8>,
    cchBufferMax: i32,
) -> i32 {
    if cchBufferMax <= 0 || lpBuffer.addr < 0x1000 {
        return 0;
    }
    let Some(bytes) = find_string(ctx, hInstance, uID) else {
        return 0;
    };
    let bytes = ansi_string(bytes);
    let copy_len = bytes.len().min(cchBufferMax as usize - 1);
    let Some(end) = lpBuffer.addr.checked_add(copy_len as u32 + 1) else {
        return 0;
    };
    if end as usize > ctx.memory.bytes.len() {
        return 0;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lpBuffer.addr as usize..)
        .and_then(|b| b.get_mut(..copy_len))
    {
        dst.copy_from_slice(&bytes[..copy_len]);
    }
    ctx.memory.write::<u8>(lpBuffer.addr + copy_len as u32, 0);
    copy_len as i32
}

#[win32_derive::dllexport]
pub fn LoadStringW(
    ctx: &mut Context,
    hInstance: HINSTANCE,
    uID: u32,
    lpBuffer: Ptr<u16>, /* WSTR */
    cchBufferMax: i32,
) -> i32 {
    // GetModuleHandle(null) hands back the image base, so a program asking for
    // its own resources passes either that or null.
    if cchBufferMax <= 0 || lpBuffer.addr < 0x1000 {
        return 0;
    }
    let Some(bytes) = find_string(ctx, hInstance, uID) else {
        return 0;
    };
    let bytes = Vec::from(bytes);
    // Copy at most cchBufferMax-1 UTF-16 units and nul-terminate, like
    // LoadStringA and the real API.
    let units = bytes.len() / 2;
    let copy_units = units.min(cchBufferMax as usize - 1);
    let copy_bytes = copy_units * 2;
    let Some(end) = lpBuffer
        .addr
        .checked_add(copy_bytes as u32 + 2)
        .map(|end| end as usize)
    else {
        return 0;
    };
    if end > ctx.memory.bytes.len() {
        return 0;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lpBuffer.addr as usize..)
        .and_then(|b| b.get_mut(..copy_bytes))
    {
        dst.copy_from_slice(&bytes[..copy_bytes]);
    }
    ctx.memory
        .write::<u16>(lpBuffer.addr + copy_bytes as u32, 0);
    copy_units as i32
}

#[cfg(test)]
mod tests {
    use super::ansi_string;

    #[test]
    fn ansi_string_decodes_resource_utf16() {
        assert_eq!(ansi_string(b"A\0B\0"), b"AB");
        assert_eq!(ansi_string(&[0x00, 0x01]), b"?");
    }
}
