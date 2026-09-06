use std::marker::PhantomData;

use crate::{FromABIParam, Memory};

/// Ptr represents a (possibly-unaligned) pointer into win32 memory for a given type `T`,
/// wrapping the zerocopy read/write operations.
pub struct Ptr<T> {
    pub addr: u32,
    _phantom: PhantomData<T>,
}

impl<T> Ptr<T> {
    pub fn new(addr: u32) -> Self {
        Self {
            addr,
            _phantom: PhantomData,
        }
    }

    pub fn advance(&mut self) {
        self.addr += std::mem::size_of::<T>() as u32;
    }
}

impl<T: zerocopy::FromBytes> Ptr<T> {
    #[track_caller]
    pub fn read(&self, memory: &Memory) -> Option<T> {
        if self.addr < 0x1000 {
            memory.null_ptr(self.addr);
            return None;
        }
        let bytes = memory
            .bytes
            .get(self.addr as usize..)
            .and_then(|bytes| bytes.get(..std::mem::size_of::<T>()))?;
        <T>::read_from_bytes(bytes).ok()
    }
}

impl<T: zerocopy::FromBytes + zerocopy::Immutable + zerocopy::KnownLayout> Ptr<T> {
    pub fn aligned_ref<'a>(&self, memory: &'a Memory) -> Option<&'a T> {
        let bytes = memory
            .bytes
            .get(self.addr as usize..)
            .and_then(|bytes| bytes.get(..std::mem::size_of::<T>()))?;
        <T>::ref_from_bytes(bytes).ok()
    }
}

impl<T: zerocopy::IntoBytes + zerocopy::Immutable> Ptr<T> {
    #[track_caller]
    pub fn write(&self, memory: &mut Memory, value: T) -> Option<()> {
        if self.addr < 0x1000 {
            memory.null_ptr(self.addr);
            return None;
        }
        let bytes = memory
            .bytes
            .get_mut(self.addr as usize..)
            .and_then(|bytes| bytes.get_mut(..std::mem::size_of::<T>()))?;
        value.write_to(bytes).ok()
    }
}

impl<T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::Immutable + zerocopy::KnownLayout>
    Ptr<T>
{
    pub fn aligned_mut<'a>(&self, memory: &'a mut Memory) -> Option<&'a mut T> {
        let bytes = memory
            .bytes
            .get_mut(self.addr as usize..)
            .and_then(|bytes| bytes.get_mut(..std::mem::size_of::<T>()))?;
        <T>::mut_from_bytes(bytes).ok()
    }
}

impl<T> std::fmt::Debug for Ptr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.addr)
    }
}

impl<T> FromABIParam for Ptr<T> {
    fn from_abi(val: u32) -> Self {
        Self::new(val)
    }
}
