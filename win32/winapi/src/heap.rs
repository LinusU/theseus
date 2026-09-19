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
pub const HEADER: u32 = 8;

/// Maximum number of occupied movable handles to skip while searching for a
/// vacant one. Bounds the collision-skip so ordinary exhaustion can't scan a
/// huge namespace; `reserve` reports failure once exceeded.
pub const MAX_SKIP: u32 = 256;

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

    /// Whether `addr` is the exact data-pointer (base) of a live allocation in
    /// this heap. Authoritative host-side bookkeeping: no header bytes are
    /// inspected, so ordinary payloads can never impersonate an allocation.
    pub fn contains(&self, addr: u32) -> bool {
        self.freelist.borrow().live.contains_key(&addr)
    }

    /// Authoritative usable size (excluding the header) for a live allocation
    /// data pointer, or 0 when `addr` is not a live allocation. Reads the
    /// recorded block size rather than trusting guest memory.
    pub fn size(&self, _mem: &mut Memory, addr: u32) -> u32 {
        let total = self.freelist.borrow().live.get(&addr).copied().unwrap_or(0);
        total.saturating_sub(HEADER)
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
    /// Authoritative registry of live allocations: block base -> total block
    /// size (including the header). Maintained by alloc/free/realloc_in_place
    /// so every allocation, free, and resize path shares the same source of
    /// truth. Used to validate fixed global-memory handles.
    live: HashMap<u32, u32>,
}

impl FreeList {
    fn new(addr: u32, size: u32) -> Self {
        FreeList {
            nodes: vec![FreeNode { addr, size }],
            live: HashMap::new(),
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
        // Key the live registry by the returned data pointer (the handle).
        self.live.insert(addr + HEADER, total);
        Some(addr + HEADER)
    }

    fn free(&mut self, mem: &mut Memory, addr: u32) {
        let hdr = addr - HEADER;
        // Use the authoritative recorded size, not the (potentially spoofed)
        // header bytes, and refuse to free an address that is not a live base.
        let Some(total) = self.live.remove(&addr) else {
            log::warn!("free of non-live allocation");
            return;
        };
        self.insert_free(hdr, total);
        // Mark the released block so a stale pointer to it no longer reads as a
        // live fixed allocation (e.g. GlobalHandle on a freed pointer).
        mem.write::<u32>(hdr, 0);
    }

    fn realloc_in_place(&mut self, mem: &mut Memory, addr: u32, new_size: u32) -> Option<u32> {
        let hdr = addr - HEADER;
        // Only a live allocation can be resized; read its recorded size rather
        // than trusting header bytes.
        let old_total = *self.live.get(&addr)?;
        let new_total = align8(new_size)?.checked_add(HEADER)?;

        if new_total == old_total {
            return Some(addr);
        }
        if new_total < old_total {
            // Shrink: return the freed tail to the free list, keep the header.
            // The tail runs [hdr + new_total, hdr + old_total).
            self.insert_free(hdr + new_total, old_total - new_total);
            self.live.insert(addr, new_total);
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
        self.live.insert(addr, new_total);
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
#[derive(Debug, PartialEq)]
struct FreeNode {
    addr: u32,
    size: u32,
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
    /// Reserve a vacant movable handle, skipping occupied values with a bounded
    /// search. Handles are drawn from the namespace above the process heap
    /// region (`heap_end + HEADER`) and never fall into the range of valid
    /// fixed data pointers. Returns None when the namespace is exhausted; a
    /// failed reservation never publishes a handle or replaces a live object.
    pub fn reserve(&mut self, heap_end: u32) -> Option<u32> {
        let ns_start = heap_end.checked_add(HEADER)?;
        let mut h = self.next_handle.max(ns_start);
        let mut skipped = 0u32;
        loop {
            if !self.movable.contains_key(&h) {
                // Reserve it and advance the counter. A wrap past u32::MAX
                // restarts at the namespace start, where the next reserve skips
                // any occupied handles.
                self.next_handle = h
                    .checked_add(1)
                    .map(|n| n.max(ns_start))
                    .unwrap_or(ns_start);
                return Some(h);
            }
            h = h
                .checked_add(1)
                .map(|n| n.max(ns_start))
                .unwrap_or(ns_start);
            skipped += 1;
            if skipped > MAX_SKIP {
                return None;
            }
        }
    }

    /// Register a movable object under a handle, inserting the reverse lookup
    /// for its backing pointer (skipped when discarded). Both maps are updated
    /// together so they never diverge.
    pub fn register(&mut self, handle: u32, obj: Movable) {
        if obj.ptr != 0 {
            self.by_ptr.insert(obj.ptr, handle);
        }
        self.movable.insert(handle, obj);
    }

    /// Reserve a handle and register `obj` under it. Returns None (leaving no
    /// registry entry) when the handle namespace is exhausted.
    pub fn add(&mut self, heap_end: u32, obj: Movable) -> Option<u32> {
        let handle = self.reserve(heap_end)?;
        self.register(handle, obj);
        Some(handle)
    }

    /// Set the initial handle counter to just past the heap region.
    pub fn init_handles(&mut self, heap_end: u32) {
        self.next_handle = heap_end.checked_add(HEADER).unwrap_or(u32::MAX);
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

    /// Deterministic xorshift PRNG for the seeded allocator test.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            (x >> 8) as u32
        }

        fn pick(&mut self, n: usize) -> usize {
            self.next() as usize % n
        }
    }

    /// Independent model of a live allocation: its data base and expected
    /// usable bytes. Mirrors the allocator's own bookkeeping so a divergence
    /// (size, overlap, or retained content) is caught by the checks.
    struct LiveAlloc {
        base: u32,
        data: Vec<u8>,
    }

    /// Check the free-list invariants against the model's running freed-byte
    /// total, within explicit heap bounds (the upstream check uses its own
    /// BASE/HEAP_SIZE fixtures).
    fn check_free(free: &FreeList, start: u32, size: u32, freed_bytes: u32) {
        for n in &free.nodes {
            assert!(n.size > 0, "zero-size free region {n:?}");
            assert!(n.addr >= start, "region {n:?} below heap");
            assert!(n.addr + n.size <= start + size, "region {n:?} beyond heap");
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

    /// Verify the independent model against the allocator: live allocations do
    /// not overlap, recorded sizes match the model, retained bytes are correct,
    /// and the free list is ordered, non-overlapping, and coalesced.
    fn check_model(
        mem: &mut Memory,
        heap: &Heap,
        free: &FreeList,
        live: &[LiveAlloc],
        freed_bytes: u32,
    ) {
        let mut sorted: Vec<&LiveAlloc> = live.iter().collect();
        sorted.sort_by_key(|a| a.base);
        for w in sorted.windows(2) {
            let a = &w[0];
            let b = &w[1];
            let a_end = a.base + heap.size(mem, a.base);
            assert!(
                a_end <= b.base,
                "live allocations overlap: {:x}+{} and {:x}",
                a.base,
                heap.size(mem, a.base),
                b.base
            );
        }
        for a in live {
            let usable = heap.size(mem, a.base);
            assert_eq!(
                usable as usize,
                a.data.len(),
                "recorded size at {:x}",
                a.base
            );
            assert!(
                mem[a.base..][..a.data.len()] == a.data[..],
                "retained bytes at {:x}",
                a.base
            );
        }
        check_free(free, 0x2000, 0x400, freed_bytes);
    }

    /// A deterministic seeded operation sequence mixing allocation, free,
    /// shrink, and growth under limited capacity, verified against an
    /// independent model of live intervals and expected contents. After
    /// releasing everything, usable capacity must be fully recovered.
    #[test]
    fn seeded_allocator_sequence() {
        const START: u32 = 0x2000;
        const SIZE: u32 = 0x400;
        let mut mem = Memory::leak_new(1 << 20);
        let heap = Heap::new(START, SIZE);
        let mut rng = Rng(0x1234_5678);
        let mut live: Vec<LiveAlloc> = vec![];
        // Current total free bytes in the heap (starts as the whole heap).
        let mut freed_bytes = SIZE;

        for step in 0..300u32 {
            let choice = rng.pick(4);
            match choice {
                0 => {
                    // Allocate a random payload.
                    let sz = 1 + rng.pick(96) as u32;
                    if let Some(base) = heap.try_alloc(&mut mem, sz) {
                        let usable = heap.size(&mut mem, base);
                        freed_bytes -= usable + HEADER;
                        let data: Vec<u8> = (0..usable)
                            .map(|i| (step as u8).wrapping_add(i as u8))
                            .collect();
                        mem[base..][..usable as usize].copy_from_slice(&data);
                        live.push(LiveAlloc { base, data });
                    }
                }
                1 => {
                    // Free a random live allocation.
                    if !live.is_empty() {
                        let i = rng.pick(live.len());
                        let a = live.remove(i);
                        let usable = heap.size(&mut mem, a.base);
                        heap.free(&mut mem, a.base);
                        freed_bytes += usable + HEADER;
                    }
                }
                2 => {
                    // Grow a random live allocation in place (may fail).
                    if !live.is_empty() {
                        let i = rng.pick(live.len());
                        let old_usable = heap.size(&mut mem, live[i].base);
                        let new_size = old_usable + 1 + rng.pick(64) as u32;
                        if let Some(base) =
                            heap.try_realloc_in_place(&mut mem, live[i].base, new_size)
                        {
                            assert_eq!(base, live[i].base);
                            let old_total = old_usable + HEADER;
                            let new_usable = heap.size(&mut mem, base);
                            let new_total = new_usable + HEADER;
                            freed_bytes -= new_total - old_total;
                            let old_len = live[i].data.len();
                            let new_len = new_usable as usize;
                            let mut data = std::mem::take(&mut live[i].data);
                            data.resize(new_len, 0);
                            for j in old_len..new_len {
                                data[j] = (step as u8).wrapping_add(j as u8);
                            }
                            // The app initializes the newly exposed region; the
                            // allocator's in-place grow leaves it untouched.
                            mem[base + old_len as u32..][..(new_len - old_len)]
                                .copy_from_slice(&data[old_len..]);
                            live[i].data = data;
                        }
                    }
                }
                3 => {
                    // Shrink a random live allocation in place.
                    if !live.is_empty() {
                        let i = rng.pick(live.len());
                        let old_usable = heap.size(&mut mem, live[i].base);
                        let new_size = old_usable.saturating_sub(1 + rng.pick(48) as u32).max(1);
                        let base = heap
                            .try_realloc_in_place(&mut mem, live[i].base, new_size)
                            .unwrap();
                        assert_eq!(base, live[i].base);
                        let old_total = old_usable + HEADER;
                        let new_usable = heap.size(&mut mem, base);
                        let new_total = new_usable + HEADER;
                        freed_bytes += old_total - new_total;
                        live[i].data.truncate(new_usable as usize);
                    }
                }
                _ => unreachable!(),
            }

            // After every operation, verify the model against the allocator.
            let free = heap.freelist.borrow();
            check_model(&mut mem, &heap, &free, &live, freed_bytes);
        }

        // Release everything; usable capacity must be fully recovered.
        for a in live {
            let usable = heap.size(&mut mem, a.base);
            heap.free(&mut mem, a.base);
            freed_bytes += usable + HEADER;
        }
        let free = heap.freelist.borrow();
        assert_eq!(
            free.nodes,
            [FreeNode {
                addr: START,
                size: SIZE
            }]
        );
        assert_eq!(freed_bytes, SIZE);
        drop(free);
        let big = heap.try_alloc(&mut mem, SIZE - HEADER).unwrap();
        assert_eq!(big, START + HEADER);
        assert_eq!(heap.size(&mut mem, big), SIZE - HEADER);
        assert!(heap.freelist.borrow().nodes.is_empty());
    }
}
