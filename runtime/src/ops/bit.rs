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
    fn bit_index_wraps_to_operand_width() {
        let mut flags = Flags::default();
        bt(0x8000u16, 32, &mut flags);
        assert!(!flags.contains(Flags::CF));
        bt(0x8000u16, 31, &mut flags);
        assert!(flags.contains(Flags::CF));
    }
}
