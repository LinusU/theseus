use super::math::sub;
use crate::{
    Context, Flags, Regs,
    memory::{MemRead, MemWrite},
    ops::int::Int,
    port_in, port_out, segofs,
};

#[derive(Debug)]
pub enum Rep {
    REP,
    REPNE,
    REPE,
}

/// A trait for the types of ints that we can run string operations for; concretely, u8/u16/u32.
/// We need Int for sub() (used in scas), MemRead/Write for reading/writing memory.
trait StringInt: Int + MemRead + MemWrite {
    fn from_eax(u: u32) -> Self;
    fn set_eax(&self, regs: &mut Regs);
}

/// Advance a string-operation index register. `addr16` is the
/// instruction's 16-bit address-size attribute (the `67` prefix or a
/// 16-bit code segment), not the machine's real-mode flag: it controls
/// whether only the low 16 bits of the index register participate.
fn advance_index(index: &mut u32, step: u32, addr16: bool, backward: bool) {
    if addr16 {
        let index16 = if backward {
            (*index as u16).wrapping_sub(step as u16)
        } else {
            (*index as u16).wrapping_add(step as u16)
        };
        *index = (*index & 0xffff_0000) | index16 as u32;
    } else if backward {
        *index = index.wrapping_sub(step);
    } else {
        *index = index.wrapping_add(step);
    }
}

impl StringInt for u8 {
    fn from_eax(u: u32) -> Self {
        u as u8
    }
    fn set_eax(&self, regs: &mut Regs) {
        // Note: cannot use `eax = Self as u8` because that clears high bits of eax.
        regs.set_al(*self);
    }
}
impl StringInt for u16 {
    fn from_eax(u: u32) -> Self {
        u as u16
    }
    fn set_eax(&self, regs: &mut Regs) {
        // Note: cannot use `eax = Self as u16` because that clears high bits of eax.
        regs.set_ax(*self);
    }
}
impl StringInt for u32 {
    fn from_eax(u: u32) -> Self {
        u
    }
    fn set_eax(&self, regs: &mut Regs) {
        regs.eax = *self;
    }
}

impl Context {
    /// REP with a 32-bit address-size attribute counts in ECX.
    pub fn rep(&mut self, rep: Rep, func: impl Fn(&mut Context)) {
        self.rep_inner(rep, false, func)
    }

    /// REP with a 16-bit address-size attribute counts in CX.
    pub fn rep16(&mut self, rep: Rep, func: impl Fn(&mut Context)) {
        self.rep_inner(rep, true, func)
    }

    fn rep_inner(&mut self, rep: Rep, cx: bool, func: impl Fn(&mut Context)) {
        while if cx {
            self.cpu.regs.get_cx() != 0
        } else {
            self.cpu.regs.ecx != 0
        } {
            func(self);
            if cx {
                self.cpu.regs.set_cx(self.cpu.regs.get_cx().wrapping_sub(1));
            } else {
                self.cpu.regs.ecx = self.cpu.regs.ecx.wrapping_sub(1);
            }
            match rep {
                Rep::REPE if !self.cpu.flags.contains(Flags::ZF) => break,
                Rep::REPNE if self.cpu.flags.contains(Flags::ZF) => break,
                _ => {}
            }
        }
    }

    pub fn addr(&self, seg: u16, offset: u32) -> u32 {
        if self.cpu.real_mode {
            segofs(seg, offset as u16)
        } else {
            offset
        }
    }

    /// Resolve a string-operation address: real mode is always seg:ofs,
    /// and in flat code a 16-bit address-size attribute truncates the
    /// index to its low 16 bits.
    fn str_addr(&self, seg: u16, offset: u32, addr16: bool) -> u32 {
        if self.cpu.real_mode || !addr16 {
            self.addr(seg, offset)
        } else {
            offset & 0xffff
        }
    }

    fn lods<S: StringInt>(&mut self, addr16: bool) {
        self.memory
            .read::<S>(self.str_addr(self.cpu.regs.ds, self.cpu.regs.esi, addr16))
            .set_eax(&mut self.cpu.regs);
        let step = std::mem::size_of::<S>() as u32;
        advance_index(
            &mut self.cpu.regs.esi,
            step,
            addr16,
            self.cpu.flags.contains(Flags::DF),
        );
    }

    pub fn lodsb(&mut self) {
        self.lods::<u8>(false)
    }
    pub fn lodsw(&mut self) {
        self.lods::<u16>(false)
    }
    pub fn lodsd(&mut self) {
        self.lods::<u32>(false)
    }
    /// 16-bit address-size form: reads DS:SI and advances SI.
    pub fn lodsb_16(&mut self) {
        self.lods::<u8>(true)
    }
    pub fn lodsw_16(&mut self) {
        self.lods::<u16>(true)
    }
    pub fn lodsd_16(&mut self) {
        self.lods::<u32>(true)
    }

    /// INS reads a value from the port in DX into the implicit ES:(E)DI
    /// destination, then advances (E)DI by the operand size.
    fn ins<S: StringInt>(&mut self, addr16: bool) {
        let width = (std::mem::size_of::<S>() * 8) as u32;
        let value = S::from_eax(port_in(self.cpu.regs.get_dx(), width));
        self.memory.write::<S>(
            self.str_addr(self.cpu.regs.es, self.cpu.regs.edi, addr16),
            value,
        );
        advance_index(
            &mut self.cpu.regs.edi,
            std::mem::size_of::<S>() as u32,
            addr16,
            self.cpu.flags.contains(Flags::DF),
        );
    }

    pub fn insb(&mut self) {
        self.ins::<u8>(false)
    }
    pub fn insw(&mut self) {
        self.ins::<u16>(false)
    }
    pub fn insd(&mut self) {
        self.ins::<u32>(false)
    }
    pub fn insb_16(&mut self) {
        self.ins::<u8>(true)
    }
    pub fn insw_16(&mut self) {
        self.ins::<u16>(true)
    }
    pub fn insd_16(&mut self) {
        self.ins::<u32>(true)
    }

    /// OUTS reads a value from the implicit DS:(E)SI source, writes it to the
    /// port in DX, then advances (E)SI by the operand size.
    fn outs<S: StringInt>(&mut self, addr16: bool) {
        let value =
            self.memory
                .read::<S>(self.str_addr(self.cpu.regs.ds, self.cpu.regs.esi, addr16));
        let width = (std::mem::size_of::<S>() * 8) as u32;
        port_out(self.cpu.regs.get_dx(), value.to_u32().unwrap_or(0), width);
        advance_index(
            &mut self.cpu.regs.esi,
            std::mem::size_of::<S>() as u32,
            addr16,
            self.cpu.flags.contains(Flags::DF),
        );
    }

    pub fn outsb(&mut self) {
        self.outs::<u8>(false)
    }
    pub fn outsw(&mut self) {
        self.outs::<u16>(false)
    }
    pub fn outsd(&mut self) {
        self.outs::<u32>(false)
    }
    pub fn outsb_16(&mut self) {
        self.outs::<u8>(true)
    }
    pub fn outsw_16(&mut self) {
        self.outs::<u16>(true)
    }
    pub fn outsd_16(&mut self) {
        self.outs::<u32>(true)
    }

    fn stos<S: StringInt>(&mut self, addr16: bool) {
        self.memory.write::<S>(
            self.str_addr(self.cpu.regs.es, self.cpu.regs.edi, addr16),
            S::from_eax(self.cpu.regs.eax),
        );
        let step = std::mem::size_of::<S>() as u32;
        advance_index(
            &mut self.cpu.regs.edi,
            step,
            addr16,
            self.cpu.flags.contains(Flags::DF),
        );
    }

    pub fn stosb(&mut self) {
        self.stos::<u8>(false)
    }
    pub fn stosw(&mut self) {
        self.stos::<u16>(false)
    }
    pub fn stosd(&mut self) {
        self.stos::<u32>(false)
    }
    /// 16-bit address-size form: writes ES:DI and advances DI.
    pub fn stosb_16(&mut self) {
        self.stos::<u8>(true)
    }
    pub fn stosw_16(&mut self) {
        self.stos::<u16>(true)
    }
    pub fn stosd_16(&mut self) {
        self.stos::<u32>(true)
    }

    fn scas<S: StringInt>(&mut self, addr16: bool) {
        let mem = self
            .memory
            .read::<S>(self.str_addr(self.cpu.regs.es, self.cpu.regs.edi, addr16));
        let _ = sub::<S>(S::from_eax(self.cpu.regs.eax), mem, &mut self.cpu.flags);
        let step = std::mem::size_of::<S>() as u32;
        advance_index(
            &mut self.cpu.regs.edi,
            step,
            addr16,
            self.cpu.flags.contains(Flags::DF),
        );
    }

    pub fn scasb(&mut self) {
        self.scas::<u8>(false)
    }
    pub fn scasw(&mut self) {
        self.scas::<u16>(false)
    }
    pub fn scasd(&mut self) {
        self.scas::<u32>(false)
    }
    /// 16-bit address-size form: compares against ES:DI and advances DI.
    pub fn scasb_16(&mut self) {
        self.scas::<u8>(true)
    }
    pub fn scasw_16(&mut self) {
        self.scas::<u16>(true)
    }
    pub fn scasd_16(&mut self) {
        self.scas::<u32>(true)
    }

    fn cmps<S: StringInt>(&mut self, addr16: bool) {
        let src = self
            .memory
            .read::<S>(self.str_addr(self.cpu.regs.ds, self.cpu.regs.esi, addr16));
        let dst = self
            .memory
            .read::<S>(self.str_addr(self.cpu.regs.es, self.cpu.regs.edi, addr16));
        let _ = sub::<S>(src, dst, &mut self.cpu.flags);
        let step = std::mem::size_of::<S>() as u32;
        let backward = self.cpu.flags.contains(Flags::DF);
        advance_index(&mut self.cpu.regs.esi, step, addr16, backward);
        advance_index(&mut self.cpu.regs.edi, step, addr16, backward);
    }

    pub fn cmpsb(&mut self) {
        self.cmps::<u8>(false)
    }
    pub fn cmpsw(&mut self) {
        self.cmps::<u16>(false)
    }
    pub fn cmpsd(&mut self) {
        self.cmps::<u32>(false)
    }
    /// 16-bit address-size form: compares DS:SI with ES:DI.
    pub fn cmpsb_16(&mut self) {
        self.cmps::<u8>(true)
    }
    pub fn cmpsw_16(&mut self) {
        self.cmps::<u16>(true)
    }
    pub fn cmpsd_16(&mut self) {
        self.cmps::<u32>(true)
    }

    fn movs<S: StringInt>(&mut self, addr16: bool) {
        let src_addr = self.str_addr(self.cpu.regs.ds, self.cpu.regs.esi, addr16);
        let val = self.memory.read::<S>(src_addr);
        let dst_addr = self.str_addr(self.cpu.regs.es, self.cpu.regs.edi, addr16);
        self.memory.write::<S>(dst_addr, val);
        let step = std::mem::size_of::<S>() as u32;
        let backward = self.cpu.flags.contains(Flags::DF);
        advance_index(&mut self.cpu.regs.esi, step, addr16, backward);
        advance_index(&mut self.cpu.regs.edi, step, addr16, backward);
    }

    pub fn movsb(&mut self) {
        self.movs::<u8>(false)
    }
    pub fn movsw(&mut self) {
        self.movs::<u16>(false)
    }
    pub fn movsd(&mut self) {
        self.movs::<u32>(false)
    }
    /// 16-bit address-size form: copies DS:SI to ES:DI and advances SI/DI.
    pub fn movsb_16(&mut self) {
        self.movs::<u8>(true)
    }
    pub fn movsw_16(&mut self) {
        self.movs::<u16>(true)
    }
    pub fn movsd_16(&mut self) {
        self.movs::<u32>(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockCache, CPU, Memory, segofs};

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
    fn lods_16_wraps_si_without_changing_high_bits() {
        let mut ctx = context();
        ctx.cpu.real_mode = true;
        ctx.cpu.regs.ds = 0x1000;
        ctx.cpu.regs.esi = 0xabcd_ffff;
        ctx.memory.write::<u8>(segofs(0x1000, 0xffff), 0x42);

        ctx.lodsb_16();

        assert_eq!(ctx.cpu.regs.get_al(), 0x42);
        assert_eq!(ctx.cpu.regs.esi, 0xabcd_0000);
    }

    #[test]
    fn rep16_uses_cx_without_changing_high_bits() {
        let mut ctx = context();
        ctx.cpu.regs.ecx = 0x0001_0001;

        ctx.rep16(Rep::REP, |_| {});

        assert_eq!(ctx.cpu.regs.ecx, 0x0001_0000);
    }

    #[test]
    fn addr16_string_ops_advance_only_the_low_index_word() {
        let mut ctx = context();
        // A 67-prefixed movsb in flat code uses SI/DI: the address is the
        // low 16 bits and only the low word advances.
        ctx.cpu.regs.esi = 0x1234_0100;
        ctx.cpu.regs.edi = 0x5678_0200;
        ctx.memory.write::<u8>(0x0100, 0x42);

        ctx.movsb_16();

        assert_eq!(ctx.memory.read::<u8>(0x0200), 0x42);
        assert_eq!(ctx.cpu.regs.esi, 0x1234_0101);
        assert_eq!(ctx.cpu.regs.edi, 0x5678_0201);
    }

    #[test]
    fn ins_and_outs_move_values_between_ports_and_memory() {
        let mut ctx = context();
        ctx.cpu.regs.edi = 0x100;
        ctx.cpu.regs.esi = 0x200;
        ctx.cpu.regs.set_dx(0x3f8);
        ctx.memory.write::<u32>(0x200, 0xaabb_ccdd);

        // port_in reads zero on this host; INS still writes and advances EDI.
        ctx.insd();
        assert_eq!(ctx.memory.read::<u32>(0x100), 0);
        assert_eq!(ctx.cpu.regs.edi, 0x104);

        // port_out is a no-op; only (E)SI advances.
        ctx.outsd();
        assert_eq!(ctx.cpu.regs.esi, 0x204);

        // DF reverses the direction.
        ctx.cpu.flags.insert(Flags::DF);
        ctx.insb();
        assert_eq!(ctx.cpu.regs.edi, 0x103);
        ctx.outsb();
        assert_eq!(ctx.cpu.regs.esi, 0x203);
    }

    #[test]
    fn rep_movsb_follows_the_direction_flag() {
        let mut ctx = context();
        ctx.cpu.flags = Flags::DF;
        ctx.cpu.regs.esi = 4;
        ctx.cpu.regs.edi = 8;
        ctx.cpu.regs.ecx = 2;
        ctx.memory.write::<u8>(3, 0x32);
        ctx.memory.write::<u8>(4, 0x31);

        ctx.rep(Rep::REP, Context::movsb);

        assert_eq!(ctx.memory.read::<u8>(8), 0x31);
        assert_eq!(ctx.memory.read::<u8>(7), 0x32);
        assert_eq!(ctx.cpu.regs.esi, 2);
        assert_eq!(ctx.cpu.regs.edi, 6);
        assert_eq!(ctx.cpu.regs.ecx, 0);
    }

    #[test]
    fn movsd_copies_dword_and_advances_esi_edi() {
        let mut ctx = context();
        ctx.cpu.regs.esi = 0x100;
        ctx.cpu.regs.edi = 0x200;
        ctx.memory.write::<u32>(0x100, 0x1234_5678);

        ctx.movsd();

        assert_eq!(ctx.memory.read::<u32>(0x200), 0x1234_5678);
        assert_eq!(ctx.cpu.regs.esi, 0x104);
        assert_eq!(ctx.cpu.regs.edi, 0x204);

        // With DF set, the pointers should decrement.
        ctx.cpu.flags.insert(Flags::DF);
        ctx.movsd();

        assert_eq!(ctx.cpu.regs.esi, 0x100);
        assert_eq!(ctx.cpu.regs.edi, 0x200);
    }

    #[test]
    fn cmpsd_compares_dwords_and_advances_esi_edi() {
        let mut ctx = context();
        ctx.cpu.regs.esi = 0x100;
        ctx.cpu.regs.edi = 0x200;
        ctx.memory.write::<u32>(0x100, 0xdead_beef);
        ctx.memory.write::<u32>(0x200, 0xdead_beef);

        ctx.cmpsd();

        assert!(ctx.cpu.flags.contains(Flags::ZF));
        assert_eq!(ctx.cpu.regs.esi, 0x104);
        assert_eq!(ctx.cpu.regs.edi, 0x204);

        ctx.memory.write::<u32>(0x104, 0xdead_beef);
        ctx.memory.write::<u32>(0x204, 0x1234_5678);
        ctx.cpu.flags.remove(Flags::ZF);

        ctx.cmpsd();

        assert!(!ctx.cpu.flags.contains(Flags::ZF));
        assert_eq!(ctx.cpu.regs.esi, 0x108);
        assert_eq!(ctx.cpu.regs.edi, 0x208);
    }
}
