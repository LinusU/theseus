use runtime::Context;

use super::*;
use crate::{
    Ptr, RECT,
    gdi32::{self, HDC},
    stub,
};

#[win32_derive::dllexport]
pub fn GetSystemMetrics(_ctx: &mut Context, nIndex: u32 /* SYSTEM_METRICS_INDEX */) -> i32 {
    // These were dumped from a win2k VM running at 640x480.
    // See retrowin32's exe/cpp/metrics.cc.
    const METRICS: [i32; 100] = [
        640, 480, 16, 16, 19, 1, 1, 3, 3, 16, 16, 32, 32, 32, 32, 19, 640, 433, 0, 1, 16, 16, 0, 0,
        0, 0, 0, 0, 112, 27, 18, 18, 4, 4, 112, 27, 4, 4, 75, 75, 0, 0, 0, 5, 0, 2, 2, 160, 24, 16,
        16, 16, 12, 15, 18, 18, 8, 160, 24, 652, 492, 648, 460, 3, 0, 0, 0, 0, 4, 4, 0, 13, 13, 0,
        0, 1, 0, 0, 640, 480, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    METRICS[nIndex as usize]
}

#[win32_derive::dllexport]
pub fn DrawTextA(
    ctx: &mut Context,
    hdc: HDC,
    lpchText: Ptr<u8>,
    cchText: i32,
    lprc: Ptr<RECT>,
    format: u32,
) -> i32 {
    const DT_SINGLELINE: u32 = 0x20;
    const DT_CALCRECT: u32 = 0x400;

    if cchText < -1
        || lprc.addr < 0x1000
        || lprc
            .addr
            .checked_add(std::mem::size_of::<RECT>() as u32)
            .is_none_or(|end| end as usize > ctx.memory.bytes.len())
    {
        return 0;
    }
    let bytes = if cchText == -1 {
        if lpchText.addr < 0x1000 || lpchText.addr as usize >= ctx.memory.bytes.len() {
            return 0;
        }
        ctx.memory.read_str(lpchText.addr).as_bytes().to_vec()
    } else {
        let count = cchText as usize;
        let Some(end) = lpchText.addr.checked_add(cchText as u32) else {
            return 0;
        };
        if count != 0 && (lpchText.addr < 0x1000 || end as usize > ctx.memory.bytes.len()) {
            return 0;
        }
        ctx.memory[lpchText.addr..][..count].to_vec()
    };
    let rect = lprc.read(&ctx.memory).unwrap();
    let mut max_width = 0;
    let mut line_height = 0;
    let mut line_count = 0;
    let lines = if format & DT_SINGLELINE != 0 {
        bytes.split(|_| false).take(1).collect::<Vec<_>>()
    } else {
        bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>()
    };
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(size) = gdi32::text_extent_for_dc(hdc, line.len()) else {
            return 0;
        };
        max_width = max_width.max(size.cx);
        line_height = line_height.max(size.cy);
        line_count += 1;
    }
    if format & DT_CALCRECT != 0 {
        let width = max_width;
        let height = line_height.saturating_mul(line_count);
        if lprc
            .write(
                &mut ctx.memory,
                RECT {
                    right: rect.left.saturating_add(width),
                    bottom: rect.top.saturating_add(height),
                    ..rect
                },
            )
            .is_none()
        {
            return 0;
        }
    }
    line_height.saturating_mul(line_count)
}

#[win32_derive::dllexport]
pub fn EnumDisplaySettingsA(
    ctx: &mut Context,
    _lpszDeviceName: Ptr<u8>,
    iModeNum: u32,
    lpDevMode: Ptr<u8>,
) -> bool {
    const ENUM_CURRENT_SETTINGS: u32 = u32::MAX;
    const ENUM_REGISTRY_SETTINGS: u32 = u32::MAX - 1;
    const DM_BITSPERPEL: u32 = 0x0004_0000;
    const DM_PELSWIDTH: u32 = 0x0008_0000;
    const DM_PELSHEIGHT: u32 = 0x0010_0000;
    const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;

    if lpDevMode.addr < 0x1000
        || !matches!(iModeNum, ENUM_CURRENT_SETTINGS | ENUM_REGISTRY_SETTINGS)
    {
        return false;
    }
    if ctx.memory.read::<u16>(lpDevMode.addr + 0x24) < 0x94 {
        return false;
    }

    ctx.memory.write::<u32>(
        lpDevMode.addr + 0x28,
        DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY,
    );
    ctx.memory.write::<u32>(lpDevMode.addr + 0x64, 32);
    ctx.memory.write::<u32>(lpDevMode.addr + 0x68, 640);
    ctx.memory.write::<u32>(lpDevMode.addr + 0x6c, 480);
    ctx.memory.write::<u32>(lpDevMode.addr + 0x74, 60);
    true
}

#[win32_derive::dllexport]
pub fn ChangeDisplaySettingsA(ctx: &mut Context, lpDevMode: Ptr<u8>, _dwFlags: u32) -> i32 {
    const DISP_CHANGE_SUCCESSFUL: i32 = 0;
    const DISP_CHANGE_BADMODE: i32 = -2;
    const DM_BITSPERPEL: u32 = 0x0004_0000;
    const DM_PELSWIDTH: u32 = 0x0008_0000;
    const DM_PELSHEIGHT: u32 = 0x0010_0000;
    const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
    const SUPPORTED_FIELDS: u32 =
        DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;

    if lpDevMode.addr == 0 {
        return DISP_CHANGE_SUCCESSFUL;
    }
    if lpDevMode.addr < 0x1000 || ctx.memory.read::<u16>(lpDevMode.addr + 0x24) < 0x94 {
        return DISP_CHANGE_BADMODE;
    }

    let fields = ctx.memory.read::<u32>(lpDevMode.addr + 0x28);
    if fields & !SUPPORTED_FIELDS != 0 {
        return DISP_CHANGE_BADMODE;
    }

    let bits_per_pixel = if fields & DM_BITSPERPEL != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + 0x68)
    } else {
        32
    };
    let width = if fields & DM_PELSWIDTH != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + 0x6c)
    } else {
        640
    };
    let height = if fields & DM_PELSHEIGHT != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + 0x70)
    } else {
        480
    };
    let frequency = if fields & DM_DISPLAYFREQUENCY != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + 0x78)
    } else {
        60
    };

    if width != 640 || height != 480 || !matches!(bits_per_pixel, 8 | 16 | 32) || frequency != 60 {
        return DISP_CHANGE_BADMODE;
    }

    if let Some(window) = state().window.borrow().as_ref() {
        window.borrow_mut().resize(ctx, width, height);
    }
    DISP_CHANGE_SUCCESSFUL
}

#[win32_derive::dllexport]
pub fn ShowCursor(_ctx: &mut Context, bShow: bool) -> i32 {
    if bShow { stub!(1) } else { stub!(0) }
}

#[win32_derive::dllexport]
pub fn CreateCursor(
    _ctx: &mut Context,
    _hInst: HINSTANCE,
    _xHotSpot: i32,
    _yHotSpot: i32,
    _nWidth: i32,
    _nHeight: i32,
    _pvANDPlane: Ptr<u8>,
    _pvXORPlane: Ptr<u8>,
) -> HCURSOR {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn ReleaseCapture(_ctx: &mut Context) -> bool {
    stub!(true)
}

#[win32_derive::dllexport]
pub fn SetCapture(_ctx: &mut Context, _hWnd: HWND) -> HWND {
    stub!(HWND::null())
}

#[win32_derive::dllexport]
pub fn WinHelpW(
    _ctx: &mut Context,
    _hWndMain: HWND,
    _lpszHelp: Ptr<u16>, /* WSTR */
    _uCommand: u32,
    _dwData: u32,
) -> bool {
    // There is no host help viewer to launch.
    false
}

#[win32_derive::dllexport]
pub fn CheckMenuItem(_ctx: &mut Context, _hMenu: HMENU, _uIDCheckItem: u32, _uCheck: u32) -> u32 {
    stub!(0) // previously unchecked
}

pub type LRESULT = i32;

#[win32_derive::dllexport]
pub fn GetMenuItemRect(
    _ctx: &mut Context,
    _hWnd: HWND,
    _hMenu: HMENU,
    _uItem: u32,
    _lprcItem: Ptr<RECT>,
) -> bool {
    // No menus are tracked in the emulated model.
    false
}

#[win32_derive::dllexport]
pub fn KillTimer(_ctx: &mut Context, _hWnd: HWND, _uIDEvent: u32) -> bool {
    // SetTimer always fails in this model, so no timers can exist to kill.
    false
}

#[win32_derive::dllexport]
pub fn MessageBoxA(
    ctx: &mut Context,
    _hWnd: HWND,
    lpText: Ptr<u8>,
    lpCaption: Ptr<u8>,
    _uType: u32, /* MESSAGEBOX_STYLE */
) -> u32 /* MESSAGEBOX_RESULT */ {
    // We have no dialogs, but the C runtime reports fatal errors this way, so
    // the text is worth surfacing.
    let read = |ptr: Ptr<u8>| {
        if ptr.addr == 0 {
            String::new()
        } else {
            ctx.memory.read_str(ptr.addr).to_owned()
        }
    };
    log::warn!("MessageBox: {} / {}", read(lpCaption), read(lpText));
    const IDOK: u32 = 1;
    IDOK
}

#[win32_derive::dllexport]
pub fn GetActiveWindow(_ctx: &mut Context) -> HWND {
    // Only ever one window, and it's always the active one.
    match state().window.borrow().as_ref() {
        Some(window) => window.borrow().hwnd,
        None => HWND::null(),
    }
}

#[win32_derive::dllexport]
pub fn SetActiveWindow(_ctx: &mut Context, hWnd: HWND) -> HWND {
    let previous = GetActiveWindow(_ctx);
    if previous.is_null() || previous != hWnd {
        return HWND::null();
    }

    use super::message::{WM, post_message};
    post_message(hWnd, WM::ACTIVATEAPP as u32, 1, 0);
    post_message(hWnd, WM::ACTIVATE as u32, 1, 0);
    post_message(hWnd, WM::SETFOCUS as u32, 0, 0);
    previous
}

#[win32_derive::dllexport]
pub fn GetLastActivePopup(_ctx: &mut Context, hWnd: HWND) -> HWND {
    // No popups, so a window is its own last active popup.
    hWnd
}

#[win32_derive::dllexport]
pub fn CharPrevA(_ctx: &mut Context, lpszStart: Ptr<u8>, lpszCurrent: Ptr<u8>) -> u32 {
    if lpszCurrent.addr > lpszStart.addr {
        lpszCurrent.addr - 1
    } else {
        lpszStart.addr
    }
}

#[win32_derive::dllexport]
pub fn MessageBoxW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _lpText: Ptr<u16>,    /* WSTR */
    _lpCaption: Ptr<u16>, /* WSTR */
    _uType: u32,          /* MESSAGEBOX_STYLE */
) -> u32 /* MESSAGEBOX_RESULT */ {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn SetMenu(_ctx: &mut Context, _hWnd: HWND, _hMenu: HMENU) -> bool {
    stub!(true) // success
}

#[win32_derive::dllexport]
pub fn SetTimer(
    _ctx: &mut Context,
    _hWnd: HWND,
    _nIDEvent: u32,
    _uElapse: u32,
    _lpTimerFunc: Ptr<()>, /* TIMERPROC */
) -> u32 {
    stub!(0) // fail
}

/// Read a NUL-terminated byte string without the UTF-8 check.
fn read_bytes0(ctx: &Context, addr: u32) -> Vec<u8> {
    let buf = &ctx.memory[addr..];
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf[..nul].to_vec()
}

/// Shared cdecl-varargs formatter for wsprintfA/W, operating on u16
/// character units so both char widths share the format-string parser.
/// `wide` selects whether %s arguments are read as UTF-16.
fn wsprintf_impl(ctx: &mut Context, fmt: &[u16], mut arg_addr: u32, wide: bool) -> Vec<u16> {
    /// Documented maximum output of wsprintf, including the nul.
    const MAX_LEN: usize = 1024;

    // The parser only speaks ASCII; non-ASCII units read as None here.
    let at = |i: usize| fmt.get(i).copied().and_then(|c| u8::try_from(c).ok());
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        i += 1;
        if c != b'%' as u16 {
            out.push(c);
            continue;
        }
        let mut left = false;
        let mut zero = false;
        loop {
            match at(i) {
                Some(b'-') => {
                    left = true;
                    i += 1;
                }
                Some(b'0') => {
                    zero = true;
                    i += 1;
                }
                Some(b'#') => i += 1,
                _ => break,
            }
        }
        let mut width = 0usize;
        while let Some(d @ b'1'..=b'9') = at(i) {
            width = width * 10 + (d - b'0') as usize;
            i += 1;
            while let Some(d @ b'0'..=b'9') = at(i) {
                // Clamped because the width sizes an allocation here, and a
                // format string can ask for gigabytes of padding.
                width = (width * 10 + (d - b'0') as usize).min(MAX_LEN);
                i += 1;
            }
        }
        if at(i) == Some(b'.') {
            i += 1;
            while matches!(at(i), Some(b'0'..=b'9')) {
                i += 1;
            }
        }
        while matches!(at(i), Some(b'l') | Some(b'h')) {
            i += 1;
        }
        let spec = at(i).unwrap_or(b'%');
        i += 1;
        let mut next_arg = || {
            let value = ctx.memory.read::<u32>(arg_addr);
            arg_addr += 4;
            value
        };
        let formatted: Vec<u16> = match spec {
            b'%' => vec![b'%' as u16],
            b'd' | b'i' => format!("{}", next_arg() as i32).encode_utf16().collect(),
            b'u' => format!("{}", next_arg()).encode_utf16().collect(),
            b'x' => format!("{:x}", next_arg()).encode_utf16().collect(),
            b'X' => format!("{:X}", next_arg()).encode_utf16().collect(),
            b'c' => vec![next_arg() as u16],
            b's' => {
                let addr = next_arg();
                if wide {
                    ctx.memory.read_wstr(addr).as_slice().to_vec()
                } else {
                    read_bytes0(ctx, addr).iter().map(|&b| b as u16).collect()
                }
            }
            _ => {
                // Consume the arg anyway: skipping it would shift every
                // argument after this one.
                next_arg();
                log::warn!("wsprintf: unhandled %{}", spec as char);
                vec![b'%' as u16, spec as u16]
            }
        };
        if formatted.len() < width {
            let pad = if zero && !left {
                b'0' as u16
            } else {
                b' ' as u16
            };
            let padding = std::iter::repeat(pad).take(width - formatted.len());
            if left {
                out.extend(formatted);
                out.extend(padding);
            } else {
                out.extend(padding);
                out.extend(formatted);
            }
        } else {
            out.extend(formatted);
        }
    }

    // The real wsprintf writes at most 1024 characters including the nul, and
    // callers size their buffers for that.
    out.truncate(MAX_LEN - 1);
    out
}

// XXX: cdecl
#[win32_derive::dllexport]
pub fn wsprintfW(ctx: &mut Context) -> i32 {
    // Cdecl varargs, see wsprintfA.
    let esp = ctx.cpu.regs.esp;
    let dst = ctx.memory.read::<u32>(esp + 4);
    let fmt_addr = ctx.memory.read::<u32>(esp + 8);
    let fmt = ctx.memory.read_wstr(fmt_addr).as_slice().to_vec();
    let out = wsprintf_impl(ctx, &fmt, esp + 12, true);
    for (j, unit) in out.iter().enumerate() {
        ctx.memory.write::<u16>(dst + j as u32 * 2, *unit);
    }
    ctx.memory.write::<u16>(dst + out.len() as u32 * 2, 0);
    out.len() as i32
}

// XXX: cdecl
#[win32_derive::dllexport]
pub fn wsprintfA(ctx: &mut Context) -> i32 {
    // Cdecl varargs: declared with no args so the wrapper leaves the caller's
    // stack alone; read everything manually.
    // [esp] = return addr, [esp+4] = dst, [esp+8] = fmt, [esp+12...] = args.
    let esp = ctx.cpu.regs.esp;
    let dst = ctx.memory.read::<u32>(esp + 4);
    let fmt_addr = ctx.memory.read::<u32>(esp + 8);
    let fmt = read_bytes0(ctx, fmt_addr)
        .iter()
        .map(|&b| b as u16)
        .collect::<Vec<_>>();
    let out = wsprintf_impl(ctx, &fmt, esp + 12, false);

    let bytes: Vec<u8> = out.iter().map(|&c| c as u8).collect();
    ctx.memory[dst..][..bytes.len()].copy_from_slice(&bytes);
    ctx.memory.write::<u8>(dst + bytes.len() as u32, 0);
    bytes.len() as i32
}

#[cfg(test)]
mod tests {
    use super::{wsprintfA, wsprintfW};
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    fn read_cstr(ctx: &Context, addr: u32) -> String {
        let buf = &ctx.memory[addr..];
        let nul = buf.iter().position(|&b| b == 0).unwrap();
        std::str::from_utf8(&buf[..nul]).unwrap().to_string()
    }

    fn read_wstr(ctx: &Context, addr: u32) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = addr;
        loop {
            let unit = ctx.memory.read::<u16>(i);
            if unit == 0 {
                return out;
            }
            out.push(unit);
            i += 2;
        }
    }

    #[test]
    fn wsprintf_formats_args_for_both_char_widths() {
        let mut ctx = context();
        // [esp]=ret, [esp+4]=dst, [esp+8]=fmt, [esp+12...]=args.
        ctx.cpu.regs.esp = 0x200;
        ctx.memory.write::<u32>(0x200, 0); // return addr
        ctx.memory.write::<u32>(0x204, 0x300); // dst
        ctx.memory.write::<u32>(0x208, 0x400); // fmt
        ctx.memory.write::<u32>(0x20c, 42); // %d arg
        ctx.memory.write::<u32>(0x210, 0x500); // %s arg
        ctx.memory[0x400..][..12].copy_from_slice(b"val=%d %s\0\0\0");
        ctx.memory[0x500..][..4].copy_from_slice(b"hey\0");
        assert_eq!(wsprintfA(&mut ctx), 10);
        assert_eq!(read_cstr(&ctx, 0x300), "val=42 hey");

        let mut ctx = context();
        ctx.cpu.regs.esp = 0x200;
        ctx.memory.write::<u32>(0x200, 0);
        ctx.memory.write::<u32>(0x204, 0x300);
        ctx.memory.write::<u32>(0x208, 0x400);
        ctx.memory.write::<u32>(0x20c, 42);
        ctx.memory.write::<u32>(0x210, 0x500);
        for (i, unit) in "val=%d %s\0".encode_utf16().enumerate() {
            ctx.memory.write::<u16>(0x400 + i as u32 * 2, unit);
        }
        for (i, unit) in "hey\0".encode_utf16().enumerate() {
            ctx.memory.write::<u16>(0x500 + i as u32 * 2, unit);
        }
        assert_eq!(wsprintfW(&mut ctx), 10);
        assert_eq!(
            read_wstr(&ctx, 0x300),
            "val=42 hey".encode_utf16().collect::<Vec<_>>()
        );
    }
}
