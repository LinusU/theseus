use runtime::Context;

use crate::{
    Ptr,
    kernel32::{self, HMODULE, State},
};

pub type HRSRC = u32;
pub type HGLOBAL = u32;

fn is_intresource(x: u32) -> bool {
    x >> 16 == 0
}

impl State {
    pub fn find_resource<'ctx>(
        &self,
        ctx: &'ctx Context,
        typ: exe::ResourceName,
        name: exe::ResourceName,
    ) -> Option<&'ctx [u8]> {
        let section = &ctx.memory[self.resources.clone()];
        let span = exe::find_resource(section, typ, name)?;
        let image_base = self.image_base;
        Some(&ctx.memory[image_base + span.start..image_base + span.end])
    }
}

#[win32_derive::dllexport]
pub fn FindResourceW(
    ctx: &mut Context,
    _hModule: HMODULE,
    lpName: Ptr<u16>,
    lpType: Ptr<u16>,
) -> HRSRC {
    let name = if is_intresource(lpName.addr) {
        exe::ResourceName::Id(lpName.addr)
    } else {
        exe::ResourceName::Name(&ctx.memory.read_wstr(lpName.addr))
    };
    let typ = if is_intresource(lpType.addr) {
        exe::ResourceName::Id(lpType.addr)
    } else {
        exe::ResourceName::Name(&ctx.memory.read_wstr(lpType.addr))
    };
    let buf = kernel32::lock().find_resource(ctx, typ, name).unwrap();
    unsafe { buf.as_ptr().byte_offset_from(ctx.memory.as_ptr()) as u32 }
}

#[win32_derive::dllexport]
pub fn LoadResource(_ctx: &mut Context, _hModule: HMODULE, hResInfo: HRSRC) -> HGLOBAL {
    hResInfo
}

#[win32_derive::dllexport]
pub fn LockResource(_ctx: &mut Context, hResData: HGLOBAL) -> u32 {
    hResData
}

#[win32_derive::dllexport]
pub fn FindResourceA(
    ctx: &mut Context,
    _hModule: HMODULE,
    lpName: Ptr<u8>,
    lpType: Ptr<u8>,
) -> HRSRC {
    let name_str;
    let name = if is_intresource(lpName.addr) {
        exe::ResourceName::Id(lpName.addr)
    } else {
        name_str = widestring::U16String::from_str(&ctx.memory.read_str(lpName.addr));
        exe::ResourceName::Name(&name_str)
    };
    let type_str;
    let typ = if is_intresource(lpType.addr) {
        exe::ResourceName::Id(lpType.addr)
    } else {
        type_str = widestring::U16String::from_str(&ctx.memory.read_str(lpType.addr));
        exe::ResourceName::Name(&type_str)
    };
    let Some(buf) = kernel32::lock().find_resource(ctx, typ, name) else {
        log::warn!("FindResourceA: resource not found");
        return 0;
    };
    unsafe { buf.as_ptr().byte_offset_from(ctx.memory.as_ptr()) as u32 }
}
