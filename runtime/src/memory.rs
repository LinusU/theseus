use widestring::U16String;

/// Memory represents the inner machine's memory, as a flat byte array (no paging etc.).
///
/// It is unsafely mutably shared across multiple threads.  In principle any mangling
/// that multi-threaded access can do could just as well be done by single-threaded code,
/// since it is fully under the control of the target executable.
pub struct Memory<'a> {
    pub bytes: &'a mut [u8],
    /// When true, panic on access to low memory.
    /// TODO: full memory mapping access controls etc.
    pub null_page: bool,
    /// Defensive sink for out-of-bounds `[]` indexing so guests do not
    /// panic the host.
    dummy: [u8; 1],
}

/// A trait for types that can be read from memory with .read().
pub trait MemRead: zerocopy::FromBytes {}
impl<T: zerocopy::FromBytes> MemRead for T {}

/// A trait for types that can be written to memory with .write().
pub trait MemWrite: zerocopy::IntoBytes + zerocopy::Immutable {}
impl<T: zerocopy::IntoBytes + zerocopy::Immutable> MemWrite for T {}

impl<'a> Memory<'a> {
    pub fn new(bytes: &'static mut [u8]) -> Self {
        Memory {
            bytes,
            null_page: true,
            dummy: [0],
        }
    }

    pub fn unsafe_clone(&mut self) -> Memory<'a> {
        Memory {
            bytes: unsafe {
                std::slice::from_raw_parts_mut(self.bytes.as_mut_ptr(), self.bytes.len())
            },
            null_page: self.null_page,
            dummy: [0],
        }
    }

    #[track_caller]
    #[inline(never)]
    pub fn null_ptr(&self, addr: u32) {
        log::error!(
            "null page read/write at {addr:#x} (caller {})",
            std::panic::Location::caller()
        );
    }

    #[track_caller]
    #[inline]
    fn check_access(&self, addr: u32) {
        if addr < 0x1000 && self.null_page {
            self.null_ptr(addr);
        }
    }

    #[track_caller]
    pub fn try_read<T: MemRead>(&self, addr: u32) -> Option<T> {
        self.check_access(addr);
        let start = addr as usize;
        let end = start.checked_add(std::mem::size_of::<T>())?;
        let bytes = self.bytes.get(start..end)?;
        T::read_from_bytes(bytes).ok()
    }

    #[track_caller]
    pub fn read<T: MemRead>(&self, addr: u32) -> T {
        match self.try_read(addr) {
            Some(val) => val,
            None => {
                log::error!(
                    "out-of-bounds {}-byte read at {addr:#x} (caller {})",
                    std::mem::size_of::<T>(),
                    std::panic::Location::caller()
                );
                let zeros = vec![0u8; std::mem::size_of::<T>()];
                T::read_from_bytes(&zeros).unwrap()
            }
        }
    }

    #[track_caller]
    pub fn try_write<T: MemWrite>(&mut self, addr: u32, val: T) -> bool {
        self.check_access(addr);
        let start = addr as usize;
        let size = std::mem::size_of::<T>();
        let Some(end) = start.checked_add(size) else {
            return false;
        };
        let Some(bytes) = self.bytes.get_mut(start..end) else {
            return false;
        };
        val.write_to(bytes).is_ok()
    }

    #[track_caller]
    pub fn write<T: MemWrite>(&mut self, addr: u32, val: T) {
        if !self.try_write(addr, val) {
            log::error!(
                "out-of-bounds {}-byte write at {addr:#x} (caller {})",
                std::mem::size_of::<T>(),
                std::panic::Location::caller()
            );
        }
    }

    #[track_caller]
    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) {
        self.check_access(addr);
        let start = addr as usize;
        let Some(end) = start.checked_add(data.len()) else {
            log::error!(
                "out-of-bounds {}-byte write at {addr:#x} (caller {})",
                data.len(),
                std::panic::Location::caller()
            );
            return;
        };
        let Some(buf) = self.bytes.get_mut(start..end) else {
            log::error!(
                "out-of-bounds {}-byte write at {addr:#x} (caller {})",
                data.len(),
                std::panic::Location::caller()
            );
            return;
        };
        buf.copy_from_slice(data);
    }

    #[track_caller]
    pub fn read_str(&self, addr: u32) -> &str {
        self.check_access(addr);
        let Some(buf) = self.bytes.get(addr as usize..) else {
            log::error!(
                "out-of-bounds string read at {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            return "";
        };
        let Some(nul) = buf.iter().position(|&c| c == 0) else {
            // A string that runs to the end of memory is malformed guest data;
            // failing the read beats panicking the host.
            log::error!(
                "unterminated string at {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            return "";
        };
        let buf = &buf[..nul];
        match std::str::from_utf8(buf) {
            Ok(str) => str,
            // ANSI (CP-1252) strings may contain bytes that are not valid
            // UTF-8; treat the read as a failure rather than panicking.
            Err(err) => {
                log::error!(
                    "non-UTF-8 string at {addr:#x} (invalid byte at +{:#x}, caller {})",
                    err.valid_up_to(),
                    std::panic::Location::caller()
                );
                ""
            }
        }
    }

    /// This returns an allocated string rather than a reference due to alignment.
    #[track_caller]
    pub fn read_wstr(&self, addr: u32) -> U16String {
        self.check_access(addr);
        let Some(buf) = self.bytes.get(addr as usize..) else {
            log::error!(
                "out-of-bounds wide string read at {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            return U16String::new();
        };
        let mut str: Vec<u16> = vec![];
        for chunk in buf.chunks_exact(2) {
            if chunk == [0, 0] {
                break;
            }
            str.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        U16String::from_vec(str)
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }
}

impl Memory<'static> {
    pub fn leak_new(size: usize) -> Self {
        // safety: safe to assume_init on zeroed u8
        let memory: Box<[u8]> = unsafe { Box::<[u8]>::new_zeroed_slice(size).assume_init() };
        let static_memory: &'static mut [u8] = Box::leak(memory);
        Memory::new(static_memory)
    }
}

impl<'a> std::ops::Index<u32> for Memory<'a> {
    type Output = u8;

    #[track_caller]
    fn index(&self, addr: u32) -> &Self::Output {
        self.check_access(addr);
        let start = addr as usize;
        if let Some(byte) = self.bytes.get(start) {
            byte
        } else {
            log::error!(
                "out-of-bounds byte read at {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            &self.dummy[0]
        }
    }
}

impl<'a> std::ops::IndexMut<u32> for Memory<'a> {
    #[track_caller]
    fn index_mut(&mut self, addr: u32) -> &mut Self::Output {
        self.check_access(addr);
        let start = addr as usize;
        if self.bytes.get(start).is_some() {
            &mut self.bytes[start]
        } else {
            log::error!(
                "out-of-bounds byte write at {addr:#x} (caller {})",
                std::panic::Location::caller()
            );
            &mut self.dummy[0]
        }
    }
}

impl<'a> std::ops::Index<std::ops::RangeFrom<u32>> for Memory<'a> {
    type Output = [u8];

    #[track_caller]
    fn index(&self, index: std::ops::RangeFrom<u32>) -> &Self::Output {
        self.check_access(index.start);
        self.bytes
            .get(index.start as usize..)
            .unwrap_or(&self.dummy[..0])
    }
}

impl<'a> std::ops::IndexMut<std::ops::RangeFrom<u32>> for Memory<'a> {
    #[track_caller]
    fn index_mut(&mut self, index: std::ops::RangeFrom<u32>) -> &mut Self::Output {
        self.check_access(index.start);
        if let Some(slice) = self.bytes.get_mut(index.start as usize..) {
            slice
        } else {
            &mut self.dummy[..0]
        }
    }
}

impl<'a> std::ops::Index<std::ops::Range<u32>> for Memory<'a> {
    type Output = [u8];

    #[track_caller]
    fn index(&self, index: std::ops::Range<u32>) -> &Self::Output {
        self.check_access(index.start);
        self.bytes
            .get(index.start as usize..index.end as usize)
            .unwrap_or(&self.dummy[..0])
    }
}

impl<'a> std::ops::IndexMut<std::ops::Range<u32>> for Memory<'a> {
    #[track_caller]
    fn index_mut(&mut self, index: std::ops::Range<u32>) -> &mut Self::Output {
        self.check_access(index.start);
        if let Some(slice) = self.bytes.get_mut(index.start as usize..index.end as usize) {
            slice
        } else {
            &mut self.dummy[..0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_str_returns_guest_string() {
        let memory = Memory::leak_new(0x2000);
        memory.bytes[0x1000..0x1006].copy_from_slice(b"hello\0");
        assert_eq!(memory.read_str(0x1000), "hello");
    }

    #[test]
    fn read_str_does_not_panic_on_unterminated_string() {
        let memory = Memory::leak_new(0x2000);
        memory.bytes[0x1fff] = b'x';
        // No NUL between 0x1fff and the end of memory.
        assert_eq!(memory.read_str(0x1fff), "");
    }

    #[test]
    fn read_str_does_not_panic_on_non_utf8_string() {
        let memory = Memory::leak_new(0x2000);
        memory.bytes[0x1000..0x1005].copy_from_slice(b"caf\xe9\0");
        assert_eq!(memory.read_str(0x1000), "");
    }

    #[test]
    fn index_does_not_panic_on_out_of_bounds() {
        let mut memory = Memory::leak_new(0x100);

        // Single-byte out-of-bounds reads return 0 and writes hit the dummy sink.
        assert_eq!(memory[0x1000], 0);
        memory[0x1000] = 0xab;
        assert_eq!(memory[0x1000], 0xab);

        // Range and RangeFrom out-of-bounds produce empty slices.
        assert_eq!(memory[0x1000..0x1001].len(), 0);
        assert_eq!(memory[0x1000..].len(), 0);
        memory[0x1000..0x1001].fill(0xcd);
        memory[0x1000..].fill(0xcd);

        // In-bound access still works and is not touched by the out-of-bounds writes.
        memory[0x10] = 0x42;
        assert_eq!(memory[0x10], 0x42);
        memory[0x10..0x12].copy_from_slice(&[0x12, 0x34]);
        assert_eq!(&memory[0x10..0x12], &[0x12, 0x34]);
    }
}
