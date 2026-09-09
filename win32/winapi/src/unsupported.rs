use runtime::{Cont, Context};

pub fn stdcall(ctx: &mut Context) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 4;
    ctx.cpu.regs.eax = 0;
    ctx.indirect(return_addr)
}

pub fn stdcall2(ctx: &mut Context) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 12;
    ctx.cpu.regs.eax = 1;
    ctx.indirect(return_addr)
}
