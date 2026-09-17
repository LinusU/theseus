//! Heap memory allocator. Used for win32 API HeapCreate() etc. implementation
//! and also for win32-visible allocations created by other calls (like in
//! DirectDraw).
//!
//! Each allocation carries an 8-byte header (the total block size in the first
//! u32, plus padding) so that the returned data pointer is always 8-byte
//! aligned, matching the documented GlobalAlloc alignment guarantee. The
//! header stores the *total* block size including the header; the usable size
//! is header - 8.

use std::{cell::RefCell, collections::HashMap};

use runtime::Memory;

/// Size of the per-block header; data pointers are this many bytes past the
/// block start, keeping them 8-byte aligned.
const HEADER: u32 = 8;

/// Round `size` up to an 8-byte multiple, or None on overflow.
fn align8(size: u32) -> Option<u32> {
    size.checked_add(7).map(|s| s & !7)
}

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

    pub fn range(&self) -> std::ops::Range<u32> {
        self.addr..self.addr + self.size
    }

    pub fn alloc(&self, mem: &mut Memory, size: u32) -> u32 {
        self.try_alloc(mem, size)
            .unwrap_or_else(|| panic!("heap size {:x} oom {:x}", self.size, size))
    }

    /// Fallible allocation; returns the 8-byte-aligned data pointer or None.
    pub fn try_alloc(&self, mem: &mut Memory, size: u32) -> Option<u32> {
        self.freelist.borrow_mut().alloc(mem, size)
    }

    #[allow(unused)]
    pub fn size(&self, mem: &mut Memory, addr: u32) -> u32 {
        if addr < HEADER {
            return 0;
        }
        mem.read::<u32>(addr - HEADER).saturating_sub(HEADER)
    }

    pub fn free(&self, mem: &mut Memory, addr: u32) {
        if addr < HEADER || !self.range().contains(&(addr - HEADER)) {
            log::error!("free of addr not on heap");
            return;
        }
        self.freelist.borrow_mut().free(mem, addr);
    }

    /// Resize an existing allocation in place if possible.
    ///
    /// Shrinking always succeeds by returning the freed tail to the free list.
    /// Growing only succeeds when the block is immediately followed by enough
    /// contiguous free space. Returns Some(addr) on success (addr unchanged),
    /// None when the request can only be satisfied by moving.
    pub fn try_realloc_in_place(&self, mem: &mut Memory, addr: u32, new_size: u32) -> Option<u32> {
        self.freelist
            .borrow_mut()
            .realloc_in_place(mem, addr, new_size)
    }
}

#[derive(Default)]
struct FreeList {
    nodes: Vec<FreeNode>,
}

impl FreeList {
    fn new(addr: u32, size: u32) -> Self {
        FreeList {
            nodes: vec![FreeNode { addr, size }],
        }
    }

    fn alloc(&mut self, mem: &mut Memory, size: u32) -> Option<u32> {
        let total = align8(size)?.checked_add(HEADER)?;
        let i = self.nodes.iter().position(|f| f.size >= total)?;
        let free = &mut self.nodes[i];
        let addr = free.addr;
        free.size -= total;
        free.addr += total;
        if free.size == 0 {
            self.nodes.remove(i);
        }
        mem.write::<u32>(addr, total);
        Some(addr + HEADER)
    }

    fn free(&mut self, mem: &mut Memory, addr: u32) {
        let hdr = addr - HEADER;
        let size = mem.read::<u32>(hdr);
        if self.nodes.iter().any(|n| n.range().contains(&hdr)) {
            log::warn!("ignoring double free");
            return;
        }
        self.insert_free(hdr, size);
        // Mark the released block so a stale pointer to it no longer reads as a
        // live fixed allocation (e.g. GlobalHandle on a freed pointer).
        mem.write::<u32>(hdr, 0);
    }

    fn realloc_in_place(&mut self, mem: &mut Memory, addr: u32, new_size: u32) -> Option<u32> {
        let hdr = addr - HEADER;
        let old_total = mem.read::<u32>(hdr);
        let new_total = align8(new_size)?.checked_add(HEADER)?;

        if new_total == old_total {
            return Some(addr);
        }
        if new_total < old_total {
            // Shrink: return the freed tail to the free list, keep the header.
            // The tail runs [hdr + new_total, hdr + old_total).
            self.insert_free(hdr + new_total, old_total - new_total);
            mem.write::<u32>(hdr, new_total);
            return Some(addr);
        }

        // Grow: absorb contiguous free space immediately after the block, which
        // starts at the block's end `hdr + old_total`.
        let extra = new_total - old_total;
        let next_addr = hdr + old_total;
        let i = self.nodes.iter().position(|n| n.addr == next_addr)?;
        if self.nodes[i].size < extra {
            return None;
        }
        let node = &mut self.nodes[i];
        node.addr += extra;
        node.size -= extra;
        let remove = node.size == 0;
        if remove {
            self.nodes.remove(i);
        }
        mem.write::<u32>(hdr, new_total);
        Some(addr)
    }

    /// Insert a free region [addr, addr+size), merging with any adjacent free
    /// nodes. The region must be 8-byte aligned and not already free.
    fn insert_free(&mut self, addr: u32, size: u32) {
        let mut insert_index = self.nodes.len();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.addr > addr {
                insert_index = i;
                break;
            }
        }

        let mut joined = false;
        if insert_index > 0 {
            let prev_i = insert_index - 1;
            let prev = &mut self.nodes[prev_i];
            if prev.addr + prev.size == addr {
                prev.size += size;
                joined = true;
            }
        }

        if insert_index < self.nodes.len() {
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
        self.addr..self.addr + self.size
    }
}

/// Process-owned bookkeeping for movable global-memory objects (GMEM_MOVEABLE).
///
/// Fixed allocations keep pointer-valued handles straight into the process
/// heap and need no registry entry. A movable object gets an opaque handle
/// (handed out from a counter that starts above the process heap region so it
/// can never collide with a live data pointer), an optional backing allocation,
/// its exposed size and a lock count. Discarded state is represented by absent
/// backing storage (`ptr == 0`).
#[derive(Default)]
pub struct GlobalMem {
    /// Movable objects indexed by their opaque handle.
    pub movable: HashMap<u32, Movable>,
    /// Reverse lookup from a movable object's backing base pointer to its
    /// handle, used by GlobalHandle.
    pub by_ptr: HashMap<u32, u32>,
    /// Next handle to hand out; monotonic and above the process heap region.
    pub next_handle: u32,
}

/// A movable global-memory object.
#[derive(Default, Clone)]
pub struct Movable {
    /// Current backing base pointer (0 when discarded).
    pub ptr: u32,
    /// Exposed usable size in bytes (excluding the allocator header).
    pub size: u32,
    /// Lock count; saturates at 255 so overflow can never turn a locked object
    /// into an apparently-unlocked one.
    pub lock: u8,
    /// Whether the object was created with GMEM_DISCARDABLE.
    pub discardable: bool,
}

impl GlobalMem {
    /// Reserve the next movable handle. Handles start above the process heap
    /// region (`heap_end + HEADER`) and increment, so no handle value can be a
    /// live heap data pointer and its `- HEADER` offset can't alias a header.
    pub fn next_handle(&mut self, heap_end: u32) -> u32 {
        let handle = self.next_handle;
        let next = handle.wrapping_add(1);
        // If the counter wraps, restart safely above the heap region; stale
        // numeric values are removed from bookkeeping on free, so reuse is OK.
        self.next_handle = if next < heap_end {
            heap_end + HEADER
        } else {
            next
        };
        handle
    }

    /// Set the initial handle counter to just past the heap region.
    pub fn init_handles(&mut self, heap_end: u32) {
        self.next_handle = heap_end + HEADER;
    }

    /// Register a movable object under a fresh handle and insert reverse
    /// lookup for its backing pointer (skipped when discarded).
    pub fn add(&mut self, heap_end: u32, obj: Movable) -> u32 {
        let handle = self.next_handle(heap_end);
        if obj.ptr != 0 {
            self.by_ptr.insert(obj.ptr, handle);
        }
        self.movable.insert(handle, obj);
        handle
    }
}

#[cfg(test)]
mod tests {
    use runtime::Memory;

    use super::*;

    /// Heap base and size for the free-list regression fixtures, sized for the
    /// allocator's 8-byte header and 8-byte-aligned blocks. Payloads 12/12/28
    /// pack contiguously into an 88-byte region at 0x1000, which needs
    /// guest-memory backing covering 0x1000..0x1088.
    const BASE: u32 = 0x1000;
    const HEAP_SIZE: u32 = 88;
    const BACKING: usize = 0x1088;

    /// Build a Memory over a locally owned byte buffer, with the null page
    /// check disabled since the heap starts at 0x1000.
    fn make_memory<'a>(bytes: &'a mut [u8]) -> Memory<'a> {
        Memory {
            bytes,
            null_page: false,
        }
    }

    /// Snapshot of the free-list nodes as (addr, size) pairs.
    fn nodes(free: &FreeList) -> Vec<(u32, u32)> {
        free.nodes.iter().map(|n| (n.addr, n.size)).collect()
    }

    /// Check the free-list invariants:
    /// - every region has positive size and lies within the heap;
    /// - regions are sorted by ascending address and do not overlap, so
    ///   consecutive regions satisfy left.addr + left.size < right.addr;
    /// - the sum of region sizes equals the total size of blocks freed so far
    ///   (including their headers).
    fn check_invariants(free: &FreeList, freed_bytes: u32) {
        for n in &free.nodes {
            assert!(n.size > 0, "zero-size free region {n:?}");
            assert!(n.addr >= BASE, "region {n:?} below heap");
            assert!(
                n.addr + n.size <= BASE + HEAP_SIZE,
                "region {n:?} beyond heap"
            );
        }
        let mut prev: Option<&FreeNode> = None;
        for n in &free.nodes {
            if let Some(p) = prev {
                assert!(
                    p.addr + p.size < n.addr,
                    "regions {p:?} and {n:?} not sorted/coalesced"
                );
            }
            prev = Some(n);
        }
        let sum: u32 = free.nodes.iter().map(|n| n.size).sum();
        assert_eq!(
            sum, freed_bytes,
            "freed {freed_bytes} but list sums to {sum}"
        );
    }

    fn alloc(free: &mut FreeList, mem: &mut Memory, payload: u32) -> u32 {
        free.alloc(mem, payload).unwrap()
    }

    /// Allocate A (12), B (12), C (28) and free them in `order` (0=A, 1=B,
    /// 2=C), checking invariants after every free. After all three are freed,
    /// require exactly one region covering the whole heap, then verify an
    /// 80-byte payload allocation succeeds and exhausts it.
    fn check_order(order: &[u8; 3]) {
        let mut bytes = [0u8; BACKING];
        let mut mem = make_memory(&mut bytes);
        let mut free = FreeList::new(BASE, HEAP_SIZE);

        let a = alloc(&mut free, &mut mem, 12);
        let b = alloc(&mut free, &mut mem, 12);
        let c = alloc(&mut free, &mut mem, 28);
        assert_eq!((a, b, c), (BASE + 8, BASE + 32, BASE + 56));

        let mut freed_bytes = 0;
        for &which in order {
            let addr = match which {
                0 => a,
                1 => b,
                2 => c,
                _ => unreachable!(),
            };
            free.free(&mut mem, addr);
            freed_bytes += match which {
                0 => 24, // 12 -> 16 payload + 8 header
                1 => 24,
                2 => 40, // 28 -> 32 payload + 8 header
                _ => unreachable!(),
            };
            check_invariants(&free, freed_bytes);
        }

        // All three allocations freed: one coalesced region covers the heap.
        assert_eq!(nodes(&free), [(BASE, HEAP_SIZE)]);

        // An 80-byte payload needs all 88 bytes (with the header) and must
        // succeed, leaving the list empty.
        let big = free.alloc(&mut mem, 80);
        assert_eq!(big, Some(BASE + 8));
        assert!(nodes(&free).is_empty());
    }

    /// The deterministic regression: freeing A, then C, then B. On the buggy
    /// implementation freeing C inserts it before A, breaking address order, so
    /// freeing B cannot fully coalesce and the final 80-byte allocation fails.
    #[test]
    fn regression_free_highest_address_block() {
        let mut bytes = [0u8; BACKING];
        let mut mem = make_memory(&mut bytes);
        let mut free = FreeList::new(BASE, HEAP_SIZE);

        let a = alloc(&mut free, &mut mem, 12);
        let b = alloc(&mut free, &mut mem, 12);
        let c = alloc(&mut free, &mut mem, 28);
        assert_eq!((a, b, c), (BASE + 8, BASE + 32, BASE + 56));

        free.free(&mut mem, a);
        assert_eq!(nodes(&free), [(BASE, 24)]);

        free.free(&mut mem, c);
        assert_eq!(nodes(&free), [(BASE, 24), (BASE + 48, 40)]);

        free.free(&mut mem, b);
        assert_eq!(nodes(&free), [(BASE, HEAP_SIZE)]);

        let big = free.alloc(&mut mem, 80);
        assert_eq!(big, Some(BASE + 8));
        assert!(nodes(&free).is_empty());
    }

    /// All six freeing orders of A, B, C must coalesce the heap into one
    /// region. Fresh allocator and backing memory per order.
    #[test]
    fn all_free_orders_coalesce() {
        check_order(&[0, 1, 2]); // A, B, C
        check_order(&[0, 2, 1]); // A, C, B
        check_order(&[1, 0, 2]); // B, A, C
        check_order(&[1, 2, 0]); // B, C, A
        check_order(&[2, 0, 1]); // C, A, B
        check_order(&[2, 1, 0]); // C, B, A
    }

    /// Freeing an already-free block again, while other allocations are still
    /// live, must leave the free-list state unchanged.
    #[test]
    fn repeated_free_of_same_block_is_ignored() {
        let mut bytes = [0u8; BACKING];
        let mut mem = make_memory(&mut bytes);
        let mut free = FreeList::new(BASE, HEAP_SIZE);

        let a = alloc(&mut free, &mut mem, 12);
        let b = alloc(&mut free, &mut mem, 12);
        let c = alloc(&mut free, &mut mem, 28);

        free.free(&mut mem, a);
        let before = nodes(&free);
        free.free(&mut mem, a);
        assert_eq!(nodes(&free), before);

        // B and C are still live.
        let _ = (b, c);
    }

    #[test]
    fn alloc_frees_reuse() {
        let mut mem = Memory::leak_new(1 << 20);
        let heap = Heap::new(0x10000, 0x10000);
        let a = heap.try_alloc(&mut mem, 16).unwrap();
        assert_eq!(a % 8, 0);
        let b = heap.try_alloc(&mut mem, 16).unwrap();
        assert_eq!(b % 8, 0);
        assert_ne!(a, b);
        assert_eq!(heap.size(&mut mem, a), 16);
        heap.free(&mut mem, a);
        // The freed block is reused by the next allocation.
        let c = heap.try_alloc(&mut mem, 16).unwrap();
        assert_eq!(c, a);
        heap.free(&mut mem, c);
        heap.free(&mut mem, b);
    }

    #[test]
    fn allocator_exhaustion() {
        let mut mem = Memory::leak_new(1 << 20);
        // A 4 KB heap fills up quickly and reports failure instead of panicking.
        let heap = Heap::new(0x20000, 0x1000);
        let mut ptrs = vec![];
        while let Some(p) = heap.try_alloc(&mut mem, 32) {
            ptrs.push(p);
        }
        assert!(ptrs.len() > 0);
        assert!(heap.try_alloc(&mut mem, 32).is_none());
        // No adjacent space: in-place growth fails cleanly.
        assert!(
            heap.try_realloc_in_place(&mut mem, ptrs[0], 0x1000)
                .is_none()
        );
        // Shrinking always works in place.
        assert_eq!(
            heap.try_realloc_in_place(&mut mem, ptrs[0], 8),
            Some(ptrs[0])
        );
        assert_eq!(heap.size(&mut mem, ptrs[0]), 8);
        // Free one block, then grow the neighbor into the freed space.
        heap.free(&mut mem, ptrs[1]);
        let grew = heap.try_realloc_in_place(&mut mem, ptrs[0], 64);
        assert!(grew.is_some());
        assert_eq!(heap.size(&mut mem, ptrs[0]), 64);
    }

    #[test]
    fn realloc_in_place_grow_shrink() {
        let mut mem = Memory::leak_new(1 << 20);
        let heap = Heap::new(0x10000, 0x10000);
        let a = heap.try_alloc(&mut mem, 8).unwrap();
        // Grow into contiguous free space.
        assert_eq!(heap.try_realloc_in_place(&mut mem, a, 32), Some(a));
        assert_eq!(heap.size(&mut mem, a), 32);
        // Shrink back.
        assert_eq!(heap.try_realloc_in_place(&mut mem, a, 8), Some(a));
        assert_eq!(heap.size(&mut mem, a), 8);
        heap.free(&mut mem, a);
        // A neighbor blocks growth.
        let b = heap.try_alloc(&mut mem, 64).unwrap();
        let c = heap.try_alloc(&mut mem, 64).unwrap();
        assert!(heap.try_realloc_in_place(&mut mem, b, 0x100).is_none());
        assert_eq!(heap.size(&mut mem, b), 64);
        heap.free(&mut mem, b);
        heap.free(&mut mem, c);
    }
}
