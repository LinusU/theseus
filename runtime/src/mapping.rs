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
    // A size that cannot be rounded up saturates past every caller's limit
    // instead of wrapping to a small bogus reservation.
    size.saturating_add(0x1000 - 1) & !(0x1000 - 1)
}

impl Mappings {
    pub fn try_reserve(&mut self, mut mapping: Mapping) -> Result<u32, &'static str> {
        let index = self.insert_index(&mut mapping)?;
        let addr = mapping.addr;
        self.mappings.insert(index, mapping);
        Ok(addr)
    }

    pub fn reserve(&mut self, mapping: Mapping) -> u32 {
        self.try_reserve(mapping)
            .expect("mapping reservation failed")
    }

    /// Allocate a guest-visible mapping, bounded by `limit` (typically the size
    /// of emulated memory). Returns `None` when no gap can hold the mapping
    /// rather than handing out an address that cannot be backed. The base is
    /// always page-aligned: Windows hands out aligned addresses for
    /// VirtualAlloc-family allocations, and a prior unaligned reservation
    /// (e.g. the 4-byte IAT) must not skew the next base.
    pub fn try_alloc(&mut self, desc: String, size: u32, limit: u32) -> Option<u32> {
        let size = round_to_page(size);
        let mut prev_end = 0u64;
        for (i, mapping) in self.mappings.iter().enumerate() {
            let start = prev_end.next_multiple_of(0x1000);
            let end = start + size as u64;
            if (mapping.addr as u64).saturating_sub(start) >= size as u64 && end <= limit as u64 {
                let addr = u32::try_from(start).ok()?;
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
        let start = prev_end.next_multiple_of(0x1000);
        let end = start.checked_add(size as u64)?;
        if end > limit as u64 {
            return None;
        }
        let addr = u32::try_from(start).ok()?;
        self.mappings.push(Mapping {
            desc,
            addr,
            size,
            section: false,
        });
        Some(addr)
    }

    /// Choose the index into self.mappings to add this mapping, potentially assigning it an address.
    fn insert_index(&self, new_mapping: &mut Mapping) -> Result<usize, &'static str> {
        // A fixed-address reservation must end inside the address space.
        let new_end = if new_mapping.addr != 0 {
            match new_mapping.addr.checked_add(new_mapping.size) {
                Some(end) => Some(end),
                None => return Err("no space for mapping"),
            }
        } else {
            None
        };
        let mut prev_end = 0;
        for (i, mapping) in self.mappings.iter().enumerate() {
            if let Some(new_end) = new_end {
                if new_end <= mapping.addr {
                    if new_mapping.addr < prev_end {
                        return Err("overlaps a previous mapping");
                    }
                    return Ok(i);
                }
            } else {
                let space = mapping.addr - prev_end;
                if space >= new_mapping.size {
                    new_mapping.addr = prev_end;
                    return Ok(i);
                }
            }
            prev_end = mapping.addr + mapping.size;
        }
        if new_mapping.addr != 0 {
            if new_mapping.addr < prev_end {
                return Err("overlaps a previous mapping");
            }
        } else {
            new_mapping.addr = prev_end;
        }
        Ok(self.mappings.len())
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
        let addr = mappings.try_alloc("test".into(), 0x1000, 0x10000);
        assert_eq!(addr, Some(0x1000));
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
    fn try_alloc_aligns_the_base_past_an_unaligned_reservation() {
        // The 4-byte IAT reservation leaves an unaligned tail; the next
        // allocation must still start on a page boundary.
        let mut mappings = Mappings::from(vec![mapping(0x0, 0x1000), mapping(0x1000, 4)]);
        assert_eq!(
            mappings.try_alloc("aligned".into(), 0x100, 0x10000),
            Some(0x2000)
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
    fn round_to_page_saturates_past_u32_max() {
        assert_eq!(round_to_page(0x1), 0x1000);
        assert_eq!(round_to_page(0x1000), 0x1000);
        assert_eq!(round_to_page(0x1001), 0x2000);
        assert_eq!(round_to_page(0xffff_f000), 0xffff_f000);
        // Sizes that cannot round up saturate high enough to fail every
        // caller's limit check instead of wrapping to a small reservation.
        assert_eq!(round_to_page(0xffff_f001), 0xffff_f000);
        assert_eq!(round_to_page(u32::MAX), 0xffff_f000);
    }

    #[test]
    fn try_alloc_rejects_sizes_past_the_address_space() {
        let mut mappings = Mappings::from(vec![mapping(0x0, 0x1000)]);
        assert_eq!(mappings.try_alloc("huge".into(), u32::MAX, 0x10000), None);
        assert_eq!(mappings.vec().len(), 1);
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
