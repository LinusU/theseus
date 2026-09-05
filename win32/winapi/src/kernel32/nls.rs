use runtime::Context;

use crate::{Ptr, dllexport::win32flags};

#[win32_derive::dllexport]
pub fn GetACP(_ctx: &mut Context) -> u32 {
    1252 // windows-1252
}

#[win32_derive::dllexport]
pub fn GetOEMCP(_ctx: &mut Context) -> u32 {
    437
}

#[win32_derive::dllexport]
pub fn GetSystemDefaultLangID(_ctx: &mut Context) -> u16 {
    0x0409
}

#[win32_derive::dllexport]
pub fn IsDBCSLeadByte(_ctx: &mut Context, _TestChar: u32) -> bool {
    false
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct CPINFO {
    pub MaxCharSize: u32,
    pub DefaultChar: [u8; 2],
    pub LeadByte: [u8; 12],
    /// Windows' CPINFO is 20 bytes: the three fields above come to 18, and the
    /// u32 alignment rounds it up. Spelled out because zerocopy won't derive
    /// IntoBytes for a struct that has padding it cannot see.
    pub _pad: [u8; 2],
}

#[win32_derive::dllexport]
pub fn GetCPInfo(ctx: &mut Context, CodePage: u32, lpCPInfo: Ptr<CPINFO>) -> bool {
    if !matches!(CodePage, 0 | 1 | 437 | 1252) {
        log::warn!("GetCPInfo: unsupported code page {CodePage}");
        return false;
    }
    // A single-byte codepage, so no lead byte ranges.
    let info = CPINFO {
        MaxCharSize: 1,
        DefaultChar: [b'?', 0],
        LeadByte: [0; 12],
        _pad: [0; 2],
    };
    ctx.memory.write(lpCPInfo.addr, info);
    true
}

// CT_CTYPE1 character classification bits.
win32flags! {
    pub struct C1 {
        const UPPER  = 0x001;
        const LOWER  = 0x002;
        const DIGIT  = 0x004;
        const SPACE  = 0x008;
        const PUNCT  = 0x010;
        const CNTRL  = 0x020;
        const BLANK  = 0x040;
        const XDIGIT = 0x080;
        const ALPHA  = 0x100;
    }
}

/// CT_CTYPE1 character classification of an ASCII-ish character.
fn ctype1(c: u32) -> C1 {
    if c > 0xff {
        return C1::ALPHA; // close enough
    }
    let c = c as u8;
    let mut t = C1::empty();
    t.set(C1::UPPER, c.is_ascii_uppercase());
    t.set(C1::LOWER, c.is_ascii_lowercase());
    t.set(C1::DIGIT, c.is_ascii_digit());
    t.set(C1::SPACE, c == b' ' || (0x9..=0xd).contains(&c));
    t.set(C1::PUNCT, c.is_ascii_punctuation());
    t.set(C1::CNTRL, c < 0x20 || c == 0x7f);
    t.set(C1::BLANK, c == b' ' || c == 0x9);
    t.set(C1::XDIGIT, c.is_ascii_hexdigit());
    t.set(C1::ALPHA, c.is_ascii_alphabetic() || c >= 0x80);
    t
}

fn read_string_type_a(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u8>> {
    if count < -1 {
        return None;
    }
    let bytes = &ctx.memory[addr..];
    let len = if count == -1 {
        bytes.iter().position(|&byte| byte == 0)? + 1
    } else {
        count as usize
    };
    Some(bytes.get(..len)?.to_vec())
}

fn read_string_type_w(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u16>> {
    if count < -1 {
        return None;
    }
    let bytes = &ctx.memory[addr..];
    if count == -1 {
        let mut out = Vec::new();
        for chunk in bytes.chunks_exact(2) {
            let value = u16::from_le_bytes([chunk[0], chunk[1]]);
            out.push(value);
            if value == 0 {
                return Some(out);
            }
        }
        return None;
    }
    let byte_len = count as usize * 2;
    let bytes = bytes.get(..byte_len)?;
    Some(
        bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect(),
    )
}

fn char_type_output_fits(ctx: &Context, addr: u32, count: usize) -> bool {
    (addr as usize)
        .checked_add(count.checked_mul(2).unwrap_or(usize::MAX))
        .is_some_and(|end| end <= ctx.memory.bytes.len())
}

#[win32_derive::dllexport]
pub fn GetStringTypeA(
    ctx: &mut Context,
    _Locale: u32,
    dwInfoType: u32,
    lpSrcStr: Ptr<u8>,
    cchSrc: i32,
    lpCharType: Ptr<u16>,
) -> bool {
    if dwInfoType != 1 {
        log::warn!("GetStringTypeA: unimplemented type {dwInfoType}");
        return false;
    }
    let Some(src) = read_string_type_a(ctx, lpSrcStr.addr, cchSrc) else {
        return false;
    };
    if !char_type_output_fits(ctx, lpCharType.addr, src.len()) {
        return false;
    }
    for (i, c) in src.into_iter().enumerate() {
        ctx.memory.write::<u16>(
            lpCharType.addr + (i * 2) as u32,
            ctype1(c as u32).bits() as u16,
        );
    }
    true
}

#[win32_derive::dllexport]
pub fn GetStringTypeExA(
    ctx: &mut Context,
    Locale: u32,
    dwInfoType: u32,
    lpSrcStr: Ptr<u8>,
    cchSrc: i32,
    lpCharType: Ptr<u16>,
) -> bool {
    GetStringTypeA(ctx, Locale, dwInfoType, lpSrcStr, cchSrc, lpCharType)
}

#[win32_derive::dllexport]
pub fn GetStringTypeW(
    ctx: &mut Context,
    dwInfoType: u32,
    lpSrcStr: Ptr<u16>,
    cchSrc: i32,
    lpCharType: Ptr<u16>,
) -> bool {
    if dwInfoType != 1 {
        log::warn!("GetStringTypeW: unimplemented type {dwInfoType}");
        return false;
    }
    let Some(src) = read_string_type_w(ctx, lpSrcStr.addr, cchSrc) else {
        return false;
    };
    if !char_type_output_fits(ctx, lpCharType.addr, src.len()) {
        return false;
    }
    for (i, c) in src.into_iter().enumerate() {
        ctx.memory.write::<u16>(
            lpCharType.addr + (i * 2) as u32,
            ctype1(c as u32).bits() as u16,
        );
    }
    true
}

/// ASCII-only character mapping for LCMapString*.
fn lcmap_char(c: u32, flags: u32) -> u32 {
    const LCMAP_LOWERCASE: u32 = 0x100;
    const LCMAP_UPPERCASE: u32 = 0x200;
    if c < 0x80 {
        if flags & LCMAP_LOWERCASE != 0 {
            return (c as u8).to_ascii_lowercase() as u32;
        }
        if flags & LCMAP_UPPERCASE != 0 {
            return (c as u8).to_ascii_uppercase() as u32;
        }
    }
    c
}

#[win32_derive::dllexport]
pub fn LCMapStringA(
    ctx: &mut Context,
    _Locale: u32,
    dwMapFlags: u32,
    lpSrcStr: Ptr<u8>,
    cchSrc: i32,
    lpDestStr: Ptr<u8>,
    cchDest: i32,
) -> i32 {
    if cchSrc < -1 || cchDest < 0 {
        return 0;
    }
    let Some(src) = read_string_type_a(ctx, lpSrcStr.addr, cchSrc) else {
        return 0;
    };
    let len = src.len() as u32;
    if cchDest == 0 {
        return len as i32;
    }
    if (cchDest as u32) < len {
        return 0;
    }
    for (i, c) in src.into_iter().enumerate() {
        ctx.memory.write::<u8>(
            lpDestStr.addr + i as u32,
            lcmap_char(c as u32, dwMapFlags) as u8,
        );
    }
    len as i32
}

#[win32_derive::dllexport]
pub fn LCMapStringW(
    ctx: &mut Context,
    _Locale: u32,
    dwMapFlags: u32,
    lpSrcStr: Ptr<u16>,
    cchSrc: i32,
    lpDestStr: Ptr<u16>,
    cchDest: i32,
) -> i32 {
    if cchSrc < -1 || cchDest < 0 {
        return 0;
    }
    let Some(src) = read_string_type_w(ctx, lpSrcStr.addr, cchSrc) else {
        return 0;
    };
    let len = src.len() as u32;
    if cchDest == 0 {
        return len as i32;
    }
    if (cchDest as u32) < len {
        return 0;
    }
    for (i, c) in src.into_iter().enumerate() {
        ctx.memory.write::<u16>(
            lpDestStr.addr + (i * 2) as u32,
            lcmap_char(c as u32, dwMapFlags) as u16,
        );
    }
    len as i32
}

fn ansi_to_wide(byte: u8) -> u16 {
    match byte {
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x8e => 0x017d,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9e => 0x017e,
        0x9f => 0x0178,
        byte => byte as u16,
    }
}

fn read_multibyte(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u8>> {
    let len = match count {
        -1 => ctx.memory[addr..]
            .iter()
            .position(|&byte| byte == 0)
            .map(|len| len + 1)?,
        count if count > 0 => count as usize,
        _ => return None,
    };
    Some(ctx.memory[addr..].get(..len)?.to_vec())
}

#[win32_derive::dllexport]
pub fn MultiByteToWideChar(
    ctx: &mut Context,
    CodePage: u32,
    _dwFlags: u32, /* MULTI_BYTE_TO_WIDE_CHAR_FLAGS */
    lpMultiByteStr: Ptr<u8>,
    cbMultiByte: i32,
    lpWideCharStr: Ptr<u16>,
    cchWideChar: i32,
) -> i32 {
    if !matches!(CodePage, 0 | 1252) {
        log::warn!("MultiByteToWideChar: unsupported code page {CodePage}");
        return 0;
    }
    let Some(src) = read_multibyte(ctx, lpMultiByteStr.addr, cbMultiByte) else {
        return 0;
    };
    let wide: Vec<u16> = src.into_iter().map(ansi_to_wide).collect();
    if cchWideChar == 0 {
        return wide.len() as i32;
    }
    if cchWideChar < 0 || wide.len() > cchWideChar as usize {
        return 0;
    }
    for (i, value) in wide.iter().copied().enumerate() {
        ctx.memory
            .write::<u16>(lpWideCharStr.addr + (i * 2) as u32, value);
    }
    wide.len() as i32
}

fn read_wide(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u16>> {
    if count == 0 || count < -1 {
        return None;
    }
    read_string_type_w(ctx, addr, count)
}

fn wide_to_ansi(wide: u16, default: u8) -> (u8, bool) {
    match (0..=u8::MAX).find(|&byte| ansi_to_wide(byte) == wide) {
        Some(byte) => (byte, false),
        None => (default, true),
    }
}

#[win32_derive::dllexport]
pub fn WideCharToMultiByte(
    ctx: &mut Context,
    CodePage: u32,
    _dwFlags: u32,
    lpWideCharStr: Ptr<u16>,
    cchWideChar: i32,
    lpMultiByteStr: Ptr<u8>,
    cbMultiByte: i32,
    lpDefaultChar: Ptr<u8>,
    lpUsedDefaultChar: Ptr<bool>,
) -> i32 {
    if !matches!(CodePage, 0 | 1252) {
        log::warn!("WideCharToMultiByte: unsupported code page {CodePage}");
        return 0;
    }
    let Some(src) = read_wide(ctx, lpWideCharStr.addr, cchWideChar) else {
        return 0;
    };
    let default = if lpDefaultChar.addr == 0 {
        b'?'
    } else {
        ctx.memory.read::<u8>(lpDefaultChar.addr)
    };
    let converted: Vec<(u8, bool)> = src
        .into_iter()
        .map(|wide| wide_to_ansi(wide, default))
        .collect();
    let used_default = converted.iter().any(|&(_, used)| used);
    if lpUsedDefaultChar.addr != 0 {
        ctx.memory
            .write::<u8>(lpUsedDefaultChar.addr, used_default as u8);
    }
    if cbMultiByte == 0 {
        return converted.len() as i32;
    }
    if cbMultiByte < 0 || converted.len() > cbMultiByte as usize {
        return 0;
    }
    for (i, (value, _)) in converted.iter().copied().enumerate() {
        ctx.memory
            .write::<u8>(lpMultiByteStr.addr + i as u32, value);
    }
    converted.len() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Context, Memory};

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
    fn lcmap_rejects_invalid_source_and_destination_counts() {
        let mut ctx = context();
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");

        assert_eq!(
            LCMapStringA(
                &mut ctx,
                0,
                0x100,
                Ptr::new(0x1000),
                -2,
                Ptr::new(0x1200),
                2,
            ),
            0
        );
        assert_eq!(
            LCMapStringW(
                &mut ctx,
                0,
                0x100,
                Ptr::new(0x1000),
                1,
                Ptr::new(0x1200),
                -1,
            ),
            0
        );
    }

    #[test]
    fn lcmap_ansi_accepts_cp1252_bytes() {
        let mut ctx = context();
        ctx.memory[0x1000..][..3].copy_from_slice(&[b'A', 0x80, 0]);

        assert_eq!(
            LCMapStringA(
                &mut ctx,
                0,
                0x200,
                Ptr::new(0x1000),
                -1,
                Ptr::new(0x1200),
                3,
            ),
            3
        );
        assert_eq!(&ctx.memory[0x1200..][..3], &[b'A', 0x80, 0]);
    }

    #[test]
    fn lcmap_w_rejects_unterminated_input() {
        let mut ctx = context();
        ctx.memory[0x3ffe..].copy_from_slice(&[b'A', 0]);
        ctx.memory.write::<u16>(0x1200, 0xffff);

        assert_eq!(
            LCMapStringW(
                &mut ctx,
                0,
                0x200,
                Ptr::new(0x3ffe),
                -1,
                Ptr::new(0x1200),
                1,
            ),
            0
        );
        assert_eq!(ctx.memory.read::<u16>(0x1200), 0xffff);
    }

    #[test]
    fn lcmap_rejects_truncated_counted_input() {
        let mut ctx = context();
        ctx.memory.write::<u8>(0x3fff, b'A');
        ctx.memory.write::<u16>(0x1200, 0xffff);

        assert_eq!(
            LCMapStringA(&mut ctx, 0, 0x200, Ptr::new(0x3fff), 2, Ptr::new(0x1200), 2,),
            0
        );
        assert_eq!(
            LCMapStringW(&mut ctx, 0, 0x200, Ptr::new(0x3fff), 1, Ptr::new(0x1200), 1,),
            0
        );
        assert_eq!(ctx.memory.read::<u16>(0x1200), 0xffff);
    }

    #[test]
    fn string_type_rejects_truncated_output() {
        let mut ctx = context();
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");

        assert!(!GetStringTypeA(
            &mut ctx,
            0,
            1,
            Ptr::new(0x1000),
            2,
            Ptr::new(0x3fff),
        ));
        assert!(!GetStringTypeW(
            &mut ctx,
            1,
            Ptr::new(0x1000),
            1,
            Ptr::new(0x3fff),
        ));
    }

    #[test]
    fn string_type_rejects_invalid_negative_counts() {
        let mut ctx = context();
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");
        ctx.memory.write::<u16>(0x1200, 0xffff);

        assert!(!GetStringTypeA(
            &mut ctx,
            0,
            1,
            Ptr::new(0x1000),
            -2,
            Ptr::new(0x1200),
        ));
        assert!(!GetStringTypeW(
            &mut ctx,
            1,
            Ptr::new(0x1000),
            -2,
            Ptr::new(0x1200),
        ));
        assert_eq!(ctx.memory.read::<u16>(0x1200), 0xffff);
    }

    #[test]
    fn string_type_reports_unterminated_or_truncated_input() {
        let mut ctx = context();
        ctx.memory[0x3ff0..].fill(0xff);
        ctx.memory.write::<u16>(0x1200, 0xffff);

        assert!(!GetStringTypeA(
            &mut ctx,
            0,
            1,
            Ptr::new(0x3ff0),
            -1,
            Ptr::new(0x1200),
        ));
        assert!(!GetStringTypeW(
            &mut ctx,
            1,
            Ptr::new(0x3ff0),
            -1,
            Ptr::new(0x1200),
        ));
        assert!(!GetStringTypeA(
            &mut ctx,
            0,
            1,
            Ptr::new(0x3fff),
            2,
            Ptr::new(0x1200),
        ));
        assert_eq!(ctx.memory.read::<u16>(0x1200), 0xffff);
    }

    #[test]
    fn get_cp_info_rejects_unknown_code_pages() {
        let mut ctx = context();

        assert!(GetCPInfo(&mut ctx, 1252, Ptr::new(0x1100)));
        assert_eq!(ctx.memory.read::<u32>(0x1100), 1);
        assert_eq!(ctx.memory.read::<u8>(0x1104), b'?');
        assert!(!GetCPInfo(&mut ctx, 65001, Ptr::new(0x1100)));
    }

    #[test]
    fn multibyte_to_wide_char_decodes_cp1252_and_includes_nul() {
        let mut ctx = context();
        ctx.memory[0x1000..][..3].copy_from_slice(&[b'A', 0x80, 0]);

        assert_eq!(
            MultiByteToWideChar(&mut ctx, 1252, 0, Ptr::new(0x1000), -1, Ptr::new(0x1100), 4,),
            3
        );
        assert_eq!(ctx.memory.read::<u16>(0x1100), b'A' as u16);
        assert_eq!(ctx.memory.read::<u16>(0x1102), 0x20ac);
        assert_eq!(ctx.memory.read::<u16>(0x1104), 0);
    }

    #[test]
    fn conversion_helpers_reject_truncated_sources() {
        let mut ctx = context();
        ctx.memory.write::<u8>(0x3fff, b'A');
        ctx.memory.write::<u16>(0x1100, 0xffff);
        ctx.memory.write::<u8>(0x1200, 0xff);
        ctx.memory.write::<u16>(0x3ffe, b'A' as u16);

        assert_eq!(
            MultiByteToWideChar(&mut ctx, 1252, 0, Ptr::new(0x3fff), 2, Ptr::new(0x1100), 1,),
            0
        );
        assert_eq!(
            WideCharToMultiByte(
                &mut ctx,
                1252,
                0,
                Ptr::new(0x3ffe),
                -1,
                Ptr::new(0x1200),
                1,
                Ptr::new(0),
                Ptr::new(0),
            ),
            0
        );
        assert_eq!(ctx.memory.read::<u16>(0x1100), 0xffff);
        assert_eq!(ctx.memory.read::<u8>(0x1200), 0xff);
    }

    #[test]
    fn wide_char_to_multibyte_encodes_cp1252_and_reports_fallbacks() {
        let mut ctx = context();
        ctx.memory.write::<u16>(0x1000, b'A' as u16);
        ctx.memory.write::<u16>(0x1002, 0x20ac);
        ctx.memory.write::<u16>(0x1004, 0);
        ctx.memory.write::<u8>(0x1200, b'_');

        assert_eq!(
            WideCharToMultiByte(
                &mut ctx,
                1252,
                0,
                Ptr::new(0x1000),
                -1,
                Ptr::new(0x1100),
                3,
                Ptr::new(0x1200),
                Ptr::new(0x1300),
            ),
            3
        );
        assert_eq!(&ctx.memory[0x1100..][..3], &[b'A', 0x80, 0]);
        assert_eq!(ctx.memory.read::<u8>(0x1300), 0);

        ctx.memory.write::<u16>(0x1000, 0x2603);
        assert_eq!(
            WideCharToMultiByte(
                &mut ctx,
                1252,
                0,
                Ptr::new(0x1000),
                1,
                Ptr::new(0x1100),
                1,
                Ptr::new(0x1200),
                Ptr::new(0x1300),
            ),
            1
        );
        assert_eq!(ctx.memory.read::<u8>(0x1100), b'_');
        assert_eq!(ctx.memory.read::<u8>(0x1300), 1);
    }
}
