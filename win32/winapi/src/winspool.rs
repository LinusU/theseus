//! Printing. There are no printers.

use runtime::Context;

#[win32_derive::dllexport]
pub fn OpenPrinterA(
    _ctx: &mut Context,
    _pPrinterName: u32,
    _phPrinter: u32,
    _pDefault: u32,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn ClosePrinter(_ctx: &mut Context, _hPrinter: u32) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn DocumentPropertiesA(
    _ctx: &mut Context,
    _hWnd: u32,
    _hPrinter: u32,
    _pDeviceName: u32,
    _pDevModeOutput: u32,
    _pDevModeInput: u32,
    _fMode: u32,
) -> i32 {
    -1
}
