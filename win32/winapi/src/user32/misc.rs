use runtime::Context;

use super::*;
use crate::{
    Ptr, RECT,
    gdi32::{self, HDC},
};

#[win32_derive::dllexport]
pub fn GetSystemMetrics(_ctx: &mut Context, nIndex: u32 /* SYSTEM_METRICS_INDEX */) -> i32 {
    // SM_CXSCREEN/SM_CYSCREEN track the current display mode; in this
    // single-window model that is the emulated window's client size, which
    // ChangeDisplaySettings and IDirectDraw::SetDisplayMode keep current.
    if nIndex <= 1
        && let Some(window) = state().window.borrow().as_ref()
    {
        let window = window.borrow();
        return if nIndex == 0 {
            window.width as i32
        } else {
            window.height as i32
        };
    }
    // These were dumped from a win2k VM running at 640x480.
    // See retrowin32's exe/cpp/metrics.cc.
    const METRICS: [i32; 100] = [
        640, 480, 16, 16, 19, 1, 1, 3, 3, 16, 16, 32, 32, 32, 32, 19, 640, 433, 0, 1, 16, 16, 0, 0,
        0, 0, 0, 0, 112, 27, 18, 18, 4, 4, 112, 27, 4, 4, 75, 75, 0, 0, 0, 5, 0, 2, 2, 160, 24, 16,
        16, 16, 12, 15, 18, 18, 8, 160, 24, 652, 492, 648, 460, 3, 0, 0, 0, 0, 4, 4, 0, 13, 13, 0,
        0, 1, 0, 0, 640, 480, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    // Out-of-range indices report 0 like Windows, not a host panic.
    METRICS.get(nIndex as usize).copied().unwrap_or(0)
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
        ctx.memory
            .bytes
            .get(lpchText.addr as usize..)
            .and_then(|b| b.get(..count))
            .map(|b| b.to_vec())
            .unwrap_or_default()
    };
    let Some(rect) = lprc.read(&ctx.memory) else {
        return 0;
    };
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

// DEVMODEA field offsets (dmDeviceName is 32 bytes, then the printer fields
// and dmFormName pad to here). All offsets are u32-aligned.
const DM_SIZE: u32 = 0x24;
const DM_FIELDS: u32 = 0x28;
const DM_BITSPERPEL_OFS: u32 = 0x68;
const DM_PELSWIDTH_OFS: u32 = 0x6c;
const DM_PELSHEIGHT_OFS: u32 = 0x70;
const DM_DISPLAYFREQUENCY_OFS: u32 = 0x78;
const DEVMODE_LEN: u32 = DM_DISPLAYFREQUENCY_OFS + 4;

/// The display modes a fixed 32bpp/60Hz adapter can offer, in the order
/// EnumDisplaySettings is expected to enumerate them.
const DISPLAY_MODES: [(u32, u32); 5] = [
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 1024),
    (1600, 1200),
];

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

    // Negative-one-sentinel indices query the current/registry mode; small
    // indices enumerate the list. Anything else reports the list exhausted.
    let (width, height) = match iModeNum {
        ENUM_CURRENT_SETTINGS | ENUM_REGISTRY_SETTINGS => DISPLAY_MODES[0],
        index if (index as usize) < DISPLAY_MODES.len() => DISPLAY_MODES[index as usize],
        _ => return false,
    };
    if !crate::ddraw::guest_range(ctx, lpDevMode.addr, DEVMODE_LEN) {
        return false;
    }
    if ctx.memory.read::<u16>(lpDevMode.addr + DM_SIZE) < 0x94 {
        return false;
    }

    ctx.memory.write::<u32>(
        lpDevMode.addr + DM_FIELDS,
        DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY,
    );
    ctx.memory
        .write::<u32>(lpDevMode.addr + DM_BITSPERPEL_OFS, 32);
    ctx.memory
        .write::<u32>(lpDevMode.addr + DM_PELSWIDTH_OFS, width);
    ctx.memory
        .write::<u32>(lpDevMode.addr + DM_PELSHEIGHT_OFS, height);
    ctx.memory
        .write::<u32>(lpDevMode.addr + DM_DISPLAYFREQUENCY_OFS, 60);
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
    if !crate::ddraw::guest_range(ctx, lpDevMode.addr, DEVMODE_LEN)
        || ctx.memory.read::<u16>(lpDevMode.addr + DM_SIZE) < 0x94
    {
        return DISP_CHANGE_BADMODE;
    }

    let fields = ctx.memory.read::<u32>(lpDevMode.addr + DM_FIELDS);
    if fields & !SUPPORTED_FIELDS != 0 {
        return DISP_CHANGE_BADMODE;
    }

    let bits_per_pixel = if fields & DM_BITSPERPEL != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + DM_BITSPERPEL_OFS)
    } else {
        32
    };
    let width = if fields & DM_PELSWIDTH != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + DM_PELSWIDTH_OFS)
    } else {
        640
    };
    let height = if fields & DM_PELSHEIGHT != 0 {
        ctx.memory.read::<u32>(lpDevMode.addr + DM_PELSHEIGHT_OFS)
    } else {
        480
    };
    let frequency = if fields & DM_DISPLAYFREQUENCY != 0 {
        ctx.memory
            .read::<u32>(lpDevMode.addr + DM_DISPLAYFREQUENCY_OFS)
    } else {
        60
    };

    // Only modes EnumDisplaySettings advertises can succeed; the others are
    // reported unsupported rather than resized to a shape nothing can show.
    if !DISPLAY_MODES.contains(&(width, height))
        || !matches!(bits_per_pixel, 8 | 16 | 32)
        || frequency != 60
    {
        return DISP_CHANGE_BADMODE;
    }

    if let Some(window) = state().window.borrow().as_ref() {
        window.borrow_mut().resize(ctx, width, height);
    }
    DISP_CHANGE_SUCCESSFUL
}

#[win32_derive::dllexport]
pub fn ShowCursor(_ctx: &mut Context, bShow: bool) -> i32 {
    state().show_cursor(bShow)
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
    // The bitmask isn't rendered, but the caller needs a real handle to
    // pass back to SetCursor.
    state().new_cursor()
}

#[win32_derive::dllexport]
pub fn GetCapture(_ctx: &mut Context) -> HWND {
    state().capture.get()
}

#[win32_derive::dllexport]
pub fn ReleaseCapture(_ctx: &mut Context) -> bool {
    state().capture.set(HWND::null());
    true
}

#[win32_derive::dllexport]
pub fn SetCapture(_ctx: &mut Context, hWnd: HWND) -> HWND {
    let prev = state().capture.get();
    state().capture.set(hWnd);
    prev
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
    // The model has no menus, so no item can be checked; the previous state
    // is always "unchecked".
    0
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
pub fn KillTimer(_ctx: &mut Context, hWnd: HWND, uIDEvent: u32) -> bool {
    state()
        .message_queue
        .borrow_mut()
        .kill_timer(hWnd, uIDEvent)
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
        if ptr.addr < 0x1000 {
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
    state().focused.set(hWnd);
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
    ctx: &mut Context,
    _hWnd: HWND,
    lpText: Ptr<u16>,    /* WSTR */
    lpCaption: Ptr<u16>, /* WSTR */
    _uType: u32,         /* MESSAGEBOX_STYLE */
) -> u32 /* MESSAGEBOX_RESULT */ {
    // We have no dialogs, but the C runtime reports fatal errors this way, so
    // the text is worth surfacing.
    let read = |ptr: Ptr<u16>| {
        if ptr.addr < 0x1000 {
            String::new()
        } else {
            String::from_utf16_lossy(ctx.memory.read_wstr(ptr.addr).as_slice())
        }
    };
    log::warn!("MessageBox: {} / {}", read(lpCaption), read(lpText));
    const IDOK: u32 = 1;
    IDOK
}

#[win32_derive::dllexport]
pub fn SetMenu(_ctx: &mut Context, _hWnd: HWND, _hMenu: HMENU) -> bool {
    // The model has no menu bar; accepting the call reports success without
    // changing anything the window can display.
    true
}

#[win32_derive::dllexport]
pub fn SetTimer(
    _ctx: &mut Context,
    hWnd: HWND,
    nIDEvent: u32,
    uElapse: u32,
    lpTimerFunc: Ptr<()>, /* TIMERPROC */
) -> u32 {
    // A non-null hWnd must name our window; anything else fails.
    if !hWnd.is_null()
        && state()
            .window
            .borrow()
            .as_ref()
            .is_none_or(|window| window.borrow().hwnd != hWnd)
    {
        return 0;
    }
    // A low non-null TIMERPROC would dispatch to a missing block and halt.
    if lpTimerFunc.addr != 0 && lpTimerFunc.addr < 0x1000 {
        return 0;
    }
    // host() can initialize SDL on first use; do not let that happen while
    // the shared queue's RefCell is borrowed.
    let now = host::host().time();
    state()
        .message_queue
        .borrow_mut()
        .set_timer(hWnd, nIDEvent, uElapse, lpTimerFunc.addr, now)
}

/// Read one u32 off the guest stack without panicking on a bad stack
/// pointer; unlike `Ptr::read` this does not reject the null page, since
/// a cdecl-varargs caller's frame is whatever the guest made it.
fn read_stack_u32(ctx: &Context, addr: u32) -> Option<u32> {
    ctx.memory
        .bytes
        .get(addr as usize..)
        .and_then(|b| b.get(..4))
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Read a NUL-terminated byte string without the UTF-8 check.
fn read_bytes0(ctx: &Context, addr: u32) -> Vec<u8> {
    let Some(buf) = ctx.memory.bytes.get(addr as usize..) else {
        return Vec::new();
    };
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
            // A format with more specifiers than the caller pushed args
            // walks arg_addr off emulated memory; read 0 rather than
            // panicking.
            let value = ctx
                .memory
                .bytes
                .get(arg_addr as usize..)
                .and_then(|b| b.get(..4))
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .unwrap_or(0);
            arg_addr = arg_addr.wrapping_add(4);
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
                if addr < 0x1000 {
                    Vec::new()
                } else if wide {
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
            let padding = std::iter::repeat_n(pad, width - formatted.len());
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
    let Some(dst) = read_stack_u32(ctx, esp.wrapping_add(4)) else {
        return 0;
    };
    let Some(fmt_addr) = read_stack_u32(ctx, esp.wrapping_add(8)) else {
        return 0;
    };
    // A format string in the null page produces no output, and avoids the
    // null-page read error `read_wstr` would log.
    if fmt_addr < 0x1000 {
        return 0;
    }
    let fmt = ctx.memory.read_wstr(fmt_addr).as_slice().to_vec();
    let out = wsprintf_impl(ctx, &fmt, esp.wrapping_add(12), true);
    // wsprintf takes no size; a destination in the null page fails, and one
    // at the edge of emulated memory writes only what fits.
    if dst < 0x1000 {
        return 0;
    }
    if let Some(buf) = ctx.memory.bytes.get_mut(dst as usize..) {
        let mut chunks = buf.chunks_exact_mut(2);
        // Keep a unit free for the terminator so a truncated result still
        // reads as a string.
        let spare = chunks.len().saturating_sub(1);
        for (chunk, unit) in chunks.by_ref().zip(out.iter().take(spare)) {
            chunk.copy_from_slice(&unit.to_le_bytes());
        }
        if let Some(chunk) = chunks.next() {
            chunk.copy_from_slice(&[0, 0]);
        }
    }
    out.len() as i32
}

// XXX: cdecl
#[win32_derive::dllexport]
pub fn wsprintfA(ctx: &mut Context) -> i32 {
    // Cdecl varargs: declared with no args so the wrapper leaves the caller's
    // stack alone; read everything manually.
    // [esp] = return addr, [esp+4] = dst, [esp+8] = fmt, [esp+12...] = args.
    let esp = ctx.cpu.regs.esp;
    let Some(dst) = read_stack_u32(ctx, esp.wrapping_add(4)) else {
        return 0;
    };
    let Some(fmt_addr) = read_stack_u32(ctx, esp.wrapping_add(8)) else {
        return 0;
    };
    // A format string in the null page produces no output.
    if fmt_addr < 0x1000 {
        return 0;
    }
    let fmt = read_bytes0(ctx, fmt_addr)
        .iter()
        .map(|&b| b as u16)
        .collect::<Vec<_>>();
    let out = wsprintf_impl(ctx, &fmt, esp.wrapping_add(12), false);

    let bytes: Vec<u8> = out.iter().map(|&c| c as u8).collect();
    // wsprintf takes no size; a destination in the null page fails, and one
    // at the edge of emulated memory writes only what fits.
    if dst < 0x1000 {
        return 0;
    }
    if let Some(buf) = ctx.memory.bytes.get_mut(dst as usize..) {
        // Keep a byte free for the terminator so a truncated result still
        // reads as a string.
        let written = bytes.len().min(buf.len().saturating_sub(1));
        buf[..written].copy_from_slice(&bytes[..written]);
        if let Some(slot) = buf.get_mut(written) {
            *slot = 0;
        }
    }
    bytes.len() as i32
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeDisplaySettingsA, EnumDisplaySettingsA, GetSystemMetrics, wsprintfA, wsprintfW,
    };
    use crate::Ptr;
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
        let buf = ctx.memory.bytes.get(addr as usize..).unwrap_or(&[]);
        let Some(nul) = buf.iter().position(|&b| b == 0) else {
            return String::new();
        };
        std::str::from_utf8(&buf[..nul])
            .unwrap_or_default()
            .to_string()
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
        // The stack, format, and string buffers sit above the null page like
        // real guest data.
        let mut ctx = context();
        // [esp]=ret, [esp+4]=dst, [esp+8]=fmt, [esp+12...]=args.
        ctx.cpu.regs.esp = 0x2000;
        ctx.memory.write::<u32>(0x2000, 0); // return addr
        ctx.memory.write::<u32>(0x2004, 0x3000); // dst
        ctx.memory.write::<u32>(0x2008, 0x3400); // fmt
        ctx.memory.write::<u32>(0x200c, 42); // %d arg
        ctx.memory.write::<u32>(0x2010, 0x3800); // %s arg
        ctx.memory[0x3400..][..12].copy_from_slice(b"val=%d %s\0\0\0");
        ctx.memory[0x3800..][..4].copy_from_slice(b"hey\0");
        assert_eq!(wsprintfA(&mut ctx), 10);
        assert_eq!(read_cstr(&ctx, 0x3000), "val=42 hey");

        let mut ctx = context();
        ctx.cpu.regs.esp = 0x2000;
        ctx.memory.write::<u32>(0x2000, 0);
        ctx.memory.write::<u32>(0x2004, 0x3000);
        ctx.memory.write::<u32>(0x2008, 0x3400);
        ctx.memory.write::<u32>(0x200c, 42);
        ctx.memory.write::<u32>(0x2010, 0x3800);
        for (i, unit) in "val=%d %s\0".encode_utf16().enumerate() {
            ctx.memory.write::<u16>(0x3400 + i as u32 * 2, unit);
        }
        for (i, unit) in "hey\0".encode_utf16().enumerate() {
            ctx.memory.write::<u16>(0x3800 + i as u32 * 2, unit);
        }
        assert_eq!(wsprintfW(&mut ctx), 10);
        assert_eq!(
            read_wstr(&ctx, 0x3000),
            "val=42 hey".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn get_system_metrics_rejects_out_of_range_indices() {
        // A concurrently-installed test window would change the defaults.
        let _guard = crate::user32::WINDOW_STATE_LOCK.lock().unwrap();
        let mut ctx = context();
        assert_eq!(GetSystemMetrics(&mut ctx, 0), 640); // SM_CXSCREEN
        assert_eq!(GetSystemMetrics(&mut ctx, 1), 480); // SM_CYSCREEN
        // Indices past the table report 0 like Windows, not a panic.
        assert_eq!(GetSystemMetrics(&mut ctx, 100), 0);
        assert_eq!(GetSystemMetrics(&mut ctx, u32::MAX), 0);
    }

    #[test]
    fn system_metrics_track_the_current_display_mode() {
        let _guard = crate::user32::WINDOW_STATE_LOCK.lock().unwrap();
        // Insert the emulated window directly: CreateWindowExA would touch
        // SDL's main-thread-only window APIs.
        let mut ctx = context();
        let window = std::rc::Rc::new(std::cell::RefCell::new(crate::user32::Window {
            hwnd: crate::user32::HWND::from_raw(1),
            style: 0,
            ex_style: 0,
            dirty: false,
            title: String::new(),
            enabled: true,
            visible: false,
            user_data: 0,
            hinstance: 0,
            id: 0,
            subclass_proc: None,
            paint_dc: None,
            x: 0,
            y: 0,
            width: 1024,
            height: 768,
            pixels: None,
            host: unsafe { std::mem::zeroed() },
            surface: None,
        }));
        crate::user32::state().window.borrow_mut().replace(window);
        assert_eq!(GetSystemMetrics(&mut ctx, 0), 1024); // SM_CXSCREEN
        assert_eq!(GetSystemMetrics(&mut ctx, 1), 768); // SM_CYSCREEN
        crate::user32::state().window.borrow_mut().take();
    }

    #[test]
    fn display_settings_enumerate_then_change() {
        let mut ctx = context();
        let devmode = || Ptr::<u8>::new(0x1000);
        // dmSize must cover the fields the APIs touch.
        ctx.memory.write::<u16>(0x1000 + 0x24, 0x9c);

        // Indexed enumeration walks the advertised list, then stops.
        assert!(EnumDisplaySettingsA(&mut ctx, Ptr::new(0), 0, devmode()));
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x68), 32); // dmBitsPerPel
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x6c), 640); // dmPelsWidth
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x70), 480); // dmPelsHeight
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x78), 60); // dmDisplayFrequency
        assert!(EnumDisplaySettingsA(&mut ctx, Ptr::new(0), 4, devmode()));
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x6c), 1600);
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 0x70), 1200);
        assert!(!EnumDisplaySettingsA(&mut ctx, Ptr::new(0), 5, devmode()));

        // An enumerated DEVMODE goes straight back to ChangeDisplaySettings.
        assert_eq!(ChangeDisplaySettingsA(&mut ctx, devmode(), 0), 0);

        // A resolution we never advertised is rejected, not resized to.
        ctx.memory.write::<u32>(0x1000 + 0x6c, 720);
        ctx.memory.write::<u32>(0x1000 + 0x70, 576);
        assert_eq!(ChangeDisplaySettingsA(&mut ctx, devmode(), 0), -2);
    }

    #[test]
    fn wsprintf_tolerates_an_out_of_range_destination() {
        let mut ctx = context();
        let len = ctx.memory.bytes.len() as u32;
        ctx.cpu.regs.esp = 0x2000;
        ctx.memory.write::<u32>(0x2000, 0);
        ctx.memory.write::<u32>(0x2004, len - 4); // only four bytes left
        ctx.memory.write::<u32>(0x2008, 0x3400);
        ctx.memory[0x3400..][..8].copy_from_slice(b"val=%d!\0");
        ctx.memory.write::<u32>(0x200c, 42);
        // Truncated to fit instead of panicking; the count is still honest.
        assert_eq!(wsprintfA(&mut ctx), 7);
        assert_eq!(read_cstr(&ctx, len - 4), "val");

        ctx.memory.write::<u32>(0x2004, len + 0x100); // wholly out of range
        assert_eq!(wsprintfA(&mut ctx), 7);

        // A destination in the null page fails instead of writing.
        ctx.memory.write::<u32>(0x2004, 0x500);
        assert_eq!(wsprintfA(&mut ctx), 0);

        // A low %s argument formats as an empty string.
        ctx.memory.write::<u32>(0x2004, 0x3000);
        ctx.memory[0x3400..][..5].copy_from_slice(b"s=%s\0");
        ctx.memory.write::<u32>(0x200c, 0x500);
        assert_eq!(wsprintfA(&mut ctx), 2);
        assert_eq!(read_cstr(&ctx, 0x3000), "s=");

        // A format pointer in the null page fails for both widths.
        ctx.memory.write::<u32>(0x2008, 0x500);
        assert_eq!(wsprintfA(&mut ctx), 0);
        assert_eq!(wsprintfW(&mut ctx), 0);
        ctx.memory.write::<u32>(0x2008, 0);
        assert_eq!(wsprintfA(&mut ctx), 0);
        assert_eq!(wsprintfW(&mut ctx), 0);
    }
}
