//! Intel VTune API stubs.
//!
//! The game loads VTUNEAPI.DLL and probes for profiling entry points. When the
//! DLL is not present it reports "VTUNE 3.0 not detected." and continues.
//! These no-op stubs produce the same effect while satisfying the dynamic
//! GetProcAddress calls.
//!
//! The actual exports use a mix of "VT..." and "Vt..." spellings; both are
//! registered so the game can find its preferred names.

use runtime::Context;

#[win32_derive::dllexport]
pub fn VTPause(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VTResume(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VTPauseSampling(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VTResumeSampling(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn CMPause(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn CMResume(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VtPause(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VtResume(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VtPauseSampling(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn VtResumeSampling(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn CmPause(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn CmResume(_ctx: &mut Context) {}
