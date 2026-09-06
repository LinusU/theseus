use runtime::Context;

use crate::{
    Ptr,
    kernel32::{lock, teb_mut},
};

const ERROR_ENVVAR_NOT_FOUND: u32 = 203;
const ERROR_INVALID_PARAMETER: u32 = 87;

/// Environment variable names compare case-insensitively on Windows; the
/// stored entry keeps its original spelling.
fn find(env: &[(String, String)], name: &str) -> Option<usize> {
    env.iter().position(|(n, _)| n.eq_ignore_ascii_case(name))
}

#[win32_derive::dllexport]
pub fn GetEnvironmentStrings(ctx: &mut Context) -> u32 {
    // A snapshot of "NAME=VALUE\0" entries with an extra nul ending the list.
    let kernel32 = lock();
    let mut block: Vec<u8> = Vec::new();
    for (name, value) in kernel32.env.iter() {
        block.extend_from_slice(name.as_bytes());
        block.push(b'=');
        block.extend_from_slice(value.as_bytes());
        block.push(0);
    }
    block.push(0);
    let Some(addr) = kernel32
        .process_heap
        .try_alloc(&mut ctx.memory, block.len() as u32)
    else {
        // GetEnvironmentStrings returns NULL on failure.
        return 0;
    };
    ctx.memory[addr..][..block.len()].copy_from_slice(&block);
    addr
}

#[win32_derive::dllexport]
pub fn GetEnvironmentStringsW(ctx: &mut Context) -> u32 {
    let kernel32 = lock();
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in kernel32.env.iter() {
        block.extend(name.encode_utf16());
        block.push(b'=' as u16);
        block.extend(value.encode_utf16());
        block.push(0);
    }
    block.push(0);
    let Some(addr) = kernel32
        .process_heap
        .try_alloc(&mut ctx.memory, (block.len() * 2) as u32)
    else {
        // GetEnvironmentStringsW returns NULL on failure.
        return 0;
    };
    for (i, c) in block.iter().enumerate() {
        ctx.memory.write::<u16>(addr + (i * 2) as u32, *c);
    }
    addr
}

#[win32_derive::dllexport]
pub fn GetEnvironmentVariableA(
    ctx: &mut Context,
    lpName: Ptr<u8>,
    lpBuffer: Ptr<u8>,
    nSize: u32,
) -> u32 {
    if lpName.addr == 0 {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return 0;
    }
    let name = ctx.memory.read_str(lpName.addr).to_string();
    let kernel32 = lock();
    let Some(value) = kernel32
        .env
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        .map(|(_, v)| v)
    else {
        teb_mut(ctx).LastErrorValue = ERROR_ENVVAR_NOT_FOUND;
        return 0;
    };
    let len = value.len() as u32;
    // When the buffer is too small the required size (nul included) is
    // returned and nothing is written.
    let needs = len + 1;
    let fits = nSize >= needs
        && lpBuffer.addr >= 0x1000
        && (lpBuffer.addr as usize)
            .checked_add(needs as usize)
            .is_some_and(|end| end <= ctx.memory.bytes.len());
    if !fits {
        return needs;
    }
    ctx.memory[lpBuffer.addr..][..len as usize].copy_from_slice(value.as_bytes());
    ctx.memory[lpBuffer.addr + len] = 0;
    len
}

#[win32_derive::dllexport]
pub fn SetEnvironmentVariableA(ctx: &mut Context, lpName: Ptr<u8>, lpValue: Ptr<u8>) -> bool {
    if lpName.addr == 0 {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return false;
    }
    let name = ctx.memory.read_str(lpName.addr).to_string();
    if name.is_empty() || name.contains('=') {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return false;
    }
    let mut kernel32 = lock();
    if lpValue.addr == 0 {
        // A NULL lpValue deletes the variable; it is not an error for the
        // variable to be absent.
        if let Some(i) = find(&kernel32.env, &name) {
            kernel32.env.remove(i);
        }
        return true;
    }
    let value = ctx.memory.read_str(lpValue.addr).to_string();
    match find(&kernel32.env, &name) {
        Some(i) => kernel32.env[i].1 = value,
        None => kernel32.env.push((name, value)),
    }
    true
}

#[win32_derive::dllexport]
pub fn FreeEnvironmentStringsA(ctx: &mut Context, penv: Ptr<u8>) -> bool {
    if penv.addr == 0 {
        return false;
    }
    // GetEnvironmentStrings allocates the snapshot from the process heap.
    lock().process_heap.free(&mut ctx.memory, penv.addr);
    true
}

#[win32_derive::dllexport]
pub fn FreeEnvironmentStringsW(ctx: &mut Context, penv: Ptr<u16>) -> bool {
    if penv.addr == 0 {
        return false;
    }
    lock().process_heap.free(&mut ctx.memory, penv.addr);
    true
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
            memory: Memory::leak_new(0x400_000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    /// The two cases share one test so they cannot interleave on the shared
    /// environment list.
    #[test]
    fn environment_variables_round_trip_and_list() {
        crate::kernel32::ensure_test_state();
        lock().env.clear();
        lock().process_heap = crate::heap::Heap::new(0x100_000, 0x100_000);
        let mut ctx = context();
        // teb_mut dereferences fs_base; give it a live region.
        ctx.cpu.regs.fs_base = 0x2000;

        ctx.memory[0x3000..][..5].copy_from_slice(b"Path\0");
        ctx.memory[0x3100..][..5].copy_from_slice(b"C:\\g\0");
        assert!(SetEnvironmentVariableA(
            &mut ctx,
            Ptr::new(0x3000),
            Ptr::new(0x3100)
        ));

        // Names match case-insensitively and report the value length.
        ctx.memory[0x3200..][..5].copy_from_slice(b"path\0");
        let got = GetEnvironmentVariableA(&mut ctx, Ptr::new(0x3200), Ptr::new(0x4000), 16);
        assert_eq!(got, 4);
        assert_eq!(&ctx.memory[0x4000..][..5], b"C:\\g\0");

        // Too-small buffers return the required size and write nothing.
        let got = GetEnvironmentVariableA(&mut ctx, Ptr::new(0x3200), Ptr::new(0x4000), 2);
        assert_eq!(got, 5);

        // A NULL value deletes; lookups then report "not found".
        assert!(SetEnvironmentVariableA(
            &mut ctx,
            Ptr::new(0x3000),
            Ptr::new(0)
        ));
        let got = GetEnvironmentVariableA(&mut ctx, Ptr::new(0x3200), Ptr::new(0x4000), 16);
        assert_eq!(got, 0);
        assert_eq!(
            crate::kernel32::teb(&mut ctx).LastErrorValue,
            ERROR_ENVVAR_NOT_FOUND
        );

        // The environment block lists the current variables.
        ctx.memory[0x3300..][..4].copy_from_slice(b"Foo\0");
        ctx.memory[0x3400..][..4].copy_from_slice(b"Bar\0");
        ctx.memory[0x3500..][..3].copy_from_slice(b"42\0");
        assert!(SetEnvironmentVariableA(
            &mut ctx,
            Ptr::new(0x3300),
            Ptr::new(0x3500)
        ));
        assert!(SetEnvironmentVariableA(
            &mut ctx,
            Ptr::new(0x3400),
            Ptr::new(0x3500)
        ));

        let block = GetEnvironmentStrings(&mut ctx);
        let expected = b"Foo=42\0Bar=42\0\0";
        assert_eq!(&ctx.memory[block..][..expected.len()], expected);
        assert!(FreeEnvironmentStringsA(&mut ctx, Ptr::new(block)));
    }
}
