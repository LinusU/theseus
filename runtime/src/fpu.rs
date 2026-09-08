//! FPU registers.

use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy)]
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
    /// Condition code bits (C0..C3 in status-word positions) from the last
    /// comparison or fxam.
    pub cc: Status,
    /// Control word, as managed by fldcw/fnstcw. We only round-trip the value;
    /// precision/rounding control bits are not honored.
    pub control: u16,
}

impl Default for FPU {
    fn default() -> Self {
        Self {
            st: [0.; 8],
            st_top: 8,
            cc: Status::C3,
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
        &mut self.st[self.st_top]
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

    /// Record a comparison result (fcom and friends) in the condition codes.
    pub fn set_cmp(&mut self, cmp: std::cmp::Ordering) {
        self.cc = match cmp {
            std::cmp::Ordering::Less => Status::C0,
            std::cmp::Ordering::Equal => Status::C3,
            std::cmp::Ordering::Greater => Status::empty(),
        };
    }

    /// fxam: classify a value into the condition codes.
    pub fn xam(&mut self, val: f64) {
        let mut cc = if val.is_nan() {
            Status::C0
        } else if val.is_infinite() {
            Status::C0 | Status::C2
        } else if val == 0.0 {
            Status::C3
        } else if val.is_subnormal() {
            Status::C2 | Status::C3
        } else {
            Status::C2 // normal finite
        };
        if val.is_sign_negative() {
            cc |= Status::C1;
        }
        self.cc = cc;
    }

    pub fn status(&self) -> u16 {
        // Our status register impl doesn't include st_top so include it here.
        let mut status = self.cc.bits();
        status |= (self.st_top as u16 & 0b111) << 11;
        status
    }

    /// Round per the control word's rounding-control bits, as fist/frndint do.
    /// The C runtime sets truncation around its double-to-int conversions, so
    /// honoring these matters.
    pub fn round(&self, val: f64) -> f64 {
        match (self.control >> 10) & 3 {
            0 => val.round_ties_even(),
            1 => val.floor(),
            2 => val.ceil(),
            _ => val.trunc(),
        }
    }
}
