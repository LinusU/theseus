//! Heap memory allocator. Used for win32 API HeapCreate() etc. implementation
//! and also for win32-visible allocations created by other calls (like in
//! DirectDraw).

use std::cell::RefCell;

use runtime::Memory;

#[derive(Default)]
pub struct Heap {
    #[allow(unused)]
    pub addr: u32,
    pub size: u32,
    freelist: RefCell<FreeList>,
}

impl Heap {
    pub fn new(addr: u32, size: u32) -> Self {
        Heap {
            addr,
            size,
            freelist: RefCell::new(FreeList::new(addr, size)),
        }
    }

    fn range(&self) -> std::ops::Range<u32> {
        // A heap whose claimed range wraps the 32-bit space saturates at the
        // top of memory rather than wrapping into low addresses.
        self.addr..self.addr.saturating_add(self.size)
    }

    pub fn try_alloc(&self, mem: &mut Memory, size: u32) -> Option<u32> {
        self.freelist.borrow_mut().alloc(mem, size)
    }

    /// The payload size of a live block, or None when `addr` does not name a
    /// live block — unlike the in-band header, this cannot be confused by a
    /// corrupted or interior header.
    pub fn block_size(&self, addr: u32) -> Option<u32> {
        if addr < 4 {
            return None;
        }
        self.freelist.borrow().block_size(addr)
    }

    /// Free a pointer previously returned by `alloc`. Returns false when the
    /// pointer is not a live block on this heap (matching HeapFree's FALSE).
    #[track_caller]
    pub fn free(&self, _mem: &mut Memory, addr: u32) -> bool {
        if addr < 4 || !self.range().contains(&(addr - 4)) {
            log::error!("free of addr not on heap");
            return false;
        }
        if !self.freelist.borrow_mut().free(addr) {
            log::warn!(
                "ignoring free of non-live block {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            return false;
        }
        true
    }
}

#[derive(Default)]
struct FreeList {
    nodes: Vec<FreeNode>,
    /// Live block headers (block address -> size including the 4-byte header).
    /// Tracking liveness keeps frees of interior or stale pointers from
    /// silently inserting overlapping free nodes that corrupt later allocs.
    live: std::collections::BTreeMap<u32, u32>,
}

impl FreeList {
    fn new(addr: u32, size: u32) -> Self {
        // HeapAlloc hands out 8-byte-aligned payloads. With the 4-byte
        // header the block itself must start at an offset of 4 mod 8;
        // sliding the first block forward sets that alignment, and every
        // later block keeps it because block sizes are rounded up to 8.
        let Ok(block) = u32::try_from(((addr as u64) + 4).next_multiple_of(8) - 4) else {
            // A base so high the aligned first block leaves the 32-bit
            // space: no usable room, and no node for `alloc` to overflow on.
            return FreeList {
                nodes: vec![],
                live: Default::default(),
            };
        };
        // The free range also stops at the top of the 32-bit space so a
        // fully-consumed tail can never wrap `alloc`'s `addr += size`.
        let usable = size.saturating_sub(block - addr).min(u32::MAX - block);
        FreeList {
            nodes: vec![FreeNode {
                addr: block,
                size: usable,
            }],
            live: Default::default(),
        }
    }

    fn alloc(&mut self, mem: &mut Memory, size: u32) -> Option<u32> {
        // The 4-byte header must not wrap the request size: a guest asking
        // for u32::MAX - 3 would otherwise be handed a tiny live block it
        // believes is nearly 4 GiB. Padding the block to a multiple of 8
        // keeps every payload 8-byte aligned (see FreeList::new).
        let size = size.checked_add(4)?.checked_next_multiple_of(8)?;
        let i = self.nodes.iter().position(|f| f.size >= size)?;
        let free = &mut self.nodes[i];
        let addr = free.addr;
        free.size -= size;
        free.addr += size;
        if free.size == 0 {
            self.nodes.remove(i);
        }
        mem.write::<u32>(addr, size);
        self.live.insert(addr, size);
        Some(addr + 4)
    }

    /// The payload size of a live block, or None when `addr` (a guest pointer,
    /// i.e. header + 4) does not name a live block.
    fn block_size(&self, addr: u32) -> Option<u32> {
        self.live.get(&(addr - 4)).map(|size| size - 4)
    }

    /// Insert the block back on the free list. Returns false when `addr` does
    /// not name a live block — a double free or a stale/interior pointer the
    /// caller may report.
    fn free(&mut self, addr: u32) -> bool {
        let addr = addr - 4;
        let Some(size) = self.live.remove(&addr) else {
            return false;
        };

        let mut insert_index = self.nodes.len();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.range().contains(&addr) {
                // address is within already free block
                return false;
            }
            if node.addr > addr {
                insert_index = i;
                break;
            }
        }

        let mut joined = false;
        if insert_index > 0 {
            // Check if merging with earlier block.
            let prev_i = insert_index - 1;
            let prev = &mut self.nodes[prev_i];
            if prev.addr + prev.size == addr {
                prev.size += size;
                joined = true;
            }
        }

        if insert_index < self.nodes.len() {
            // Check if merging with later block.
            let next = &mut self.nodes[insert_index];
            if addr + size == next.addr {
                if joined {
                    let next_size = next.size;
                    let prev = &mut self.nodes[insert_index - 1];
                    prev.size += next_size;
                    self.nodes.remove(insert_index);
                } else {
                    next.addr -= size;
                    next.size += size;
                    joined = true;
                }
            }
        }

        if !joined {
            let free = FreeNode { addr, size };
            self.nodes.insert(insert_index, free);
        }
        true
    }
}

/// Entry in the FreeList.
#[derive(Debug)]
struct FreeNode {
    addr: u32,
    size: u32,
}

impl FreeNode {
    fn range(&self) -> std::ops::Range<u32> {
        self.addr..self.addr.saturating_add(self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frees_merge_neighbors_in_address_order() {
        let mut mem = Memory::leak_new(0x10_000);
        let mut list = FreeList::new(0x4000, 0x304);
        // Split the range into three adjacent live blocks.
        let a = list.alloc(&mut mem, 0xfc).unwrap();
        let b = list.alloc(&mut mem, 0xfc).unwrap();
        let c = list.alloc(&mut mem, 0xfc).unwrap();
        assert_eq!(list.nodes.len(), 0);

        // Free low, high, then middle: the high block sorts above every
        // existing node, and the middle block must merge with both.
        list.free(a);
        list.free(c);
        list.free(b);

        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.nodes[0].addr, 0x4004);
        assert_eq!(list.nodes[0].size, 0x300);
        assert_eq!(list.alloc(&mut mem, 0x2fc), Some(0x4008));
    }

    #[test]
    fn alloc_returns_eight_byte_aligned_payloads() {
        let mut mem = Memory::leak_new(0x10_000);
        // Both an 8-aligned and a 4-mod-8 heap base yield 8-aligned payloads.
        for base in [0x4000, 0x4004] {
            let mut list = FreeList::new(base, 0x400);
            for size in [1u32, 3, 0xf, 0x10] {
                let addr = list.alloc(&mut mem, size).unwrap();
                assert_eq!(addr % 8, 0, "payload {addr:#x} not 8-aligned");
            }
        }
    }

    #[test]
    fn alloc_rejects_sizes_that_overflow_the_header() {
        let mut mem = Memory::leak_new(0x10_000);
        let mut list = FreeList::new(0x4000, 0x300);
        // A request whose size+4 header adjustment wraps to a small value
        // must fail instead of vending a tiny live block.
        assert_eq!(list.alloc(&mut mem, u32::MAX - 3), None);
        assert_eq!(list.alloc(&mut mem, u32::MAX), None);
        // The free list is untouched and still serves normal requests.
        assert_eq!(list.alloc(&mut mem, 0xfc), Some(0x4008));
    }

    #[test]
    fn free_rejects_interior_stale_and_foreign_pointers() {
        let mut mem = Memory::leak_new(0x10_000);
        let mut list = FreeList::new(0x4000, 0x400);
        let a = list.alloc(&mut mem, 0xfc).unwrap();
        let b = list.alloc(&mut mem, 0xfc).unwrap();

        // Interior, already-freed, and never-allocated pointers all fail
        // without touching the free list.
        assert!(!list.free(a + 8));
        assert!(!list.free(0x4300));
        assert!(list.free(a));
        assert!(!list.free(a));
        assert!(list.free(b));
        // Both blocks merged with each other and the free tail.
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.nodes[0].size, 0x3fc);
    }

    #[test]
    fn heaps_at_the_top_of_the_address_space_cannot_wrap() {
        let mut mem = Memory::leak_new(0x10_000);
        // A base whose aligned first block leaves the 32-bit space entirely.
        let mut list = FreeList::new(0xFFFF_FFFF, 0x1000);
        assert_eq!(list.alloc(&mut mem, 8), None);
        // A base near the top: the free tail is clamped so consuming it
        // cannot overflow the next node address.
        let mut list = FreeList::new(0xFFFF_FF00, 0x2000);
        assert!(list.nodes[0].size < 0x100);
        while list.alloc(&mut mem, 8).is_some() {}
        // The clamped tail is exhausted: the next request cannot wrap the
        // node address because the whole range ends at u32::MAX.
        assert_eq!(list.alloc(&mut mem, 8), None);
    }
}
