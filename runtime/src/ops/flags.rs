use crate::{Context, Flags};

pub fn stc(ctx: &mut Context) {
    ctx.cpu.flags.insert(Flags::CF);
}

pub fn clc(ctx: &mut Context) {
    ctx.cpu.flags.remove(Flags::CF);
}

pub fn cmc(ctx: &mut Context) {
    let carry = !ctx.cpu.flags.contains(Flags::CF);
    ctx.cpu.flags.set(Flags::CF, carry);
}

pub fn std(ctx: &mut Context) {
    ctx.cpu.flags.insert(Flags::DF);
}

pub fn cld(ctx: &mut Context) {
    ctx.cpu.flags.remove(Flags::DF);
}

pub fn sahf(ctx: &mut Context) {
    // SAHF loads only SF/ZF/AF/PF/CF from AH; reserved bits are ignored.
    let flags = Flags::from_bits_truncate(ctx.cpu.regs.get_ah() as u32);
    ctx.cpu.flags.set(Flags::SF, flags.contains(Flags::SF));
    ctx.cpu.flags.set(Flags::ZF, flags.contains(Flags::ZF));
    ctx.cpu.flags.set(Flags::AF, flags.contains(Flags::AF));
    ctx.cpu.flags.set(Flags::PF, flags.contains(Flags::PF));
    ctx.cpu.flags.set(Flags::CF, flags.contains(Flags::CF));
}

pub fn lahf(ctx: &mut Context) {
    let mut ah = 0x02;
    if ctx.cpu.flags.contains(Flags::SF) {
        ah |= 0x80;
    }
    if ctx.cpu.flags.contains(Flags::ZF) {
        ah |= 0x40;
    }
    if ctx.cpu.flags.contains(Flags::AF) {
        ah |= 0x10;
    }
    if ctx.cpu.flags.contains(Flags::PF) {
        ah |= 0x04;
    }
    if ctx.cpu.flags.contains(Flags::CF) {
        ah |= 0x01;
    }
    ctx.cpu.regs.set_ah(ah);
}

#[cfg(test)]
mod tests {
    use super::{cmc, lahf, sahf};
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
    fn cmc_inverts_carry_without_changing_other_flags() {
        let mut ctx = context();
        ctx.cpu.flags = Flags::CF | Flags::ZF;

        cmc(&mut ctx);
        assert!(!ctx.cpu.flags.contains(Flags::CF));
        assert!(ctx.cpu.flags.contains(Flags::ZF));

        cmc(&mut ctx);
        assert!(ctx.cpu.flags.contains(Flags::CF));
    }

    #[test]
    fn lahf_packs_status_flags_without_changing_al() {
        let mut ctx = context();
        ctx.cpu.flags = Flags::SF | Flags::ZF | Flags::AF | Flags::PF | Flags::CF;
        ctx.cpu.regs.set_ah(0);
        ctx.cpu.regs.set_al(0x34);

        lahf(&mut ctx);

        assert_eq!(ctx.cpu.regs.get_ah(), 0xd7);
        assert_eq!(ctx.cpu.regs.get_al(), 0x34);
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

    #[test]
    fn sahf_ignores_reserved_ah_bits() {
        let mut ctx = context();
        ctx.cpu.flags = Flags::empty();
        // lahf leaves bit 1 set; sahf must ignore it rather than panic.
        ctx.cpu.regs.set_ah(0xd7);
        sahf(&mut ctx);
        assert!(
            ctx.cpu
                .flags
                .contains(Flags::SF | Flags::ZF | Flags::AF | Flags::PF | Flags::CF)
        );
    }
}
