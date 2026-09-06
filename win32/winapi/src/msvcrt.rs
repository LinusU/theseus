use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use runtime::Context;

use crate::{Ptr, kernel32::lock};

// Exception filter return values used in `__except` expressions.
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

#[win32_derive::dllexport(cdecl)]
pub fn _XcptFilter(_ctx: &mut Context, _xcptnum: u32, _pxcptinfoptrs: u32) -> i32 {
    // No per-signal disposition is modeled; continue searching.
    EXCEPTION_CONTINUE_SEARCH
}

#[win32_derive::dllexport(cdecl)]
pub fn __getmainargs(
    ctx: &mut Context,
    argc: Ptr<i32>,
    argv: Ptr<u32>,
    envp: Ptr<u32>,
    _dowildcard: i32,
    _startinfo: Ptr<()>,
) -> i32 {
    // Hand the CRT a minimal argv: just the program name taken from the
    // process command line.
    let cmdline = {
        let kernel32 = lock();
        ctx.memory
            .read_str(kernel32.command_line.command_line_8)
            .to_owned()
    };
    let exe = cmdline.split(' ').next().unwrap_or("");
    let kernel32 = lock();
    // The command line is guest data; an unallocatable argv reports the
    // documented nonzero failure rather than panicking the host.
    let Some(name_len) = exe.len().checked_add(1).and_then(|n| u32::try_from(n).ok()) else {
        return -1;
    };
    let Some(name) = kernel32.process_heap.try_alloc(&mut ctx.memory, name_len) else {
        return -1;
    };
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(name as usize..)
        .and_then(|b| b.get_mut(..exe.len()))
    {
        dst.copy_from_slice(exe.as_bytes());
    }
    ctx.memory.write::<u8>(name + exe.len() as u32, 0);
    let Some(argv_buf) = kernel32.process_heap.try_alloc(&mut ctx.memory, 8) else {
        return -1;
    };
    ctx.memory.write::<u32>(argv_buf, name);
    ctx.memory.write::<u32>(argv_buf + 4, 0);
    let Some(envp_buf) = kernel32.process_heap.try_alloc(&mut ctx.memory, 4) else {
        return -1;
    };
    ctx.memory.write::<u32>(envp_buf, 0);
    // A caller that passes out-of-range out-pointers gets a truncated result
    // rather than a host panic.
    if argc.addr != 0 {
        argc.write(&mut ctx.memory, 1);
    }
    if argv.addr != 0 {
        argv.write(&mut ctx.memory, argv_buf);
    }
    if envp.addr != 0 {
        envp.write(&mut ctx.memory, envp_buf);
    }
    0
}

/// Guest-memory ints behind __p__fmode/__p__commode, allocated lazily from the
/// process heap so the addresses stay valid for the whole run.
static FMODE: OnceLock<u32> = OnceLock::new();
static COMMODE: OnceLock<u32> = OnceLock::new();

const _O_TEXT: u32 = 0x4000;

#[win32_derive::dllexport(cdecl)]
pub fn __p__commode(ctx: &mut Context) -> u32 {
    *COMMODE.get_or_init(|| {
        let Some(addr) = lock().process_heap.try_alloc(&mut ctx.memory, 4) else {
            return 0;
        };
        ctx.memory.write::<u32>(addr, 0);
        addr
    })
}

#[win32_derive::dllexport(cdecl)]
pub fn __p__fmode(ctx: &mut Context) -> u32 {
    *FMODE.get_or_init(|| {
        let Some(addr) = lock().process_heap.try_alloc(&mut ctx.memory, 4) else {
            return 0;
        };
        ctx.memory.write::<u32>(addr, _O_TEXT);
        addr
    })
}

#[win32_derive::dllexport(cdecl)]
pub fn __set_app_type(_ctx: &mut Context, _at: i32) {}

#[win32_derive::dllexport(cdecl)]
pub fn __setusermatherr(_ctx: &mut Context, _pf: u32) {
    // No user math-error handler is modeled; the default remains in effect.
}

// data:
// _acmdln
// _adjust_fdiv

// _controlfp operates on an abstract control word with fields that do not
// line up with the x87 FPU control word. These are the _MCW_* masks.
const MCW_EM: u32 = 0x0008_001f;
const MCW_IC: u32 = 0x0004_0000;
const MCW_RC: u32 = 0x0000_0300;
const MCW_PC: u32 = 0x0003_0000;
const MCW_DN: u32 = 0x0300_0000;

/// The abstract denormal-control field (bits 24-25); x87 has no equivalent.
static DENORMAL: AtomicU32 = AtomicU32::new(0);

/// The abstract _EM_* exception bits are a permutation of the x87
/// control-word mask bits, not a verbatim copy:
///   abstract: INEXACT=0 UNDERFLOW=1 OVERFLOW=2 ZERODIVIDE=3 INVALID=4
///             DENORMAL=19
///   x87:      IM=0 DM=1 ZM=2 OM=3 UM=4 PM=5
fn em_to_abstract(x87: u32) -> u32 {
    (x87 & 0x20) >> 5 // PM -> _EM_INEXACT
        | (x87 & 0x10) >> 3 // UM -> _EM_UNDERFLOW
        | (x87 & 0x08) >> 1 // OM -> _EM_OVERFLOW
        | (x87 & 0x04) << 1 // ZM -> _EM_ZERODIVIDE
        | (x87 & 0x01) << 4 // IM -> _EM_INVALID
        | (x87 & 0x02) << 18 // DM -> _EM_DENORMAL
}

/// The inverse of `em_to_abstract`: which x87 mask bits an abstract
/// _EM_* mask/value selects.
fn em_to_x87(em: u32) -> u16 {
    ((em & 0x01) << 5 // _EM_INEXACT -> PM
        | (em & 0x02) << 3 // _EM_UNDERFLOW -> UM
        | (em & 0x04) << 1 // _EM_OVERFLOW -> OM
        | (em & 0x08) >> 1 // _EM_ZERODIVIDE -> ZM
        | (em & 0x10) >> 4 // _EM_INVALID -> IM
        | (em & 0x0008_0000) >> 18) as u16 // _EM_DENORMAL -> DM
}

/// Translate the x87 control word into the abstract _controlfp value.
fn abstract_control(control: u16) -> u32 {
    let control = control as u32;
    // The six low x87 bits are the exception masks, permuted into the
    // abstract _EM_* layout.
    let mut value = em_to_abstract(control & 0x3f);
    // Rounding control: same 2-bit encoding, moved from bits 10-11 to 8-9.
    value |= ((control >> 10) & 0b11) << 8;
    // Precision control: x87 {00=24, 10=53, 11=64} to abstract {0,1,2}.
    let pc = match (control >> 8) & 0b11 {
        0b10 => 1,
        0b11 => 2,
        _ => 0,
    };
    value |= pc << 16;
    // Infinity control: x87 bit 12 to abstract bit 18.
    value |= ((control >> 12) & 1) << 18;
    value | DENORMAL.load(Ordering::Relaxed)
}

/// Apply `value & mask` onto the x87 control word, translating fields back.
fn apply_control(control: u16, value: u32, mask: u32) -> u16 {
    let mut control = control;
    if mask & MCW_EM != 0 {
        let m = em_to_x87(mask & MCW_EM);
        control = (control & !m) | (em_to_x87(value) & m);
    }
    if mask & MCW_RC != 0 {
        control = (control & !(0b11 << 10)) | (((value >> 8) & 0b11) as u16) << 10;
    }
    if mask & MCW_PC != 0 {
        let x87 = match (value >> 16) & 0b11 {
            1 => 0b10, // _PC_53
            2 => 0b11, // _PC_64
            _ => 0b00, // _PC_24
        };
        control = (control & !(0b11 << 8)) | x87 << 8;
    }
    if mask & MCW_IC != 0 {
        control = (control & !(1 << 12)) | (((value >> 18) & 1) as u16) << 12;
    }
    if mask & MCW_DN != 0 {
        DENORMAL.store(value & MCW_DN, Ordering::Relaxed);
    }
    control
}

#[win32_derive::dllexport(cdecl)]
pub fn _controlfp(ctx: &mut Context, new: u32, mask: u32) -> u32 {
    let control = apply_control(ctx.cpu.fpu.control, new, mask);
    ctx.cpu.fpu.control = control;
    abstract_control(control)
}

// Exception disposition returned by language-specific SEH handlers.
const EXCEPTION_CONTINUE_SEARCH_DISPOSITION: i32 = 1;

#[win32_derive::dllexport(cdecl)]
pub fn _except_handler3(
    _ctx: &mut Context,
    _exception_record: u32,
    _registration_frame: u32,
    _context: u32,
    _dispatcher: u32,
) -> i32 {
    // No SEH scope table is modeled; continue searching for a handler.
    EXCEPTION_CONTINUE_SEARCH_DISPOSITION
}

#[win32_derive::dllexport(cdecl)]
pub fn _exit(_ctx: &mut Context, status: i32) {
    std::process::exit(status);
}

#[win32_derive::dllexport(cdecl)]
pub fn _initterm(ctx: &mut Context, begin: Ptr<u32>, end: Ptr<u32>) {
    // The CRT runs each function pointer in [begin, end); they are
    // parameterless, so the stdcall/cdecl distinction cannot matter.
    // A bad `end` walks off emulated memory, so reads are bounds-checked.
    let mut entry = begin.addr;
    while entry < end.addr {
        let Some(f) = Ptr::<u32>::new(entry).read(&ctx.memory) else {
            break;
        };
        if f != 0 {
            let cont = ctx.indirect(f);
            ctx.call32_x86(cont, vec![]);
        }
        // A wrapped cursor would walk the whole address space again.
        let Some(next) = entry.checked_add(4) else {
            break;
        };
        entry = next;
    }
}

#[win32_derive::dllexport(cdecl)]
pub fn exit(_ctx: &mut Context, status: i32) {
    // No CRT atexit-handler model exists; exit forwards to the host directly.
    std::process::exit(status);
}

// MSDN: "Calling rand before any call to srand generates the same sequence as calling srand with seed passed as 1."
static mut RAND_STATE: u32 = 1;

#[win32_derive::dllexport(cdecl)]
pub fn rand(_ctx: &mut Context) -> u32 {
    // The MSVC runtime's linear congruential generator.
    unsafe {
        RAND_STATE = RAND_STATE.wrapping_mul(214013).wrapping_add(2531011);
        (RAND_STATE >> 16) & 0x7fff
    }
}

#[win32_derive::dllexport(cdecl)]
pub fn srand(_ctx: &mut Context, seed: u32) {
    unsafe {
        RAND_STATE = seed;
    }
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
            memory: Memory::leak_new(0x1000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn controlfp_permutates_exception_masks() {
        let mut ctx = context();
        // Masking only _EM_ZERODIVIDE (abstract bit 3) sets only x87 ZM
        // (bit 2) — the abstract and x87 bit layouts differ.
        _controlfp(&mut ctx, 0x08, MCW_EM);
        assert_eq!(ctx.cpu.fpu.control & 0x3f, 0x04);
        // Reading the word back reports the same abstract bits.
        assert_eq!(_controlfp(&mut ctx, 0, 0) & MCW_EM, 0x08);
        // Unmasking everything clears all six x87 mask bits.
        _controlfp(&mut ctx, 0, MCW_EM);
        assert_eq!(ctx.cpu.fpu.control & 0x3f, 0);
        // Masking everything sets all six, including _EM_DENORMAL at
        // abstract bit 19 -> x87 DM (bit 1).
        _controlfp(&mut ctx, MCW_EM, MCW_EM);
        assert_eq!(ctx.cpu.fpu.control & 0x3f, 0x3f);
        assert_eq!(_controlfp(&mut ctx, 0, 0) & MCW_EM, MCW_EM);
    }

    #[test]
    fn controlfp_maps_rounding_and_precision_fields() {
        let mut ctx = context();
        // _RC_DOWN (abstract 0x100) selects x87 round-down (bits 10-11 = 01).
        _controlfp(&mut ctx, 0x100, MCW_RC);
        assert_eq!((ctx.cpu.fpu.control >> 10) & 3, 1);
        assert_eq!(_controlfp(&mut ctx, 0, 0) & MCW_RC, 0x100);
        // _PC_53 (abstract 0x10000) selects x87 precision control 10.
        _controlfp(&mut ctx, 0x1_0000, MCW_PC);
        assert_eq!((ctx.cpu.fpu.control >> 8) & 3, 0b10);
        assert_eq!(_controlfp(&mut ctx, 0, 0) & MCW_PC, 0x1_0000);
    }
}
