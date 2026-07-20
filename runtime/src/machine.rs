use crate::{Cont, ContFn, Flags, Memory, Regs, SegOfs, fpu::FPU, mmx::MMX, segofs};

#[derive(Default)]
pub struct CPU {
    pub regs: Regs,
    pub flags: Flags,
    pub fpu: FPU,
    pub mmx: MMX,
    pub real_mode: bool,
}

impl CPU {
    pub fn dump(&self) {
        self.regs.dump();
        if self.real_mode {
            self.regs.dump_segments();
        }
        // self.flags.dump();
        // self.fpu.dump();
        // self.mmx.dump();
    }
}

/// Address -> block lookup.
///
/// The generated block table is sorted, so a binary search over it works, but
/// indirect jumps are on the hot path of every call through a function pointer
/// or COM vtable. This turns the search's chain of dependent loads into a
/// single probe in the common case.
pub struct BlockMap {
    /// Power-of-two sized, holding indices into `blocks`, or EMPTY.
    slots: Box<[u32]>,
    mask: u32,
    blocks: &'static [(u32, ContFn)],
}

impl BlockMap {
    const EMPTY: u32 = u32::MAX;

    /// The lookup table for the program's blocks, built once and shared by
    /// every thread.
    pub fn get_or_init(blocks: &'static [(u32, ContFn)]) -> &'static BlockMap {
        static MAP: std::sync::OnceLock<BlockMap> = std::sync::OnceLock::new();
        MAP.get_or_init(|| BlockMap::new(blocks))
    }

    fn new(blocks: &'static [(u32, ContFn)]) -> Self {
        // A load factor of at most 1/2 keeps probe chains short.
        let capacity = (blocks.len() * 2).next_power_of_two().max(2);
        let mask = capacity as u32 - 1;
        let mut slots = vec![Self::EMPTY; capacity].into_boxed_slice();
        for (index, &(addr, _)) in blocks.iter().enumerate() {
            let mut slot = Self::hash(addr) & mask;
            while slots[slot as usize] != Self::EMPTY {
                slot = (slot + 1) & mask;
            }
            slots[slot as usize] = index as u32;
        }
        BlockMap {
            slots,
            mask,
            blocks,
        }
    }

    /// Fibonacci hashing. Code addresses are dense and share their high bits,
    /// so the multiply is what spreads them across the table.
    fn hash(addr: u32) -> u32 {
        addr.wrapping_mul(0x9E37_79B9)
    }

    pub fn get(&self, addr: u32) -> Option<ContFn> {
        let mut slot = Self::hash(addr) & self.mask;
        loop {
            let index = self.slots[slot as usize];
            if index == Self::EMPTY {
                return None;
            }
            let (block_addr, func) = self.blocks[index as usize];
            if block_addr == addr {
                return Some(func);
            }
            slot = (slot + 1) & self.mask;
        }
    }
}

pub struct Context {
    pub cpu: CPU,
    pub thread_handle: u32,
    pub thread_id: u32,
    pub memory: Memory,
    pub blocks: &'static [(u32, ContFn)],
    pub block_map: &'static BlockMap,
    pub recent: [ContFn; 4],
}

impl Context {
    /// Given an address (jump target), look up the Cont registered for it.
    pub fn indirect16(&self, addr: SegOfs) -> Cont {
        self.indirect(addr.abs())
    }

    /// Given an address (jump target), look up the Cont registered for it.
    pub fn indirect32(&self, addr: u32) -> Cont {
        self.indirect(addr)
    }

    /// Given an address (jump target), look up the Cont registered for it.
    pub fn indirect(&self, addr: u32) -> Cont {
        if addr == 0 {
            self.dump();
            panic!("jmp to null ptr");
        }
        let Some(func) = self.block_map.get(addr) else {
            self.dump();
            crate::log_missing_addr(addr);
            panic!(
                "jmp to unknown addr {addr:#010x}; \
                 re-run tc with --entry-points-file (see THESEUS_MISSING_ADDRS)"
            );
        };
        Cont(func)
    }

    pub fn proc_addr(&mut self, func: ContFn) -> u32 {
        self.blocks
            .iter()
            .find(|&(_, f)| std::ptr::fn_addr_eq(*f, func))
            .unwrap()
            .0
    }
}

impl Context {
    pub fn dump_stack32(&self) {
        let esp = self.cpu.regs.esp;
        println!("stack:");
        for i in 0..8 {
            let addr = esp + i * 4;
            if addr + 4 > self.memory.bytes.len() as u32 {
                break;
            }
            println!("{addr:08x} {:08x}", self.memory.read::<u32>(addr));
        }
    }

    pub fn dump_memory16(&self, seg: u16, ofs: u16, count: u16) {
        for i in 0..count {
            let Some(ofs) = ofs.checked_add(i * 2) else {
                break;
            };
            let addr = segofs(seg, ofs);
            if addr + 2 > self.memory.bytes.len() as u32 {
                break;
            }
            println!("{seg:04x}:{ofs:04x} {:04x}", self.memory.read::<u16>(addr));
        }
    }

    pub fn dump_stack16(&self) {
        let seg = self.cpu.regs.get_ss();
        let sp = self.cpu.regs.get_sp();
        println!("stack:");
        self.dump_memory16(seg, sp, 8);
    }

    pub fn dump(&self) {
        self.cpu.dump();
        if self.cpu.real_mode {
            self.dump_stack16();
        } else {
            self.dump_stack32();
        }
    }

    pub fn dump_dosbox(&self, ip: u16) {
        // 0813:0000FF30  xchg si,ax
        // EAX:0000000C EBX:00000001 ECX:00000005 EDX:00000D0B
        // ESI:0000F060 EDI:0000011F EBP:00000100 ESP:0000FFF4
        // DS:0813 ES:0813 FS:0000 GS:0000 SS:0813 CF:1 ZF:0 SF:0 OF:0 IF:1
        println!("{cs:04X}:{ip:08X}", cs = self.cpu.regs.cs);
        println!(
            "EAX:{:08X} EBX:{:08X} ECX:{:08X} EDX:{:08X}",
            self.cpu.regs.eax, self.cpu.regs.ebx, self.cpu.regs.ecx, self.cpu.regs.edx
        );
        println!(
            "ESI:{:08X} EDI:{:08X} EBP:{:08X} ESP:{:08X}",
            self.cpu.regs.esi, self.cpu.regs.edi, self.cpu.regs.ebp, self.cpu.regs.esp
        );
        println!(
            "DS:{:04X} ES:{:04X} FS:{:04X} GS:{:04X} SS:{:04X}",
            self.cpu.regs.ds,
            self.cpu.regs.es,
            self.cpu.regs.fs,
            self.cpu.regs.gs,
            self.cpu.regs.ss
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dump_ctx(ctx: &Context) {
    ctx.dump();
}
