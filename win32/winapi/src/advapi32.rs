//! advapi32: the registry, plus GetUserName.
//!
//! The registry is an in-memory tree. It starts out empty unless
//! THESEUS_REGISTRY names a file in Windows `.reg` syntax (see [`Registry::load`]),
//! which is how a program's installer-written settings are supplied. Writes
//! update the tree but are not persisted.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use runtime::Context;

use crate::Ptr;

pub type HKEY = u32;

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_MORE_DATA: u32 = 234;

const HKEY_CLASSES_ROOT: HKEY = 0x8000_0000;
const HKEY_CURRENT_USER: HKEY = 0x8000_0001;
const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002;
const HKEY_USERS: HKEY = 0x8000_0003;

const REG_SZ: u32 = 1;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;

#[derive(Debug, Clone)]
pub enum Value {
    Sz(String),
    Dword(u32),
    Binary(Vec<u8>),
}

impl Value {
    fn reg_type(&self) -> u32 {
        match self {
            Value::Sz(_) => REG_SZ,
            Value::Dword(_) => REG_DWORD,
            Value::Binary(_) => REG_BINARY,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Value::Sz(s) => {
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0);
                bytes
            }
            Value::Dword(d) => d.to_le_bytes().to_vec(),
            Value::Binary(b) => b.clone(),
        }
    }
}

#[derive(Default)]
pub struct Registry {
    /// Full key path (root name plus subkeys, case-insensitive) => values by
    /// name (case-insensitive; the default value is the empty name).
    keys: HashMap<String, HashMap<String, Value>>,
    /// Open key handles => key path.
    handles: HashMap<HKEY, String>,
    next_handle: HKEY,
}

fn root_name(hkey: HKEY) -> Option<&'static str> {
    Some(match hkey {
        HKEY_CLASSES_ROOT => "HKEY_CLASSES_ROOT",
        HKEY_CURRENT_USER => "HKEY_CURRENT_USER",
        HKEY_LOCAL_MACHINE => "HKEY_LOCAL_MACHINE",
        HKEY_USERS => "HKEY_USERS",
        _ => return None,
    })
}

fn normalize(path: &str) -> String {
    path.trim_matches('\\').to_ascii_lowercase()
}

impl Registry {
    /// The full path for a subkey relative to an open key or root handle.
    fn resolve(&self, hkey: HKEY, sub_key: &str) -> Option<String> {
        let base = match root_name(hkey) {
            Some(root) => root.to_string(),
            None => self.handles.get(&hkey)?.clone(),
        };
        Some(if sub_key.is_empty() {
            base
        } else {
            format!("{base}\\{}", sub_key.trim_matches('\\'))
        })
    }

    fn open(&mut self, path: String) -> HKEY {
        let handle = self.next_handle.max(0x100);
        self.next_handle = handle + 1;
        self.handles.insert(handle, path);
        handle
    }

    pub fn key_exists(&self, path: &str) -> bool {
        let path = normalize(path);
        // A key also exists if it has subkeys.
        self.keys
            .keys()
            .any(|k| *k == path || k.starts_with(&format!("{path}\\")))
    }

    pub fn set(&mut self, path: &str, name: &str, value: Value) {
        self.keys
            .entry(normalize(path))
            .or_default()
            .insert(name.to_ascii_lowercase(), value);
    }

    pub fn get(&self, path: &str, name: &str) -> Option<&Value> {
        self.keys
            .get(&normalize(path))?
            .get(&name.to_ascii_lowercase())
    }

    /// Load a file in regedit's `.reg` export syntax:
    ///
    /// ```text
    /// [HKEY_LOCAL_MACHINE\SOFTWARE\Vendor\Game]
    /// "Name"="string value"
    /// "Flag"=dword:00000001
    /// @="default value"
    /// ```
    pub fn load(&mut self, text: &str) {
        let mut key: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with("Windows Registry") {
                continue;
            }
            if let Some(k) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                self.keys.entry(normalize(k)).or_default();
                key = Some(k.to_string());
                continue;
            }
            let Some(key) = &key else {
                log::warn!("registry: value outside a key: {line:?}");
                continue;
            };
            let Some((name, value)) = line.split_once('=') else {
                log::warn!("registry: bad line {line:?}");
                continue;
            };
            let name = if name == "@" {
                ""
            } else {
                name.trim_matches('"')
            };
            let value = if let Some(hex) = value.strip_prefix("dword:") {
                Value::Dword(u32::from_str_radix(hex, 16).unwrap_or(0))
            } else if let Some(hex) = value.strip_prefix("hex:") {
                Value::Binary(
                    hex.split(',')
                        .filter_map(|b| u8::from_str_radix(b.trim(), 16).ok())
                        .collect(),
                )
            } else {
                let s = value.trim_matches('"').replace("\\\\", "\\").replace("\\\"", "\"");
                Value::Sz(s)
            };
            self.set(key, name, value);
        }
    }
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

pub fn registry() -> MutexGuard<'static, Option<Registry>> {
    let mut lock = REGISTRY.lock().unwrap();
    if lock.is_none() {
        let mut registry = Registry::default();
        #[cfg(not(target_family = "wasm"))]
        if let Ok(path) = std::env::var("THESEUS_REGISTRY") {
            match std::fs::read_to_string(&path) {
                Ok(text) => registry.load(&text),
                Err(err) => log::error!("THESEUS_REGISTRY={path}: {err}"),
            }
        }
        *lock = Some(registry);
    }
    lock
}

fn read_opt_str(ctx: &Context, ptr: Ptr<u8>) -> String {
    if ptr.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_str(ptr.addr).to_string()
    }
}

#[win32_derive::dllexport]
pub fn RegCloseKey(_ctx: &mut Context, hKey: HKEY) -> u32 /* WIN32_ERROR */ {
    if let Some(registry) = registry().as_mut() {
        registry.handles.remove(&hKey);
    }
    ERROR_SUCCESS
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

fn open_key(ctx: &mut Context, hkey: HKEY, sub_key: &str, create: bool, phkResult: Ptr<HKEY>) -> u32 {
    let mut lock = registry();
    let registry = lock.as_mut().unwrap();
    let Some(path) = registry.resolve(hkey, sub_key) else {
        log::warn!("registry: bad key handle {hkey:#x}");
        return ERROR_FILE_NOT_FOUND;
    };
    if !registry.key_exists(&path) {
        if !create {
            log::warn!("RegOpenKeyEx({path:?}): not found");
            return ERROR_FILE_NOT_FOUND;
        }
        registry.keys.entry(normalize(&path)).or_default();
    }
    let handle = registry.open(path);
    drop(lock);
    phkResult.write(&mut ctx.memory, handle);
    ERROR_SUCCESS
}

#[win32_derive::dllexport]
pub fn RegOpenKeyExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpSubKey: Ptr<u8>,
    _ulOptions: u32,
    _samDesired: u32, /* REG_SAM_FLAGS */
    phkResult: Ptr<HKEY>,
) -> u32 /* WIN32_ERROR */ {
    let sub_key = read_opt_str(ctx, lpSubKey);
    open_key(ctx, hKey, &sub_key, false, phkResult)
}

#[win32_derive::dllexport]
pub fn RegCreateKeyExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpSubKey: Ptr<u8>,
    _Reserved: u32,
    _lpClass: u32,
    _dwOptions: u32,
    _samDesired: u32,
    _lpSecurityAttributes: u32,
    phkResult: Ptr<HKEY>,
    lpdwDisposition: Ptr<u32>,
) -> u32 /* WIN32_ERROR */ {
    const REG_CREATED_NEW_KEY: u32 = 1;
    const REG_OPENED_EXISTING_KEY: u32 = 2;
    let sub_key = read_opt_str(ctx, lpSubKey);
    let existed = {
        let lock = registry();
        let registry = lock.as_ref().unwrap();
        registry
            .resolve(hKey, &sub_key)
            .is_some_and(|path| registry.key_exists(&path))
    };
    let ret = open_key(ctx, hKey, &sub_key, true, phkResult);
    if ret == ERROR_SUCCESS && lpdwDisposition.addr != 0 {
        let disposition = if existed {
            REG_OPENED_EXISTING_KEY
        } else {
            REG_CREATED_NEW_KEY
        };
        lpdwDisposition.write(&mut ctx.memory, disposition);
    }
    ret
}

#[win32_derive::dllexport]
pub fn RegCreateKeyExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpSubKey: Ptr<u16>,
    _Reserved: u32,
    _lpClass: u32,
    _dwOptions: u32,
    _samDesired: u32,
    _lpSecurityAttributes: u32,
    phkResult: Ptr<HKEY>,
    _lpdwDisposition: u32,
) -> u32 /* WIN32_ERROR */ {
    let sub_key = if lpSubKey.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_wstr(lpSubKey.addr).to_string_lossy()
    };
    open_key(ctx, hKey, &sub_key, true, phkResult)
}

/// Copy a value out to the caller's buffer, following RegQueryValueEx's
/// protocol for sizes.
fn query_value(
    ctx: &mut Context,
    hkey: HKEY,
    name: &str,
    lpType: Ptr<u32>,
    lpData: u32,
    lpcbData: Ptr<u32>,
) -> u32 {
    let value = {
        let lock = registry();
        let registry = lock.as_ref().unwrap();
        let Some(path) = registry.handles.get(&hkey) else {
            log::warn!("RegQueryValueEx: bad key handle {hkey:#x}");
            return ERROR_FILE_NOT_FOUND;
        };
        match registry.get(path, name) {
            Some(value) => value.clone(),
            None => {
                log::warn!("RegQueryValueEx({path}\\{name}): not found");
                return ERROR_FILE_NOT_FOUND;
            }
        }
    };
    if lpType.addr != 0 {
        lpType.write(&mut ctx.memory, value.reg_type());
    }
    let bytes = value.to_bytes();
    if lpcbData.addr == 0 {
        return ERROR_SUCCESS;
    }
    let size = lpcbData.read(&ctx.memory).unwrap();
    lpcbData.write(&mut ctx.memory, bytes.len() as u32);
    if lpData == 0 {
        return ERROR_SUCCESS;
    }
    if (size as usize) < bytes.len() {
        return ERROR_MORE_DATA;
    }
    ctx.memory[lpData..][..bytes.len()].copy_from_slice(&bytes);
    ERROR_SUCCESS
}

#[win32_derive::dllexport]
pub fn RegQueryValueExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: Ptr<u8>,
    _lpReserved: u32,
    lpType: Ptr<u32>,
    lpData: u32,
    lpcbData: Ptr<u32>,
) -> u32 /* WIN32_ERROR */ {
    let name = read_opt_str(ctx, lpValueName);
    query_value(ctx, hKey, &name, lpType, lpData, lpcbData)
}

#[win32_derive::dllexport]
pub fn RegQueryValueExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: Ptr<u16>,
    _lpReserved: u32,
    lpType: Ptr<u32>,
    lpData: u32,
    lpcbData: Ptr<u32>,
) -> u32 /* WIN32_ERROR */ {
    let name = if lpValueName.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_wstr(lpValueName.addr).to_string_lossy()
    };
    // TODO: string values should come back as UTF-16 here.
    query_value(ctx, hKey, &name, lpType, lpData, lpcbData)
}

fn set_value(ctx: &mut Context, hkey: HKEY, name: &str, reg_type: u32, data: u32, size: u32) -> u32 {
    let bytes = ctx.memory[data..][..size as usize].to_vec();
    let value = match reg_type {
        REG_SZ | 2 /* REG_EXPAND_SZ */ => {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Value::Sz(String::from_utf8_lossy(&bytes[..end]).to_string())
        }
        REG_DWORD if bytes.len() == 4 => Value::Dword(u32::from_le_bytes(bytes.try_into().unwrap())),
        _ => Value::Binary(bytes),
    };
    let mut lock = registry();
    let registry = lock.as_mut().unwrap();
    let Some(path) = registry.handles.get(&hkey).cloned() else {
        log::warn!("RegSetValueEx: bad key handle {hkey:#x}");
        return ERROR_FILE_NOT_FOUND;
    };
    log::info!("RegSetValueEx({path}\\{name}) = {value:?}");
    registry.set(&path, name, value);
    ERROR_SUCCESS
}

#[win32_derive::dllexport]
pub fn RegSetValueExA(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: Ptr<u8>,
    _Reserved: u32,
    dwType: u32, /* REG_VALUE_TYPE */
    lpData: u32,
    cbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let name = read_opt_str(ctx, lpValueName);
    set_value(ctx, hKey, &name, dwType, lpData, cbData)
}

#[win32_derive::dllexport]
pub fn RegSetValueExW(
    ctx: &mut Context,
    hKey: HKEY,
    lpValueName: Ptr<u16>,
    _Reserved: u32,
    dwType: u32, /* REG_VALUE_TYPE */
    lpData: u32,
    cbData: u32,
) -> u32 /* WIN32_ERROR */ {
    let name = if lpValueName.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_wstr(lpValueName.addr).to_string_lossy()
    };
    // TODO: string data arrives as UTF-16 here.
    set_value(ctx, hKey, &name, dwType, lpData, cbData)
}

#[win32_derive::dllexport]
pub fn RegDeleteKeyA(ctx: &mut Context, hKey: HKEY, lpSubKey: Ptr<u8>) -> u32 /* WIN32_ERROR */ {
    let sub_key = read_opt_str(ctx, lpSubKey);
    let mut lock = registry();
    let registry = lock.as_mut().unwrap();
    match registry.resolve(hKey, &sub_key) {
        Some(path) => {
            registry.keys.remove(&normalize(&path));
            ERROR_SUCCESS
        }
        None => ERROR_FILE_NOT_FOUND,
    }
}

#[win32_derive::dllexport]
pub fn RegDeleteValueA(ctx: &mut Context, hKey: HKEY, lpValueName: Ptr<u8>) -> u32 /* WIN32_ERROR */ {
    let name = read_opt_str(ctx, lpValueName).to_ascii_lowercase();
    let mut lock = registry();
    let registry = lock.as_mut().unwrap();
    let Some(path) = registry.handles.get(&hKey).cloned() else {
        return ERROR_FILE_NOT_FOUND;
    };
    match registry.keys.get_mut(&normalize(&path)).and_then(|values| values.remove(&name)) {
        Some(_) => ERROR_SUCCESS,
        None => ERROR_FILE_NOT_FOUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_reg_file() {
        let mut registry = Registry::default();
        registry.load(
            r#"Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\SOFTWARE\Vendor\Game]
"Path"="C:\\Games\\"
"Count"=dword:0000000a
@="default"
"#,
        );
        assert!(registry.key_exists("HKEY_LOCAL_MACHINE\\Software\\vendor\\game"));
        assert!(registry.key_exists("HKEY_LOCAL_MACHINE\\SOFTWARE\\Vendor"));
        assert!(!registry.key_exists("HKEY_LOCAL_MACHINE\\SOFTWARE\\Other"));
        match registry.get("HKEY_LOCAL_MACHINE\\SOFTWARE\\Vendor\\Game", "path") {
            Some(Value::Sz(s)) => assert_eq!(s, "C:\\Games\\"),
            other => panic!("{other:?}"),
        }
        match registry.get("HKEY_LOCAL_MACHINE\\SOFTWARE\\Vendor\\Game", "Count") {
            Some(Value::Dword(10)) => {}
            other => panic!("{other:?}"),
        }
        match registry.get("HKEY_LOCAL_MACHINE\\SOFTWARE\\Vendor\\Game", "") {
            Some(Value::Sz(s)) => assert_eq!(s, "default"),
            other => panic!("{other:?}"),
        }
    }
}
