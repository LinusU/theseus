//! Instruction stream traversal, scanning for basic blocks.

mod block;
mod coverage;
mod ip;
mod jump_table;
mod report;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub use ip::IP;
pub use report::Report;

use crate::{AddrInfo, Block, BlockType, Import, Module, State, memory::Memory};

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
    pub fn run(self, state: &mut State) -> (HashMap<u32, Block>, Report) {
        let mut traverse = Traverse::new(state, &self);
        traverse.run();
        let report = traverse.generate_report();
        let blocks = traverse.blocks.into_iter().collect();
        (blocks, report)
    }
}

/// Queue of IPs that we will visit to read more basic blocks.
/// Internally tracks whether we've visited an IP before.
#[derive(Default)]
struct IPQueue {
    /// Queue of IPs we intend to gather code from.
    queue: VecDeque<IP>,

    /// Lower-confidence code addresses (from scans); validated before decoding.
    /// TODO: switch from u32 to IP, share more code.
    candidates: VecDeque<u32>,

    /// Addresses we've visited already and decided don't contain code.
    /// (Addresses that did contain code are inserted in Traverse.blocks.)
    invalid: HashSet<u32>,

    /// Addresses promoted from low-confidence scans. A block discovered only
    /// by a scan whose start is later covered mid-instruction by another
    /// decode is a stale guess and is evicted; a real control-flow edge to
    /// the same address upgrades its provenance so it can never be evicted.
    low_confidence: HashSet<u32>,
}

impl IPQueue {
    pub fn enqueue(&mut self, ip: IP) {
        // log::info!("enqueue {ip}");
        // let IP::Seg(seg, ofs) = ip else { panic!() };
        // if ofs > 0x8000 {
        //     panic!();
        // }
        self.low_confidence.remove(&ip.to_addr());
        self.queue.push_back(ip);
    }

    pub fn pop(&mut self, blocks: &BTreeMap<u32, Block>) -> Option<IP> {
        while let Some(ip) = self.queue.pop_front() {
            let addr = ip.to_addr();
            if blocks.contains_key(&addr) || self.invalid.contains(&addr) {
                continue;
            }
            return Some(ip);
        }
        None
    }

    fn add_candidate(&mut self, addr: u32) {
        self.candidates.push_back(addr);
    }

    /// TODO: switch from u32 to IP, share more code.
    pub fn pop_candidate(&mut self, blocks: &BTreeMap<u32, Block>) -> Option<u32> {
        while let Some(addr) = self.candidates.pop_front() {
            if blocks.contains_key(&addr) || self.invalid.contains(&addr) {
                continue;
            }
            return Some(addr);
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.candidates.is_empty()
    }
}

/// Data managed while traversing the executable code.
struct Traverse<'a> {
    gather: &'a Gather,
    module: &'a Module,
    mem: &'a Memory,
    addr_info: &'a HashMap<u32, AddrInfo>,

    /// Address of IAT entry => function it refers to.
    iat_refs: HashMap<u32, &'a Import>,
    queue: IPQueue,
    /// Jump table addresses already scanned.
    seen_tables: HashSet<u32>,
    /// Ranges within code sections that are known to be data (e.g. jump tables).
    data_ranges: Vec<std::ops::Range<u32>>,
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
            seen_tables: HashSet::new(),
            data_ranges: Vec::new(),
            blocks: Default::default(),
        }
    }

    // True when `addr` points at a non-empty, non-invalid x86 instruction.
    // Used before enqueuing branch targets so we do not create blocks from
    // data that happens to look like a conditional jump.
    fn is_valid_instr_start(&self, addr: u32) -> bool {
        let Some(bytes) = self.mem.bytes.get(addr as usize..) else {
            return false;
        };
        if bytes.is_empty() {
            return false;
        }
        let mut decoder = iced_x86::Decoder::with_ip(
            self.module.bitness(),
            bytes,
            addr as u64,
            iced_x86::DecoderOptions::NONE,
        );
        let instr = decoder.decode();
        !instr.is_invalid() && instr.len() != 0
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
                self.iat_refs.insert(import.iat_addr, import);
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
                        // A range may overlap already-decoded code; skip past
                        // an existing block rather than decoding it again.
                        if let Some(next) = self.blocks.get(&ip.to_addr()).and_then(|b| {
                            if let BlockType::Instrs(instrs) = &b.ty {
                                instrs.last().map(|last| last.next_ip())
                            } else {
                                None
                            }
                        }) {
                            ip = next;
                            continue;
                        }
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
                    self.scan_gaps_for_prologues();
                    if self.queue.is_empty() {
                        break;
                    }
                    log::info!("prologue scan added new candidates");
                    self.drain();
                }
            }
        }

        self.report_coverage();

        self.generate_report();
    }

    /// Process the high-confidence queue to exhaustion, interleaved with promoting
    /// scanned (lower-confidence) candidates one at a time.
    fn drain(&mut self) {
        loop {
            while let Some(ip) = self.queue.pop(&self.blocks) {
                self.process(ip);
            }

            let Some(addr) = self.queue.pop_candidate(&self.blocks) else {
                break;
            };
            // Never split an existing block based on a mere scan hit; direct
            // control flow that reaches the address will do that instead.
            if self.find_containing_block(addr).is_some() {
                continue;
            }
            if !self.looks_like_code(addr) {
                continue;
            }
            // enqueue() clears low_confidence as a side effect of any real
            // edge reaching the address, so mark the candidate after it.
            self.queue.enqueue(self.module.local_addr(addr));
            self.queue.low_confidence.insert(addr);
        }
    }

    fn process(&mut self, ip: IP) {
        let addr = ip.to_addr();

        // If this ip is contained within an existing block, it means it is a
        // jmp within some other code.
        // Re-queue the other block for re-parsing after this one so that it can be split.
        if let Some(baddr) = self.find_containing_block(addr)
            && let Some(block) = self.blocks.remove(&baddr)
            && let BlockType::Instrs(instrs) = &block.ty
        {
            self.queue.enqueue(instrs[0].ip);
        }

        match self.decode_one(ip) {
            Ok(block) => {
                self.evict_stale_candidates(&block);
                self.blocks.insert(addr, block);
            }
            Err(e) => {
                log::warn!("omitting {ip}: {e}");
                self.queue.invalid.insert(addr);
            }
        }
    }

    /// A scan-candidate block whose start is strictly inside another block's
    /// instruction range is a stale guess: the real decode stopped there
    /// because the address is mid-instruction, not an instruction boundary.
    /// Drop it so the branch targets it reported cannot leak into codegen.
    /// Blocks discovered through real control-flow edges are never removed —
    /// a genuine jump into the middle of a block keeps the existing split
    /// behavior in `process`.
    fn evict_stale_candidates(&mut self, block: &Block) {
        if self.queue.low_confidence.is_empty() {
            return;
        }
        let BlockType::Instrs(instrs) = &block.ty else {
            return;
        };
        let Some(first) = instrs.first().unwrap().ip.to_addr().checked_add(1) else {
            return;
        };
        let end = instrs.last().unwrap().next_ip().to_addr();
        if first >= end {
            return;
        }
        let stale: Vec<u32> = self
            .blocks
            .range(first..end)
            .map(|(&addr, _)| addr)
            .filter(|addr| self.queue.low_confidence.contains(addr))
            .collect();
        for addr in stale {
            log::info!("evicting mid-instruction scan guess {addr:08x}");
            self.blocks.remove(&addr);
            self.queue.invalid.insert(addr);
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

    fn looks_like_ascii(&self, data: &[u8]) -> bool {
        for (i, c) in data.iter().copied().enumerate() {
            if c == 0 {
                return i > 8; // minimum string length
            }
            if !(c == 0xa || (0x20..0x7f).contains(&c)) {
                return false;
            }
        }
        false
    }

    /// Check if the data at the given address looks like it might be utf16 text.
    fn looks_like_utf16(&self, data: &[u8]) -> bool {
        for (i, [lo, hi]) in data.as_chunks::<2>().0.iter().copied().enumerate() {
            if hi != 0 {
                return false;
            }
            if lo == 0 {
                return i > 4; // minimum string length
            }
            if lo > 0x7f {
                return false;
            }
        }
        false
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
        if self.looks_like_utf16(&data[..len]) {
            return false;
        }
        if self.looks_like_ascii(&data[..len]) {
            return false;
        }
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

    fn decode_one(&mut self, block_ip: IP) -> anyhow::Result<Block> {
        let instrs = block::BlockDecoder::new(self, block_ip).go()?;
        let info = self.addr_info.get(&block_ip.to_addr());
        Ok(Block {
            name: info.map(|info| info.name.clone()),
            ty: BlockType::Instrs(instrs),
        })
    }

    fn scan_for_pointers(&mut self) {
        if self.module.segment_addressed() {
            log::error!("--scan-memory not supported for segmented (DOS) modules");
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
            for window in data.windows(4) {
                let value = u32::from_le_bytes(window.try_into().unwrap());
                if code.contains(&value) {
                    found.push(value);
                }
            }
        }
        for value in found {
            self.queue.add_candidate(value);
        }
    }

    /// Search uncovered code ranges for `push ebp; mov ebp, esp` function
    /// prologues, adding them as candidates. `mov ebp, esp` has two valid
    /// encodings: MSVC emits `8B EC`, but `89 E5` appears in hand-written
    /// code and other compilers' output.
    fn scan_gaps_for_prologues(&mut self) {
        for gap in self.gaps() {
            let data = self.mem.slice(gap.start, gap.end - gap.start);
            if data.len() < 3 {
                continue;
            }
            for i in 0..data.len() - 2 {
                let prologue = matches!(
                    (&data[i], &data[i + 1], &data[i + 2]),
                    (0x55, 0x8b, 0xec) | (0x55, 0x89, 0xe5)
                );
                if prologue {
                    let addr = gap.start + i as u32;
                    self.queue.add_candidate(addr);
                }
            }
        }
    }

    pub fn generate_report(&self) -> Report {
        use report::*;
        let mut report = Report::default();
        for (&addr, imp) in self.iat_refs.iter() {
            report.iat.push(IATEntry {
                addr,
                func: format!("{dll}!{func}", dll = imp.dll, func = imp.func),
            });
        }
        report
    }
}
