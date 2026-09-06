//! String functions: lstr*, CompareString*.

use runtime::Context;

use crate::Ptr;

/// The result of CompareString*.
#[derive(Debug, PartialEq, Eq, win32_derive::ABIEnum)]
pub enum CSTR {
    LESS_THAN = 1,
    EQUAL = 2,
    GREATER_THAN = 3,
}

const NORM_IGNORECASE: u32 = 1;

fn read_c_string(ctx: &Context, addr: u32) -> Option<Vec<u8>> {
    let buf = ctx.memory.bytes.get(addr as usize..)?;
    let len = buf.iter().position(|&byte| byte == 0)?;
    Some(buf[..len].to_vec())
}

fn range_fits(ctx: &Context, addr: u32, len: usize) -> bool {
    (addr as usize)
        .checked_add(len)
        .is_some_and(|end| end <= ctx.memory.bytes.len())
}

#[win32_derive::dllexport]
pub fn lstrlenA(ctx: &mut Context, lpString: Ptr<u8>) -> i32 {
    let Some(string) = read_c_string(ctx, lpString.addr) else {
        log::error!("lstrlenA: unterminated string");
        return 0;
    };
    string.len() as i32
}

#[win32_derive::dllexport]
pub fn lstrcpyA(ctx: &mut Context, lpString1: Ptr<u8>, lpString2: Ptr<u8>) -> u32 {
    let Some(src) = read_c_string(ctx, lpString2.addr) else {
        log::error!("lstrcpyA: unterminated source string");
        return 0;
    };
    let Some(output_len) = src.len().checked_add(1) else {
        return 0;
    };
    if !range_fits(ctx, lpString1.addr, output_len) {
        return 0;
    }
    ctx.memory[lpString1.addr..][..src.len()].copy_from_slice(&src);
    ctx.memory.write::<u8>(lpString1.addr + src.len() as u32, 0);
    lpString1.addr
}

#[win32_derive::dllexport]
pub fn lstrcatA(ctx: &mut Context, lpString1: Ptr<u8>, lpString2: Ptr<u8>) -> u32 {
    let Some(dst) = read_c_string(ctx, lpString1.addr) else {
        log::error!("lstrcatA: unterminated destination string");
        return 0;
    };
    let Some(src) = read_c_string(ctx, lpString2.addr) else {
        log::error!("lstrcatA: unterminated source string");
        return 0;
    };
    let Some(dst_addr) = lpString1.addr.checked_add(dst.len() as u32) else {
        return 0;
    };
    let Some(output_len) = dst
        .len()
        .checked_add(src.len())
        .and_then(|len| len.checked_add(1))
    else {
        return 0;
    };
    if !range_fits(ctx, lpString1.addr, output_len) {
        return 0;
    }
    ctx.memory[dst_addr..][..src.len()].copy_from_slice(&src);
    ctx.memory.write::<u8>(dst_addr + src.len() as u32, 0);
    lpString1.addr
}

#[win32_derive::dllexport]
pub fn lstrcmpA(ctx: &mut Context, lpString1: Ptr<u8>, lpString2: Ptr<u8>) -> i32 {
    let Some(a) = read_c_string(ctx, lpString1.addr) else {
        log::error!("lstrcmpA: unterminated first string");
        return 0;
    };
    let Some(b) = read_c_string(ctx, lpString2.addr) else {
        log::error!("lstrcmpA: unterminated second string");
        return 0;
    };
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn read_counted_a(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u8>> {
    if count < -1 {
        return None;
    }
    if count < 0 {
        read_c_string(ctx, addr)
    } else {
        let bytes = ctx.memory.bytes.get(addr as usize..)?;
        Some(bytes.get(..count as usize)?.to_vec())
    }
}

fn read_counted_w(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u16>> {
    if count < -1 {
        return None;
    }
    let bytes = ctx.memory.bytes.get(addr as usize..)?;
    if count < 0 {
        let mut out = Vec::new();
        for chunk in bytes.chunks_exact(2) {
            let c = u16::from_le_bytes([chunk[0], chunk[1]]);
            if c == 0 {
                return Some(out);
            }
            out.push(c);
        }
        return None;
    }
    let byte_len = (count as usize).checked_mul(2)?;
    let bytes = bytes.get(..byte_len)?;
    Some(
        bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect(),
    )
}

fn compare_ordering(ord: std::cmp::Ordering) -> i32 {
    let result = match ord {
        std::cmp::Ordering::Less => CSTR::LESS_THAN,
        std::cmp::Ordering::Equal => CSTR::EQUAL,
        std::cmp::Ordering::Greater => CSTR::GREATER_THAN,
    };
    result as i32
}

#[win32_derive::dllexport]
pub fn CompareStringA(
    ctx: &mut Context,
    _Locale: u32,
    dwCmpFlags: u32,
    lpString1: Ptr<u8>,
    cchCount1: i32,
    lpString2: Ptr<u8>,
    cchCount2: i32,
) -> i32 {
    let Some(mut a) = read_counted_a(ctx, lpString1.addr, cchCount1) else {
        return 0;
    };
    let Some(mut b) = read_counted_a(ctx, lpString2.addr, cchCount2) else {
        return 0;
    };
    if dwCmpFlags & NORM_IGNORECASE != 0 {
        a.make_ascii_lowercase();
        b.make_ascii_lowercase();
    }
    compare_ordering(a.cmp(&b))
}

#[win32_derive::dllexport]
pub fn CompareStringW(
    ctx: &mut Context,
    _Locale: u32,
    dwCmpFlags: u32,
    lpString1: Ptr<u16>,
    cchCount1: i32,
    lpString2: Ptr<u16>,
    cchCount2: i32,
) -> i32 {
    let Some(mut a) = read_counted_w(ctx, lpString1.addr, cchCount1) else {
        return 0;
    };
    let Some(mut b) = read_counted_w(ctx, lpString2.addr, cchCount2) else {
        return 0;
    };
    if dwCmpFlags & NORM_IGNORECASE != 0 {
        for c in a.iter_mut().chain(b.iter_mut()) {
            if *c < 0x80 {
                *c = (*c as u8).to_ascii_lowercase() as u16;
            }
        }
    }
    compare_ordering(a.cmp(&b))
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
    fn ansi_string_helpers_preserve_bytes_and_overlap() {
        let mut ctx = context();
        ctx.memory[0x1000..][..3].copy_from_slice(&[0x80, b'B', 0]);

        assert_eq!(lstrlenA(&mut ctx, Ptr::new(0x1000)), 2);
        assert_eq!(
            lstrcpyA(&mut ctx, Ptr::new(0x1001), Ptr::new(0x1000)),
            0x1001
        );
        assert_eq!(&ctx.memory[0x1001..][..3], &[0x80, b'B', 0]);

        ctx.memory[0x1100..][..3].copy_from_slice(&[0x80, b'C', 0]);
        ctx.memory[0x1200..][..2].copy_from_slice(b"X\0");
        assert_eq!(
            lstrcatA(&mut ctx, Ptr::new(0x1200), Ptr::new(0x1100)),
            0x1200
        );
        assert_eq!(&ctx.memory[0x1200..][..4], &[b'X', 0x80, b'C', 0]);

        ctx.memory[0x1300..][..2].copy_from_slice(&[0x80, 0]);
        ctx.memory[0x1400..][..2].copy_from_slice(&[0x81, 0]);
        assert_eq!(lstrcmpA(&mut ctx, Ptr::new(0x1300), Ptr::new(0x1400)), -1);

        ctx.memory[0x3ff0..].fill(0xff);
        assert_eq!(lstrlenA(&mut ctx, Ptr::new(0x3ff0)), 0);
        assert_eq!(lstrcpyA(&mut ctx, Ptr::new(0x1200), Ptr::new(0x3ff0)), 0);
    }

    #[test]
    fn ansi_string_helpers_reject_truncated_output() {
        let mut ctx = context();
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");
        ctx.memory[0x1100..][..2].copy_from_slice(b"X\0");
        ctx.memory[0x3ffe..].copy_from_slice(b"X\0");

        assert_eq!(lstrcpyA(&mut ctx, Ptr::new(0x3fff), Ptr::new(0x1000)), 0);
        assert_eq!(lstrcatA(&mut ctx, Ptr::new(0x3ffe), Ptr::new(0x1100)), 0);
    }

    #[test]
    fn compare_string_a_rejects_truncated_counted_input() {
        let mut ctx = context();
        ctx.memory.write::<u8>(0x3fff, b'A');
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");

        assert_eq!(
            CompareStringA(&mut ctx, 0, 0, Ptr::new(0x3fff), 2, Ptr::new(0x1000), 1,),
            0
        );
    }

    #[test]
    fn compare_string_rejects_invalid_negative_counts() {
        let mut ctx = context();
        ctx.memory[0x1000..][..2].copy_from_slice(b"A\0");
        ctx.memory[0x1100..][..2].copy_from_slice(b"a\0");

        assert_eq!(
            CompareStringA(
                &mut ctx,
                0,
                NORM_IGNORECASE,
                Ptr::new(0x1000),
                -2,
                Ptr::new(0x1100),
                -1,
            ),
            0
        );
        assert_eq!(
            CompareStringA(
                &mut ctx,
                0,
                NORM_IGNORECASE,
                Ptr::new(0x1000),
                -1,
                Ptr::new(0x1100),
                -1,
            ),
            CSTR::EQUAL as i32,
        );

        ctx.memory[0x3ff0..].fill(0xff);
        assert_eq!(
            CompareStringW(&mut ctx, 0, 0, Ptr::new(0x3ff0), -1, Ptr::new(0x1000), -1,),
            0
        );
        assert_eq!(
            CompareStringW(&mut ctx, 0, 0, Ptr::new(0x1000), 1, Ptr::new(0x3fff), 1,),
            0
        );
    }
}
