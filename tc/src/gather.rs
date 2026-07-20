//! Instruction stream traversal, scanning for basic blocks.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use runtime::SegOfs;

use crate::{AddrInfo, Block, BlockType, Import, Instr, Module, State, memory::Memory};

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
/// where table is a constant, return the address of the table.
fn is_jump_table_ref(instr: &iced_x86::Instruction) -> Option<u32> {
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
    Some(table)
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum IP {
    Flat(u32),
    Seg(SegOfs),
}

impl From<u32> for IP {
    fn from(addr: u32) -> Self {
        IP::Flat(addr)
    }
}

impl From<(u16, u16)> for IP {
    fn from(tuple: (u16, u16)) -> Self {
        IP::Seg(tuple.into())
    }
}

impl IP {
    pub fn seg(&self) -> u16 {
        match *self {
            IP::Flat(_) => unreachable!(),
            IP::Seg(addr) => addr.seg,
        }
    }

    pub fn to_addr(&self) -> u32 {
        match *self {
            IP::Flat(addr) => addr,
            IP::Seg(addr) => addr.abs(),
        }
    }

    pub fn local(&self) -> u32 {
        match *self {
            IP::Flat(ip) => ip,
            IP::Seg(addr) => addr.ofs as u32,
        }
    }

    pub fn with_local(&self, local: u32) -> IP {
        match *self {
            IP::Flat(_) => IP::Flat(local),
            IP::Seg(addr) => IP::Seg((addr.seg, local as u16).into()),
        }
    }
}

impl std::fmt::Display for IP {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            IP::Flat(ip) => write!(f, "{ip:08x}"),
            IP::Seg(addr) => write!(f, "{addr}"),
        }
    }
}

impl std::fmt::Debug for IP {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Clone)]
pub enum EntryPoint {
    Single(IP),
    Range(std::ops::Range<IP>),
}

#[derive(Default)]
pub struct Gather {
    pub scan_immediates: bool,
    pub scan_memory: bool,
    pub scan_prologues: bool,

    pub entry_points: Vec<EntryPoint>,
}

impl Gather {
    pub fn run(self, state: &mut State) -> HashMap<u32, Block> {
        let mut traverse = Traverse::new(state, &self);
        traverse.run();
        traverse.blocks.into_iter().collect()
    }
}

/// Wrap a VecDeque<IP> just so we add some logic every time it's called.
#[derive(Default)]
struct IPQueue(VecDeque<IP>);
impl IPQueue {
    pub fn enqueue(&mut self, ip: IP) {
        // log::info!("enqueue {ip}");
        // let IP::Seg(seg, ofs) = ip else { panic!() };
        // if ofs > 0x8000 {
        //     panic!();
        // }
        self.0.push_back(ip);
    }
    pub fn pop(&mut self) -> Option<IP> {
        self.0.pop_front()
    }
}

struct Traverse<'a> {
    gather: &'a Gather,
    module: &'a Module,
    mem: &'a Memory,
    addr_info: &'a HashMap<u32, AddrInfo>,

    iat_refs: HashMap<u32, &'a Import>,
    queue: IPQueue,
    /// Lower-confidence code addresses (from scans); validated before decoding.
    candidates: VecDeque<u32>,
    /// All addresses ever considered as candidates, to avoid rescanning.
    candidate_seen: HashSet<u32>,
    /// Jump table addresses already scanned.
    seen_tables: HashSet<u32>,
    /// Ranges within code sections that are known to be data (e.g. jump tables).
    data_ranges: Vec<std::ops::Range<u32>>,
    invalid: HashSet<u32>,
    blocks: BTreeMap<u32, Block>,
}

impl<'a> Traverse<'a> {
    fn new(state: &'a mut State, gather: &'a Gather) -> Traverse<'a> {
        Traverse {
            gather,
            module: &state.module,
            mem: &state.mem,
            addr_info: &state.addr_info,

            iat_refs: Default::default(),
            queue: IPQueue::default(),
            candidates: VecDeque::new(),
            candidate_seen: HashSet::new(),
            seen_tables: HashSet::new(),
            data_ranges: Vec::new(),
            invalid: HashSet::new(),
            blocks: Default::default(),
        }
    }

    fn run(&mut self) {
        if let Module::Windows(module) = self.module {
            for import in &module.imports {
                if !import.data {
                    let func = format!("{}::{}", import.dll, import.func);
                    self.blocks.insert(
                        import.addr,
                        Block {
                            name: None, // block.name() will use the stdcall name
                            ty: BlockType::Stdcall(func),
                        },
                    );
                }
                self.iat_refs.insert(import.iat_addr, &import);
            }
        }

        for (&addr, info) in self.addr_info.iter() {
            if info.is_extern {
                self.blocks.insert(
                    addr,
                    Block {
                        name: Some(info.name.clone()),
                        ty: BlockType::Extern(addr),
                    },
                );
            }
        }

        self.queue.enqueue(self.module.entry_point());
        for entry_point in self.gather.entry_points.iter() {
            match entry_point {
                EntryPoint::Single(addr) => self.queue.enqueue(*addr),
                EntryPoint::Range(r) => {
                    let mut ip = r.start;
                    while ip < r.end {
                        let Ok(block) = self.decode_one(ip) else {
                            log::warn!("failed to decode range {r:#?} at {}", ip);
                            break;
                        };
                        let BlockType::Instrs(instrs) = &block.ty else {
                            unreachable!();
                        };
                        let next = instrs.last().unwrap().next_ip();
                        self.blocks.insert(ip.to_addr(), block);
                        ip = next;
                    }
                }
            }
        }
        if self.gather.scan_memory {
            self.scan_for_pointers();
        }

        self.drain();

        if self.gather.scan_prologues {
            if self.module.segment_addressed() {
                log::warn!("--scan-prologues not supported for segmented (DOS) modules");
            } else {
                loop {
                    let added = self.scan_gaps_for_prologues();
                    if added == 0 {
                        break;
                    }
                    log::info!("prologue scan: {added} new candidates");
                    self.drain();
                }
            }
        }

        self.report_coverage();
    }

    /// Process the high-confidence queue to exhaustion, interleaved with promoting
    /// scanned (lower-confidence) candidates one at a time.
    fn drain(&mut self) {
        loop {
            while let Some(ip) = self.queue.pop() {
                self.process(ip);
            }
            let Some(addr) = self.candidates.pop_front() else {
                break;
            };
            if self.blocks.contains_key(&addr) || self.invalid.contains(&addr) {
                continue;
            }
            // Never split an existing block based on a mere scan hit; direct
            // control flow that reaches the address will do that instead.
            if self.find_containing_block(addr).is_some() {
                continue;
            }
            if !self.looks_like_code(addr) {
                continue;
            }
            self.queue.enqueue(self.module.local_addr(addr));
        }
    }

    fn process(&mut self, ip: IP) {
        let addr = ip.to_addr();
        if self.blocks.contains_key(&addr) || self.invalid.contains(&addr) {
            return;
        }

        // If this ip is contained within an existing block, it means it is a
        // jmp within some other code.
        // Re-queue the other block for re-parsing after this one so that it can be split.
        if let Some(baddr) = self.find_containing_block(addr) {
            if let Some(block) = self.blocks.remove(&baddr) {
                if let BlockType::Instrs(instrs) = &block.ty {
                    self.queue.enqueue(instrs[0].ip);
                }
            }
        }

        match self.decode_one(ip) {
            Ok(block) => {
                self.blocks.insert(addr, block);
            }
            Err(e) => {
                log::warn!("omitting {ip}: {e}");
                self.invalid.insert(addr);
            }
        }
    }

    /// If addr falls in the middle of an existing block, return that block's address.
    fn find_containing_block(&self, addr: u32) -> Option<u32> {
        let (&baddr, block) = self.blocks.range(..addr).last()?;
        if let BlockType::Instrs(instrs) = &block.ty {
            let range =
                instrs.first().unwrap().ip.to_addr()..instrs.last().unwrap().next_ip().to_addr();
            if range.contains(&addr) {
                return Some(baddr);
            }
        }
        None
    }

    fn add_candidate(&mut self, addr: u32) -> bool {
        if !self.candidate_seen.insert(addr) {
            return false;
        }
        self.candidates.push_back(addr);
        true
    }

    /// Cheap validation for scanned code address candidates: the bytes must
    /// decode as plausible instructions.
    fn looks_like_code(&self, addr: u32) -> bool {
        if !self.module.code_memory().contains(&addr) {
            return false;
        }
        let data = self.mem.slice_all(addr);
        if data.len() < 2 || data[0] == 0 {
            return false;
        }
        let len = data.len().min(64);
        let mut decoder = iced_x86::Decoder::with_ip(
            self.module.bitness(),
            &data[..len],
            addr as u64,
            iced_x86::DecoderOptions::NONE,
        );
        let mut n = 0;
        while decoder.can_decode() {
            let instr = decoder.decode();
            if instr.is_invalid() {
                // Truncated final instruction is fine; garbage is not.
                return n > 0 && decoder.position() + 16 > len;
            }
            n += 1;
            use iced_x86::FlowControl::*;
            match instr.flow_control() {
                Return | UnconditionalBranch | IndirectBranch | Interrupt => break,
                _ => {}
            }
            if n >= 4 {
                break;
            }
        }
        n >= 1
    }

    /// Read a jump table: consecutive pointers into code, stopping at the first
    /// entry that doesn't look like one. Returns the number of entries found.
    fn scan_jump_table(&mut self, table: u32) -> usize {
        if !self.seen_tables.insert(table) {
            return 0;
        }
        let code = self.module.code_memory();
        let mut addr = table;
        let mut count = 0;
        while count < 2048 {
            if addr as usize + 4 > self.mem.bytes.len() {
                break;
            }
            let target = self.mem.read::<u32>(addr);
            if !code.contains(&target) || !self.looks_like_code(target) {
                break;
            }
            self.queue.enqueue(self.module.local_addr(target));
            addr += 4;
            count += 1;
        }
        if count > 0 {
            // Mark the table as data so prologue scanning doesn't look inside it.
            self.data_ranges.push(table..addr);
        }
        count
    }

    /// A `call [addr]` through a non-IAT slot: if the slot statically holds a
    /// code pointer (e.g. a function pointer variable), queue it.
    fn scan_pointer_slot(&mut self, slot: u32) {
        if slot as usize + 4 > self.mem.bytes.len() {
            return;
        }
        let target = self.mem.read::<u32>(slot);
        if self.module.code_memory().contains(&target) {
            self.add_candidate(target);
        }
    }

    fn decode_one(&mut self, block_ip: IP) -> anyhow::Result<Block> {
        // log::info!("decode block {block_ip}");
        let block_addr = block_ip.to_addr();
        if block_addr > self.mem.bytes.len() as u32 {
            anyhow::bail!("ip out of bounds");
        }
        let data = self.mem.slice_all(block_addr);
        if data.len() > 0x10 && data[..0x10].iter().all(|&b| b == 0) {
            anyhow::bail!("block appears zero-filled");
        }

        // Code addresses noticed along the way, processed after the decode loop
        // (decoding borrows self.mem).
        let mut found_tables: Vec<u32> = Vec::new();
        let mut found_slots: Vec<u32> = Vec::new();
        let mut found_imms: Vec<u32> = Vec::new();

        let mut instrs = Vec::new();
        let decoder = iced_x86::Decoder::with_ip(
            self.module.bitness(),
            data,
            block_ip.local() as u64,
            iced_x86::DecoderOptions::NONE,
        );
        for instr in decoder {
            let ip = block_ip.with_local(instr.ip32());
            // log::info!("{ip:08x} {instr}", ip = instr.ip32());
            if self.blocks.contains_key(&ip.to_addr()) {
                // Hit a point covered by another block, e.g. a jump target
                break;
            }

            if instr.mnemonic() == iced_x86::Mnemonic::Out && !self.module.is_dos() {
                anyhow::bail!("'out' instruction in non-DOS code");
            }

            let new_instr = instrs.push_mut(Instr {
                ip,
                iced: instr,
                hint: None,
            });

            if self.gather.scan_immediates {
                for i in 0..instr.op_count() {
                    if instr.op_kind(i) == iced_x86::OpKind::Immediate32 {
                        let imm = instr.immediate32();
                        if self.module.code_memory().contains(&imm) {
                            found_imms.push(imm);
                        }
                    }
                }
            }

            if instr.flow_control() == iced_x86::FlowControl::Next {
                let next_ip = block_ip.with_local(instr.next_ip32());
                let next_bytes = &data[(next_ip.to_addr() - block_addr) as usize..];
                if next_bytes.len() > 0x10 && next_bytes[..0x10].iter().all(|&b| b == 0) {
                    anyhow::bail!("suspicious block of 0");
                }
                continue;
            }
            let ip = block_ip.with_local(instr.ip32());
            use iced_x86::Mnemonic::*;
            match instr.mnemonic() {
                Call | Jmp | Jcxz | Je | Jne | Jb | Js | Jns | Ja | Jae | Jl | Jge | Jecxz | Jg
                | Jle | Jo | Jno | Jp | Jnp | Jbe | Loop | Loope | Loopne => {
                    match instr.op0_kind() {
                        iced_x86::OpKind::NearBranch16 => self
                            .queue
                            .enqueue(block_ip.with_local(instr.near_branch16() as u32)),
                        iced_x86::OpKind::NearBranch32 => self
                            .queue
                            .enqueue(block_ip.with_local(instr.near_branch32())),
                        iced_x86::OpKind::FarBranch16 => {
                            let ip =
                                IP::Seg((instr.far_branch_selector(), instr.far_branch16()).into());
                            self.queue.enqueue(ip);
                        }
                        iced_x86::OpKind::Memory => {
                            if let Some(addr) = is_abs_memory_ref(&instr) {
                                if let Some(imp) = self.iat_refs.get(&addr) {
                                    new_instr.hint =
                                        Some(format!("{}::{}_stdcall", imp.dll, imp.func));
                                    if instr.mnemonic() == iced_x86::Mnemonic::Call {
                                        continue; // don't end block here
                                    }
                                } else {
                                    // A call/jmp through a function pointer variable.
                                    if !self.module.segment_addressed() {
                                        found_slots.push(addr);
                                    }
                                    log::warn!("{ip} {instr}  ; indirect via memory");
                                }
                            } else if !self.module.segment_addressed()
                                && let Some(table) = is_jump_table_ref(&instr)
                            {
                                found_tables.push(table);
                            } else {
                                log::warn!("{ip} {instr}  ; indirect via memory");
                            }
                        }
                        iced_x86::OpKind::Register => {
                            log::warn!("{ip} {instr}  ; indirect via register");
                        }
                        d => anyhow::bail!("unhandled jmp {d:?}"),
                    }
                    if instr.mnemonic() != Jmp {
                        self.queue.enqueue(block_ip.with_local(instr.next_ip32()));
                    }
                }
                Ret | Retf | Iret => {}
                Into => {}        // terminates
                Int1 | Int3 => {} // breakpoint
                Int => {
                    self.queue.enqueue(block_ip.with_local(instr.next_ip32()));
                }
                Syscall | Sysexit | Sysret => anyhow::bail!("syscall not implemented"),
                INVALID => anyhow::bail!("invalid code found"),
                _ => todo!("{ip} control flow {}", instr),
            }
            break;
        }

        for table in found_tables {
            let n = self.scan_jump_table(table);
            log::info!("jump table at {table:08x}: {n} entries");
        }
        for slot in found_slots {
            self.scan_pointer_slot(slot);
        }
        for imm in found_imms {
            self.add_candidate(imm);
        }

        let info = self.addr_info.get(&block_ip.to_addr());
        Ok(Block {
            name: info.map(|info| info.name.clone()),
            ty: BlockType::Instrs(instrs),
        })
    }

    fn scan_for_pointers(&mut self) {
        if self.module.segment_addressed() {
            log::warn!("--scan-memory not supported for segmented (DOS) modules");
            return;
        }
        let code = self.module.code_memory();
        let mut found = Vec::new();
        for mapping in self.mem.mappings.vec() {
            if mapping.addr == 0 || mapping.addr == code.start {
                continue;
            }
            log::info!("scanning mapping {:?}", mapping);
            let data = self.mem.slice(mapping.addr, mapping.size);
            for ofs in 0..data.len().saturating_sub(4) {
                let value =
                    u32::from_le_bytes([data[ofs], data[ofs + 1], data[ofs + 2], data[ofs + 3]]);
                if code.contains(&value) {
                    found.push(value);
                }
            }
        }
        for value in found {
            self.add_candidate(value);
        }
    }

    /// Merged, sorted spans of all discovered blocks plus known data ranges.
    fn covered_ranges(&self) -> Vec<std::ops::Range<u32>> {
        let mut spans: Vec<std::ops::Range<u32>> = Vec::new();
        for block in self.blocks.values() {
            if let BlockType::Instrs(instrs) = &block.ty {
                spans.push(
                    instrs.first().unwrap().ip.to_addr()..instrs.last().unwrap().next_ip().to_addr(),
                );
            }
        }
        spans.extend(self.data_ranges.iter().cloned());
        spans.sort_by_key(|r| r.start);
        let mut merged: Vec<std::ops::Range<u32>> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
                _ => merged.push(span),
            }
        }
        merged
    }

    /// Uncovered ranges within the code section.
    fn gaps(&self) -> Vec<std::ops::Range<u32>> {
        let code = self.module.code_memory();
        let mut gaps = Vec::new();
        let mut pos = code.start;
        for r in self.covered_ranges() {
            if r.end <= code.start {
                continue;
            }
            if r.start >= code.end {
                break;
            }
            if r.start > pos {
                gaps.push(pos..r.start.min(code.end));
            }
            pos = pos.max(r.end);
            if pos >= code.end {
                break;
            }
        }
        if pos < code.end {
            gaps.push(pos..code.end);
        }
        gaps
    }

    /// Search uncovered code ranges for `push ebp; mov ebp, esp` function
    /// prologues, adding them as candidates. Returns how many new ones we found.
    fn scan_gaps_for_prologues(&mut self) -> usize {
        let mut found = Vec::new();
        for gap in self.gaps() {
            let data = self.mem.slice(gap.start, gap.end - gap.start);
            if data.len() < 3 {
                continue;
            }
            for i in 0..data.len() - 2 {
                if data[i] == 0x55 && data[i + 1] == 0x8b && data[i + 2] == 0xec {
                    found.push(gap.start + i as u32);
                }
            }
        }
        let mut added = 0;
        for addr in found {
            if self.add_candidate(addr) {
                added += 1;
            }
        }
        added
    }

    fn report_coverage(&self) {
        let code = self.module.code_memory();
        let total = code.end - code.start;
        if total == 0 {
            return;
        }
        let mut covered = 0u32;
        for r in self.covered_ranges() {
            let start = r.start.max(code.start);
            let end = r.end.min(code.end);
            if start < end {
                covered += end - start;
            }
        }
        let blocks = self
            .blocks
            .values()
            .filter(|b| matches!(b.ty, BlockType::Instrs(_)))
            .count();
        log::info!(
            "code coverage: {covered:#x}/{total:#x} bytes ({:.1}%), {blocks} blocks",
            covered as f64 / total as f64 * 100.0
        );
        let mut gaps = self.gaps();
        gaps.sort_by_key(|r| std::cmp::Reverse(r.end - r.start));
        for gap in gaps.iter().take(5) {
            log::info!(
                "  uncovered: {:08x}..{:08x} ({:#x} bytes)",
                gap.start,
                gap.end,
                gap.end - gap.start
            );
        }
    }
}
