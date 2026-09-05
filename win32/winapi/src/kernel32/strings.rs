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
    let buf = &ctx.memory[addr..];
    let len = buf.iter().position(|&byte| byte == 0)?;
    Some(buf[..len].to_vec())
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
    let dst_addr = lpString1.addr + dst.len() as u32;
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
        Some(ctx.memory[addr..][..count as usize].to_vec())
    }
}

fn read_counted_w(ctx: &Context, addr: u32, count: i32) -> Option<Vec<u16>> {
    if count < -1 {
        return None;
    }
    let mut out = Vec::new();
    let mut addr = addr;
    if count < 0 {
        loop {
            let c = ctx.memory.read::<u16>(addr);
            if c == 0 {
                break;
            }
            out.push(c);
            addr += 2;
        }
    } else {
        for _ in 0..count {
            out.push(ctx.memory.read::<u16>(addr));
            addr += 2;
        }
    }
    Some(out)
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
    }
}
