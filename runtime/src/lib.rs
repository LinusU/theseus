mod exe;
mod flags;
pub mod fpu;
mod machine;
mod mapping;
mod memory;
mod mmx;
mod ops;
pub mod profile;
mod registers;

pub use exe::EXEData;
pub use flags::Flags;
pub use machine::{BlockCache, CPU, Context};
pub use mapping::{Mapping, Mappings, round_to_page};
pub use memory::Memory;
pub use ops::*;
pub use registers::Regs;

#[repr(C)]
#[derive(
    zerocopy::FromBytes, zerocopy::IntoBytes, Debug, Clone, Copy, PartialEq, PartialOrd, Eq,
)]
pub struct SegOfs {
    pub ofs: u16,
    pub seg: u16,
}

impl SegOfs {
    pub const fn new(seg: u16, ofs: u16) -> SegOfs {
        SegOfs { seg, ofs }
    }

    pub const fn abs(&self) -> u32 {
        segofs(self.seg, self.ofs)
    }
}

impl From<(u16, u16)> for SegOfs {
    fn from((seg, ofs): (u16, u16)) -> Self {
        SegOfs::new(seg, ofs)
    }
}

impl std::fmt::Display for SegOfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{seg:04x}:{ofs:04x}", seg = self.seg, ofs = self.ofs)
    }
}

pub type ContFn = fn(&mut Context) -> Cont;

#[derive(Clone, Copy)]
pub struct Cont(pub ContFn);

/// When making a call from host to to x86 code, we need a valid return address
/// that is associated with a real function so that the final 'ret' from the
/// called function succeeds, but we never invoke it.
pub const RETURN_FROM_X86_ADDR32: u32 = 0xffff_fffe;
pub const RETURN_FROM_X86_ADDR16: SegOfs = SegOfs::new(0xffff, 0xfffe);

/// `call32_x86` calls can nest (x86 code calls an API that calls back into x86
/// code), and each pushes a return address of its own: RETURN_FROM_X86_ADDR32
/// for the outermost, one less for each level inside it.
const MAX_CALL_DEPTH: u32 = 0x1000;

/// Whether `addr` is a return address pushed by `call32_x86`.
pub fn is_return_marker(addr: u32) -> bool {
    addr <= RETURN_FROM_X86_ADDR32 && addr > RETURN_FROM_X86_ADDR32 - MAX_CALL_DEPTH
}

thread_local! {
    /// How many `call32_x86` calls are running on this thread.
    static CALL_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The return marker x86 code last jumped to.
    static RETURNED_TO: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Note which return marker the x86 code is returning to (see `call32_x86`).
pub(crate) fn note_return(addr: u32) {
    RETURNED_TO.set(addr);
}

/// Unwinds Rust frames up to the `call32_x86` at this depth.
struct ReturnTo(u32);

/// Record a code address the static analysis missed, so it can be fed back
/// into tc via --entry-points-file. Set THESEUS_MISSING_ADDRS to a file path.
pub fn log_missing_addr(addr: u32) {
    #[cfg(not(target_family = "wasm"))]
    if let Ok(path) = std::env::var("THESEUS_MISSING_ADDRS") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{addr:x}");
        }
    }
}

/// Called by generated code for a statically-known jump target that the
/// analysis didn't produce a block for.
pub fn unknown_block(addr: u32) -> Cont {
    log_missing_addr(addr);
    panic!(
        "jmp to unknown block {addr:#010x}; \
         re-run tc with --entry-points-file (see THESEUS_MISSING_ADDRS)"
    );
}

impl Context {
    /// Call an x86 function, only returning once the function returns.
    ///
    /// The arguments are pushed as for stdcall; a cdecl callee leaves them on
    /// the stack for the caller to pop.
    ///
    /// This watches for the return address rather than for esp coming back to
    /// where it started: a callee may pop a different amount (cdecl vs
    /// stdcall). And an exception handler that catches never returns at all
    /// but jumps into the catching frame (see winapi's kernel32/seh.rs), after
    /// which the program runs on inside this call until it returns to the
    /// return address of some call further out, maybe much later. That call
    /// is then where it returns: the Rust frames in between are unwound.
    pub fn call32_x86(&mut self, f: Cont, args: Vec<u32>) {
        for arg in args.into_iter().rev() {
            self.push32(arg);
        }
        let depth = CALL_DEPTH.get();
        assert!(depth < MAX_CALL_DEPTH, "call32_x86 nested too deeply");
        CALL_DEPTH.set(depth + 1);
        // Note that return_from_x86 is never called.  When the x86 code returns
        // to it, we abort the loop before invoking the continuation.
        self.push32(RETURN_FROM_X86_ADDR32 - depth);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.cpu_loop(f)));
        CALL_DEPTH.set(depth);
        let returned_to = match result {
            Ok(()) => RETURN_FROM_X86_ADDR32 - RETURNED_TO.get(),
            Err(payload) => match payload.downcast::<ReturnTo>() {
                Ok(to) => to.0,
                Err(payload) => std::panic::resume_unwind(payload),
            },
        };
        if returned_to < depth {
            std::panic::resume_unwind(Box::new(ReturnTo(returned_to)));
        }
        assert_eq!(returned_to, depth, "x86 code returned to a finished call32_x86");
    }

    /// Run x86 code until it returns to a `call32_x86` return address.
    pub fn cpu_loop(&mut self, mut f: Cont) {
        let mut i = 0;
        let done: ContFn = Context::return_from_x86;
        let profiling = profile::enabled();
        while f.0 as usize != done as usize {
            if profiling {
                profile::count(f.0 as usize);
            }
            self.recent[i] = f.0;
            i = (i + 1) % self.recent.len();
            f = f.0(self);
        }
    }

    pub fn return_from_x86(&mut self) -> Cont {
        panic!();
    }
}

/// Combine a seg:ofs address into a single flat u32 address.
pub const fn segofs(seg: u16, off: u16) -> u32 {
    ((seg as u32) << 4) + (off as u32)
}
