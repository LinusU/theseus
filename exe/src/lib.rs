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
}
