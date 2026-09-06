use runtime::Context;

use crate::{Ptr, kernel32::lock};

use super::state::LoadedModule;

pub type HMODULE = u32;

fn parse_pe(buf: &[u8]) -> Option<exe::PE> {
    // PE files may have a DOS stub; skip it when present.
    if buf.starts_with(b"MZ") {
        let dos = exe::DOS::parse(buf).ok()?;
        let offset = dos.header.e_lfanew as usize;
        return exe::PE::parse(&buf[offset..]).ok();
    }
    exe::PE::parse(buf).ok()
}

/// DLLs provides LoadLibrary and GetProcAddress implementations.
/// It's a trait so it can be hooked by unpackers that want to implement custom logic.
pub trait DLLs: Send {
    /// Register a statically imported module as loaded by the process loader.
    fn register_module(&mut self, dll: &str);

    /// Register a function as available through GetProcAddress. Called from
    /// generated init code, once per function the translator reserved an
    /// address for.
    fn register_export(&mut self, dll: &str, func: &str, addr: u32);

    fn load_library(&mut self, filename: &str) -> HMODULE;
    fn module_handle(&self, dll: &str) -> Option<HMODULE>;
    fn get_proc_address(&mut self, hmodule: HMODULE, proc_name: &str) -> u32;
}

/// Default implementation of DLLs.
/// Functions the translated program can look up by name at runtime.
///
/// A statically linked import is dispatched by the translator, but a program
/// that goes through LoadLibrary/GetProcAddress needs an actual address to
/// call. The translator picks one per function (the same synthetic addresses it
/// uses for imports, so they resolve through the block table) and registers it
/// here from the generated init code.
#[derive(Default)]
pub struct Exports {
    /// (dll, function) -> address, with dll lowercased and without ".dll".
    functions: Vec<(String, String, u32)>,
    /// Module handles handed out by LoadLibrary, in registration order.
    modules: Vec<String>,
}

/// Module handles are synthetic; the value only has to be non-null and
/// distinguishable.
const MODULE_HANDLE_BASE: HMODULE = 0xd11_0000;

fn normalize_module_name(name: &str) -> String {
    let name = name.rsplit(['\\', '/']).next().unwrap();
    // Lowercase first: stripping ".dll" before folding case misses "Foo.Dll".
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".dll").unwrap_or(&name).to_string()
}

impl DLLs for Exports {
    fn register_module(&mut self, dll: &str) {
        let dll = normalize_module_name(dll);
        if !self.modules.contains(&dll) {
            self.modules.push(dll);
        }
    }

    /// Register a function as available through GetProcAddress. Called from
    /// generated init code, once per function the translator reserved an
    /// address for.
    fn register_export(&mut self, dll: &str, func: &str, addr: u32) {
        let dll = normalize_module_name(dll);
        self.register_module(&dll);
        self.functions.push((dll, func.to_string(), addr));
    }

    fn load_library(&mut self, filename: &str) -> HMODULE {
        let Some(hmodule) = self.module_handle(filename) else {
            return 0;
        };
        hmodule
    }

    fn module_handle(&self, name: &str) -> Option<HMODULE> {
        let name = normalize_module_name(name);
        let index = self.modules.iter().position(|module| *module == name)?;
        Some(MODULE_HANDLE_BASE + index as u32)
    }

    fn get_proc_address(&mut self, hmodule: HMODULE, proc_name: &str) -> u32 {
        let Some(index) = hmodule.checked_sub(MODULE_HANDLE_BASE) else {
            return 0;
        };
        let Some(module) = self.modules.get(index as usize) else {
            return 0;
        };
        let Some((_, _, addr)) = self
            .functions
            .iter()
            .find(|(dll, name, _)| dll == module && name == proc_name)
        else {
            return 0;
        };

        *addr
    }
}

/// Register a statically imported module as loaded by the process loader.
pub fn register_module(dll: &str) {
    lock().dlls.register_module(dll);
}

/// Register a function as available through GetProcAddress; the entry point
/// the generated init code calls.
pub fn register_export(dll: &str, func: &str, addr: u32) {
    lock().dlls.register_export(dll, func, addr);
}

#[win32_derive::dllexport]
pub fn GetModuleFileNameA(
    ctx: &mut Context,
    _hModule: HMODULE,
    lpFilename: Ptr<u8>,
    nSize: u32,
) -> u32 {
    // The module's path isn't recorded, but its name is the first token of
    // the command line the process was launched with.
    let cmdline = ctx.memory.read_str(lock().command_line.command_line_8);
    let name = cmdline.split(' ').next().unwrap_or(cmdline).to_owned();
    if nSize == 0 || lpFilename.addr == 0 {
        return 0;
    }
    let copy = name.len().min(nSize as usize - 1);
    ctx.memory[lpFilename.addr..][..copy].copy_from_slice(&name.as_bytes()[..copy]);
    ctx.memory.write::<u8>(lpFilename.addr + copy as u32, 0);
    copy as u32
}

#[win32_derive::dllexport]
pub fn GetModuleHandleA(ctx: &mut Context, lpModuleName: Ptr<u8>) -> HMODULE {
    let kernel32 = lock();
    let Some(name) =
        (lpModuleName.addr != 0).then(|| ctx.memory.read_str(lpModuleName.addr).to_owned())
    else {
        // A null name asks for the running executable itself.
        return kernel32.image_base;
    };
    match kernel32.dlls.module_handle(&name) {
        Some(handle) => handle,
        None => {
            log::warn!("GetModuleHandleA({name}): not loaded");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn LoadLibraryA(ctx: &mut Context, lpLibFileName: Ptr<u8>) -> HMODULE {
    let filename = ctx.memory.read_str(lpLibFileName.addr).to_owned();
    if let Some(hmodule) = lock().dlls.module_handle(&filename) {
        return hmodule;
    }

    // Try to load a resource-only DLL from the current working directory.
    let path = std::env::current_dir().unwrap_or_default().join(&filename);
    let Ok(buf) = std::fs::read(&path) else {
        log::warn!("LoadLibrary({filename}): not supported, returning null");
        return 0;
    };
    let pe = match parse_pe(&buf) {
        Some(pe) => pe,
        None => {
            log::warn!("LoadLibrary({filename}): not a valid PE, returning null");
            return 0;
        }
    };
    let Some(rsrc) = pe.sections.iter().find(|s| s.name() == Ok(".rsrc")) else {
        log::warn!("LoadLibrary({filename}): no .rsrc section, returning null");
        return 0;
    };

    let rsrc_rva = rsrc.VirtualAddress;
    let rsrc_vsize = rsrc.VirtualSize;
    let rsrc_off = rsrc.PointerToRawData as usize;
    let rsrc_len = rsrc_vsize as usize;
    let rsrc_end = (rsrc_off + rsrc_len).min(buf.len());
    let rsrc_data = &buf[rsrc_off..rsrc_end];
    let copy_len_u32 = rsrc_data.len().min(rsrc_len) as u32;

    let memory_size = ctx.memory.bytes.len() as u32;
    let mut state = lock();
    let Some(image_base) = state.mappings.try_alloc(
        format!("{} .rsrc", filename),
        rsrc_rva + rsrc_vsize,
        memory_size,
    ) else {
        log::warn!("LoadLibrary({filename}): could not allocate .rsrc mapping");
        return 0;
    };
    let rsrc_addr = image_base + rsrc_rva;
    ctx.memory[rsrc_addr..rsrc_addr + copy_len_u32]
        .copy_from_slice(&rsrc_data[..copy_len_u32 as usize]);

    state.dlls.register_module(&filename);
    let hmodule = state.dlls.module_handle(&filename).unwrap();
    state.loaded_modules.insert(
        hmodule,
        LoadedModule {
            image_base,
            resources: rsrc_addr..rsrc_addr + rsrc_vsize,
        },
    );
    hmodule
}

#[win32_derive::dllexport]
pub fn FreeLibrary(_ctx: &mut Context, _hLibModule: HMODULE) -> bool {
    // Our modules are always resident.
    true
}

#[win32_derive::dllexport]
pub fn GetProcAddress(ctx: &mut Context, hModule: HMODULE, lpProcName: Ptr<u8>) -> u32 {
    // A name below 0x1000 is really an ordinal, per the API's convention.
    let name = if lpProcName.addr < 0x1000 {
        format!("ordinal{}", lpProcName.addr)
    } else {
        ctx.memory.read_str(lpProcName.addr).to_owned()
    };
    let addr = lock().dlls.get_proc_address(hModule, &name);
    if addr == 0 {
        log::warn!("GetProcAddress({hModule:#x}, {name}): not supported, returning null");
    }
    addr
}

#[cfg(test)]
mod tests {
    use super::{DLLs, Exports, MODULE_HANDLE_BASE};

    #[test]
    fn registered_static_modules_are_case_insensitive() {
        let mut exports = Exports::default();
        exports.register_module("KERNEL32.DLL");
        exports.register_module("user32");

        assert_eq!(exports.module_handle("kernel32"), Some(MODULE_HANDLE_BASE));
        assert_eq!(
            exports.module_handle("USER32.DLL"),
            Some(MODULE_HANDLE_BASE + 1)
        );
        assert_eq!(exports.module_handle("missing"), None);
    }
}
