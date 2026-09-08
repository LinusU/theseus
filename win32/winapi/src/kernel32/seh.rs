//! Structured exception handling: RaiseException and RtlUnwind.
//!
//! Windows keeps a per-thread chain of EXCEPTION_REGISTRATION records
//! (`{next, handler}`) headed at fs:[0]. RaiseException walks it, calling each
//! handler until one takes the exception; RtlUnwind calls the handlers of the
//! frames being abandoned so they can run cleanup, and pops them. The handlers
//! are program code: the Microsoft C runtime's `__CxxFrameHandler` (C++
//! try/catch, which `throw` implements by calling RaiseException with code
//! 0xe06d7363) and `_except_handler3` (`__try/__except`). Getting these two
//! functions right is all it takes for the program's own exception handling to
//! work.
//!
//! A handler that finds a matching `catch` never returns: it unwinds (calling
//! RtlUnwind), then resets esp and jumps into the middle of the catching
//! function, and the rest of the program runs from there, inside the
//! `call32_x86` that invoked the handler. That costs a few Rust stack frames per
//! caught exception, which is fine for the error-path use these programs make
//! of exceptions but would not survive thousands of them.

use runtime::Context;

use crate::kernel32;

const EXCEPTION_UNWINDING: u32 = 2;
const EXCEPTION_EXIT_UNWIND: u32 = 4;
const STATUS_UNWIND: u32 = 0xc000_0027;
const MSVC_CPP_EXCEPTION: u32 = 0xe06d_7363;
const CHAIN_END: u32 = 0xffff_ffff;

// EXCEPTION_DISPOSITION
const EXCEPTION_CONTINUE_EXECUTION: u32 = 0;
const EXCEPTION_CONTINUE_SEARCH: u32 = 1;

/// sizeof(EXCEPTION_RECORD): code, flags, record, address, count, info[15].
const RECORD_SIZE: u32 = 80;
/// sizeof(CONTEXT) on x86.
const CONTEXT_SIZE: u32 = 0x2cc;
/// Room for the DISPATCHER_CONTEXT the handlers are passed (unused by them).
const DISPATCHER_SIZE: u32 = 8;

fn exception_list(ctx: &Context) -> u32 {
    ctx.memory.read::<u32>(ctx.cpu.regs.fs_base)
}

fn set_exception_list(ctx: &mut Context, frame: u32) {
    let fs_base = ctx.cpu.regs.fs_base;
    ctx.memory.write::<u32>(fs_base, frame);
}

/// Fill in a CONTEXT from the current registers, as a handler would see it.
fn write_context(ctx: &mut Context, addr: u32, eip: u32, esp: u32) {
    ctx.memory[addr..addr + CONTEXT_SIZE].fill(0);
    let regs = &ctx.cpu.regs;
    let fields = [
        (0x00, 0x1_0007), // ContextFlags: CONTEXT_FULL
        (0x9c, regs.edi),
        (0xa0, regs.esi),
        (0xa4, regs.ebx),
        (0xa8, regs.edx),
        (0xac, regs.ecx),
        (0xb0, regs.eax),
        (0xb4, regs.ebp),
        (0xb8, eip),
        (0xbc, 0x1b), // SegCs
        (0xc0, ctx.cpu.flags.bits()),
        (0xc4, esp),
        (0xc8, 0x23), // SegSs
    ];
    for (offset, value) in fields {
        ctx.memory.write::<u32>(addr + offset, value);
    }
}

fn alloc(ctx: &mut Context, size: u32) -> u32 {
    let addr = kernel32::lock().process_heap.alloc(&mut ctx.memory, size);
    ctx.memory[addr..addr + size].fill(0);
    addr
}

fn free(ctx: &mut Context, addr: u32) {
    kernel32::lock().process_heap.free(&mut ctx.memory, addr);
}

/// The mangled type name of a thrown C++ object, from the throw's arguments
/// (magic, object, ThrowInfo). Follows the MSVC RTTI structures, which on x86
/// hold absolute pointers.
fn cpp_exception_type(ctx: &Context, record: u32) -> Option<String> {
    let read = |addr: u32| -> Option<u32> { (addr >= 0x1000).then(|| ctx.memory.read::<u32>(addr)) };
    let throw_info = read(record + 0x14 + 8)?; // ExceptionInformation[2]
    let catchable_types = read(throw_info + 12)?; // ThrowInfo.pCatchableTypeArray
    let first = read(catchable_types + 4)?; // CatchableTypeArray.arrayOfCatchableTypes[0]
    let type_descriptor = read(first + 4)?; // CatchableType.pType
    if type_descriptor < 0x1000 {
        return None;
    }
    Some(ctx.memory.read_str(type_descriptor + 8).to_string()) // TypeDescriptor.name
}

fn describe(ctx: &Context, record: u32) -> String {
    let code = ctx.memory.read::<u32>(record);
    let nparams = ctx.memory.read::<u32>(record + 0x10);
    if code == MSVC_CPP_EXCEPTION && nparams == 3 {
        if let Some(name) = cpp_exception_type(ctx, record) {
            return format!("C++ exception of type {name}");
        }
    }
    format!("exception {code:#x}")
}

/// The address the current API call will return to. The stdcall wrapper leaves
/// the return address at esp; for a direct call from generated code it is the
/// RETURN_FROM_X86 sentinel and the real one is in eip_context.
fn caller(ctx: &Context) -> u32 {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    if return_addr == runtime::RETURN_FROM_X86_ADDR32 {
        ctx.cpu.regs.eip_context
    } else {
        return_addr
    }
}

/// Call an exception handler: `EXCEPTION_DISPOSITION handler(record, frame,
/// context, dispatcher)`, cdecl.
fn call_handler(ctx: &mut Context, handler: u32, record: u32, frame: u32, context: u32) -> u32 {
    let handler = ctx.indirect(handler);
    ctx.call32_x86(handler, vec![record, frame, context, context + CONTEXT_SIZE]);
    ctx.cpu.regs.esp += 16; // cdecl: the caller pops
    ctx.cpu.regs.eax
}

#[win32_derive::dllexport]
pub fn RaiseException(
    ctx: &mut Context,
    dwExceptionCode: u32,
    dwExceptionFlags: u32,
    nNumberOfArguments: u32,
    lpArguments: u32,
) {
    let return_addr = caller(ctx);
    let nparams = nNumberOfArguments.min(15);

    let scratch = alloc(ctx, RECORD_SIZE + CONTEXT_SIZE + DISPATCHER_SIZE);
    let record = scratch;
    let context = record + RECORD_SIZE;
    for (offset, value) in [
        (0, dwExceptionCode),
        (4, dwExceptionFlags),
        (0xc, return_addr),
        (0x10, nparams),
    ] {
        ctx.memory.write::<u32>(record + offset, value);
    }
    for i in 0..nparams {
        let value = ctx.memory.read::<u32>(lpArguments + i * 4);
        ctx.memory.write::<u32>(record + 0x14 + i * 4, value);
    }
    let esp = ctx.cpu.regs.esp + 20; // as after RaiseException returns
    write_context(ctx, context, return_addr, esp);
    log::info!("RaiseException({dwExceptionCode:#x}): {}", describe(ctx, record));

    let mut frame = exception_list(ctx);
    while frame != CHAIN_END && frame != 0 {
        let next = ctx.memory.read::<u32>(frame);
        let handler = ctx.memory.read::<u32>(frame + 4);
        match call_handler(ctx, handler, record, frame, context) {
            EXCEPTION_CONTINUE_SEARCH => frame = next,
            EXCEPTION_CONTINUE_EXECUTION => {
                free(ctx, scratch);
                return;
            }
            disposition => panic!("SEH handler returned disposition {disposition}"),
        }
    }
    let what = describe(ctx, record);
    panic!("RaiseException: unhandled {what}");
}

/// Calls the handler of every frame from the top of the chain down to (not
/// including) `TargetFrame`, with EXCEPTION_UNWINDING set so they only clean
/// up, and pops them. Returns with eax set to `ReturnValue`.
///
/// Windows would then continue at `TargetIp` with the registers restored to
/// their values on entry. Every caller passes the address right after its
/// call, and the handlers preserve the callee-saved registers, so returning
/// normally is the same thing.
#[win32_derive::dllexport]
pub fn RtlUnwind(
    ctx: &mut Context,
    TargetFrame: u32,
    TargetIp: u32,
    ExceptionRecord: u32,
    ReturnValue: u32,
) -> u32 {
    let return_addr = caller(ctx);
    log::info!("RtlUnwind(frame={TargetFrame:#x}, ip={TargetIp:#x})");

    let scratch = alloc(ctx, RECORD_SIZE + CONTEXT_SIZE + DISPATCHER_SIZE);
    let record = if ExceptionRecord != 0 {
        ExceptionRecord
    } else {
        ctx.memory.write::<u32>(scratch, STATUS_UNWIND);
        scratch
    };
    let mut flags = ctx.memory.read::<u32>(record + 4) | EXCEPTION_UNWINDING;
    if TargetFrame == 0 {
        flags |= EXCEPTION_EXIT_UNWIND;
    }
    ctx.memory.write::<u32>(record + 4, flags);
    let context = scratch + RECORD_SIZE;
    let esp = ctx.cpu.regs.esp + 20;
    write_context(ctx, context, return_addr, esp);
    ctx.memory.write::<u32>(context + 0xb0, ReturnValue); // Eax

    loop {
        let frame = exception_list(ctx);
        if frame == CHAIN_END || frame == TargetFrame {
            break;
        }
        if frame == 0 {
            log::warn!("RtlUnwind: exception chain ended before target frame {TargetFrame:#x}");
            break;
        }
        let next = ctx.memory.read::<u32>(frame);
        let handler = ctx.memory.read::<u32>(frame + 4);
        let disposition = call_handler(ctx, handler, record, frame, context);
        if disposition != EXCEPTION_CONTINUE_SEARCH {
            log::warn!("RtlUnwind: handler returned disposition {disposition}");
        }
        set_exception_list(ctx, next);
    }

    free(ctx, scratch);
    ReturnValue
}
