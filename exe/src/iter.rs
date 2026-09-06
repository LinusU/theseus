pub struct PodIterator<'m, T: zerocopy::FromBytes> {
    buf: &'m [u8],
    _marker: std::marker::PhantomData<&'m T>,
}

impl<'m, T: zerocopy::FromBytes> std::iter::Iterator for PodIterator<'m, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.len() < size_of::<T>() {
            return None;
        }
        let (obj, buf) = <T>::read_from_prefix(self.buf).ok()?;
        self.buf = buf;
        Some(obj)
    }
}

pub fn iter_pod<'a, T: zerocopy::FromBytes>(memory: &'a [u8]) -> PodIterator<'a, T> {
    PodIterator {
        buf: memory,
        _marker: std::marker::PhantomData,
    }
}

/// Iterate `count` items starting at `addr`. A range that falls outside the
/// buffer yields an empty iterator rather than panicking: callers parse
/// possibly-truncated host files at load time.
pub fn iter_pod_n<'a, T: zerocopy::FromBytes>(
    memory: &'a [u8],
    addr: u32,
    count: u32,
) -> PodIterator<'a, T> {
    let buf = (count as usize)
        .checked_mul(size_of::<T>())
        .and_then(|len| (addr as usize).checked_add(len))
        .and_then(|end| memory.get(addr as usize..end));
    iter_pod(buf.unwrap_or(&[]))
}
