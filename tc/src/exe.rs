//! EXE loading.

use anyhow::{Context, Result};
use runtime::segofs;

use crate::{DOSModule, Import, Module, WindowsModule, memory::Memory};

pub fn load_exe(mem: &mut Memory, buf: Vec<u8>) -> Result<Module> {
    match exe::parse(&buf).context("parsing executable")? {
        exe::Parse::PE(pe) => Ok(Module::Windows(load_pe(mem, &buf, pe)?)),
        exe::Parse::DOS(dos) => Ok(Module::DOS(load_dos(mem, &buf, dos)?)),
    }
}

fn load_dos(mem: &mut Memory, buf: &[u8], dos: exe::DOS) -> Result<DOSModule> {
    let psp_segment = dos::DOSBOX_SEG;

    mem.reserve("psp".into(), segofs(psp_segment, 0), 0x100);

    let load_segment = psp_segment + 0x10;
    let load_addr = segofs(load_segment, 0);
    let data = buf.get(dos.image_offset()..).unwrap_or(&[]);
    mem.reserve("dos data".into(), load_addr, data.len() as u32);
    if !data.is_empty() {
        let data_len = data.len() as u32;
        let data_slice = mem.slice_mut(load_addr, data_len);
        data_slice.copy_from_slice(data);
        dos.apply_relocations(load_segment, data_slice);
    }

    Ok(DOSModule {
        is_com: false,
        psp_segment,
        load_segment: psp_segment + 0x10 + dos.header.e_cs,
        stack_segment: load_segment + dos.header.e_ss,
        stack_pointer: dos.header.e_sp,
        entry_point: dos.header.e_ip,
        code_memory: (load_addr..load_addr + data.len() as u32),
    })
}

fn load_pe(mem: &mut Memory, buf: &[u8], f: exe::PE) -> Result<WindowsModule> {
    mem.mappings
        .try_alloc("null page".into(), 0x1000, Memory::LIMIT)
        .expect("null page mapping could not be allocated");

    let image_base = f.opt_header.ImageBase;
    mem.try_reserve("exe header".into(), image_base, 0x1000)
        .ok_or_else(|| anyhow::anyhow!("could not reserve exe header at {image_base:#x}"))?;
    mem.write_bytes(image_base, &buf[..0x1000.min(buf.len())]);
    let mut code_range = None;
    for sec in &f.sections {
        // A repacked image can carry section RVAs that overflow the 32-bit
        // address space; skipping the section beats mapping it at a wrapped
        // low address.
        let Some(addr) = image_base.checked_add(sec.VirtualAddress) else {
            log::warn!("skipping out-of-range section {:?}", sec.name());
            continue;
        };
        let size = runtime::round_to_page(sec.SizeOfRawData.max(sec.VirtualSize));
        let Some(addr) = mem.try_reserve(sec.name().unwrap_or("<invalid>").to_string(), addr, size)
        else {
            log::warn!("skipping overlapping section {:?}", sec.name());
            continue;
        };

        use exe::pe::IMAGE_SCN;
        let flags = IMAGE_SCN::from_bits_truncate(sec.Characteristics);
        let load_data =
            flags.contains(IMAGE_SCN::CODE) || flags.contains(IMAGE_SCN::INITIALIZED_DATA);
        if load_data {
            let start = sec.PointerToRawData as usize;
            let end = start.saturating_add(sec.SizeOfRawData as usize);
            let data = buf.get(start..end.min(buf.len())).unwrap_or(&[]);
            mem.write_bytes(addr, data);
        }
        if flags.contains(IMAGE_SCN::CODE) || flags.contains(IMAGE_SCN::MEM_EXECUTE) {
            // A section can legitimately run to the top of the address
            // space; clamp the end rather than wrapping it.
            let end = addr.saturating_add(sec.SizeOfRawData);
            match &mut code_range {
                None => code_range = Some(addr..end),
                Some(range) => {
                    range.start = range.start.min(addr);
                    range.end = range.end.max(end);
                }
            }
        }
    }

    let resources = f
        .get_data_directory(exe::pe::IMAGE_DIRECTORY_ENTRY::RESOURCE)
        .and_then(|dir| {
            let addr = image_base.checked_add(dir.VirtualAddress)?;
            Some(addr..addr.saturating_add(dir.Size))
        });

    let imports = read_imports(&f, mem);

    Ok(WindowsModule {
        imports,
        image_base,
        entry_point: image_base
            .checked_add(f.opt_header.AddressOfEntryPoint)
            .ok_or_else(|| anyhow::anyhow!("entry point RVA out of range"))?,
        code_memory: code_range.unwrap_or(0..0),
        resources,
        vtables: Default::default(),
        dynamic_exports: Default::default(),
    })
}

fn is_data(dll: &str, func: &str) -> bool {
    if dll == "msvcrt" {
        return matches!(func, "_adjust_fdiv" | "_acmdln");
    }
    false
}

/// Read the file's imported symbols.
fn read_imports(pe_file: &exe::PE, mem: &Memory) -> Vec<Import> {
    let mut imports = vec![];
    let Some(dir) = pe_file.get_data_directory(exe::pe::IMAGE_DIRECTORY_ENTRY::IMPORT) else {
        return imports;
    };
    let image_base = pe_file.opt_header.ImageBase;
    let image = mem.slice_all(image_base);
    let Some(dir) = dir.as_slice(image) else {
        return imports;
    };
    for imp in exe::read_imports(dir) {
        let name = std::str::from_utf8(imp.image_name(image))
            .unwrap_or("<invalid>")
            .to_lowercase();
        let name = name.trim_end_matches(".dll");
        for (addr, entry) in imp.iat_iter(image) {
            let func = match entry.as_import_symbol(image) {
                exe::ImportSymbol::Name(name) => {
                    std::str::from_utf8(name).unwrap_or("<invalid>").to_string()
                }
                exe::ImportSymbol::Ordinal(n) => format!("ordinal{n}"),
            };
            let data = is_data(name, &func);
            let Some(iat_addr) = image_base.checked_add(addr) else {
                log::warn!("skipping out-of-range IAT entry {addr:#x} for {name}::{func}");
                continue;
            };
            imports.push(Import {
                dll: name.to_string(),
                func,
                iat_addr,
                addr: 0,
                data,
            });
        }
    }
    imports
}

#[cfg(test)]
mod tests {
    use super::*;
    use exe::pe::{
        IMAGE_DATA_DIRECTORY, IMAGE_DIRECTORY_ENTRY, IMAGE_FILE_HEADER, IMAGE_OPTIONAL_HEADER32,
        IMAGE_SCN, IMAGE_SECTION_HEADER,
    };
    use zerocopy::FromBytes;

    #[test]
    fn read_imports_tolerates_out_of_range_directory() {
        let mut mem = Memory::default();
        mem.reserve("image".into(), 0x400000, 0x1000);

        let mut data_directory: Box<[IMAGE_DATA_DIRECTORY]> =
            (0..16).map(|_| IMAGE_DATA_DIRECTORY::default()).collect();
        data_directory[IMAGE_DIRECTORY_ENTRY::IMPORT as usize] = IMAGE_DATA_DIRECTORY {
            VirtualAddress: 0x10000,
            Size: 0x100,
        };

        let header = <exe::pe::IMAGE_FILE_HEADER>::read_from_prefix(&[0u8; 20])
            .unwrap()
            .0;
        let mut opt_header = <exe::pe::IMAGE_OPTIONAL_HEADER32>::read_from_prefix(&[0u8; 224])
            .unwrap()
            .0;
        opt_header.ImageBase = 0x400000;

        let pe = exe::PE {
            header,
            opt_header,
            data_directory,
            sections: vec![].into_boxed_slice(),
        };

        assert!(read_imports(&pe, &mem).is_empty());
    }

    #[test]
    fn load_pe_tolerates_truncated_section_data() {
        let image_base = 0x400000;
        let mut mem = Memory::default();
        // A 50-byte "file" whose single section claims 0x200 bytes at offset 100.
        let buf = vec![0u8; 50];

        let header = <IMAGE_FILE_HEADER>::read_from_prefix(&[0u8; 20]).unwrap().0;
        let mut opt_header = <IMAGE_OPTIONAL_HEADER32>::read_from_prefix(&[0u8; 96])
            .unwrap()
            .0;
        opt_header.ImageBase = image_base;

        let mut name = [0u8; 8];
        name[..4].copy_from_slice(b"test");
        let section = IMAGE_SECTION_HEADER {
            Name: name,
            VirtualAddress: 0x1000,
            VirtualSize: 0x100,
            SizeOfRawData: 0x200,
            PointerToRawData: 100,
            Characteristics: (IMAGE_SCN::CODE | IMAGE_SCN::INITIALIZED_DATA).bits(),
            ..Default::default()
        };

        let pe = exe::PE {
            header,
            opt_header,
            data_directory: (0..16).map(|_| IMAGE_DATA_DIRECTORY::default()).collect(),
            sections: vec![section].into_boxed_slice(),
        };

        let module = load_pe(&mut mem, &buf, pe).unwrap();
        assert_eq!(module.image_base, image_base);
    }
}
