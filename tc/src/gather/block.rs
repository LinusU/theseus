//! Decoding of basic blocks -- sequences of instructions that have no
//! internal control flow.
//!
//! The larger gathering process repeatedly invokes this to actually parse the
//! instruction stream.

use std::collections::HashMap;

use crate::Instr;

use super::{IP, Traverse};

/// If the instruction looks like
///   foo [x]
/// where x is a constant, return the value of x.
fn is_abs_memory_ref(instr: &iced_x86::Instruction) -> Option<u32> {
    let iced_x86::OpKind::Memory = instr.op0_kind() else {
        return None;
    };
    let iced_x86::Register::None = instr.memory_base() else {
        return None;
    };
    let iced_x86::Register::None = instr.memory_index() else {
        return None;
    };
    Some(instr.memory_displacement32())
}

/// If the instruction looks like a switch dispatch
///   jmp/call [reg*4 + table]
/// where table is a constant, return the address of the table and the register
/// indexing it.
fn is_jump_table_ref(instr: &iced_x86::Instruction) -> Option<(u32, iced_x86::Register)> {
    let iced_x86::OpKind::Memory = instr.op0_kind() else {
        return None;
    };
    let iced_x86::Register::None = instr.memory_base() else {
        return None;
    };
    if instr.memory_index() == iced_x86::Register::None {
        return None;
    }
    if instr.memory_index_scale() != 4 {
        return None;
    }
    let table = instr.memory_displacement32();
    if table < 0x1000 {
        return None;
    }
    Some((table, instr.memory_index()))
}

/// If the instruction bounds a register to a small range — `and reg, mask` or
/// `cmp reg, limit` — return how many values it can then hold. Compilers emit
/// one of these right before a switch dispatch, which tells us exactly how long
/// the jump table is.
fn is_index_bound(instr: &iced_x86::Instruction) -> Option<(iced_x86::Register, usize)> {
    use iced_x86::Mnemonic::*;
    if !matches!(instr.mnemonic(), And | Cmp) {
        return None;
    }
    if instr.op0_kind() != iced_x86::OpKind::Register {
        return None;
    }
    let imm = match instr.op1_kind() {
        iced_x86::OpKind::Immediate8 => instr.immediate8() as u32,
        iced_x86::OpKind::Immediate8to32 | iced_x86::OpKind::Immediate32 => instr.immediate32(),
        _ => return None,
    };
    // An `and` masks to 0..=mask, a `cmp` guards indices 0..=limit; both give
    // the same count. Anything large is not a switch bound.
    let count = (imm as usize).checked_add(1)?;
    if count > 1024 {
        return None;
    }
    // A mask only bounds the index if it is contiguous: `and eax, 0x30` leaves
    // values up to 0x30, not 0x31 of them.
    if instr.mnemonic() == And && !count.is_power_of_two() {
        return None;
    }
    // Masking a sub-register says nothing about the register the dispatch
    // indexes with: `and al, 0xf` leaves the rest of eax untouched. Keyed by
    // the full register, which is how the dispatch looks it up.
    let reg = instr.op0_register();
    if reg != reg.full_register32() {
        return None;
    }
    Some((reg.full_register(), count))
}

/// Data gathered while decoding one block.
pub struct BlockDecoder<'a, 'b> {
    traverse: &'a mut Traverse<'b>,
    block_ip: IP,

    // Code addresses noticed along the way, processed after the decode loop
    // (decoding borrows self.mem).
    // (table address, entry count if a bounds check revealed it)
    found_tables: Vec<(u32, Option<usize>)>,
    // Index bounds seen so far in this block, keyed by register.
    index_bounds: HashMap<iced_x86::Register, usize>,
}

impl<'a, 'b> BlockDecoder<'a, 'b> {
    pub fn new(traverse: &'a mut Traverse<'b>, block_ip: IP) -> Self {
        BlockDecoder {
            traverse,
            block_ip,
            found_tables: Default::default(),
            index_bounds: Default::default(),
        }
    }

    /// Check the instruction stream for a block of 0 bytes, and bail if so.
    /// A common occurrence when we accidentally start decoding data memory.
    fn check_empty(data: &[u8]) -> anyhow::Result<()> {
        if data.len() > 0x10 && data[..0x10].iter().all(|&b| b == 0) {
            anyhow::bail!("suspicious block of 0");
        }
        Ok(())
    }

    /// Check an instruction for validity, bailing if it is not.
    /// This can happen when decoding randomly invalid data.
    fn check_instr(&self, instr: &iced_x86::Instruction) -> anyhow::Result<()> {
        match instr.mnemonic() {
            iced_x86::Mnemonic::Out => {
                if !self.traverse.module.is_dos() {
                    anyhow::bail!("'out' instruction in non-DOS code");
                }
            }
            iced_x86::Mnemonic::INVALID => anyhow::bail!("invalid instruction"),
            _ => {}
        }
        Ok(())
    }

    /// Scan the immediate parameters to an instruction for values that look like code pointers.
    /// E.g.
    ///   mov eax, somevalue
    /// where somevalue is an address within the code segment.
    /// Very low confidence.
    fn scan_immediates(&mut self, ip: IP, instr: &iced_x86::Instruction) {
        if self.traverse.module.segment_addressed() {
            log::error!("--scan-immediates not supported for segmented (DOS) modules");
            return;
        }

        for i in 0..instr.op_count() {
            if instr.op_kind(i) == iced_x86::OpKind::Immediate32 {
                let imm = instr.immediate32();
                if self.traverse.module.code_memory().contains(&imm) {
                    log::info!("{ip} {instr}  ; {imm:x} looks like a code pointer");
                    assert!(!self.traverse.module.segment_addressed());
                    self.traverse.queue.add_candidate(imm);
                }
            }
        }
    }

    pub fn go(&mut self) -> anyhow::Result<Vec<Instr>> {
        let block_ip = self.block_ip;

        // log::info!("decode block {block_ip}");
        let block_addr = block_ip.to_addr();
        if block_addr > self.traverse.mem.bytes.len() as u32 {
            anyhow::bail!("ip out of bounds");
        }
        let data = self.traverse.mem.slice_all(block_addr);

        let mut instrs = Vec::new();
        let mut decoder = iced_x86::Decoder::with_ip(
            self.traverse.module.bitness(),
            data,
            block_ip.local() as u64,
            iced_x86::DecoderOptions::NONE,
        );
        while decoder.can_decode() {
            let ip = block_ip.with_local(decoder.ip() as u32);
            if self.traverse.blocks.contains_key(&ip.to_addr()) {
                // Hit a point covered by another block, e.g. a jump target
                break;
            }
            Self::check_empty(&data[decoder.position()..])?;

            let instr = decoder.decode();
            // log::info!("{ip:08x} {instr}", ip = instr.ip32());

            self.check_instr(&instr)?;

            if let Some((reg, count)) = is_index_bound(&instr) {
                self.index_bounds.insert(reg, count);
            }

            instrs.push(Instr {
                ip,
                iced: instr,
                hint: None,
            });
            let new_instr = instrs.last_mut().unwrap();

            if self.traverse.gather.scan_immediates {
                self.scan_immediates(ip, &instr);
            }

            if instr.flow_control() == iced_x86::FlowControl::Next {
                continue;
            }

            let ip = block_ip.with_local(instr.ip32());
            use iced_x86::Mnemonic::*;
            match instr.mnemonic() {
                Call | Jmp | Jcxz | Je | Jne | Jb | Js | Jns | Ja | Jae | Jl | Jge | Jecxz | Jg
                | Jle | Jo | Jno | Jp | Jnp | Jbe | Loop | Loope | Loopne => {
                    self.control_flow(ip, new_instr)?;
                    if instr.mnemonic() == iced_x86::Mnemonic::Call && new_instr.hint.is_some() {
                        // call was resolved to a builtin function call, don't end block here
                        continue;
                    }
                    if instr.mnemonic() != Jmp {
                        // enqueue next instruction as a new block
                        let ip = self.block_ip.with_local(instr.next_ip32());
                        self.traverse.queue.enqueue(ip);
                    }
                }
                Ret | Retf | Iret | Iretd | Ud2 => {}
                Into => {
                    let ip = self.block_ip.with_local(instr.next_ip32());
                    self.traverse.queue.enqueue(ip);
                }
                Int1 | Int3 => {} // breakpoint
                Int => {
                    let ip = block_ip.with_local(instr.next_ip32());
                    self.traverse.queue.enqueue(ip);
                }
                Syscall | Sysexit | Sysret => anyhow::bail!("syscall not implemented"),
                _ => todo!("{ip} control flow {}", instr),
            }
            break;
        }

        for (table, count) in self.found_tables.iter().copied() {
            let n = self.traverse.scan_jump_table(table, count);
            log::info!("jump table at {table:08x}: {n} entries");
        }

        Ok(instrs)
    }

    /// Given a control flow instruction (e.g. jne or call), enqueue the target of the jump.
    fn control_flow(&mut self, ip: IP, new_instr: &mut Instr) -> anyhow::Result<()> {
        let instr = &new_instr.iced;
        match instr.op0_kind() {
            iced_x86::OpKind::NearBranch16 => {
                let ip = self.block_ip.with_local(instr.near_branch16() as u32);
                self.traverse.queue.enqueue(ip)
            }
            iced_x86::OpKind::NearBranch32 => {
                let ip = self.block_ip.with_local(instr.near_branch32());
                self.traverse.queue.enqueue(ip)
            }
            iced_x86::OpKind::FarBranch16 => {
                let ip = IP::Seg((instr.far_branch_selector(), instr.far_branch16()).into());
                self.traverse.queue.enqueue(ip);
            }
            iced_x86::OpKind::FarBranch32 => {
                let ip = IP::Flat(instr.far_branch32());
                self.traverse.queue.enqueue(ip);
            }
            iced_x86::OpKind::Memory => self.control_flow_indirect(ip, new_instr)?,
            iced_x86::OpKind::Register => {
                // jmp [reg]  for some register
                // log::warn!("{ip} {instr}  ; indirect via register");
            }
            d => anyhow::bail!("unhandled jmp {d:?}"),
        }

        Ok(())
    }

    /// Given a control flow instruction to an indirect memory address like
    ///   jne [some expression]
    /// attempt to enqueue the target of the jump.
    fn control_flow_indirect(&mut self, ip: IP, new_instr: &mut Instr) -> anyhow::Result<()> {
        let instr = &new_instr.iced;
        if self.traverse.module.segment_addressed() {
            log::warn!("{ip} {instr}  ; indirect unimplemented");
            return Ok(());
        }

        if let Some(addr) = is_abs_memory_ref(&instr) {
            // `jmp [addr]` for some constant addr
            if let Some(imp) = self.traverse.iat_refs.get(&addr) {
                // `call [foo@IAT]` means `call foo`, the IAT is the pointer to the real function.
                new_instr.hint = Some(format!("{}::{}_stdcall", imp.dll, imp.func));
            } else {
                if addr as usize + 4 > self.traverse.mem.bytes.len() {
                    anyhow::bail!("jmp to invalid address");
                }
                let target = self.traverse.mem.read::<u32>(addr);
                if self.traverse.module.code_memory().contains(&target) {
                    self.traverse.queue.add_candidate(target);
                }
                log::warn!("{ip} {instr}  ; indirect via memory");
            }
        } else if let Some((table, index)) = is_jump_table_ref(&instr) {
            let count = self.index_bounds.get(&index.full_register()).copied();
            self.found_tables.push((table, count));
        } else {
            log::warn!("{ip} {instr}  ; indirect via memory");
        }
        Ok(())
    }
}
