mod dos;
mod exports;
mod file;
mod imports;
mod iter;
pub mod pe;
mod relocations;
mod resources;

use anyhow::anyhow;
pub use dos::DOS;
pub use exports::*;
pub use imports::*;
pub use pe::PE;
pub use relocations::*;
pub use resources::*;

/// Read a C-style nul terminated string from a buffer.
/// Various PE structures use these, sometimes with an optional nul.
pub(crate) fn c_str(buf: &[u8]) -> &[u8] {
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    &buf[..len]
}

pub enum Parse {
    PE(PE),
    DOS(DOS),
}

pub fn parse(buf: &[u8]) -> anyhow::Result<Parse> {
    let dos = DOS::parse(buf).map_err(|err| anyhow!("reading DOS header: {}", err))?;

    let pe_offset = dos.header.e_lfanew as usize;
    if pe_offset < buf.len() && pe::has_pe_signature(&buf[pe_offset..]) {
        let pe = PE::parse(&buf[pe_offset..])?;
        Ok(Parse::PE(pe))
    } else {
        Ok(Parse::DOS(dos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::IMAGE_SECTION_HEADER;

    #[test]
    fn kkrunchy_header() {
        let header = IMAGE_SECTION_HEADER {
            Name: *b"kkrunchy",
            ..Default::default()
        };
        assert_eq!(header.name().unwrap(), "kkrunchy");
    }

    use std::io::Write;

    #[test]
    fn dos_header() {
        let mut buf: Vec<u8> = Vec::new();
        buf.write_all(b"MZ").unwrap();
        buf.write_all(&[0; 0x3a]).unwrap();
        buf.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
        assert!(parse(&buf).is_ok()); // no crash
    }

    #[test]
    fn truncated_and_malformed_inputs_do_not_panic() {
        // A LoadLibrary'd file can be anything on disk; parsing must fail
        // rather than panic.
        assert!(DOS::parse(b"").is_err());
        assert!(DOS::parse(b"MZ").is_err());
        assert!(PE::parse(&[]).is_err());
        assert!(PE::parse(b"PE\0\0").is_err());

        // A DOS header pointing its PE header past the end of the file.
        let mut buf = b"MZ".to_vec();
        buf.resize(0x3c, 0);
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&buf).is_ok()); // no PE signature: treated as DOS image
        // One byte short of a full DOS header.
        assert!(parse(&buf[..0x3d]).is_err());
    }

    #[test]
    fn malformed_resource_section_does_not_panic() {
        use crate::ResourceName;
        let query = || (ResourceName::Id(2), ResourceName::Id(1));

        // Empty and truncated sections have no entries.
        assert!(find_resource(&[], query().0, query().1).is_none());
        assert!(find_resource(&[0; 8], query().0, query().1).is_none());

        // A directory claiming more entries than the buffer holds.
        let mut section = vec![0u8; 16];
        section[12..14].copy_from_slice(&0xffffu16.to_le_bytes()); // named
        section[14..16].copy_from_slice(&0xffffu16.to_le_bytes()); // id
        assert!(find_resource(&section, query().0, query().1).is_none());

        // A named entry pointing outside the section.
        let mut section = vec![0u8; 16 + 8];
        section[14..16].copy_from_slice(&1u16.to_le_bytes()); // one id entry
        section[16..20].copy_from_slice(&0x8000_fff0u32.to_le_bytes()); // name
        section[20..24].copy_from_slice(&0x8000_fff0u32.to_le_bytes()); // dir ptr
        assert!(find_resource(&section, query().0, query().1).is_none());
    }

    #[test]
    fn malformed_import_descriptors_do_not_panic() {
        use crate::imports::ILTEntry;
        use zerocopy::FromBytes;

        // Build an import directory with one descriptor whose fields all point
        // outside the (tiny) image buffer.
        let mut import_dir = vec![0u8; 20];
        import_dir[0..4].copy_from_slice(&200u32.to_le_bytes()); // OriginalFirstThunk
        import_dir[12..16].copy_from_slice(&100u32.to_le_bytes()); // Name
        import_dir[16..20].copy_from_slice(&50u32.to_le_bytes()); // FirstThunk

        let image = vec![0u8; 64];
        for imp in crate::read_imports(&import_dir) {
            // These used to slice the image buffer directly and panic.
            assert!(imp.image_name(&image).is_empty());
            assert_eq!(imp.ilt(&image).count(), 0);

            let ilt = <ILTEntry>::read_from_prefix(&300u32.to_le_bytes())
                .unwrap()
                .0;
            assert!(matches!(
                ilt.as_import_symbol(&image),
                crate::ImportSymbol::Name(&[])
            ));
        }
    }

    #[test]
    fn data_directory_slice_rejects_overflow_and_out_of_range() {
        use crate::file::IMAGE_DATA_DIRECTORY;

        // A zero-size directory with an out-of-range start must not produce a slice.
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: 100,
            Size: 0,
        };
        assert!(dir.as_slice(&[0; 64]).is_none());

        // VirtualAddress + Size must not wrap around and create a tiny "valid" slice.
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: 1,
            Size: u32::MAX,
        };
        assert!(dir.as_slice(&[0; 64]).is_none());

        // A plain out-of-range range is rejected.
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: 0x10,
            Size: 0x100,
        };
        assert!(dir.as_slice(&[0; 32]).is_none());

        // A fully in-range directory returns its slice.
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: 0,
            Size: 32,
        };
        assert_eq!(dir.as_slice(&[0; 64]).unwrap().len(), 32);
    }

    #[test]
    fn export_directory_rejects_truncated_and_bad_pointers() {
        use crate::exports::read_exports;

        // A section too short for the export directory header returns None.
        assert!(read_exports(&[0u8; 20]).is_none());

        // A 64-byte image with an out-of-range name pointer and one name table
        // entry whose string pointer is also out of range.
        let mut image = vec![0u8; 64];
        image[0..4].copy_from_slice(&100u32.to_le_bytes()); // AddressOfNames[0]
        image[8..10].copy_from_slice(&0u16.to_le_bytes()); // AddressOfNameOrdinals[0]

        let mut section = vec![0u8; 40];
        section[12..16].copy_from_slice(&100u32.to_le_bytes()); // Name RVA
        section[24..28].copy_from_slice(&1u32.to_le_bytes()); // NumberOfNames
        section[28..32].copy_from_slice(&0u32.to_le_bytes()); // AddressOfNames
        section[36..40].copy_from_slice(&8u32.to_le_bytes()); // AddressOfNameOrdinals

        let Some(dir) = read_exports(&section) else {
            panic!("read_exports should parse a 40-byte section");
        };
        assert!(dir.name(&image).is_empty());
        assert_eq!(dir.fns(&image).count(), 0);
        assert_eq!(dir.names(&image).count(), 1);
    }

    #[test]
    fn apply_relocs_tolerates_malformed_blocks_and_unknown_types() {
        use crate::relocations::apply_relocs;

        // A block whose SizeOfBlock claims more bytes than the buffer holds.
        let mut relocs = vec![0u8; 8];
        relocs[0..4].copy_from_slice(&0x1000u32.to_le_bytes());
        relocs[4..8].copy_from_slice(&0xFFFFu32.to_le_bytes());

        let mut writes = vec![];
        apply_relocs(
            0,
            0x10000,
            &relocs,
            |_addr| 0,
            |addr, val| writes.push((addr, val)),
        );
        assert!(writes.is_empty());

        // A block with an unknown relocation type and a valid type-3 entry.
        let mut relocs = vec![0u8; 8 + 4];
        relocs[0..4].copy_from_slice(&0x1000u32.to_le_bytes()); // VirtualAddress
        relocs[4..8].copy_from_slice(&12u32.to_le_bytes()); // SizeOfBlock (header + 2 entries)
        relocs[8..10].copy_from_slice(&0xF002u16.to_le_bytes()); // unknown type, offset 2
        relocs[10..12].copy_from_slice(&0x3003u16.to_le_bytes()); // type 3, offset 3

        use std::cell::RefCell;

        let image = RefCell::new([0u8; 8]);
        image.borrow_mut()[3..7].copy_from_slice(&0x1234u32.to_le_bytes());
        apply_relocs(
            0x1000,
            0x2000,
            &relocs,
            |addr| {
                let img = image.borrow();
                let start = (addr - 0x1000) as usize;
                u32::from_le_bytes(img[start..start + 4].try_into().unwrap_or([0; 4]))
            },
            |addr, val| {
                let start = (addr - 0x1000) as usize;
                image.borrow_mut()[start..start + 4].copy_from_slice(&val.to_le_bytes());
            },
        );

        // The unknown type is ignored; the type-3 entry adds the 0x1000 base offset.
        assert_eq!(
            u32::from_le_bytes(image.borrow()[3..7].try_into().unwrap()),
            0x2234
        );
    }

    #[test]
    fn section_align_without_flag_returns_default() {
        use crate::file::{IMAGE_SCN, IMAGE_SECTION_HEADER};

        // No IMAGE_SCN_ALIGN_* bits means the default alignment; align() must
        // not underflow on `1 << (value - 1)`.
        let header = IMAGE_SECTION_HEADER::default();
        assert_eq!(header.characteristics().unwrap().align(), 1);

        // The encoded field counts from IMAGE_SCN_ALIGN_1BYTES = 1.
        for (flag, want) in [
            (0x0010_0000, 1),
            (0x0020_0000, 2),
            (0x0030_0000, 4),
            (0x0040_0000, 8),
        ] {
            let header = IMAGE_SECTION_HEADER {
                Characteristics: flag,
                ..Default::default()
            };
            let scn: IMAGE_SCN = header.characteristics().unwrap();
            assert_eq!(scn.align(), want, "flag {flag:#x}");
        }
    }
}
