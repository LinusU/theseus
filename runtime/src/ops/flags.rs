use crate::{Context, Flags};

pub fn stc(ctx: &mut Context) {
    ctx.cpu.flags.insert(Flags::CF);
}

pub fn clc(ctx: &mut Context) {
    ctx.cpu.flags.remove(Flags::CF);
}

pub fn std(ctx: &mut Context) {
    ctx.cpu.flags.insert(Flags::DF);
}

pub fn cld(ctx: &mut Context) {
    ctx.cpu.flags.remove(Flags::DF);
}

pub fn sahf(ctx: &mut Context) {
    // This constructs flags from the AH register, but only specific flags.
    let flags = Flags::from_bits(ctx.cpu.regs.get_ah() as u32).unwrap();
    ctx.cpu.flags.set(Flags::SF, flags.contains(Flags::SF));
    ctx.cpu.flags.set(Flags::ZF, flags.contains(Flags::ZF));
    ctx.cpu.flags.set(Flags::AF, flags.contains(Flags::AF));
    ctx.cpu.flags.set(Flags::PF, flags.contains(Flags::PF));
    ctx.cpu.flags.set(Flags::CF, flags.contains(Flags::CF));
}

#[cfg(test)]
mod tests {
    use super::sahf;
    use crate::{BlockCache, CPU, Context, Flags, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x1000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn sahf_restores_auxiliary_carry() {
        let mut ctx = context();
        ctx.cpu.flags.insert(Flags::AF);
        ctx.cpu.regs.set_ah(0);
        sahf(&mut ctx);
        assert!(!ctx.cpu.flags.contains(Flags::AF));

        ctx.cpu.regs.set_ah(0x10);
        sahf(&mut ctx);
        assert!(ctx.cpu.flags.contains(Flags::AF));
    }
}
