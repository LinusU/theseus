#![allow(clippy::upper_case_acronyms)]

mod dosapi;
mod timer;
mod vga;

use std::{
    cell::{RefCell, RefMut},
    sync::LazyLock,
};

use host::SingleThreader;
use runtime::{CPU, Context, EXEData, Mappings, Memory, Regs, segofs};
use zerocopy::FromBytes;

use crate::{timer::PIT, vga::VGA};

/// DOSBox-X loads com files into this segment.
pub const DOSBOX_SEG: u16 = 0x813;

#[repr(C)]
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable,
)]
pub struct IVTEntry {
    seg: u16,
    ofs: u16,
}

impl IVTEntry {
    fn is_null(&self) -> bool {
        *self == IVTEntry::from((0, 0))
    }
}

impl From<(u16, u16)> for IVTEntry {
    fn from((seg, ofs): (u16, u16)) -> Self {
        IVTEntry { seg, ofs }
    }
}

impl From<IVTEntry> for (u16, u16) {
    fn from(IVTEntry { seg, ofs }: IVTEntry) -> Self {
        (seg, ofs)
    }
}

pub fn ivt<'a>(mem: &'a mut Memory) -> &'a mut [IVTEntry] {
    <[IVTEntry]>::mut_from_prefix_with_elems(mem.bytes, 0x400)
        .unwrap()
        .0
}

#[repr(C)]
#[derive(zerocopy::IntoBytes, zerocopy::Immutable)]
struct PSP {
    int20: [u8; 2],
    memory_top: u16,
    /// TODO: other fields, pad out for now
    padding1: [u8; 0x28],
    /// environment segment
    environment: u16,
    padding2: [u8; 0x52],
    args_len: u8,
    args: [u8; 0x7f],
}

impl PSP {
    fn new() -> Self {
        PSP {
            int20: [0xcd, 0x20],
            memory_top: 0,
            padding1: [0; 0x28],
            environment: 0,
            padding2: [0; 0x52],
            args_len: 0,
            args: [0; 0x7f],
        }
    }

    fn set_args(&mut self, args: &str) {
        // The PSP command tail is 127 bytes; DOS truncates a longer tail.
        let args = &args.as_bytes()[..args.len().min(0x7e)];
        self.args_len = args.len() as u8;
        self.args[..args.len()].copy_from_slice(args);
        self.args[args.len()] = b'\r';
    }
}

use zerocopy::byteorder::little_endian::U16;

/// Memory Control Block
/// Note that owner/size are unaligned, so we use zerocopy's unaligned U16 not u16.
#[repr(C)]
#[derive(
    Debug, zerocopy::FromBytes, zerocopy::KnownLayout, zerocopy::IntoBytes, zerocopy::Immutable,
)]
struct MCB {
    /// 'M': more in chain, 'Z': last MCB
    typ: u8,
    /// PSP segment of block owner
    owner: U16,
    /// Size of block in paragraphs
    size: U16,
    reserved: [u8; 3],
    owner_name: [u8; 8],
}

pub fn load(exe: &EXEData, command_line: Option<&str>) -> Context {
    host::init();

    let memory_size = 1 << 20;
    let mut memory = Memory::leak_new(memory_size as usize);
    // programs can write to memory address zero to overwrite the IVT
    memory.null_page = false;

    let ivt = ivt(&mut memory);
    // cpu exception handler
    ivt[0] = IVTEntry::from((0xf000, 0xca60)); // from dosbox
    // TSR handler
    // ivt[0x2f] = IVTEntry::from((0xf000, 0xd220)); // from dosbox
    // expanded memory manager
    // This is present if dosbox configured with ems=true, but leave out for now.
    // ivt[0x67] = IVTEntry(0xc401, 0x4); // from dosbox

    let mut state = state();
    state.psp_segment = (exe.image_base >> 4) as u16;

    // TODO: values copied from dosbox
    let mcb = state.program_mcb(&mut memory).unwrap();
    *mcb = MCB {
        typ: b'M',
        owner: U16::new(0x813),
        size: U16::new(0x2cb1),
        reserved: [0; 3],
        owner_name: [0; 8],
    };

    // from dosbox
    let environment = [
        b"COMSPEC=Z:\\COMMAND.COM".as_slice(),
        b"PATH=Z:\\;Z:\\SYSTEM;Z:\\BIN;Z:\\DOS;Z:\\4DOS;Z:\\DEBUG;Z:\\TEXTUTIL",
        b"PROMPT=$P$G",
        b"BLASTER=A220 I7 D1 H5 P330 T6",
        b"",     // list terminator
        b"\x01", // count of following strings
        b"S:\\READ.EXE",
        //b"UNT.COM\0",  // saw this in dosbox memory, but I think isn't needed
    ]
    .join(b"\0".as_slice());
    let environment_segment = 0x7ca; // from dosbox
    memory.write_bytes(segofs(environment_segment, 0), &environment);

    let mut psp = PSP::new();
    psp.memory_top = 0x9fff; // from dosbox
    psp.environment = environment_segment;
    psp.set_args(command_line.unwrap_or(""));
    memory.write(exe.image_base, psp);

    let mut regs = Regs::default();
    let mut mappings = Mappings::default();
    (exe.init)(&mut regs, &mut memory, &mut mappings);

    let mut ctx = Context {
        cpu: CPU::default(),
        thread_handle: 0,
        thread_id: 1,
        memory,
        blocks: exe.blocks,
        cache: Default::default(),
        recent: [Context::return_from_x86; 4],
    };
    ctx.cpu.regs = regs;
    ctx.cpu.real_mode = true;
    ctx
}

pub fn start(ctx: &mut Context, exe: &EXEData) {
    assert!(ctx.cpu.real_mode);

    let mut f = exe.entry_point;
    let mut i = 0;
    loop {
        if i % 0x2000 == 0 {
            check_interrupts(ctx);
        }
        f = f.0(ctx);
        i += 1;
    }
}

pub fn run(exe: &EXEData) {
    let mut ctx = load(exe, None);
    start(&mut ctx, exe);
}

static STATE: LazyLock<SingleThreader<RefCell<State>>> =
    LazyLock::new(|| SingleThreader::new(RefCell::new(State::new())));

pub type ReadFileFn = Box<dyn FnMut(&str) -> Option<Vec<u8>>>;

pub struct State {
    psp_segment: u16,
    pit: PIT,
    vga: Option<VGA>,
    pub read_file: Option<ReadFileFn>,
    files: Vec<Option<dosapi::File>>,
}

impl State {
    fn new() -> Self {
        let mut files: Vec<Option<dosapi::File>> = vec![];
        // Initial files: stdin, stdout, stderr, stdaux, stdprn; file handles are indexes into this vector
        // TODO: the JFT belongs in the PSP I guess.
        files.resize_with(5, || Some(Default::default()));
        State {
            psp_segment: 0,
            pit: PIT::default(),
            vga: None,
            read_file: None,
            files,
        }
    }

    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let read_file = self.read_file.as_mut()?;
        read_file(path)
    }

    fn program_mcb<'a>(&self, mem: &'a mut Memory) -> Option<&'a mut MCB> {
        let start = segofs(self.psp_segment.wrapping_sub(1), 0) as usize;
        let end = start.checked_add(0x10)?;
        let slice = mem.bytes.get_mut(start..end)?;
        MCB::mut_from_bytes(slice).ok()
    }
}

pub fn state() -> RefMut<'static, State> {
    STATE.get().borrow_mut()
}

/// int10 is graphics calls.
fn int10(ctx: &mut Context) {
    let func = ctx.cpu.regs.get_ah();
    match func {
        0x0 => {
            let mode = ctx.cpu.regs.get_al();
            if mode == 0x13 {
                state().vga = Some(VGA::new());
            } else {
                log::error!("TODO: int 10h video mode {mode:x}");
            }
        }
        _ => log::error!("TODO: int 10h (video) call {func:02x}"),
    }
}

/// int 2f: multiplex interrupt for calling into TSR.
/// https://en.wikibooks.org/wiki/First_steps_towards_system_programming_under_MS-DOS_7/Selected_interrupt_handlers#8.03_Interrupt_handlers,_loaded_by_drivers_and_TSR_programs
fn int2f(ctx: &mut Context, next_ip: u16) {
    let entry = ivt(&mut ctx.memory)[0x2f];
    if !entry.is_null() {
        log::info!("calling tsr {:x?}, esp={:x}", entry, ctx.cpu.regs.get_sp());
        // TODO: centralize calling into x86 like in runtime::call32_x86.
        // but interrupt calls are different because they also push/pop flags, hmm.
        let orig_sp = ctx.cpu.regs.get_sp();
        ctx.push16(0); // flags
        ctx.push16(ctx.cpu.regs.get_cs()); // cs
        ctx.push16(next_ip); // ip

        let mut f = ctx.jmpf16(entry.seg, entry.ofs);
        // TODO: loop until return address is popped, like runtime::cpu_loop.
        while ctx.cpu.regs.get_sp() != orig_sp {
            log::info!("loop esp={:x}", ctx.cpu.regs.get_sp());
            // TODO: interrupts are disabled when calling interrupts, but also it appears
            // the dosbox interrupt handlers immediately reenable interrupts when they are invoked.
            // if i % 0x2000 == 0 {
            //     state().check_interrupts(ctx);
            // }
            f = f.0(ctx);
        }
    } else {
        log::error!("TODO: int2f TSR query, ax={:x}", ctx.cpu.regs.get_ax());
    }
}

// TODO: should this take a Cont for next instead?
pub fn int(ctx: &mut Context, next_ip: u16, interrupt: u8) -> runtime::Cont {
    // https://en.wikibooks.org/wiki/First_steps_towards_system_programming_under_MS-DOS_7/Selected_interrupt_handlers
    match interrupt {
        0x10 => int10(ctx),
        0x16 => {
            // TODO: dos int 0x16, keyboard?
            ctx.cpu.flags.insert(runtime::Flags::ZF);
        }
        0x21 => {
            if let Some(next) = dosapi::int21(ctx) {
                return next;
            }
        }
        0x2f => int2f(ctx, next_ip),
        _ => log::error!("TODO: dos int {interrupt:x}h"),
    }
    // TODO: interrupts are fall calls with far returns
    ctx.indirect16((ctx.cpu.regs.cs, next_ip).into())
}

pub fn out(ctx: &mut Context, port: u16, data: u8) {
    match port {
        0x20 => { /* end of interrupt, ignore */ }
        0x40..=0x43 => state().pit.out(ctx, port, data),
        0x3C0..=0x3DF => {
            // The VGA device only exists after a video mode is set;
            // earlier port writes are dropped rather than panicking.
            if let Some(vga) = state().vga.as_mut() {
                vga.io_out(port, data)
            }
        }
        _ => log::error!("TODO: out({:#x}, {:#x})", port, data),
    }
}

pub fn dump_com(ctx: &mut Context) -> &[u8] {
    let addr = segofs(DOSBOX_SEG, 0x100);
    let data = ctx.memory.bytes.get(addr as usize..).unwrap_or(&[]);
    let end = data
        .iter()
        .rposition(|&x| x != 0)
        .map(|end| end + 1)
        .unwrap_or(0);
    data.get(..end).unwrap_or(&[])
}

/// Poll the devices that feed guest interrupts and the display.
///
/// The `state()` RefCell must not stay borrowed while the timer handler or
/// the host poll run: the handler is guest code that can call back into
/// `state()` (port I/O, int21), and a nested borrow would panic.
fn check_interrupts(ctx: &mut Context) {
    let handler: (u16, u16) = ivt(&mut ctx.memory)[8].into();
    let ticks = state().pit.due_ticks(host::host().time());
    for _ in 0..ticks {
        timer::call_timer(ctx, handler);
    }
    if let Some(mut vga) = state().vga.take() {
        vga.update_screen(ctx);
        state().vga = Some(vga);
    }
}

/// Handler for address 0; what happens if the entry point returns.
pub fn exit(_ctx: &mut Context) -> runtime::Cont {
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Cont, Context, Memory};

    fn timer_handler(ctx: &mut Context) -> Cont {
        // The fix being guarded: check_interrupts must not hold the State
        // RefCell borrow while the timer handler runs guest code. If it did,
        // this reentrant state() call would panic.
        let _ = state().pit;
        ctx.iret16()
    }

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 1,
            memory: Memory::leak_new(0x4000),
            blocks: &[(0x1100, timer_handler)],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn check_interrupts_runs_timer_handler_that_touches_state() {
        host::init();

        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.esp = 0x3000;
        ctx.cpu.regs.cs = 0x100;

        let ivt = ivt(&mut ctx.memory);
        ivt[8] = IVTEntry::from((0x100, 0x100));

        state().pit.fire_next();

        // This panicked before the RefCell borrow was narrowed: the handler
        // itself calls state(), which would nest with check_interrupts' borrow.
        check_interrupts(&mut ctx);
    }
}
