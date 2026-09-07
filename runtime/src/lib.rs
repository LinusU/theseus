#![allow(clippy::upper_case_acronyms)]

mod exe;
mod flags;
mod fpu;
mod machine;
mod mapping;
mod memory;
mod mmx;
mod ops;
mod registers;
mod xmm;

pub use exe::EXEData;
pub use flags::Flags;
pub use fpu::F80;
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

static RDTSC_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn rdtsc() -> u64 {
    RDTSC_START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos() as u64
}

pub fn port_in(port: u16, width: u32) -> u32 {
    log::warn!("port input {port:#x}/{width} is unavailable; returning zero");
    0
}

pub fn port_out(port: u16, data: u32, width: u32) {
    log::warn!("port output {port:#x}/{width}: {data:#x} ignored");
}

pub fn unhandled_interrupt(vector: u8, ip: u32) -> ! {
    log::error!("unhandled x86 interrupt {vector:#x} at {ip:#x}");
    std::process::exit(1)
}

pub fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    match (leaf, subleaf) {
        (0, 0) => (1, 0x756e_6547, 0x6c65_746e, 0x4965_6e69),
        (1, 0) => (
            0x0000_0633,
            0,
            0,
            // FPU | TSC | CX8 | CMOV | MMX | FXSR | SSE | SSE2: the feature
            // bits for instructions this machine actually implements.
            (1 << 0)
                | (1 << 4)
                | (1 << 8)
                | (1 << 15)
                | (1 << 23)
                | (1 << 24)
                | (1 << 25)
                | (1 << 26),
        ),
        _ => (0, 0, 0, 0),
    }
}

pub fn xgetbv(index: u32) -> (u32, u32) {
    match index {
        0 => (1, 0),
        _ => (0, 0),
    }
}

pub fn bswap(value: u32) -> u32 {
    value.swap_bytes()
}

/// BOUND raises #BR when `index` lies outside the inclusive [lower, upper]
/// range loaded from memory.
pub fn bound(index: i32, lower: i32, upper: i32) -> bool {
    !(lower..=upper).contains(&index)
}

pub type ContFn = fn(&mut Context) -> Cont;

#[derive(Clone, Copy)]
pub struct Cont(pub ContFn);

/// A continuation that terminates the process, used instead of panicking when
/// the guest reaches a state the emulator cannot recover from.
pub fn halt(_: &mut Context) -> Cont {
    std::process::exit(1)
}

/// When making a call from host to to x86 code, we need a valid return address
/// that is associated with a real function so that the final 'ret' from the
/// called function succeeds, but we never invoke it.
pub const RETURN_FROM_X86_ADDR32: u32 = 0xffff_fffe;
pub const RETURN_FROM_X86_ADDR16: SegOfs = SegOfs::new(0xffff, 0xfffe);

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
    log::error!(
        "jmp to unknown block {addr:#010x}; \
         re-run tc with --entry-points-file (see THESEUS_MISSING_ADDRS)"
    );
    Cont(halt)
}

impl Context {
    /// Call an x86 stdcall function, only returning once the function returns.
    pub fn call32_x86(&mut self, f: Cont, args: Vec<u32>) {
        let esp = self.cpu.regs.esp;
        for arg in args.into_iter().rev() {
            self.push32(arg);
        }
        // Note that return_from_x86 is never called.  When the x86 code returns
        // to it, the stack will have been popped so that esp matches our initial
        // esp and we abort the loop before invoking the continuation.
        self.push32(RETURN_FROM_X86_ADDR32);

        self.cpu_loop(f, esp);
    }

    pub fn cpu_loop(&mut self, mut f: Cont, target_esp: u32) {
        let mut i = 0;
        // The stack grows down, so a callee's frame always sits below the
        // target; an esp that climbs above it means the return already
        // consumed the frame (a corrupt or convention-mismatched return)
        // and `!=` would spin on a value that can never match.
        while self.cpu.regs.esp != target_esp {
            if (self.cpu.regs.esp.wrapping_sub(target_esp)) as i32 > 0 {
                log::error!(
                    "cpu_loop: esp {:#x} ran past return target {target_esp:#x}; aborting",
                    self.cpu.regs.esp
                );
                return;
            }
            self.recent[i] = f.0;
            i = (i + 1) % self.recent.len();
            f = f.0(self);
        }
    }

    pub fn return_from_x86(&mut self) -> Cont {
        log::error!("return_from_x86 invoked unexpectedly");
        Cont(halt)
    }
}

/// Combine a seg:ofs address into a single flat u32 address.
pub const fn segofs(seg: u16, off: u16) -> u32 {
    ((seg as u32) << 4) + (off as u32)
}

/// Combine a seg:ofs address where the offset is 32 bits: a 67h-prefixed
/// operand in a 16-bit module uses 32-bit addressing and the effective
/// address is not truncated to 16 bits.
pub const fn segofs32(seg: u16, off: u32) -> u32 {
    ((seg as u32) << 4).wrapping_add(off)
}

#[cfg(test)]
mod tests {
    use super::{BlockCache, CPU, Cont, Context, Memory, cpuid, xgetbv};

    #[test]
    fn cpu_loop_bails_when_esp_overshoots_the_return_target() {
        // A callee whose return pops more than was pushed lands above the
        // target; the loop must stop rather than spin on a never-match.
        fn overshoot(ctx: &mut Context) -> Cont {
            ctx.cpu.regs.esp += 0x10;
            Cont(overshoot)
        }
        let mut ctx = Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        };
        ctx.cpu.regs.esp = 0x1000;
        ctx.cpu_loop(Cont(overshoot), 0x1008);
        // One step ran (esp 0x1000 -> 0x1010), then the loop bailed.
        assert_eq!(ctx.cpu.regs.esp, 0x1010);
    }

    #[test]
    fn cpu_loop_returns_on_an_exact_target_match() {
        fn arrive(ctx: &mut Context) -> Cont {
            ctx.cpu.regs.esp += 4;
            Cont(arrive)
        }
        let mut ctx = Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        };
        ctx.cpu.regs.esp = 0x1000;
        ctx.cpu_loop(Cont(arrive), 0x1008);
        assert_eq!(ctx.cpu.regs.esp, 0x1008);
    }

    #[test]
    fn cpuid_reports_the_supported_basic_leaves() {
        assert_eq!(cpuid(0, 0), (1, 0x756e_6547, 0x6c65_746e, 0x4965_6e69));
        assert_eq!(
            cpuid(1, 0),
            (
                0x0000_0633,
                0,
                0,
                (1 << 0)
                    | (1 << 4)
                    | (1 << 8)
                    | (1 << 15)
                    | (1 << 23)
                    | (1 << 24)
                    | (1 << 25)
                    | (1 << 26)
            )
        );
        assert_eq!(cpuid(1, 1), (0, 0, 0, 0));
        assert_eq!(cpuid(0x8000_0000, 0), (0, 0, 0, 0));
    }

    #[test]
    fn xgetbv_reports_only_x87_xcr0() {
        assert_eq!(xgetbv(0), (1, 0));
        assert_eq!(xgetbv(1), (0, 0));
    }
}
