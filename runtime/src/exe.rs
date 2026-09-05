use crate::{Cont, ContFn, Memory, Regs, mapping::Mappings};

pub type Block = (u32, ContFn);

pub struct EXEData {
    pub image_base: u32,
    pub resources: std::ops::Range<u32>,
    pub blocks: &'static [Block],
    pub init: fn(&mut Regs, &mut Memory, &mut Mappings),
    pub entry_point: Cont,
}
