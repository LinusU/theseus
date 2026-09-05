//! FPU registers.

use bitflags::bitflags;

#[repr(C, packed)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    zerocopy::FromBytes,
    zerocopy::Immutable,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
)]
pub struct F80 {
    pub significand: u64,
    pub sign_exponent: u16,
}

impl F80 {
    pub fn to_f64(self) -> f64 {
        let significand = self.significand;
        let sign_exponent = self.sign_exponent;
        let sign = if sign_exponent & 0x8000 != 0 {
            -1.0
        } else {
            1.0
        };
        let exponent = sign_exponent & 0x7fff;
        let value = match exponent {
            0..=0x7ffe => {
                let exponent = exponent.max(1) as i32 - 16383;
                let significand = (significand as f64) * 2f64.powi(-63);
                if exponent < -1022 {
                    if exponent < -1074 {
                        0.0
                    } else {
                        significand * 2f64.powi(-1022) * 2f64.powi(exponent + 1022)
                    }
                } else {
                    significand * 2f64.powi(exponent)
                }
            }
            0x7fff if significand == 1 << 63 => f64::INFINITY,
            0x7fff => f64::NAN,
            _ => unreachable!(),
        };
        value.copysign(sign)
    }

    pub fn from_f64(value: f64) -> Self {
        let bits = value.to_bits();
        let sign = ((bits >> 63) as u16) << 15;
        let exponent = ((bits >> 52) & 0x7ff) as u16;
        let fraction = bits & ((1u64 << 52) - 1);
        let (exponent, significand) = match exponent {
            0 if fraction == 0 => (0, 0),
            0 => {
                let highest_bit = 63 - fraction.leading_zeros();
                (15309 + highest_bit as u16, fraction << (63 - highest_bit))
            }
            0x7ff if fraction == 0 => (0x7fff, 1 << 63),
            0x7ff => (0x7fff, (1 << 63) | (fraction << 11)),
            exponent => (exponent + 15360, (1 << 63) | (fraction << 11)),
        };
        Self {
            significand,
            sign_exponent: sign | exponent,
        }
    }
}

bitflags! {
    pub struct Status: u16 {
        const C3 = 1 << 14;
        const C2 = 1 << 10;
        const C1 = 1 << 9;
        const C0 = 1 << 8;
    }
}

pub struct FPU {
    /// FPU ST0 through ST7 registers.
    pub st: [f64; 8],
    /// Index of top of FPU stack; 8 when stack empty.
    pub st_top: usize,
    /// The result of the last fcmp, used to generate status word.
    pub cmp: std::cmp::Ordering,
    /// The x87 condition-code bits produced by the last status-producing
    /// instruction.
    pub condition: u16,
    /// Control word, as managed by fldcw/fnstcw. Precision control is not
    /// modeled, but the rounding-control bits are honored.
    pub control: u16,
}

impl Default for FPU {
    fn default() -> Self {
        Self {
            st: [0.; 8],
            st_top: 8,
            cmp: std::cmp::Ordering::Equal,
            condition: Status::C3.bits(),
            control: 0x037f,
        }
    }
}

impl FPU {
    fn exception(_msg: &str) {
        // TODO: modify state bits etc.
        // At least ignoring these may allow programs to make some progress.
        // See note in https://github.com/joncampbell123/dosbox-x/issues/94 ,
        // "I've seen DOSBox SVN bail out on perfectly good demoscene programs because
        // of [not allowing underflow]."
        // Don't log because anatyda underflows thousands of times, eek.
        // log::warn!("{}", msg);
    }

    /// Get st(0), the current top of the FPU stack.
    pub fn st0(&mut self) -> &mut f64 {
        let offset = self.st_offset(0);
        &mut self.st[offset]
    }

    pub fn push(&mut self, val: f64) {
        if self.st_top == 0 {
            Self::exception("fpu stack overflow");
            return;
        }
        self.st_top -= 1;
        self.st[self.st_top] = val;
    }

    pub fn pop(&mut self) {
        if self.st_top == 8 {
            Self::exception("fpu stack underflow");
            return;
        }
        self.st_top += 1;
    }

    /// FDECSTP moves the TOP pointer down without writing a value;
    /// the new ST(0) reads whatever the physical register last held.
    pub fn dec_top(&mut self) {
        if self.st_top == 0 {
            Self::exception("fpu stack overflow");
            return;
        }
        self.st_top -= 1;
    }

    /// Index in self.st for a given ST0, ST1 etc reg.
    fn st_offset(&self, ofs: usize) -> usize {
        let new = self.st_top + ofs;
        if new >= 8 {
            Self::exception("fpu stack underflow");
            return 7;
        }
        new
    }

    pub fn swap(&mut self, o1: usize, o2: usize) {
        let o1 = self.st_offset(o1);
        let o2 = self.st_offset(o2);
        self.st.swap(o1, o2);
    }

    pub fn get(&self, ofs: usize) -> f64 {
        self.st[self.st_offset(ofs)]
    }

    pub fn set(&mut self, ofs: usize, val: f64) {
        self.st[self.st_offset(ofs)] = val;
    }

    pub fn set_cmp(&mut self, cmp: std::cmp::Ordering) {
        self.cmp = cmp;
        self.condition = match cmp {
            std::cmp::Ordering::Less => Status::C0.bits(),
            std::cmp::Ordering::Equal => Status::C3.bits(),
            std::cmp::Ordering::Greater => 0,
        };
    }

    pub fn compare(&mut self, left: f64, right: f64) {
        let Some(cmp) = left.partial_cmp(&right) else {
            self.cmp = std::cmp::Ordering::Equal;
            self.condition = (Status::C0 | Status::C2 | Status::C3).bits();
            return;
        };
        self.set_cmp(cmp);
    }

    /// FCOMI/FUCOMI-family compares report through EFLAGS rather than the
    /// FPU status word: unordered sets ZF=PF=CF, less sets CF, equal sets ZF.
    pub fn compare_flags(&self, left: f64, right: f64, flags: &mut crate::Flags) {
        let (zf, pf, cf) = match left.partial_cmp(&right) {
            Some(std::cmp::Ordering::Equal) => (true, false, false),
            Some(std::cmp::Ordering::Less) => (false, false, true),
            Some(std::cmp::Ordering::Greater) => (false, false, false),
            None => (true, true, true),
        };
        flags.set(crate::Flags::ZF, zf);
        flags.set(crate::Flags::PF, pf);
        flags.set(crate::Flags::CF, cf);
    }

    pub fn examine(&mut self) {
        if self.st_top == 8 {
            self.condition = (Status::C0 | Status::C3).bits();
            return;
        }

        let value = self.get(0);
        let mut condition = if value.is_nan() {
            Status::C0.bits()
        } else if value.is_infinite() {
            (Status::C2 | Status::C0).bits()
        } else if value == 0.0 {
            Status::C3.bits()
        } else if value.is_subnormal() {
            (Status::C3 | Status::C2).bits()
        } else {
            Status::C2.bits()
        };
        if value.is_sign_negative() {
            condition |= Status::C1.bits();
        }
        self.condition = condition;
    }

    pub fn status(&self) -> u16 {
        // Our status register impl doesn't include st_top so include it here.
        self.condition | (self.st_top as u16 & 0b111) << 11
    }

    fn tag_word(&self) -> u16 {
        let mut tags = 0xffff;
        let active = 8 - self.st_top;
        for logical in 0..active {
            let physical = (self.st_top + logical) & 7;
            let value = self.st[physical];
            let tag = if value == 0.0 {
                1
            } else if value.is_nan() || value.is_infinite() || value.is_subnormal() {
                2
            } else {
                0
            };
            tags = (tags & !(3 << (physical * 2))) | (tag << (physical * 2));
        }
        tags
    }

    pub fn store_env(&mut self, memory: &mut crate::Memory<'_>, addr: u32) {
        memory.write(addr, self.control);
        memory.write(addr.wrapping_add(4), self.status());
        memory.write(addr.wrapping_add(8), self.tag_word());
        memory.write(addr.wrapping_add(12), 0u32);
        memory.write(addr.wrapping_add(16), 0u16);
        memory.write(addr.wrapping_add(18), 0u16);
        memory.write(addr.wrapping_add(20), 0u32);
        memory.write(addr.wrapping_add(24), 0u16);
        self.control |= 0x003f;
    }

    pub fn store_env16(&mut self, memory: &mut crate::Memory<'_>, addr: u32) {
        memory.write(addr, self.control);
        memory.write(addr.wrapping_add(2), self.status());
        memory.write(addr.wrapping_add(4), self.tag_word());
        memory.write(addr.wrapping_add(6), 0u16);
        memory.write(addr.wrapping_add(8), 0u16);
        memory.write(addr.wrapping_add(10), 0u16);
        memory.write(addr.wrapping_add(12), 0u16);
        self.control |= 0x003f;
    }

    fn load_status(&mut self, status: u16, tag: u16) {
        self.condition = status & (Status::C0 | Status::C1 | Status::C2 | Status::C3).bits();
        self.st_top = if tag == u16::MAX {
            8
        } else {
            ((status >> 11) & 0b111) as usize
        };
        self.cmp = if self.condition & Status::C3.bits() != 0 {
            std::cmp::Ordering::Equal
        } else if self.condition & Status::C0.bits() != 0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        };
    }

    pub fn load_env(&mut self, memory: &crate::Memory<'_>, addr: u32) {
        self.control = memory.read(addr);
        let status: u16 = memory.read(addr.wrapping_add(4));
        let tag: u16 = memory.read(addr.wrapping_add(8));
        self.load_status(status, tag);
    }

    pub fn load_env16(&mut self, memory: &crate::Memory<'_>, addr: u32) {
        self.control = memory.read(addr);
        let status: u16 = memory.read(addr.wrapping_add(2));
        let tag: u16 = memory.read(addr.wrapping_add(4));
        self.load_status(status, tag);
    }

    pub fn init(&mut self) {
        self.control = 0x037f;
        self.st_top = 8;
        self.condition = 0;
        self.cmp = std::cmp::Ordering::Greater;
    }

    pub fn save(&mut self, memory: &mut crate::Memory<'_>, addr: u32) {
        self.store_env(memory, addr);
        for (index, value) in self.st.iter().enumerate() {
            memory.write(
                addr.wrapping_add(28 + index as u32 * 10),
                F80::from_f64(*value),
            );
        }
        self.init();
    }

    pub fn save16(&mut self, memory: &mut crate::Memory<'_>, addr: u32) {
        self.store_env16(memory, addr);
        for (index, value) in self.st.iter().enumerate() {
            memory.write(
                addr.wrapping_add(14 + index as u32 * 10),
                F80::from_f64(*value),
            );
        }
        self.init();
    }

    pub fn restore(&mut self, memory: &crate::Memory<'_>, addr: u32) {
        self.load_env(memory, addr);
        for (index, value) in self.st.iter_mut().enumerate() {
            *value = memory
                .read::<F80>(addr.wrapping_add(28 + index as u32 * 10))
                .to_f64();
        }
    }

    pub fn restore16(&mut self, memory: &crate::Memory<'_>, addr: u32) {
        self.load_env16(memory, addr);
        for (index, value) in self.st.iter_mut().enumerate() {
            *value = memory
                .read::<F80>(addr.wrapping_add(14 + index as u32 * 10))
                .to_f64();
        }
    }

    pub fn round(&self, val: f64) -> f64 {
        match (self.control >> 10) & 0b11 {
            0 => val.round_ties_even(),
            1 => val.floor(),
            2 => val.ceil(),
            3 => val.trunc(),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{F80, FPU, Status};

    #[test]
    fn fxam_sets_x87_condition_codes() {
        let condition_mask = (Status::C0 | Status::C1 | Status::C2 | Status::C3).bits();
        for (value, expected) in [
            (-2.0, Status::C1 | Status::C2),
            (0.0, Status::C3),
            (-0.0, Status::C1 | Status::C3),
            (f64::INFINITY, Status::C2 | Status::C0),
            (f64::MIN_POSITIVE / 2.0, Status::C3 | Status::C2),
            (f64::NAN, Status::C0),
        ] {
            let mut fpu = FPU::default();
            fpu.push(value);
            fpu.examine();
            assert_eq!(fpu.status() & condition_mask, expected.bits());
        }
    }

    #[test]
    fn st0_handles_an_empty_stack_without_indexing_past_registers() {
        let mut fpu = FPU::default();
        assert_eq!(*fpu.st0(), 0.0);
    }

    #[test]
    fn fxam_marks_an_empty_stack() {
        let mut fpu = FPU::default();
        fpu.examine();

        assert_eq!(
            fpu.status() & (Status::C0 | Status::C1 | Status::C2 | Status::C3).bits(),
            (Status::C0 | Status::C3).bits()
        );
    }

    #[test]
    fn fcom_marks_nan_as_unordered() {
        let mut fpu = FPU::default();
        fpu.compare(f64::NAN, 1.0);

        let unordered = (Status::C0 | Status::C2 | Status::C3).bits();
        assert_eq!(fpu.status() & unordered, unordered);
    }

    #[test]
    fn f80_round_trips_f64_values() {
        for value in [
            0.0,
            -0.0,
            f64::from_bits(1),
            -f64::from_bits(1),
            1.0,
            -2.5,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(F80::from_f64(value).to_f64().to_bits(), value.to_bits());
        }
    }

    #[test]
    fn f80_preserves_nan() {
        assert!(F80::from_f64(f64::NAN).to_f64().is_nan());
    }

    #[test]
    fn f80_has_x87_memory_size() {
        assert_eq!(std::mem::size_of::<F80>(), 10);
    }

    #[test]
    fn fninit_resets_status_and_stack() {
        let mut fpu = FPU::default();
        fpu.push(1.0);
        fpu.set_cmp(std::cmp::Ordering::Less);
        fpu.init();

        assert_eq!(fpu.status(), 0);
        assert_eq!(fpu.st_top, 8);
        assert_eq!(fpu.control, 0x037f);
    }

    #[test]
    fn frndint_uses_the_x87_rounding_control() {
        let mut fpu = FPU::default();
        assert_eq!(fpu.round(1.5), 2.0);
        assert_eq!(fpu.round(-1.5), -2.0);

        fpu.control = (fpu.control & !0x0c00) | 0x0400;
        assert_eq!(fpu.round(1.9), 1.0);
        assert_eq!(fpu.round(-1.1), -2.0);

        fpu.control = (fpu.control & !0x0c00) | 0x0800;
        assert_eq!(fpu.round(1.1), 2.0);
        assert_eq!(fpu.round(-1.9), -1.0);

        fpu.control = (fpu.control & !0x0c00) | 0x0c00;
        assert_eq!(fpu.round(1.9), 1.0);
        assert_eq!(fpu.round(-1.9), -1.0);
    }

    #[test]
    fn fsave_and_frstor_round_trip_state() {
        let mut memory = crate::Memory::leak_new(0x2000);
        let mut fpu = FPU::default();
        fpu.control = 0x027f;
        fpu.push(1.25);
        fpu.push(-2.5);
        fpu.set_cmp(std::cmp::Ordering::Less);
        fpu.save(&mut memory, 0x1000);

        assert_eq!(fpu.st_top, 8);
        assert_eq!(fpu.control, 0x037f);
        assert_eq!(fpu.condition, 0);

        fpu.restore(&memory, 0x1000);
        assert_eq!(fpu.control, 0x027f);
        assert_eq!(fpu.get(0), -2.5);
        assert_eq!(fpu.get(1), 1.25);
        assert_eq!(fpu.condition, Status::C0.bits());
    }

    #[test]
    fn frstor_preserves_an_empty_stack() {
        let mut memory = crate::Memory::leak_new(0x2000);
        let mut fpu = FPU::default();
        fpu.save(&mut memory, 0x1000);
        fpu.restore(&memory, 0x1000);

        assert_eq!(fpu.st_top, 8);
    }

    #[test]
    fn real_mode_fsave_uses_the_14_byte_environment() {
        let mut memory = crate::Memory::leak_new(0x2000);
        let mut fpu = FPU::default();
        fpu.control = 0x027f;
        fpu.push(1.25);
        fpu.push(-2.5);
        fpu.set_cmp(std::cmp::Ordering::Less);

        fpu.save16(&mut memory, 0x1000);

        assert_eq!(memory.read::<u16>(0x1000), 0x027f);
        assert_eq!(memory.read::<u16>(0x1002), 6 << 11 | Status::C0.bits());
        assert_eq!(memory.read::<u16>(0x1004), 0x0fff);
        assert_eq!(fpu.st_top, 8);

        fpu.restore16(&memory, 0x1000);
        assert_eq!(fpu.get(0), -2.5);
        assert_eq!(fpu.get(1), 1.25);
        assert_eq!(fpu.condition, Status::C0.bits());
    }

    #[test]
    fn fsave_marks_subnormal_values_as_special() {
        let mut memory = crate::Memory::leak_new(0x2000);
        let mut fpu = FPU::default();
        fpu.push(f64::MIN_POSITIVE / 2.0);
        fpu.store_env(&mut memory, 0x1000);

        assert_eq!(memory.read::<u16>(0x1008), 0xbfff);
    }

    #[test]
    fn fsave_masks_exceptions_after_saving_control_word() {
        let mut memory = crate::Memory::leak_new(0x2000);
        let mut fpu = FPU::default();
        fpu.control = 0;

        fpu.store_env(&mut memory, 0x1000);

        assert_eq!(memory.read::<u16>(0x1000), 0);
        assert_eq!(fpu.control & 0x003f, 0x003f);
    }

    #[test]
    fn compare_flags_reports_through_eflags() {
        use crate::Flags;
        let fpu = FPU::default();
        let mut flags = Flags::empty();

        fpu.compare_flags(1.0, 2.0, &mut flags);
        assert_eq!(flags, Flags::CF);

        fpu.compare_flags(2.0, 2.0, &mut flags);
        assert_eq!(flags, Flags::ZF);

        fpu.compare_flags(3.0, 2.0, &mut flags);
        assert_eq!(flags, Flags::empty());

        fpu.compare_flags(f64::NAN, 2.0, &mut flags);
        assert_eq!(flags, Flags::ZF | Flags::PF | Flags::CF);
    }
}
