use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use runtime::Context;

pub type HKEY = u32;

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_MORE_DATA: u32 = 234;

const REG_CREATED_NEW_KEY: u32 = 1;
const REG_OPENED_EXISTING_KEY: u32 = 2;

/// A minimal in-memory registry. Keys exist only after a successful
/// RegCreateKeyEx; values persist for the life of the process.
#[derive(Default)]
struct Registry {
    /// Open handles → normalized key path.
    handles: HashMap<HKEY, String>,
    /// Normalized key paths that exist.
    keys: HashSet<String>,
    /// (key path, value name) → (REG_* type, data).
    values: HashMap<(String, String), (u32, Vec<u8>)>,
    next_handle: u32,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        next_handle: 0xA000_0000,
        ..Registry::default()
    })
});

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// The normalized path a handle opens beneath: predefined roots keep their
/// raw value, opened handles resolve to their key's path.
fn key_path(reg: &Registry, hkey: HKEY) -> Option<String> {
    if let Some(path) = reg.handles.get(&hkey) {
        return Some(path.clone());
    }
    // Predefined roots (HKEY_CLASSES_ROOT ..= HKEY_PERFORMANCE_NLSTEXT) live
    // at 0x8000000x and are always open.
    if (0x8000_0000..=0x8000_000D).contains(&hkey) {
        return Some(format!("ROOT{hKey:08X}", hKey = hkey));
    }
    None
}

fn open_key(reg: &mut Registry, path: String) -> HKEY {
    let handle = reg.next_handle;
    reg.next_handle += 1;
    reg.handles.insert(handle, path);
    handle
}

/// The caller-visible key path: base handle's path plus the subkey, with
/// registry case-insensitivity folded in.
fn subkey_path(reg: &Registry, hkey: HKEY, subkey: &str) -> Option<String> {
    let base = key_path(reg, hkey)?;
    Some(format!("{base}\\{}", subkey.to_uppercase()))
}

fn write_out<T>(ctx: &mut Context, addr: u32, value: T)
where
    T: zerocopy::IntoBytes + zerocopy::Immutable + zerocopy::KnownLayout,
{
    if addr != 0 {
        // An out-of-range out-pointer loses the result rather than
        // panicking the host.
        let _ = crate::Ptr::<T>::new(addr).write(&mut ctx.memory, value);
    }
}

#[win32_derive::dllexport]
pub fn RegCloseKey(_ctx: &mut Context, hKey: HKEY) -> u32 /* WIN32_ERROR */ {
    let mut reg = registry();
    if reg.handles.remove(&hKey).is_some() {
        ERROR_SUCCESS
    } else {
        ERROR_INVALID_HANDLE
    }
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
    if !crate::ddraw::guest_range(ctx, lpBuffer.addr, name.len() as u32 + 1) {
        return false;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lpBuffer.addr as usize..)
        .and_then(|b| b.get_mut(..name.len()))
    {
        dst.copy_from_slice(name);
    }
    let output_len = name.len() + 1;
    if crate::Ptr::<u8>::new(lpBuffer.addr + name.len() as u32)
        .write(&mut ctx.memory, 0)
        .is_none()
        || pcbBuffer
            .write(&mut ctx.memory, output_len as u32)
            .is_none()
    {
        return false;
    }
    true
}

fn reg_create_key(
    ctx: &mut Context,
    hkey: HKEY,
    subkey: String,
    phk_result: u32,
    lpdw_disposition: u32,
) -> u32 {
    let mut reg = registry();
    let Some(path) = subkey_path(&reg, hkey, &subkey) else {
        return ERROR_INVALID_HANDLE;
    };
    let existed = !reg.keys.insert(path.clone());
    let handle = open_key(&mut reg, path);
    write_out(ctx, phk_result, handle);
    write_out(
        ctx,
        lpdw_disposition,
        if existed {
            REG_OPENED_EXISTING_KEY
        } else {
            REG_CREATED_NEW_KEY
        },
    );
    ERROR_SUCCESS
}

#[win32_derive::dllexport]
pub fn RegCreateKeyExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpSubKey: u32, /* WSTR */
    _Reserved: u32,
    _lpClass: u32,              /* WSTR */
    _dwOptions: u32,            /* REG_OPEN_CREATE_OPTIONS */
    _samDesired: u32,           /* REG_SAM_FLAGS */
    _lpSecurityAttributes: u32, /* SECURITY_ATTRIBUTES */
    phkResult: u32,
    lpdwDisposition: u32, /* REG_CREATE_KEY_DISPOSITION */
) -> u32 /* WIN32_ERROR */ {
    if phkResult < 0x1000 || (lpSubKey != 0 && lpSubKey < 0x1000) {
        return ERROR_INVALID_PARAMETER;
    }
    let subkey = ctx.memory.read_wstr(lpSubKey).to_string_lossy();
    reg_create_key(ctx, hKey, subkey, phkResult, lpdwDisposition)
}

#[win32_derive::dllexport]
pub fn RegOpenKeyExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpSubKey: u32, /* STR */
    _ulOptions: u32,
    _samDesired: u32, /* REG_SAM_FLAGS */
    phkResult: u32,
) -> u32 /* WIN32_ERROR */ {
    if phkResult < 0x1000 || (lpSubKey != 0 && lpSubKey < 0x1000) {
        return ERROR_INVALID_PARAMETER;
    }
    let subkey = ctx.memory.read_str(lpSubKey);
    let mut reg = registry();
    let Some(path) = subkey_path(&reg, hKey, subkey) else {
        return ERROR_INVALID_HANDLE;
    };
    if !reg.keys.contains(&path) {
        return ERROR_FILE_NOT_FOUND;
    }
    let handle = open_key(&mut reg, path);
    write_out(ctx, phkResult, handle);
    ERROR_SUCCESS
}

/// A null name asks for the key's default value; a low non-null pointer is
/// invalid and yields `None`.
fn value_name(ctx: &Context, addr: u32, wide: bool) -> Option<String> {
    if addr == 0 {
        Some(String::new())
    } else if addr < 0x1000 {
        None
    } else if wide {
        Some(ctx.memory.read_wstr(addr).to_string_lossy().to_uppercase())
    } else {
        Some(ctx.memory.read_str(addr).to_uppercase())
    }
}

fn reg_query_value(
    ctx: &mut Context,
    hkey: HKEY,
    name: String,
    lp_type: u32,
    lp_data: u32,
    lpcb_data: u32,
) -> u32 {
    // Null out-pointers are legal "don't care" markers, but a low non-null
    // pointer would read or write the emulated null page.
    if (lp_type != 0 && lp_type < 0x1000)
        || (lpcb_data != 0 && lpcb_data < 0x1000)
        || (lp_data != 0 && lp_data < 0x1000)
    {
        return ERROR_INVALID_PARAMETER;
    }
    let reg = registry();
    let Some(path) = key_path(&reg, hkey) else {
        return ERROR_INVALID_HANDLE;
    };
    let Some(&(typ, ref data)) = reg.values.get(&(path, name)) else {
        return ERROR_FILE_NOT_FOUND;
    };
    write_out(ctx, lp_type, typ);
    let room = if lpcb_data == 0 {
        0
    } else {
        crate::Ptr::<u32>::new(lpcb_data)
            .read(&ctx.memory)
            .unwrap_or(0) as usize
    };
    if lp_data == 0 {
        // A size query reports the needed byte count.
        write_out(ctx, lpcb_data, data.len() as u32);
        return ERROR_SUCCESS;
    }
    if room < data.len() {
        write_out(ctx, lpcb_data, data.len() as u32);
        return ERROR_MORE_DATA;
    }
    let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lp_data as usize..lp_data as usize + data.len())
    else {
        return ERROR_INVALID_PARAMETER;
    };
    dst.copy_from_slice(data);
    write_out(ctx, lpcb_data, data.len() as u32);
    ERROR_SUCCESS
}

#[win32_derive::dllexport]
pub fn RegQueryValueExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: u32, /* STR */
    _lpReserved: u32,
    lpType: u32, /* REG_VALUE_TYPE */
    lpData: u32,
    lpcbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let Some(name) = value_name(ctx, lpValueName, false) else {
        return ERROR_INVALID_PARAMETER;
    };
    reg_query_value(ctx, hKey, name, lpType, lpData, lpcbData)
}

#[win32_derive::dllexport]
pub fn RegQueryValueExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: u32, /* WSTR */
    _lpReserved: u32,
    lpType: u32, /* REG_VALUE_TYPE */
    lpData: u32,
    lpcbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let Some(name) = value_name(ctx, lpValueName, true) else {
        return ERROR_INVALID_PARAMETER;
    };
    reg_query_value(ctx, hKey, name, lpType, lpData, lpcbData)
}

#[win32_derive::dllexport]
pub fn RegSetValueExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: u32, /* WSTR */
    _Reserved: u32,
    dwType: u32, /* REG_VALUE_TYPE */
    lpData: u32,
    cbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let Some(name) = value_name(ctx, lpValueName, true) else {
        return ERROR_INVALID_PARAMETER;
    };
    let mut reg = registry();
    let Some(path) = key_path(&reg, hKey) else {
        return ERROR_INVALID_HANDLE;
    };
    let data = if lpData == 0 {
        Vec::new()
    } else if lpData < 0x1000 {
        return ERROR_INVALID_PARAMETER;
    } else {
        let Some(bytes) = ctx
            .memory
            .bytes
            .get(lpData as usize..lpData as usize + cbData as usize)
        else {
            return ERROR_INVALID_PARAMETER;
        };
        bytes.to_vec()
    };
    reg.values.insert((path, name), (dwType, data));
    ERROR_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Memory};

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

    #[test]
    fn registry_apis_reject_out_of_range_guest_pointers() {
        let mut ctx = context();
        // HKEY_CLASSES_ROOT is a predefined base that is always open.
        const HKCR: HKEY = 0x8000_0000;

        // A claimed-large buffer whose address cannot hold the name fails
        // rather than panicking.
        ctx.memory.write::<u32>(0x2000, 64);
        assert!(!GetUserNameA(
            &mut ctx,
            crate::Ptr::new(0xffff_fff0),
            crate::Ptr::new(0x2000)
        ));
        assert!(!GetUserNameA(
            &mut ctx,
            crate::Ptr::new(0x3ffe),
            crate::Ptr::new(0x2000)
        ));

        // A valid buffer with a non-null but out-of-range size pointer fails.
        assert!(!GetUserNameA(
            &mut ctx,
            crate::Ptr::new(0x2000),
            crate::Ptr::new(0xffff_fff0)
        ));

        // A value set through a good pointer can be queried; a bad data
        // pointer and a bad size pointer return errors instead of panicking.
        ctx.memory[0x3000..][..4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            RegSetValueExW(&mut ctx, HKCR, 0, 0, 1, 0x3000, 4),
            ERROR_SUCCESS
        );
        ctx.memory.write::<u32>(0x2000, 64);
        assert_eq!(
            RegQueryValueExW(&mut ctx, HKCR, 0, 0, 0xffff_fff0, 0xffff_fff0, 0x2000),
            ERROR_INVALID_PARAMETER
        );
        // Setting through a bad data pointer is an explicit error.
        assert_eq!(
            RegSetValueExW(&mut ctx, HKCR, 0, 0, 1, 0xffff_fff0, 8),
            ERROR_INVALID_PARAMETER
        );

        // Low non-null value-name pointers are invalid, not the default value.
        assert_eq!(
            RegQueryValueExA(&mut ctx, HKCR, 0x500, 0, 0, 0, 0),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegQueryValueExW(&mut ctx, HKCR, 0x500, 0, 0, 0, 0),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegSetValueExW(&mut ctx, HKCR, 0x500, 0, 1, 0x3000, 4),
            ERROR_INVALID_PARAMETER
        );

        // Low out-pointers fail instead of touching the null page.
        assert_eq!(
            RegQueryValueExW(&mut ctx, HKCR, 0, 0, 0x500, 0x2000, 0x2000),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegQueryValueExW(&mut ctx, HKCR, 0, 0, 0, 0x500, 0x2000),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegSetValueExW(&mut ctx, HKCR, 0, 0, 1, 0x500, 4),
            ERROR_INVALID_PARAMETER
        );

        // Sub-0x1000 subkey or result pointers are rejected.
        assert_eq!(
            RegOpenKeyExA(&mut ctx, HKCR, 0x500, 0, 0, 0x2000),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegOpenKeyExA(&mut ctx, HKCR, 0x3000, 0, 0, 0x500),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegCreateKeyExW(&mut ctx, HKCR, 0x500, 0, 0, 0, 0, 0, 0x2000, 0),
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            RegCreateKeyExW(&mut ctx, HKCR, 0x3000, 0, 0, 0, 0, 0, 0x500, 0),
            ERROR_INVALID_PARAMETER
        );
    }
}
