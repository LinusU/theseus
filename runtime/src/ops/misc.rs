use crate::{Context, Flags, segofs};

impl Context {
    pub fn push32(&mut self, x: u32) {
        self.cpu.regs.esp -= 4;
        self.memory.write::<u32>(self.cpu.regs.esp, x);
    }

    pub fn push16(&mut self, x: u16) {
        let sp = self.cpu.regs.get_sp().wrapping_sub(2);
        self.cpu.regs.set_sp(sp);
        self.memory.write::<u16>(segofs(self.cpu.regs.ss, sp), x);
    }

    pub fn pop32(&mut self) -> u32 {
        let x = self.memory.read::<u32>(self.cpu.regs.esp);
        self.cpu.regs.esp += 4;
        x
    }

    pub fn pop16(&mut self) -> u16 {
        let sp = self.cpu.regs.get_sp();
        let x = self.memory.read::<u16>(segofs(self.cpu.regs.ss, sp));
        self.cpu.regs.set_sp(sp.wrapping_add(2));
        x
    }

    pub fn pushad(&mut self) {
        if self.cpu.real_mode {
            let sp = self.cpu.regs.get_sp();
            self.push16(self.cpu.regs.get_ax());
            self.push16(self.cpu.regs.get_cx());
            self.push16(self.cpu.regs.get_dx());
            self.push16(self.cpu.regs.get_bx());
            self.push16(sp);
            self.push16(self.cpu.regs.get_bp());
            self.push16(self.cpu.regs.get_si());
            self.push16(self.cpu.regs.get_di());
        } else {
            let esp = self.cpu.regs.esp;
            self.push32(self.cpu.regs.eax);
            self.push32(self.cpu.regs.ecx);
            self.push32(self.cpu.regs.edx);
            self.push32(self.cpu.regs.ebx);
            self.push32(esp);
            self.push32(self.cpu.regs.ebp);
            self.push32(self.cpu.regs.esi);
            self.push32(self.cpu.regs.edi);
        }
    }

    pub fn popad(&mut self) {
        if self.cpu.real_mode {
            let di = self.pop16();
            let si = self.pop16();
            let bp = self.pop16();
            self.pop16();
            let bx = self.pop16();
            let dx = self.pop16();
            let cx = self.pop16();
            let ax = self.pop16();
            self.cpu.regs.set_di(di);
            self.cpu.regs.set_si(si);
            self.cpu.regs.set_bp(bp);
            self.cpu.regs.set_bx(bx);
            self.cpu.regs.set_dx(dx);
            self.cpu.regs.set_cx(cx);
            self.cpu.regs.set_ax(ax);
        } else {
            self.cpu.regs.edi = self.pop32();
            self.cpu.regs.esi = self.pop32();
            self.cpu.regs.ebp = self.pop32();
            self.pop32();
            self.cpu.regs.ebx = self.pop32();
            self.cpu.regs.edx = self.pop32();
            self.cpu.regs.ecx = self.pop32();
            self.cpu.regs.eax = self.pop32();
        }
    }

    pub fn enter(&mut self, bytes: u16, nesting: u8) {
        assert_eq!(nesting, 0);
        if self.cpu.real_mode {
            self.push16(self.cpu.regs.get_bp());
            self.cpu.regs.set_bp(self.cpu.regs.get_sp());
            self.cpu
                .regs
                .set_sp(self.cpu.regs.get_sp().wrapping_sub(bytes));
        } else {
            self.push32(self.cpu.regs.ebp);
            self.cpu.regs.ebp = self.cpu.regs.esp;
            self.cpu.regs.esp -= bytes as u32;
        }
    }

    pub fn leave(self: &mut Context) {
        if self.cpu.real_mode {
            self.cpu.regs.set_sp(self.cpu.regs.get_bp());
            let bp = self.pop16();
            self.cpu.regs.set_bp(bp);
        } else {
            self.cpu.regs.esp = self.cpu.regs.ebp;
            self.cpu.regs.ebp = self.pop32();
        }
    }

    pub fn sete(self: &Context) -> u8 {
        self.cpu.flags.contains(Flags::ZF) as u8
    }

    pub fn setne(self: &Context) -> u8 {
        !self.cpu.flags.contains(Flags::ZF) as u8
    }

    pub fn setg(self: &Context) -> u8 {
        (!self.cpu.flags.contains(Flags::ZF)
            && self.cpu.flags.contains(Flags::SF) == self.cpu.flags.contains(Flags::OF))
            as u8
    }

    pub fn setge(self: &Context) -> u8 {
        (self.cpu.flags.contains(Flags::SF) == self.cpu.flags.contains(Flags::OF)) as u8
    }

    pub fn setl(self: &Context) -> u8 {
        (self.cpu.flags.contains(Flags::SF) != self.cpu.flags.contains(Flags::OF)) as u8
    }

    pub fn setle(self: &Context) -> u8 {
        (self.cpu.flags.contains(Flags::ZF)
            || self.cpu.flags.contains(Flags::SF) != self.cpu.flags.contains(Flags::OF))
            as u8
    }

    pub fn seta(self: &Context) -> u8 {
        (!self.cpu.flags.contains(Flags::CF) && !self.cpu.flags.contains(Flags::ZF)) as u8
    }

    pub fn setae(self: &Context) -> u8 {
        !self.cpu.flags.contains(Flags::CF) as u8
    }

    pub fn setb(self: &Context) -> u8 {
        self.cpu.flags.contains(Flags::CF) as u8
    }

    pub fn setbe(self: &Context) -> u8 {
        (self.cpu.flags.contains(Flags::CF) || self.cpu.flags.contains(Flags::ZF)) as u8
    }

    pub fn seto(self: &Context) -> u8 {
        self.cpu.flags.contains(Flags::OF) as u8
    }

    pub fn setno(self: &Context) -> u8 {
        (!self.cpu.flags.contains(Flags::OF)) as u8
    }

    pub fn sets(self: &Context) -> u8 {
        self.cpu.flags.contains(Flags::SF) as u8
    }

    pub fn setns(self: &Context) -> u8 {
        (!self.cpu.flags.contains(Flags::SF)) as u8
    }

    pub fn setp(self: &Context) -> u8 {
        self.cpu.flags.contains(Flags::PF) as u8
    }

    pub fn setnp(self: &Context) -> u8 {
        (!self.cpu.flags.contains(Flags::PF)) as u8
    }

    pub fn sti(&mut self) {
        // TODO: self.cpu.flags.insert(Flags::IF);
    }

    pub fn cli(&mut self) {
        // TODO: self.cpu.flags.remove(Flags::IF);
    }

    pub fn xlat(&mut self) {
        let offset = if self.cpu.real_mode {
            self.cpu.regs.get_bx() as u32
        } else {
            self.cpu.regs.ebx
        };
        let offset = offset.wrapping_add(self.cpu.regs.get_al() as u32);
        let value = self.memory.read::<u8>(self.addr(self.cpu.regs.ds, offset));
        self.cpu.regs.set_al(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockCache, CPU, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x20_000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn real_mode_stack_preserves_esp_high_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ss = 0x1000;
        ctx.cpu.regs.esp = 0xabcd_0000;

        ctx.push16(0x1234);
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_fffe);
        assert_eq!(ctx.pop16(), 0x1234);
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_0000);
    }

    #[test]
    fn real_mode_enter_leave_use_bp_and_sp() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ss = 0x1000;
        ctx.cpu.regs.esp = 0xabcd_0100;
        ctx.cpu.regs.ebp = 0xfeed_0200;

        ctx.enter(4, 0);
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_00fa);
        assert_eq!(ctx.cpu.regs.ebp, 0xfeed_00fe);

        ctx.leave();
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_0100);
        assert_eq!(ctx.cpu.regs.ebp, 0xfeed_0200);
    }

    #[test]
    fn real_mode_pushad_popad_use_16_bit_registers() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ss = 0x1000;
        ctx.cpu.regs.esp = 0xabcd_0100;
        ctx.cpu.regs.eax = 0x1111_0001;
        ctx.cpu.regs.ecx = 0x2222_0002;
        ctx.cpu.regs.edx = 0x3333_0003;
        ctx.cpu.regs.ebx = 0x4444_0004;
        ctx.cpu.regs.ebp = 0x5555_0005;
        ctx.cpu.regs.esi = 0x6666_0006;
        ctx.cpu.regs.edi = 0x7777_0007;

        ctx.pushad();
        ctx.cpu.regs.eax = 0;
        ctx.cpu.regs.ecx = 0;
        ctx.cpu.regs.edx = 0;
        ctx.cpu.regs.ebx = 0;
        ctx.cpu.regs.ebp = 0;
        ctx.cpu.regs.esi = 0;
        ctx.cpu.regs.edi = 0;
        ctx.popad();

        assert_eq!(ctx.cpu.regs.esp, 0xabcd_0100);
        assert_eq!(ctx.cpu.regs.eax, 0x0000_0001);
        assert_eq!(ctx.cpu.regs.ecx, 0x0000_0002);
        assert_eq!(ctx.cpu.regs.edx, 0x0000_0003);
        assert_eq!(ctx.cpu.regs.ebx, 0x0000_0004);
        assert_eq!(ctx.cpu.regs.ebp, 0x0000_0005);
        assert_eq!(ctx.cpu.regs.esi, 0x0000_0006);
        assert_eq!(ctx.cpu.regs.edi, 0x0000_0007);
    }
}
