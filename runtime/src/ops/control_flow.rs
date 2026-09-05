use crate::{Cont, ContFn, Context, Flags, RETURN_FROM_X86_ADDR32, segofs};

impl Context {
    pub fn call32(&mut self, ret: u32, addr: Cont) -> Cont {
        self.push32(ret);
        addr
    }

    pub fn call16(&mut self, ret: u16, addr: Cont) -> Cont {
        self.push16(ret);
        addr
    }

    pub fn callf16(&mut self, ret: u16, seg: u16, addr: Cont) -> Cont {
        self.push16(self.cpu.regs.cs);
        self.push16(ret);
        self.cpu.regs.cs = seg;
        addr
    }

    pub fn callf32(&mut self, ret: u32, seg: u16, addr: Cont) -> Cont {
        self.push32(self.cpu.regs.cs as u32);
        self.push32(ret);
        self.cpu.regs.cs = seg;
        addr
    }

    /// Call a ContFn (builtin implementation) synchronously, without returning a continuation.
    pub fn call_builtin(&mut self, from: u32, func: ContFn) {
        // Because ContFn is stdcall it expects to pop a return address off the stack.
        // Ensure it is valid, though we ignore it.
        self.push32(RETURN_FROM_X86_ADDR32);
        self.cpu.regs.eip_context = from;
        func(self); // pops the above return address
    }

    pub fn je(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::ZF) {
            return x;
        }
        from
    }

    pub fn jne(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::ZF) {
            return x;
        }
        from
    }

    pub fn jb(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::CF) {
            return x;
        }
        from
    }

    pub fn js(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::SF) {
            return x;
        }
        from
    }

    pub fn jns(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::SF) {
            return x;
        }
        from
    }

    pub fn jo(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::OF) {
            return x;
        }
        from
    }

    pub fn jno(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::OF) {
            return x;
        }
        from
    }

    pub fn jp(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::PF) {
            return x;
        }
        from
    }

    pub fn jnp(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::PF) {
            return x;
        }
        from
    }

    pub fn ja(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::CF) && !self.cpu.flags.contains(Flags::ZF) {
            return x;
        }
        from
    }

    pub fn jae(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::CF) {
            return x;
        }
        from
    }

    pub fn jl(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::SF) != self.cpu.flags.contains(Flags::OF) {
            return x;
        }
        from
    }

    pub fn jge(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::SF) == self.cpu.flags.contains(Flags::OF) {
            return x;
        }
        from
    }

    pub fn jcxz(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.regs.get_cx() == 0 {
            return x;
        }
        from
    }

    pub fn jecxz(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.regs.ecx == 0 {
            return x;
        }
        from
    }

    pub fn jg(&mut self, from: Cont, x: Cont) -> Cont {
        if !self.cpu.flags.contains(Flags::ZF)
            && self.cpu.flags.contains(Flags::SF) == self.cpu.flags.contains(Flags::OF)
        {
            return x;
        }
        from
    }

    pub fn jle(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::ZF)
            || self.cpu.flags.contains(Flags::SF) != self.cpu.flags.contains(Flags::OF)
        {
            return x;
        }
        from
    }

    pub fn jbe(&mut self, from: Cont, x: Cont) -> Cont {
        if self.cpu.flags.contains(Flags::CF) || self.cpu.flags.contains(Flags::ZF) {
            return x;
        }
        from
    }

    pub fn ret32(&mut self, n: u16) -> Cont {
        let ret = self.pop32();
        if self.cpu.real_mode {
            self.cpu.regs.set_sp(self.cpu.regs.get_sp().wrapping_add(n));
        } else {
            self.cpu.regs.esp += n as u32;
        }
        self.indirect(ret)
    }

    pub fn ret16(&mut self, n: u16) -> Cont {
        let ret = self.pop16();
        if self.cpu.real_mode {
            self.cpu.regs.set_sp(self.cpu.regs.get_sp().wrapping_add(n));
        } else {
            self.cpu.regs.esp = self.cpu.regs.esp.wrapping_add(n as u32);
        }
        self.indirect16((self.cpu.regs.cs, ret).into())
    }

    pub fn iret16(&mut self) -> Cont {
        let ip = self.pop16();
        let cs = self.pop16();
        log::info!("iret16 {cs:x} {ip:x}");
        self.cpu.regs.set_cs(cs);
        let flags = self.pop16();
        self.cpu.flags = Flags::from_bits_truncate(flags as u32 & !2);
        self.indirect(segofs(cs, ip))
    }

    pub fn iret32(&mut self) -> Cont {
        let ip = self.pop32();
        let cs = self.pop32();
        self.cpu.regs.set_cs(cs as u16);
        let flags = self.pop32();
        self.cpu.flags = Flags::from_bits_truncate(flags & !2);
        self.indirect(ip)
    }

    pub fn retf16(&mut self, n: u16) -> Cont {
        let ip = self.pop16();
        let cs = self.pop16();
        if self.cpu.real_mode {
            self.cpu.regs.set_sp(self.cpu.regs.get_sp().wrapping_add(n));
        } else {
            self.cpu.regs.esp = self.cpu.regs.esp.wrapping_add(n as u32);
        }
        self.jmpf16(cs, ip)
    }

    pub fn retf32(&mut self, n: u16) -> Cont {
        let ip = self.pop32();
        let cs = self.pop32();
        self.cpu.regs.esp += n as u32;
        self.cpu.regs.set_cs(cs as u16);
        self.indirect(ip)
    }

    pub fn jmpf16(&mut self, seg: u16, ofs: u16) -> Cont {
        self.cpu.regs.set_cs(seg);
        self.indirect(segofs(seg, ofs))
    }

    pub fn loop_(&mut self, from: Cont, x: Cont) -> Cont {
        let count = if self.cpu.real_mode {
            let count = self.cpu.regs.get_cx().wrapping_sub(1);
            self.cpu.regs.set_cx(count);
            count as u32
        } else {
            self.cpu.regs.ecx = self.cpu.regs.ecx.wrapping_sub(1);
            self.cpu.regs.ecx
        };
        if count != 0 { x } else { from }
    }

    pub fn loope(&mut self, from: Cont, x: Cont) -> Cont {
        let count = if self.cpu.real_mode {
            let count = self.cpu.regs.get_cx().wrapping_sub(1);
            self.cpu.regs.set_cx(count);
            count as u32
        } else {
            self.cpu.regs.ecx = self.cpu.regs.ecx.wrapping_sub(1);
            self.cpu.regs.ecx
        };
        if count != 0 && self.cpu.flags.contains(Flags::ZF) {
            x
        } else {
            from
        }
    }

    pub fn loopne(&mut self, from: Cont, x: Cont) -> Cont {
        let count = if self.cpu.real_mode {
            let count = self.cpu.regs.get_cx().wrapping_sub(1);
            self.cpu.regs.set_cx(count);
            count as u32
        } else {
            self.cpu.regs.ecx = self.cpu.regs.ecx.wrapping_sub(1);
            self.cpu.regs.ecx
        };
        if count != 0 && !self.cpu.flags.contains(Flags::ZF) {
            x
        } else {
            from
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockCache, CPU, Memory};

    fn from(_: &mut Context) -> Cont {
        Cont(from)
    }

    fn taken(_: &mut Context) -> Cont {
        Cont(taken)
    }

    static BLOCKS: [(u32, ContFn); 1] = [(0x1234, from)];

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x20_000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [from; 4],
        }
    }

    #[test]
    fn flat_mode_ret16_uses_full_stack_pointer_cleanup() {
        let mut ctx = context();
        ctx.cpu.regs.cs = 0;
        ctx.cpu.regs.esp = 0xfffc;
        ctx.memory.write::<u16>(0xfffc, 0x1234);
        ctx.blocks = &BLOCKS;

        let next = ctx.ret16(4);

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.esp, 0x10002);
    }

    #[test]
    fn flat_mode_retf16_uses_full_stack_pointer_cleanup() {
        let mut ctx = context();
        ctx.cpu.regs.esp = 0xfff8;
        ctx.memory.write::<u16>(0xfff8, 0x1234);
        ctx.memory.write::<u16>(0xfffa, 0);
        ctx.blocks = &BLOCKS;

        let next = ctx.retf16(4);

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.esp, 0x10000);
    }

    #[test]
    fn real_mode_ret32_preserves_esp_high_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ss = 0;
        ctx.cpu.regs.esp = 0xabcd_fffa;
        ctx.memory.write::<u32>(0xfffa, 0x1234);
        ctx.blocks = &BLOCKS;

        let next = ctx.ret32(4);

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_0002);
    }

    #[test]
    fn real_mode_ret16_preserves_esp_high_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.cs = 0;
        ctx.cpu.regs.ss = 0x100;
        ctx.cpu.regs.esp = 0xabcd_0100;
        ctx.memory.write::<u16>(segofs(0x100, 0x100), 0x1234);
        ctx.blocks = &BLOCKS;

        let next = ctx.ret16(4);

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.esp, 0xabcd_0106);
    }

    #[test]
    fn real_mode_loop_uses_cx_without_changing_high_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ecx = 0xabcd_0001;

        let next = ctx.loop_(Cont(from), Cont(taken));

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.ecx, 0xabcd_0000);
    }

    #[test]
    fn real_mode_loopne_uses_cx_for_count_and_zero_flag() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ecx = 0xabcd_0002;

        let next = ctx.loopne(Cont(from), Cont(taken));

        let expected: ContFn = taken;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.ecx, 0xabcd_0001);

        ctx.cpu.flags.insert(Flags::ZF);
        let next = ctx.loopne(Cont(from), Cont(taken));
        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.ecx, 0xabcd_0000);
    }

    #[test]
    fn callf32_pushes_flat_far_return_state() {
        let mut ctx = context();
        ctx.cpu.regs.cs = 0x0023;
        ctx.cpu.regs.esp = 0x100;

        let next = ctx.callf32(0x1234, 0x001b, Cont(from));

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.cs, 0x001b);
        assert_eq!(ctx.cpu.regs.esp, 0xf8);
        assert_eq!(ctx.memory.read::<u32>(0xf8), 0x1234);
        assert_eq!(ctx.memory.read::<u32>(0xfc), 0x0023);
    }

    #[test]
    fn retf32_restores_flat_far_return_state() {
        let mut ctx = context();
        ctx.cpu.regs.esp = 0x100;
        ctx.memory.write::<u32>(0x100, 0x1234);
        ctx.memory.write::<u32>(0x104, 0x001b);
        ctx.blocks = &BLOCKS;

        let next = ctx.retf32(4);

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.cs, 0x001b);
        assert_eq!(ctx.cpu.regs.esp, 0x10c);
    }

    #[test]
    fn iret32_restores_flat_return_state() {
        let mut ctx = context();
        ctx.cpu.regs.esp = 0x100;
        ctx.memory.write::<u32>(0x100, 0x1234);
        ctx.memory.write::<u32>(0x104, 0x001b);
        ctx.memory.write::<u32>(0x108, 0x0202);
        ctx.blocks = &BLOCKS;

        let next = ctx.iret32();

        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.cs, 0x001b);
        assert_eq!(ctx.cpu.regs.esp, 0x10c);
        assert_eq!(ctx.cpu.flags.bits(), Flags::IF.bits());
    }

    #[test]
    fn iret16_ignores_reserved_flags_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.cs = 0;
        ctx.cpu.regs.ss = 0x100;
        ctx.cpu.regs.esp = 0xabcd_0100;
        ctx.memory.write::<u16>(segofs(0x100, 0x100), 0x1234);
        ctx.memory.write::<u16>(segofs(0x100, 0x102), 0);
        ctx.memory.write::<u16>(segofs(0x100, 0x104), 0x0202);
        ctx.blocks = &BLOCKS;

        ctx.iret16();

        assert_eq!(ctx.cpu.regs.cs, 0);
        assert_eq!(ctx.cpu.flags.bits(), Flags::IF.bits());
    }

    #[test]
    fn overflow_and_parity_jumps_follow_flags() {
        let mut ctx = context();
        let from = Cont(from);
        let taken = Cont(taken);

        ctx.cpu.flags = Flags::OF | Flags::PF;
        assert_eq!(ctx.jo(from, taken).0 as usize, taken.0 as usize);
        assert_eq!(ctx.jno(from, taken).0 as usize, from.0 as usize);
        assert_eq!(ctx.jp(from, taken).0 as usize, taken.0 as usize);
        assert_eq!(ctx.jnp(from, taken).0 as usize, from.0 as usize);

        ctx.cpu.flags = Flags::empty();
        assert_eq!(ctx.jo(from, taken).0 as usize, from.0 as usize);
        assert_eq!(ctx.jno(from, taken).0 as usize, taken.0 as usize);
        assert_eq!(ctx.jp(from, taken).0 as usize, from.0 as usize);
        assert_eq!(ctx.jnp(from, taken).0 as usize, taken.0 as usize);
    }

    #[test]
    fn loope_requires_nonzero_count_and_zero_flag() {
        let mut ctx = context();
        ctx.cpu.regs.ecx = 2;
        ctx.cpu.flags = Flags::ZF;

        let next = ctx.loope(Cont(from), Cont(taken));
        let expected: ContFn = taken;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.ecx, 1);

        ctx.cpu.flags = Flags::empty();
        let next = ctx.loope(Cont(from), Cont(taken));
        let expected: ContFn = from;
        assert!(std::ptr::fn_addr_eq(next.0, expected));
        assert_eq!(ctx.cpu.regs.ecx, 0);
    }
}
