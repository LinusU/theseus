#![allow(unused)]

pub use runtime::Mapping;

#[derive(Default)]
/// Memory represents the process memory after loading an executable.
// TODO: this is maybe redundant with runtime's memory, should they be merged?
pub struct Memory {
    pub mappings: runtime::Mappings,
    pub bytes: Vec<u8>,
}

impl Memory {
    /// Largest address the translator can hand out; matches the runtime's
    /// flat 256 MiB address space.
    pub const LIMIT: u32 = 256 << 20;
}

impl Memory {
    pub fn try_reserve(&mut self, name: String, addr: u32, size: u32) -> Option<u32> {
        let Ok(addr) = self.mappings.try_reserve(Mapping {
            desc: name,
            addr,
            section: true,
            size,
        }) else {
            return None;
        };
        let len = (addr + size) as usize;
        if len > self.bytes.len() {
            self.bytes.resize(len, 0);
        }
        Some(addr)
    }

    pub fn reserve(&mut self, name: String, addr: u32, size: u32) -> u32 {
        self.try_reserve(name, addr, size)
            .expect("tc Memory mapping reservation failed")
    }

    pub fn read<T: zerocopy::FromBytes>(&self, addr: u32) -> T {
        <T>::read_from_prefix(&self.bytes[addr as usize..])
            .unwrap()
            .0
    }

    pub fn try_read<T: zerocopy::FromBytes>(&self, addr: u32) -> Option<T> {
        <T>::read_from_prefix(self.bytes.get(addr as usize..)?)
            .ok()
            .map(|(val, _)| val)
    }

    pub fn write<T: zerocopy::IntoBytes + zerocopy::Immutable>(&mut self, addr: u32, val: T) {
        val.write_to_prefix(&mut self.bytes[addr as usize..])
            .unwrap();
    }

    pub fn try_write<T: zerocopy::IntoBytes + zerocopy::Immutable>(
        &mut self,
        addr: u32,
        val: T,
    ) -> bool {
        let Some(buf) = self.bytes.get_mut(addr as usize..) else {
            return false;
        };
        val.write_to_prefix(buf).is_ok()
    }

    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let start = addr as usize;
        let end = start
            .checked_add(data.len())
            .expect("write_bytes length overflow");
        let buf = self
            .bytes
            .get_mut(start..end)
            .expect("write_bytes out of range");
        buf.copy_from_slice(data);
    }

    pub fn slice(&self, addr: u32, len: u32) -> &[u8] {
        let start = addr as usize;
        let end = start
            .checked_add(len as usize)
            .expect("slice length overflow");
        self.bytes.get(start..end).expect("slice out of range")
    }

    pub fn slice_all(&self, addr: u32) -> &[u8] {
        self.bytes.get(addr as usize..).unwrap_or(&[])
    }

    pub fn slice_mut(&mut self, addr: u32, len: u32) -> &mut [u8] {
        let start = addr as usize;
        let end = start
            .checked_add(len as usize)
            .expect("slice_mut length overflow");
        self.bytes
            .get_mut(start..end)
            .expect("slice_mut out of range")
    }
}
