use super::int::Int;
use crate::Flags;

fn test_bit<I: Int>(x: I, bit: u32, flags: &mut Flags) -> I {
    let shift = bit as usize & (I::bits() - 1);
    let value = (x >> shift) & I::one();
    flags.set(Flags::CF, value == I::one());
    value
}

pub fn bt<I: Int>(x: I, bit: u32, flags: &mut Flags) {
    test_bit(x, bit, flags);
}

pub fn bts<I: Int>(x: I, bit: u32, flags: &mut Flags) -> I {
    test_bit(x, bit, flags);
    x | (I::one() << (bit as usize & (I::bits() - 1)))
}

pub fn btr<I: Int>(x: I, bit: u32, flags: &mut Flags) -> I {
    test_bit(x, bit, flags);
    x & !(I::one() << (bit as usize & (I::bits() - 1)))
}

pub fn btc<I: Int>(x: I, bit: u32, flags: &mut Flags) -> I {
    test_bit(x, bit, flags);
    x ^ (I::one() << (bit as usize & (I::bits() - 1)))
}

/// ARPL compares the RPL (low two bits) of two segment selectors. When the
/// destination's RPL is lower than the source's, the destination is raised
/// to match and ZF is set; otherwise ZF is cleared and the destination is
/// unchanged (returned as None).
pub fn arpl(dest: u16, src: u16, flags: &mut Flags) -> Option<u16> {
    let needs_adjust = dest & 3 < src & 3;
    flags.set(Flags::ZF, needs_adjust);
    needs_adjust.then(|| dest & !3 | src & 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_operations_set_carry_and_update_selected_bit() {
        let mut flags = Flags::default();
        assert_eq!(bts(0b0001u8, 4, &mut flags), 0b0001_0001);
        assert!(!flags.contains(Flags::CF));
        assert_eq!(btr(0b0001_0001u8, 0, &mut flags), 0b0001_0000);
        assert!(flags.contains(Flags::CF));
        assert_eq!(btc(0b0001_0000u8, 4, &mut flags), 0);
        assert!(flags.contains(Flags::CF));
    }

    #[test]
    fn arpl_adjusts_the_destination_rpl() {
        let mut flags = Flags::default();
        assert_eq!(arpl(0x0008, 0x001b, &mut flags), Some(0x000b));
        assert!(flags.contains(Flags::ZF));
        assert_eq!(arpl(0x001b, 0x0008, &mut flags), None);
        assert!(!flags.contains(Flags::ZF));
    }

    #[test]
    fn bit_index_wraps_to_operand_width() {
        let mut flags = Flags::default();
        bt(0x8000u16, 32, &mut flags);
        assert!(!flags.contains(Flags::CF));
        bt(0x8000u16, 31, &mut flags);
        assert!(flags.contains(Flags::CF));
    }
}
