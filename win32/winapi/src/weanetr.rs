use runtime::{Cont, Context};

fn thiscall_return(ctx: &mut Context, stack_args: u16) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 4 + (stack_args as u32) * 4;
    ctx.cpu.regs.eax = 0;
    ctx.indirect(return_addr)
}

// The Populous executable links against the weanetr.dll MLDPlay class using
// __thiscall, where the 'this' pointer is passed in ECX and any stack
// arguments are callee-cleaned. The following thunks mirror the correct
// stack cleanup for each arg-count used by the MLDPlay methods.

pub fn thunk_0_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 0)
}

pub fn thunk_1_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 1)
}

pub fn thunk_2_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 2)
}

pub fn thunk_3_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 3)
}

pub fn thunk_4_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 4)
}

pub fn thunk_5_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 5)
}

pub fn thunk_6_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 6)
}

pub fn thunk_7_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 7)
}

pub fn thunk_8_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 8)
}
