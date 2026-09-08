use runtime::Context;

use crate::{Ptr, dllexport::win32flags};

#[win32_derive::dllexport]
pub fn GetACP(_ctx: &mut Context) -> u32 {
    1252 // windows-1252
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
pub fn GetCPInfo(ctx: &mut Context, _CodePage: u32, lpCPInfo: Ptr<CPINFO>) -> bool {
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
    let len = if cchSrc < 0 {
        ctx.memory.read_str(lpSrcStr.addr).len() + 1
    } else {
        cchSrc as usize
    };
    for i in 0..len as u32 {
        let c = ctx.memory.read::<u8>(lpSrcStr.addr + i);
        ctx.memory
            .write::<u16>(lpCharType.addr + i * 2, ctype1(c as u32).bits() as u16);
    }
    true
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
    let len = if cchSrc < 0 {
        let mut n = 0;
        while ctx.memory.read::<u16>(lpSrcStr.addr + n * 2) != 0 {
            n += 1;
        }
        n + 1
    } else {
        cchSrc as u32
    };
    for i in 0..len {
        let c = ctx.memory.read::<u16>(lpSrcStr.addr + i * 2);
        ctx.memory
            .write::<u16>(lpCharType.addr + i * 2, ctype1(c as u32).bits() as u16);
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
    let len = if cchSrc < 0 {
        ctx.memory.read_str(lpSrcStr.addr).len() as u32 + 1
    } else {
        cchSrc as u32
    };
    if cchDest == 0 {
        return len as i32;
    }
    if (cchDest as u32) < len {
        return 0;
    }
    for i in 0..len {
        let c = ctx.memory.read::<u8>(lpSrcStr.addr + i);
        ctx.memory
            .write::<u8>(lpDestStr.addr + i, lcmap_char(c as u32, dwMapFlags) as u8);
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
    let len = if cchSrc < 0 {
        let mut n = 0;
        while ctx.memory.read::<u16>(lpSrcStr.addr + n * 2) != 0 {
            n += 1;
        }
        n + 1
    } else {
        cchSrc as u32
    };
    if cchDest == 0 {
        return len as i32;
    }
    if (cchDest as u32) < len {
        return 0;
    }
    for i in 0..len {
        let c = ctx.memory.read::<u16>(lpSrcStr.addr + i * 2);
        ctx.memory.write::<u16>(
            lpDestStr.addr + i * 2,
            lcmap_char(c as u32, dwMapFlags) as u16,
        );
    }
    len as i32
}

#[win32_derive::dllexport]
pub fn MultiByteToWideChar(
    _ctx: &mut Context,
    _CodePage: u32,
    _dwFlags: u32, /* MULTI_BYTE_TO_WIDE_CHAR_FLAGS */
    _lpMultiByteStr: Ptr<u8>,
    _cbMultiByte: i32,
    _lpWideCharStr: Ptr<u16>,
    _cchWideChar: i32,
) -> i32 {
    0
    /*
    match CodePage {
        Err(value) => unimplemented!("MultiByteToWideChar code page {value}"),
        _ => {} // treat all others as ansi for now
    }
    // TODO: obey dwFlags
    dwFlags.unwrap();

    let src_addr = lpMultiByteStr;
    let src_len = match cbMultiByte {
        0 => return 0,                                     // TODO: invalid param
        -1 => sys.mem().slicez(src_addr).len() as u32 + 1, // include nul
        len => len as u32,
    };

    let dst = &mut lpWideCharStr;
    if let Some(buf) = dst {
        if buf.len() == 0 {
            *dst = None;
        }
    }

    // TODO: reuse the conversion in winapi/string.rs.
    match dst {
        None => src_len,
        Some(dst) => {
            let src = sys.mem().sub32(src_addr, src_len);
            let mut len = 0;
            for &c in src {
                if c > 0x7f {
                    unimplemented!("unicode");
                }
                dst.put_pod(len, c as u16);
                len += 1;
            }
            len
        }
    }
    */
}

#[win32_derive::dllexport]
pub fn WideCharToMultiByte(
    _ctx: &mut Context,
    _CodePage: u32,
    _dwFlags: u32,
    _lpWideCharStr: Ptr<u16>,
    _cchWideChar: i32,
    _lpMultiByteStr: Ptr<u8>,
    _cbMultiByte: i32,
    _lpDefaultChar: Ptr<u8>,
    _lpUsedDefaultChar: Ptr<bool>,
) -> i32 {
    0
    /*
    match CodePage {
        Err(value) => unimplemented!("WideCharToMultiByte code page {value}"),
        _ => {} // treat all others as ansi for now
    }
    dwFlags.unwrap();

    let src = {
        let len = match cchWideChar {
            0 => todo!(),
            -1 => strlen16(sys.mem().slice(lpWideCharStr..)) + 1, // include nul
            len => len as usize,
        };
        sys.mem().sub32(lpWideCharStr, len as u32 * 2)
    };

    let dst = if cbMultiByte > 0 {
        sys.mem().sub32_mut(lpMultiByteStr, cbMultiByte as u32)
    } else {
        &mut []
    };

    for (i, c) in src.into_iter_pod::<u16>().enumerate() {
        if c > 0x7f {
            unimplemented!("unicode");
        }
        if i < dst.len() {
            dst[i] = c as u8;
        }
    }

    if let Some(used) = lpUsedDefaultChar {
        *used = 0;
    }

    src.len() as u32 / 2
    */
}

#[win32_derive::dllexport]
pub fn GetOEMCP(_ctx: &mut Context) -> u32 {
    todo!()
}

#[win32_derive::dllexport]
pub fn GetThreadLocale(_ctx: &mut Context) -> u32 {
    0x0409 // en-US
}

/// Values for GetLocaleInfo, for the en-US locale.
fn locale_info(lctype: u32) -> &'static str {
    match lctype & 0xffff {
        0x0001 => "0409",                    // LOCALE_ILANGUAGE
        0x0002 => "English (United States)", // LOCALE_SLANGUAGE
        0x0003 => "ENU",                     // LOCALE_SABBREVLANGNAME
        0x0006 => "United States",           // LOCALE_SCOUNTRY
        0x0007 => "USA",                     // LOCALE_SABBREVCTRYNAME
        0x000c => ",",                       // LOCALE_SLIST
        0x000d => "1",                       // LOCALE_IMEASURE
        0x000e => ".",                       // LOCALE_SDECIMAL
        0x000f => ",",                       // LOCALE_STHOUSAND
        0x0010 => "3;0",                     // LOCALE_SGROUPING
        0x0011 => "2",                       // LOCALE_IDIGITS
        0x0012 => "1",                       // LOCALE_ILZERO
        0x0014 => "$",                       // LOCALE_SCURRENCY
        0x0015 => "USD",                     // LOCALE_SINTLSYMBOL
        0x0016 => ".",                       // LOCALE_SMONDECIMALSEP
        0x0017 => ",",                       // LOCALE_SMONTHOUSANDSEP
        0x0018 => "3;0",                     // LOCALE_SMONGROUPING
        0x0019 => "2",                       // LOCALE_ICURRDIGITS
        0x001b => "0",                       // LOCALE_ICURRENCY
        0x001c => "0",                       // LOCALE_INEGCURR
        0x001d => "/",                       // LOCALE_SDATE
        0x001e => ":",                       // LOCALE_STIME
        0x001f => "M/d/yy",                  // LOCALE_SSHORTDATE
        0x0020 => "dddd, MMMM dd, yyyy",     // LOCALE_SLONGDATE
        0x0021 => "0",                       // LOCALE_IDATE
        0x0022 => "0",                       // LOCALE_ILDATE
        0x0023 => "0",                       // LOCALE_ITIME
        0x0025 => "0",                       // LOCALE_ITLZERO
        0x0028 => "AM",                      // LOCALE_S1159
        0x0029 => "PM",                      // LOCALE_S2359
        0x1003 => "h:mm:ss tt",              // LOCALE_STIMEFORMAT
        0x1010 => "1",                       // LOCALE_INEGNUMBER
        _ => {
            log::warn!("GetLocaleInfo: unknown LCTYPE {lctype:#x}");
            ""
        }
    }
}

#[win32_derive::dllexport]
pub fn GetLocaleInfoA(
    ctx: &mut Context,
    _Locale: u32,
    LCType: u32,
    lpLCData: Ptr<u8>,
    cchData: i32,
) -> i32 {
    let info = locale_info(LCType);
    let needed = info.len() as i32 + 1;
    if cchData == 0 {
        return needed;
    }
    if cchData < needed {
        return 0; // ERROR_INSUFFICIENT_BUFFER
    }
    crate::kernel32::write_cstr(ctx, lpLCData, cchData as u32, info.as_bytes());
    needed
}

#[win32_derive::dllexport]
pub fn GetLocaleInfoW(
    ctx: &mut Context,
    _Locale: u32,
    LCType: u32,
    lpLCData: Ptr<u16>,
    cchData: i32,
) -> i32 {
    let info = locale_info(LCType);
    let needed = info.len() as i32 + 1;
    if cchData == 0 {
        return needed;
    }
    if cchData < needed {
        return 0; // ERROR_INSUFFICIENT_BUFFER
    }
    let mut addr = lpLCData.addr;
    for c in info.encode_utf16().chain(std::iter::once(0)) {
        ctx.memory.write::<u16>(addr, c);
        addr += 2;
    }
    needed
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
