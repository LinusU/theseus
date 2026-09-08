//! .ini file access (GetPrivateProfileString etc.).
//!
//! No ini files are read or written; every lookup reports the caller's default.

use runtime::Context;

use crate::Ptr;

#[win32_derive::dllexport]
pub fn GetPrivateProfileIntA(
    _ctx: &mut Context,
    _lpAppName: Ptr<u8>,
    _lpKeyName: Ptr<u8>,
    nDefault: i32,
    _lpFileName: Ptr<u8>,
) -> u32 {
    nDefault as u32
}

#[win32_derive::dllexport]
pub fn GetPrivateProfileStringA(
    ctx: &mut Context,
    _lpAppName: Ptr<u8>,
    _lpKeyName: Ptr<u8>,
    lpDefault: Ptr<u8>,
    lpReturnedString: Ptr<u8>,
    nSize: u32,
    _lpFileName: Ptr<u8>,
) -> u32 {
    let default = if lpDefault.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_str(lpDefault.addr).to_string()
    };
    let n = write_cstr(ctx, lpReturnedString, nSize, default.as_bytes());
    n as u32
}

#[win32_derive::dllexport]
pub fn WritePrivateProfileStringA(
    ctx: &mut Context,
    lpAppName: Ptr<u8>,
    lpKeyName: Ptr<u8>,
    lpString: Ptr<u8>,
    lpFileName: Ptr<u8>,
) -> bool {
    let app = ctx.memory.read_str(lpAppName.addr);
    let key = if lpKeyName.addr == 0 {
        "(null)"
    } else {
        ctx.memory.read_str(lpKeyName.addr)
    };
    let value = if lpString.addr == 0 {
        "(null)"
    } else {
        ctx.memory.read_str(lpString.addr)
    };
    let file = ctx.memory.read_str(lpFileName.addr);
    log::warn!("WritePrivateProfileStringA({file}: [{app}] {key}={value}): dropped");
    true
}

/// Copy `s` into a caller buffer of `size` bytes as a NUL-terminated string,
/// truncating as needed. Returns the number of characters written (excluding
/// the NUL), as the Get*String family of functions do.
pub fn write_cstr(ctx: &mut Context, buf: Ptr<u8>, size: u32, s: &[u8]) -> usize {
    if buf.addr == 0 || size == 0 {
        return 0;
    }
    let n = s.len().min(size as usize - 1);
    ctx.memory[buf.addr..][..n].copy_from_slice(&s[..n]);
    ctx.memory.write::<u8>(buf.addr + n as u32, 0);
    n
}
