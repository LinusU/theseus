#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use zerocopy::FromBytes;

use crate::{c_str, iter::iter_pod_n};

#[derive(Debug, zerocopy::FromBytes)]
#[repr(C)]
pub struct IMAGE_EXPORT_DIRECTORY {
    Characteristics: u32,
    TimeDateStamp: u32,
    MajorVersion: u16,
    MinorVersion: u16,
    Name: u32,
    /// Ordinal offset. Symbol DLL@x => functions[Base + x].
    pub Base: u32,
    NumberOfFunctions: u32,
    NumberOfNames: u32,
    AddressOfFunctions: u32,
    AddressOfNames: u32,
    AddressOfNameOrdinals: u32,
}

impl IMAGE_EXPORT_DIRECTORY {
    #[allow(dead_code)]
    pub fn name<'a>(&self, image: &'a [u8]) -> &'a [u8] {
        c_str(image.get(self.Name as usize..).unwrap_or(&[]))
    }

    /// Returns an iterator of function addresses in ordinal order.
    pub fn fns(&self, image: &[u8]) -> impl Iterator<Item = u32> {
        iter_pod_n::<u32>(image, self.AddressOfFunctions, self.NumberOfFunctions)
    }

    /// Returns an iterator of (name, index) pairs, where index is an index into fn()s.
    pub fn names<'a>(&self, image: &'a [u8]) -> impl Iterator<Item = (&'a [u8], u16)> {
        let names = iter_pod_n::<u32>(image, self.AddressOfNames, self.NumberOfNames);
        let ords = iter_pod_n::<u16>(image, self.AddressOfNameOrdinals, self.NumberOfNames);

        let ni = names.map(move |addr| c_str(image.get(addr as usize..).unwrap_or(&[])));
        ni.zip(ords)
    }
}

pub fn read_exports(section: &[u8]) -> Option<IMAGE_EXPORT_DIRECTORY> {
    <IMAGE_EXPORT_DIRECTORY>::read_from_prefix(section)
        .ok()
        .map(|(d, _)| d)
}
