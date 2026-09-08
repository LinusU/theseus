use runtime::Context;

use crate::stub;

pub type HKEY = u32;

const ERROR_FILE_NOT_FOUND: u32 = 2;

#[win32_derive::dllexport]
pub fn RegCloseKey(_ctx: &mut Context, _hKey: HKEY) -> u32 /* WIN32_ERROR */ {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn GetUserNameA(
    ctx: &mut Context,
    lpBuffer: crate::Ptr<u8>,
    pcbBuffer: crate::Ptr<u32>,
) -> bool {
    let name = b"user";
    let size = pcbBuffer.read(&ctx.memory).unwrap_or(0);
    if (size as usize) < name.len() + 1 {
        return false;
    }
    ctx.memory[lpBuffer.addr..][..name.len()].copy_from_slice(name);
    ctx.memory.write::<u8>(lpBuffer.addr + name.len() as u32, 0);
    pcbBuffer.write(&mut ctx.memory, name.len() as u32 + 1);
    true
}

#[win32_derive::dllexport]
pub fn RegCreateKeyExW(
    _ctx: &mut Context,
    _hKey: HKEY,
    _lpSubKey: u32, /* WSTR */
    _Reserved: u32,
    _lpClass: u32,              /* WSTR */
    _dwOptions: u32,            /* REG_OPEN_CREATE_OPTIONS */
    _samDesired: u32,           /* REG_SAM_FLAGS */
    _lpSecurityAttributes: u32, /* SECURITY_ATTRIBUTES */
    _phkResult: HKEY,
    _lpdwDisposition: u32, /* REG_CREATE_KEY_DISPOSITION */
) -> u32 /* WIN32_ERROR */ {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn RegOpenKeyExA(
    _ctx: &mut Context,
    _hKey: HKEY,
    _lpSubKey: u32, /* STR */
    _ulOptions: u32,
    _samDesired: u32, /* REG_SAM_FLAGS */
    _phkResult: HKEY,
) -> u32 /* WIN32_ERROR */ {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn RegQueryValueExA(
    _ctx: &mut Context,
    _hKey: HKEY,
    _lpValueName: u32, /* STR */
    _lpReserved: u32,
    _lpType: u32, /* REG_VALUE_TYPE */
    _lpData: u32,
    _lpcbData: u32,
) -> u32 /* WIN32_ERROR */ {
    stub!(ERROR_FILE_NOT_FOUND)
}

#[win32_derive::dllexport]
pub fn RegQueryValueExW(
    _ctx: &mut Context,
    _hKey: HKEY,
    _lpValueName: u32, /* WSTR */
    _lpReserved: u32,
    _lpType: u32, /* REG_VALUE_TYPE */
    _lpData: u32,
    _lpcbData: u32,
) -> u32 /* WIN32_ERROR */ {
    stub!(ERROR_FILE_NOT_FOUND)
}

#[win32_derive::dllexport]
pub fn RegSetValueExW(
    _ctx: &mut Context,
    _hKey: HKEY,
    _lpValueName: u32, /* WSTR */
    _Reserved: u32,
    _dwType: u32, /* REG_VALUE_TYPE */
    _lpData: u32,
    _cbData: u32,
) -> u32 /* WIN32_ERROR */ {
    stub!(0)
}

/// Fake key handle handed out by RegCreateKeyExA; the registry isn't stored.
const FAKE_HKEY: HKEY = 0x8000_0001;

#[win32_derive::dllexport]
pub fn RegCreateKeyExA(
    ctx: &mut Context,
    _hKey: HKEY,
    lpSubKey: crate::Ptr<u8>,
    _Reserved: u32,
    _lpClass: u32,
    _dwOptions: u32,
    _samDesired: u32,
    _lpSecurityAttributes: u32,
    phkResult: crate::Ptr<HKEY>,
    lpdwDisposition: crate::Ptr<u32>,
) -> u32 /* WIN32_ERROR */ {
    const REG_CREATED_NEW_KEY: u32 = 1;
    let sub_key = ctx.memory.read_str(lpSubKey.addr);
    log::warn!("RegCreateKeyExA({sub_key:?}): registry not stored");
    phkResult.write(&mut ctx.memory, FAKE_HKEY);
    if lpdwDisposition.addr != 0 {
        lpdwDisposition.write(&mut ctx.memory, REG_CREATED_NEW_KEY);
    }
    0
}

#[win32_derive::dllexport]
pub fn RegDeleteKeyA(_ctx: &mut Context, _hKey: HKEY, _lpSubKey: u32 /* STR */) -> u32 /* WIN32_ERROR */ {
    0
}

#[win32_derive::dllexport]
pub fn RegDeleteValueA(_ctx: &mut Context, _hKey: HKEY, _lpValueName: u32 /* STR */) -> u32 /* WIN32_ERROR */ {
    0
}

#[win32_derive::dllexport]
pub fn RegSetValueExA(
    ctx: &mut Context,
    _hKey: HKEY,
    lpValueName: crate::Ptr<u8>,
    _Reserved: u32,
    _dwType: u32, /* REG_VALUE_TYPE */
    _lpData: u32,
    _cbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let name = if lpValueName.addr == 0 {
        "(default)".to_string()
    } else {
        ctx.memory.read_str(lpValueName.addr).to_string()
    };
    log::warn!("RegSetValueExA({name:?}): registry not stored");
    0
}
