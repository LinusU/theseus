#[derive(Default, Clone)]
pub struct Mapping {
    pub desc: String,
    pub addr: u32,
    pub size: u32,
    // If true, created from a file section (not dynamically created)
    pub section: bool,
}

impl Mapping {
    pub fn range(&self) -> std::ops::Range<u32> {
        self.addr..self.addr + self.size
    }
    pub fn contains(&self, addr: u32) -> bool {
        self.range().contains(&addr)
    }
}

impl std::fmt::Debug for Mapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:08x} {} ({:#x} bytes)",
            self.addr, self.desc, self.size
        )
    }
}

#[derive(Debug, Default)]
pub struct Mappings {
    mappings: Vec<Mapping>,
}

pub fn round_to_page(size: u32) -> u32 {
    (size + 0x1000 - 1) & !(0x1000 - 1)
}

impl Mappings {
    pub fn reserve(&mut self, mut mapping: Mapping) -> u32 {
        let index = self.insert_index(&mut mapping);
        let addr = mapping.addr;
        self.mappings.insert(index, mapping);
        addr
    }

    pub fn alloc(&mut self, desc: String, size: u32) -> u32 {
        let size = round_to_page(size);
        let mut new_mapping = Mapping {
            desc,
            addr: 0,
            section: false,
            size,
        };

        let index = self.insert_index(&mut new_mapping);
        let addr = new_mapping.addr;
        self.mappings.insert(index, new_mapping);
        addr
    }

    /// Choose the index into self.mappings to add this mapping, potentially assigning it an address.
    fn insert_index(&self, new_mapping: &mut Mapping) -> usize {
        // A fixed-address reservation must end inside the address space.
        let new_end = if new_mapping.addr != 0 {
            Some(
                new_mapping
                    .addr
                    .checked_add(new_mapping.size)
                    .unwrap_or_else(|| panic!("no space for {new_mapping:#x?}")),
            )
        } else {
            None
        };
        let mut prev_end = 0;
        for (i, mapping) in self.mappings.iter().enumerate() {
            if let Some(new_end) = new_end {
                if new_end <= mapping.addr {
                    if new_mapping.addr < prev_end {
                        panic!("{new_mapping:#x?} overlaps a previous mapping");
                    }
                    return i;
                }
            } else {
                let space = mapping.addr - prev_end;
                if space >= new_mapping.size {
                    new_mapping.addr = prev_end;
                    return i;
                }
            }
            prev_end = mapping.addr + mapping.size;
        }
        if new_mapping.addr != 0 {
            assert!(new_mapping.addr >= prev_end);
        } else {
            new_mapping.addr = prev_end;
        }
        self.mappings.len()
    }

    pub fn dump(&self) {
        println!("{:#x?}", self.mappings);
    }

    pub fn vec(&self) -> &Vec<Mapping> {
        &self.mappings
    }

    pub fn from(mappings: Vec<Mapping>) -> Self {
        Self { mappings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(addr: u32, size: u32) -> Mapping {
        Mapping {
            desc: String::new(),
            addr,
            size,
            section: false,
        }
    }

    #[test]
    fn reserve_fixed_mapping_in_gap_keeps_address() {
        let mut mappings = Mappings::from(vec![mapping(0x1000, 0x1000), mapping(0x9000, 0x1000)]);
        let addr = mappings.reserve(mapping(0x3000, 0x1000));
        assert_eq!(addr, 0x3000);
        assert_eq!(
            mappings.vec().iter().map(|m| m.addr).collect::<Vec<_>>(),
            [0x1000, 0x3000, 0x9000]
        );
    }

    #[test]
    #[should_panic(expected = "overlaps")]
    fn reserve_fixed_mapping_overlapping_previous_panics() {
        let mut mappings = Mappings::from(vec![mapping(0x1000, 0x1000), mapping(0x9000, 0x1000)]);
        // Starts inside the first mapping and ends in the gap; the gap check
        // alone cannot see the overlap with the previous mapping.
        mappings.reserve(mapping(0x1500, 0x4000));
    }

    #[test]
    #[should_panic(expected = "no space")]
    fn reserve_fixed_mapping_past_address_space_panics() {
        let mut mappings = Mappings::from(vec![mapping(0x1000, 0x1000)]);
        mappings.reserve(mapping(0xffff_f000, 0x2000));
    }

    #[test]
    fn alloc_fills_the_first_gap() {
        // Real setups always reserve the null page first, so address 0 is
        // never handed out by an alloc.
        let mut mappings = Mappings::from(vec![mapping(0x0, 0x1000), mapping(0x9000, 0x1000)]);
        let addr = mappings.alloc("test".into(), 0x1000);
        assert_eq!(addr, 0x1000);
    }
}
