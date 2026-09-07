//! The system API exposed by DOS, e.g. opening files.

use runtime::{Context, segofs};

use crate::{IVTEntry, ivt, state};

/// Open file.
#[derive(Default)]
pub struct File {
    buf: Vec<u8>,
    /// Current read/write offset.
    ofs: u32,
}

macro_rules! trace {
    ($func:expr, $($arg:expr),*) => {
        {
            let args: &[String] = &[$(format!("{}={:x?}", std::stringify!($arg), $arg)),*];
            log::info!("dos: {}({})", $func, args.join(" "));
        }
    };
    ($func:expr) => { trace!($func,) }
}

/// int21 is used for system calls, like file i/o and exiting.
pub fn int21(ctx: &mut Context) -> Option<runtime::Cont> {
    let func = ctx.cpu.regs.get_ah();
    match func {
        // write to stdout
        0x09 => {
            let addr = segofs(ctx.cpu.regs.get_ds(), ctx.cpu.regs.get_dx());
            // An out-of-range pointer or a missing '$' terminator prints
            // what is readable instead of panicking the host.
            let buf = ctx.memory.bytes.get(addr as usize..).unwrap_or(&[]);
            let end = buf.iter().position(|&c| c == b'$').unwrap_or(buf.len());
            let buf = &buf[..end];
            //trace!("write_stdout", buf);
            use std::io::Write;
            let _ = std::io::stdout().lock().write_all(buf);
            ctx.cpu.regs.set_al(b'$');
        }
        // write to interrupt table
        0x25 => {
            let int = ctx.cpu.regs.get_al();
            let (seg, ofs) = (ctx.cpu.regs.get_ds(), ctx.cpu.regs.get_dx());
            trace!("write_ivt", int, seg, ofs);
            ivt(&mut ctx.memory)[int as usize] = IVTEntry::from((seg, ofs));
        }
        // get DOS version
        0x30 => {
            trace!("get_dos_version");
            // these values match dosbox
            ctx.cpu.regs.set_ax(5);
            ctx.cpu.regs.set_bx(0xff00);
            ctx.cpu.regs.set_cx(0);
        }
        // terminate and stay resident
        0x31 => {
            let exit_code = ctx.cpu.regs.get_al();
            let size = ctx.cpu.regs.get_dx() as u32 * 0x10;
            trace!("TSR", exit_code, size);
            ctx.cpu.dump();
            let ret = ivt(&mut ctx.memory)[0x22];
            if ret.is_null() {
                log::error!("TSR exiting with no next step");
                std::process::exit(exit_code as i32);
            }
            return Some(ctx.jmpf16(ret.seg, ret.ofs));
        }
        // read from interrupt table
        0x35 => {
            let int = ctx.cpu.regs.get_al();
            trace!("read_ivt", int);
            let IVTEntry { seg, ofs } = ivt(&mut ctx.memory)[int as usize];
            ctx.cpu.regs.set_es(seg);
            ctx.cpu.regs.set_bx(ofs);
        }
        // get an access handle
        0x3d => {
            let access = ctx.cpu.regs.get_al();
            let addr = segofs(ctx.cpu.regs.get_ds(), ctx.cpu.regs.get_dx());
            let name = ctx.memory.read_str(addr);
            trace!("handle_get", access, name);
            if access != 0 {
                log::warn!("TODO: file access {access:x}");
            }
            let mut state = state();
            let Some(buf) = state.read_file(name) else {
                log::warn!("open {name:?}: not found");
                ctx.cpu.regs.set_ax(/* file not found */ 2);
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            };
            if state.files.len() >= 0xff {
                // Handles index `files` directly; once 255 are live a new
                // open must fail rather than wrap the index onto an
                // already-open file.
                ctx.cpu.regs.set_ax(/* too many open files */ 4);
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            }
            let handle = state.files.len();
            state.files.push(File { buf, ofs: 0 });
            ctx.cpu.regs.set_ax(handle as u16);
            ctx.cpu.flags.remove(runtime::Flags::CF);
        }
        // delete an access handle
        0x3e => {
            let handle = ctx.cpu.regs.get_bx();
            trace!("handle_delete", handle);
            let mut state = state();
            if state.files.get_mut(handle as usize).is_none() {
                ctx.cpu.regs.set_ax(6); // invalid handle
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            }
            log::warn!("TODO: close file");
            ctx.cpu.regs.set_al(1); // docs say AX is clobbered, match dosbox for now
            ctx.cpu.flags.remove(runtime::Flags::CF); // no error
        }
        // write to file
        0x40 => {
            use std::io::Write;
            let handle = ctx.cpu.regs.get_bx();
            let len = ctx.cpu.regs.get_cx();
            let addr = segofs(ctx.cpu.regs.get_ds(), ctx.cpu.regs.get_dx());
            trace!("handle_write", handle, len, addr);
            let Some(buf) = (addr as usize)
                .checked_add(len as usize)
                .and_then(|end| ctx.memory.bytes.get(addr as usize..end))
            else {
                ctx.cpu.regs.set_ax(5); // access denied
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            };
            match handle {
                1 => drop(std::io::stdout().lock().write_all(buf)),
                2 => drop(std::io::stderr().lock().write_all(buf)),
                _ => log::error!("TODO: dos write to file {handle} {buf:?}"),
            }
            ctx.cpu.regs.set_ax(len); // bytes written
            ctx.cpu.flags.remove(runtime::Flags::CF); // no error
        }
        // set file's access point
        0x42 => {
            let origin = ctx.cpu.regs.get_al();
            let handle = ctx.cpu.regs.get_bx();
            let offset =
                (((ctx.cpu.regs.get_cx() as u32) << 16) | (ctx.cpu.regs.get_dx() as u32)) as i32;
            trace!("handle_seek", handle, origin, offset);

            let mut state = state();
            let Some(file) = state.files.get_mut(handle as usize) else {
                ctx.cpu.regs.set_ax(6); // invalid handle
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            };
            let offset = match origin {
                0 => offset,
                1 => (file.ofs as i32).wrapping_add(offset),
                2 => (file.buf.len() as i32).wrapping_add(offset),
                _ => {
                    ctx.cpu.regs.set_ax(1); // invalid function
                    ctx.cpu.flags.insert(runtime::Flags::CF);
                    return None;
                }
            } as u32;
            file.ofs = offset;

            ctx.cpu.flags.remove(runtime::Flags::CF); // no error
            ctx.cpu.regs.set_dx((offset >> 16) as u16);
            ctx.cpu.regs.set_ax(offset as u16);
        }
        // file i/o
        0x44 => {
            let cmd = ctx.cpu.regs.get_al();
            match cmd {
                // get handle info
                0 => {
                    let handle = ctx.cpu.regs.get_bx();
                    trace!("handle_info", handle);
                    log::warn!("TODO: dos file i/o get handle info handle={handle:x}");
                    ctx.cpu.flags.remove(runtime::Flags::CF); // no error
                    // dx: file attributes, see book for tables
                    // TODO: for now we hardcode responses
                    if handle == 4 {
                        ctx.cpu.regs.set_ax(0x80e0); // from dosbox
                        ctx.cpu.regs.set_dx(0x80e0); // from dosbox
                    } else {
                        ctx.cpu.regs.set_ax(0x80d3); // from dosbox
                        ctx.cpu.regs.set_dx(0x80d3); // from dosbox
                    }
                }
                _ => log::error!("TODO: dos file i/o cmd={cmd:x}"),
            }
        }
        // release memory block
        0x49 => {
            let seg = ctx.cpu.regs.es;
            trace!("memory_release", seg);
            log::warn!("TODO: release memory seg {seg:x}");
            ctx.cpu.flags.remove(runtime::Flags::CF); // no error
        }
        // resize memory block
        0x4a => {
            let size = ctx.cpu.regs.get_bx(); // in paragraphs
            let seg = ctx.cpu.regs.es;
            trace!("memory_resize", seg, size);

            let state = state();
            if seg != state.psp_segment {
                ctx.cpu.regs.set_ax(9); // invalid memory block address
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            }
            let Some(mcb) = state.program_mcb(&mut ctx.memory) else {
                ctx.cpu.regs.set_ax(9); // invalid memory block address
                ctx.cpu.flags.insert(runtime::Flags::CF);
                return None;
            };
            mcb.size.set(size);

            ctx.cpu.flags.remove(runtime::Flags::CF); // no error
            // leave bx alone, indicating the requested amount was allocated
            // ctx.cpu.regs.set_bx(available);
            // TODO: dosbox sets this, but it's not clear why -- docs say it should return a success code.
            ctx.cpu.regs.set_ax(ctx.cpu.regs.es);
        }
        // load a program for execution
        0x4b => {
            let func = ctx.cpu.regs.get_al();
            let cmd = ctx
                .memory
                .read_str(segofs(ctx.cpu.regs.get_ds(), ctx.cpu.regs.get_dx()));
            let params_addr = segofs(ctx.cpu.regs.get_es(), ctx.cpu.regs.get_bx());
            trace!("load_program", func, cmd, params_addr);

            match func {
                // Load-and-execute / load-only require a child process
                // context this runtime does not implement; refuse the call
                // with a DOS error rather than panicking the host.
                0 | 1 => {
                    log::warn!("DOS exec subfunction {func} for {cmd:?} is not supported");
                    ctx.cpu.regs.set_ax(5); // access denied
                    ctx.cpu.flags.insert(runtime::Flags::CF);
                    return None;
                }
                3 => {
                    // overlay load
                    let seg = ctx.memory.read::<u16>(params_addr);
                    let relo = ctx.memory.read::<u16>(params_addr + 2);

                    let Some(buf) = state().read_file(cmd) else {
                        ctx.cpu.regs.set_ax(2); // file not found
                        ctx.cpu.flags.insert(runtime::Flags::CF);
                        return None;
                    };
                    let Ok(header) = exe::DOS::parse(&buf) else {
                        ctx.cpu.regs.set_ax(11); // invalid format
                        ctx.cpu.flags.insert(runtime::Flags::CF);
                        return None;
                    };
                    let load_addr = segofs(seg, 0);
                    let data = &buf[header.image_offset()..];
                    log::info!("load {cmd:?} load_addr={seg:x}:0 size={:x}", buf.len());
                    let Some(end) = (load_addr as usize).checked_add(data.len()) else {
                        ctx.cpu.regs.set_ax(8); // insufficient memory
                        ctx.cpu.flags.insert(runtime::Flags::CF);
                        return None;
                    };
                    if end > ctx.memory.bytes.len() {
                        ctx.cpu.regs.set_ax(8); // insufficient memory
                        ctx.cpu.flags.insert(runtime::Flags::CF);
                        return None;
                    }
                    ctx.memory.write_bytes(load_addr, data);
                    log::info!("TODO: relocations {relo:x}");

                    ctx.cpu.flags.remove(runtime::Flags::CF); // no error
                    // on success, no register values are known; match dosbox here
                    ctx.cpu.regs.set_ax(0);
                    ctx.cpu.regs.set_dx(0);
                }
                _ => {
                    ctx.cpu.regs.set_ax(1); // invalid function
                    ctx.cpu.flags.insert(runtime::Flags::CF);
                    return None;
                }
            }
        }
        // error exit
        0x4c => {
            let code = ctx.cpu.regs.get_al();
            trace!("exit", code);
            std::process::exit(code as i32);
        }
        // get psp segment
        0x51 => {
            trace!("get_psp");
            ctx.cpu.regs.set_bx(state().psp_segment);
        }
        _ => log::error!("TODO: dos int 21h ({func:02x})"),
    }
    None
}
