use runtime::{Cont, Context};

pub fn stdcall(ctx: &mut Context) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 4;
    ctx.cpu.regs.eax = 0;
    ctx.indirect(return_addr)
}
