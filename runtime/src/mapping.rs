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

    /// Like `alloc`, but bounded by `limit` (typically the size of emulated
    /// memory): returns `None` when no gap can hold the mapping rather than
    /// handing out an address that cannot be backed.
    pub fn try_alloc(&mut self, desc: String, size: u32, limit: u32) -> Option<u32> {
        let size = round_to_page(size);
        let mut prev_end = 0u64;
        for (i, mapping) in self.mappings.iter().enumerate() {
            let end = prev_end + size as u64;
            if (mapping.addr as u64).saturating_sub(prev_end) >= size as u64 && end <= limit as u64
            {
                let addr = prev_end as u32;
                self.mappings.insert(
                    i,
                    Mapping {
                        desc,
                        addr,
                        size,
                        section: false,
                    },
                );
                return Some(addr);
            }
            prev_end = mapping.addr as u64 + mapping.size as u64;
        }
        let end = prev_end.checked_add(size as u64)?;
        if end > limit as u64 {
            return None;
        }
        let addr = u32::try_from(prev_end).ok()?;
        self.mappings.push(Mapping {
            desc,
            addr,
            size,
            section: false,
        });
        Some(addr)
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

    /// Release a dynamically created mapping by its base address, as
    /// VirtualFree's MEM_RELEASE does. Mappings created from the loaded
    /// image (`section`) cannot be released.
    pub fn free(&mut self, addr: u32) -> bool {
        let Some(i) = self.mappings.iter().position(|m| m.addr == addr) else {
            return false;
        };
        if self.mappings[i].section {
            return false;
        }
        self.mappings.remove(i);
        true
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

    #[test]
    fn try_alloc_respects_the_limit() {
        let mut mappings = Mappings::from(vec![mapping(0x0, 0x1000)]);
        assert_eq!(mappings.try_alloc("a".into(), 0x1000, 0x4000), Some(0x1000));
        assert_eq!(mappings.try_alloc("b".into(), 0x1000, 0x4000), Some(0x2000));
        assert_eq!(mappings.try_alloc("c".into(), 0x1000, 0x4000), Some(0x3000));
        // The tail is full; nothing is placed past the limit.
        assert_eq!(mappings.try_alloc("d".into(), 0x1000, 0x4000), None);
        assert_eq!(mappings.try_alloc("e".into(), 0x8000, 0x4000), None);
        assert_eq!(
            mappings.vec().iter().map(|m| m.addr).collect::<Vec<_>>(),
            [0x0, 0x1000, 0x2000, 0x3000]
        );
    }

    #[test]
    fn try_alloc_uses_the_first_gap() {
        let mut mappings = Mappings::from(vec![mapping(0x0, 0x1000), mapping(0x9000, 0x1000)]);
        assert_eq!(
            mappings.try_alloc("gap".into(), 0x2000, 0x10000),
            Some(0x1000)
        );
        // The gap can no longer hold this and the tail would exceed the
        // limit.
        assert_eq!(mappings.try_alloc("wide".into(), 0x8000, 0xa000), None);
        // Larger than any gap but still inside the limit: placed on the
        // tail after the last mapping.
        assert_eq!(
            mappings.try_alloc("tail".into(), 0x7000, 0x12000),
            Some(0xa000)
        );
    }

    #[test]
    fn free_releases_allocations_but_not_sections() {
        let mut mappings = Mappings::from(vec![
            Mapping {
                desc: "image".into(),
                addr: 0x1000,
                size: 0x1000,
                section: true,
            },
            mapping(0x9000, 0x1000),
        ]);
        assert!(!mappings.free(0x1000)); // a loaded section
        assert!(!mappings.free(0x5000)); // no mapping starts there
        assert!(mappings.free(0x9000));
        assert_eq!(mappings.vec().len(), 1);
    }
}
