use std::{cell::RefCell, rc::Rc};

use runtime::Context;

use crate::{
    FromABIParam, POINT, Ptr, RECT,
    gdi32::{self, Brush, COLORREF, DC, HBRUSH, HDC},
    kernel32,
    user32::{self, HCURSOR, HICON, HINSTANCE, HMENU, HWND, State, WM, state},
};

pub struct Window {
    /// There is a single unique HWND for each window, it's not a refcounted handle.
    pub hwnd: HWND,
    pub style: u32,
    pub ex_style: u32,
    pub dirty: bool, // triggers WM_PAINT
    /// SetWindowText/GetWindowText title text.
    pub title: String,
    /// Keyboard/mouse input enable state from EnableWindow.
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Option<u32>,
    pub host: host::Window,
    pub surface: Option<host::Surface>,
}

/// The window's pixel buffer comes out of the 64 MiB process heap and the
/// dims reach the host's real window, so bound them: 0x4000 per side is
/// the largest square whose RGBA size still fits a u32.
const MAX_WINDOW_DIM: u32 = 0x4000;

impl Window {
    pub fn resize(&mut self, ctx: &mut Context, width: u32, height: u32) {
        let width = width.min(MAX_WINDOW_DIM);
        let height = height.min(MAX_WINDOW_DIM);
        self.width = width;
        self.height = height;
        self.host.resize(width, height);
        if let Some(pixels) = self.pixels {
            kernel32::lock().process_heap.free(&mut ctx.memory, pixels);
            self.pixels = None;
            self.surface = None;
        }
    }

    pub fn rect(&self) -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        }
    }

    /// The guest address of the window's RGBA backing, allocated on first
    /// use. `None` when the process heap cannot satisfy the request.
    pub fn ensure_pixels(&mut self, ctx: &mut Context) -> Option<u32> {
        if self.pixels.is_none() {
            // Two clamped-to-0x4000 dims overflow a u32 product; a
            // window that large simply cannot get a backing buffer.
            let size = u32::try_from(self.width as u64 * self.height as u64 * 4).ok()?;
            self.pixels = kernel32::lock()
                .process_heap
                .try_alloc(&mut ctx.memory, size);
        }
        self.pixels
    }

    pub fn flush(&mut self, ctx: &mut Context) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let stride = self.width * 4;
        let Some(pixels) = self.pixels else {
            // Nothing was ever drawn into this window's buffer.
            return;
        };
        let pixel_bytes = self.height as usize * stride as usize;
        let Some(pixels) = ctx
            .memory
            .bytes
            .get_mut(pixels as usize..pixels as usize + pixel_bytes)
        else {
            log::warn!("window pixel buffer out of range; skipping flush");
            return;
        };
        let surface = self
            .surface
            .get_or_insert_with(|| self.host.create_surface(self.width, self.height));
        surface.set_pixels(pixels, stride);
        self.host.render(surface);
    }
}

#[derive(Default)]
struct CreateWindowArgs {
    name: String,
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
}

const CW_USEDEFAULT: u32 = 0x8000_0000;

pub struct CW(u32);
impl CW {
    fn value(&self) -> Option<u32> {
        if self.0 == CW_USEDEFAULT {
            None
        } else {
            Some(self.0)
        }
    }
}
impl FromABIParam for CW {
    fn from_abi(val: u32) -> Self {
        Self(val)
    }
}
impl std::fmt::Debug for CW {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 == CW_USEDEFAULT {
            write!(f, "CW_USEDEFAULT")
        } else {
            write!(f, "{:#x}", self.0)
        }
    }
}

impl State {
    fn create_window(&self, args: CreateWindowArgs) -> HWND {
        let width = args.width.unwrap_or(640).min(MAX_WINDOW_DIM);
        let height = args.height.unwrap_or(480).min(MAX_WINDOW_DIM);

        let hwnd = HWND::from_raw(1);
        let window = Rc::new(RefCell::new(Window {
            hwnd,
            style: args.style,
            ex_style: args.ex_style,
            dirty: true,
            title: args.name.clone(),
            enabled: true,
            x: args.x,
            y: args.y,
            width,
            height,
            host: host::host().create_window(&args.name, width, height),
            pixels: None,
            surface: None,
        }));
        *self.window.borrow_mut() = Some(window.clone());
        self.message_queue.borrow_mut().window = Some(window);
        hwnd
    }
}

#[win32_derive::dllexport]
pub fn CreateWindowExA(
    ctx: &mut Context,
    dwExStyle: u32, /* WINDOW_EX_STYLE */
    _lpClassName: Ptr<u8>,
    lpWindowName: Ptr<u8>,
    dwStyle: u32, /* WINDOW_STYLE */
    X: i32,
    Y: i32,
    nWidth: CW,
    nHeight: CW,
    _hWndParent: HWND,
    _hMenu: HMENU,
    _hInstance: HINSTANCE,
    _lpParam: Ptr<()>,
) -> HWND {
    let name = ctx.memory.read_str(lpWindowName.addr);
    state().create_window(CreateWindowArgs {
        name: name.into(),
        style: dwStyle,
        ex_style: dwExStyle,
        x: X,
        y: Y,
        width: nWidth.value(),
        height: nHeight.value(),
    })
}

#[win32_derive::dllexport]
pub fn CreateWindowExW(
    ctx: &mut Context,
    dwExStyle: u32,         /* WINDOW_EX_STYLE */
    _lpClassName: Ptr<u16>, /* WSTR */
    lpWindowName: Ptr<u16>, /* WSTR */
    dwStyle: u32,           /* WINDOW_STYLE */
    X: i32,
    Y: i32,
    nWidth: CW,
    nHeight: CW,
    _hWndParent: HWND,
    _hMenu: HMENU,
    _hInstance: HINSTANCE,
    _lpParam: Ptr<()>,
) -> HWND {
    let name = ctx.memory.read_wstr(lpWindowName.addr);
    state().create_window(CreateWindowArgs {
        name: name.to_string_lossy(),
        style: dwStyle,
        ex_style: dwExStyle,
        x: X,
        y: Y,
        width: nWidth.value(),
        height: nHeight.value(),
    })
}

#[win32_derive::dllexport]
pub fn IsWindow(_ctx: &mut Context, hWnd: HWND) -> bool {
    let window = state().window.borrow();
    window
        .as_ref()
        .is_some_and(|window| window.borrow().hwnd == hWnd)
}

#[win32_derive::dllexport]
pub fn GetWindowLongA(_ctx: &mut Context, hWnd: HWND, nIndex: i32) -> i32 {
    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;

    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return 0;
    };
    let window = window.borrow();
    if hWnd != window.hwnd {
        return 0;
    }

    match nIndex {
        GWL_STYLE => window.style as i32,
        GWL_EXSTYLE => window.ex_style as i32,
        _ => {
            log::warn!("GetWindowLongA: unsupported index {nIndex}");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn SetWindowLongA(_ctx: &mut Context, hWnd: HWND, nIndex: i32, dwNewLong: i32) -> i32 {
    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;

    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return 0;
    };
    let mut window = window.borrow_mut();
    if hWnd != window.hwnd {
        return 0;
    }

    match nIndex {
        GWL_STYLE => std::mem::replace(&mut window.style, dwNewLong as u32) as i32,
        GWL_EXSTYLE => std::mem::replace(&mut window.ex_style, dwNewLong as u32) as i32,
        _ => {
            log::warn!("SetWindowLongA: unsupported index {nIndex}");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn DestroyWindow(_ctx: &mut Context, hWnd: HWND) -> bool {
    let state = state();
    let matches = state
        .window
        .borrow()
        .as_ref()
        .is_some_and(|window| window.borrow().hwnd == hWnd);
    if !matches {
        return false;
    }
    if state.focused.get() == hWnd {
        state.focused.set(HWND::null());
    }
    state.window.borrow_mut().take();
    state.message_queue.borrow_mut().window = None;
    true
}

#[win32_derive::dllexport]
pub fn ShowWindow(
    _ctx: &mut Context,
    hWnd: HWND,
    _nCmdShow: u32, /* SHOW_WINDOW_CMD */
) -> bool {
    // The window comes up focused; games often wait for activation before
    // running their main loop.
    use super::message::{WM, post_message};
    post_message(hWnd, WM::SHOWWINDOW as u32, 1, 0);
    post_message(hWnd, WM::ACTIVATEAPP as u32, 1, 0);
    post_message(hWnd, WM::ACTIVATE as u32, 1, 0); // WA_ACTIVE
    post_message(hWnd, WM::SETFOCUS as u32, 0, 0);
    let state = state();
    let is_window = state
        .window
        .borrow()
        .as_ref()
        .is_some_and(|window| window.borrow().hwnd == hWnd);
    if is_window {
        state.focused.set(hWnd);
    }
    true
}

#[win32_derive::dllexport]
pub fn SetForegroundWindow(_ctx: &mut Context, hWnd: HWND) -> bool {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    if window.borrow().hwnd != hWnd {
        return false;
    }

    use super::message::{WM, post_message};
    post_message(hWnd, WM::ACTIVATEAPP as u32, 1, 0);
    post_message(hWnd, WM::ACTIVATE as u32, 1, 0);
    post_message(hWnd, WM::SETFOCUS as u32, 0, 0);
    state().focused.set(hWnd);
    true
}

#[win32_derive::dllexport]
pub fn MoveWindow(
    ctx: &mut Context,
    hWnd: HWND,
    X: i32,
    Y: i32,
    nWidth: i32,
    nHeight: i32,
    bRepaint: bool,
) -> bool {
    let state = state();
    let window = state.window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    window.x = X;
    window.y = Y;
    // A negative size clamps to zero like SetWindowPos, not up to
    // MAX_WINDOW_DIM via the u32 cast.
    window.resize(ctx, nWidth.max(0) as u32, nHeight.max(0) as u32);
    if bRepaint {
        // ...
    };
    true // sucess
}

#[win32_derive::dllexport]
pub fn UpdateWindow(_ctx: &mut Context, hWnd: HWND) -> bool {
    // A dirty window gets a WM_PAINT from the next queue pump, which is
    // what UpdateWindow's synchronous paint achieves here.
    let state = state();
    let window = state.window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    window.dirty = true;
    true
}

#[win32_derive::dllexport]
pub fn DefWindowProcA(
    ctx: &mut Context,
    hWnd: HWND,
    msg: Result<WM, u32>,
    wParam: u32,
    lParam: u32,
) -> u32 {
    DefWindowProcW(ctx, hWnd, msg, wParam, lParam)
}

#[win32_derive::dllexport]
pub fn DefWindowProcW(
    ctx: &mut Context,
    hWnd: HWND,
    msg: Result<WM, u32>,
    _wParam: u32,
    _lParam: u32,
) -> u32 {
    let msg = match msg {
        Ok(msg) => msg,
        Err(n) => {
            log::warn!("DefWindowProc: unhandled message type {:x}", n);
            return 0;
        }
    };

    match msg {
        WM::PAINT => {
            if let Some(window) = state().window.borrow().as_ref() {
                window.borrow_mut().dirty = false;
            }
        }
        WM::ERASEBKGND => {
            let window = state().window.borrow();
            let Some(window) = window.as_ref() else {
                return 0;
            };
            let mut window = window.borrow_mut();
            if window.hwnd != hWnd {
                return 0;
            }
            let wndclass = state().wndclass.borrow();
            let Some(wndclass) = wndclass.as_ref() else {
                return 0;
            };
            let Some(color) = wndclass.background.as_ref().and_then(|b| b.0) else {
                return 0;
            };
            let Some(pixels) = window.ensure_pixels(ctx) else {
                return 0;
            };
            let pixel_count = (window.width * window.height) as usize;
            let Some(buf) = ctx
                .memory
                .bytes
                .get_mut(pixels as usize..(pixels + pixel_count as u32 * 4) as usize)
            else {
                return 0;
            };
            use zerocopy::FromBytes;
            let Ok(pixels) = <[[u8; 4]]>::mut_from_bytes_with_elems(buf, pixel_count) else {
                return 0;
            };
            pixels.fill(color.to_pixel());
            return 1;
        }
        _ => {}
    }
    0
}

#[win32_derive::dllexport]
pub fn SetFocus(_ctx: &mut Context, hWnd: HWND) -> HWND {
    let state = state();
    let valid = hWnd.is_null()
        || state
            .window
            .borrow()
            .as_ref()
            .is_some_and(|window| window.borrow().hwnd == hWnd);
    if !valid {
        return HWND::null();
    }

    let previous = state.focused.replace(hWnd);
    if previous != hWnd {
        use super::message::{WM, post_message};
        if let Some(previous) = previous.to_option() {
            post_message(previous, WM::KILLFOCUS as u32, hWnd.to_raw(), 0);
        }
        if let Some(hWnd) = hWnd.to_option() {
            post_message(hWnd, WM::SETFOCUS as u32, previous.to_raw(), 0);
        }
    }
    previous
}

#[win32_derive::dllexport]
pub fn GetFocus(_ctx: &mut Context) -> HWND {
    state().focused.get()
}

#[repr(C)]
#[derive(Debug, zerocopy::FromBytes)]
pub struct WNDCLASS {
    style: u32,       /* WNDCLASS_STYLES */
    lpfnWndProc: u32, /* WNDPROC */
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: HINSTANCE,
    hIcon: HICON,
    hCursor: HCURSOR,
    hbrBackground: HBRUSH,
    lpszMenuName: u32,
    lpszClassName: u32,
}

/// A class is registered under a string name or an atom in the low word.
#[derive(Debug, PartialEq)]
pub enum ClassName {
    Atom(u16),
    Name(String),
}

pub struct WndClass {
    pub wndproc: runtime::Cont,
    pub background: Option<gdi32::Brush>,
    /// The name or atom the class was registered under, so GetClassName and
    /// UnregisterClass can match it.
    pub name: Option<ClassName>,
    /// The atom handed out by RegisterClass, also accepted by UnregisterClass.
    pub atom: u16,
}

impl State {
    pub fn register_class(&self, mut wnd_class: WndClass) -> u16 {
        let atom = self.next_class_atom.get();
        // Wrap within the class-atom range rather than overflowing the
        // counter when a guest registers classes in a loop.
        self.next_class_atom
            .set(if atom == u16::MAX { 0xC000 } else { atom + 1 });
        wnd_class.atom = atom;
        *self.wndclass.borrow_mut() = Some(wnd_class);
        atom
    }
}

/// COLOR_xxx for GetSysColor etc.
#[derive(Debug, Eq, PartialEq, win32_derive::ABIEnum)]
pub enum COLOR {
    SCROLLBAR = 0,
    BACKGROUND = 1,
    ACTIVECAPTION = 2,
    INACTIVECAPTION = 3,
    MENU = 4,
    WINDOW = 5,
    WINDOWFRAME = 6,
    MENUTEXT = 7,
    WINDOWTEXT = 8,
    CAPTIONTEXT = 9,
    ACTIVEBORDER = 10,
    INACTIVEBORDER = 11,
    APPWORKSPACE = 12,
    HIGHLIGHT = 13,
    HIGHLIGHTTEXT = 14,
    BTNFACE = 15,
    BTNSHADOW = 16,
    GRAYTEXT = 17,
    BTNTEXT = 18,
    INACTIVECAPTIONTEXT = 19,
    BTNHIGHLIGHT = 20,
}

impl COLOR {
    /// The standard Windows default color scheme; the window-frame family
    /// keeps the existing silver choice used for class backgrounds.
    fn to_colorref(&self) -> COLORREF {
        use COLOR::*;
        match self {
            SCROLLBAR | MENU | WINDOW | WINDOWFRAME | ACTIVEBORDER | INACTIVEBORDER | BTNFACE
            | INACTIVECAPTIONTEXT => COLORREF::from_rgb(0xc0, 0xc0, 0xc0),
            BACKGROUND => COLORREF::from_rgb(0x00, 0x80, 0x80),
            ACTIVECAPTION | HIGHLIGHT => COLORREF::from_rgb(0x00, 0x00, 0x80),
            INACTIVECAPTION | APPWORKSPACE | BTNSHADOW | GRAYTEXT => {
                COLORREF::from_rgb(0x80, 0x80, 0x80)
            }
            MENUTEXT | WINDOWTEXT | BTNTEXT => COLORREF::from_rgb(0x00, 0x00, 0x00),
            CAPTIONTEXT | HIGHLIGHTTEXT | BTNHIGHLIGHT => COLORREF::from_rgb(0xff, 0xff, 0xff),
        }
    }
}

#[win32_derive::dllexport]
pub fn RegisterClassA(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>) -> u16 {
    register_class(ctx, lpWndClass, false)
}

#[win32_derive::dllexport]
pub fn RegisterClassW(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>) -> u16 {
    register_class(ctx, lpWndClass, true)
}

fn register_class(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>, wide: bool) -> u16 {
    let Some(wndclass) = lpWndClass.read(&ctx.memory) else {
        return 0;
    };
    // A class without a window procedure is invalid; resolving the address
    // would panic on null.
    if wndclass.lpfnWndProc == 0 {
        return 0;
    }
    let background = if wndclass.hbrBackground.is_null() {
        None
    } else if wndclass.hbrBackground.to_raw() < 32 {
        // An out-of-range system color index can't be mapped to a brush;
        // treat it like a null background rather than rejecting the class.
        COLOR::try_from(wndclass.hbrBackground.to_raw())
            .ok()
            .map(|color| Brush(Some(color.to_colorref())))
    } else {
        match gdi32::lock().objects.get(wndclass.hbrBackground) {
            Some(gdi32::Object::Brush(brush)) => Some(brush.clone()),
            _ => return 0,
        }
    };
    // lpszClassName is a string pointer, or an atom in the low word.
    let name = if wndclass.lpszClassName == 0 {
        None
    } else if wndclass.lpszClassName >> 16 == 0 {
        Some(ClassName::Atom(wndclass.lpszClassName as u16))
    } else if wide {
        Some(ClassName::Name(
            ctx.memory
                .read_wstr(wndclass.lpszClassName)
                .to_string_lossy(),
        ))
    } else {
        Some(ClassName::Name(
            ctx.memory.read_str(wndclass.lpszClassName).to_owned(),
        ))
    };
    state().register_class(WndClass {
        wndproc: ctx.indirect(wndclass.lpfnWndProc),
        background,
        name,
        atom: 0,
    })
}

fn unregister_class(ctx: &mut Context, addr: u32, wide: bool) -> bool {
    let mut slot = state().wndclass.borrow_mut();
    let Some(class) = slot.as_ref() else {
        return false;
    };
    let matches = if addr >> 16 == 0 {
        // An atom value matches the atom this class was registered under or
        // the atom RegisterClass returned.
        matches!(&class.name, Some(ClassName::Atom(a)) if *a == addr as u16)
            || class.atom == addr as u16
    } else {
        let queried = if wide {
            ctx.memory.read_wstr(addr).to_string_lossy()
        } else {
            ctx.memory.read_str(addr).to_owned()
        };
        matches!(&class.name, Some(ClassName::Name(name)) if name.eq_ignore_ascii_case(&queried))
    };
    if matches {
        slot.take();
    }
    matches
}

#[win32_derive::dllexport]
pub fn UnregisterClassA(ctx: &mut Context, lpClassName: Ptr<u8>, _hInstance: HINSTANCE) -> bool {
    unregister_class(ctx, lpClassName.addr, false)
}

#[win32_derive::dllexport]
pub fn UnregisterClassW(ctx: &mut Context, lpClassName: Ptr<u16>, _hInstance: HINSTANCE) -> bool {
    unregister_class(ctx, lpClassName.addr, true)
}

#[repr(C)]
#[derive(Debug, zerocopy::IntoBytes, zerocopy::Immutable, zerocopy::FromBytes)]
pub struct PAINTSTRUCT {
    hdc: HDC,
    fErase: u32,
    rcPaint: RECT,
    reserved: [u32; 10],
}

#[win32_derive::dllexport]
pub fn BeginPaint(ctx: &mut Context, hWnd: HWND, lpPaint: Ptr<PAINTSTRUCT>) -> HDC {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return HDC::null();
    };
    let window = window.borrow();
    if window.hwnd != hWnd {
        return HDC::null();
    }

    let has_brush = {
        let wndclass = state().wndclass.borrow();
        wndclass
            .as_ref()
            .and_then(|w| w.background.as_ref())
            .is_some()
    };
    let rcPaint = window.rect();
    drop(window);

    let hdc = GetDC(ctx, hWnd);
    if hdc.is_null() {
        return hdc;
    }

    let erased = if has_brush {
        let ret = user32::SendMessageW(ctx, hWnd, WM::ERASEBKGND as u32, hdc.to_raw(), 0);
        if ret == 0 {
            DefWindowProcW(ctx, hWnd, Ok(WM::ERASEBKGND), hdc.to_raw(), 0) != 0
        } else {
            true
        }
    } else {
        false
    };

    if lpPaint
        .write(
            &mut ctx.memory,
            PAINTSTRUCT {
                hdc,
                fErase: !erased as u32,
                rcPaint,
                reserved: [0; 10],
            },
        )
        .is_none()
    {
        gdi32::lock().release_dc(hdc);
        return HDC::null();
    }
    hdc
}

#[win32_derive::dllexport]
pub fn EndPaint(ctx: &mut Context, hWnd: HWND, lpPaint: Ptr<PAINTSTRUCT>) -> bool {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    let Some(paint) = lpPaint.read(&ctx.memory) else {
        return false;
    };
    gdi32::lock().release_dc(paint.hdc);
    window.dirty = false;
    window.flush(ctx);
    true
}

#[win32_derive::dllexport]
pub fn GetDC(ctx: &mut Context, hWnd: HWND) -> HDC {
    if hWnd.is_null() {
        // A null HWND asks for a DC covering the whole screen; there is no
        // desktop to draw on, so hand out a screen-sized memory DC.
        let Some(pixels) = kernel32::lock()
            .process_heap
            .try_alloc(&mut ctx.memory, 640 * 480 * 4)
        else {
            return HDC::null();
        };
        let bitmap = gdi32::Bitmap::new_simple(640, 480, pixels);
        return gdi32::lock().new_memory_dc(bitmap);
    }

    let state = state();
    let window = state.window.borrow();
    let Some(window) = window.as_ref() else {
        return HDC::null();
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return HDC::null();
    }

    let Some(pixels) = window.ensure_pixels(ctx) else {
        return HDC::null();
    };
    let bitmap = gdi32::Bitmap::new_simple(window.width, window.height, pixels);

    let mut lock = gdi32::lock();
    let (hbitmap, bitmap) = lock.new_bitmap_handle(bitmap);
    let dc = DC::new(hbitmap, bitmap, &mut lock.objects);
    // dc.hwnd = Some(hWnd);
    lock.dcs.add(dc)
}

#[win32_derive::dllexport]
pub fn ReleaseDC(ctx: &mut Context, hWnd: HWND, hDC: HDC) -> i32 {
    if !hWnd.is_null() {
        let window = user32::state().window.borrow();
        if let Some(window) = window.as_ref() {
            window.borrow_mut().flush(ctx);
        }
    }
    gdi32::lock().release_dc(hDC);
    1 // success
}

#[win32_derive::dllexport]
pub fn InvalidateRect(_ctx: &mut Context, hWnd: HWND, _lpRect: Ptr<RECT>, _bErase: bool) -> bool {
    let window = user32::state().window.borrow();
    let Some(window) = window.as_ref() else {
        // No window exists yet; only the whole-screen null form succeeds.
        return hWnd.is_null();
    };
    let mut window = window.borrow_mut();
    // A null hWnd invalidates the whole screen; otherwise it must name the
    // one emulated window.
    if !hWnd.is_null() && window.hwnd != hWnd {
        return false;
    }
    window.dirty = true;
    true
}

#[win32_derive::dllexport]
pub fn GetDesktopWindow(_ctx: &mut Context) -> HWND {
    // The model's desktop is the null handle: GetDC(NULL), MapWindowPoints and
    // friends all treat it as the screen.
    HWND::null()
}

#[win32_derive::dllexport]
pub fn GetClientRect(ctx: &mut Context, hWnd: HWND, lpRect: Ptr<RECT>) -> bool {
    let rect = {
        let window = state().window.borrow();
        let Some(window) = window.as_ref() else {
            return false;
        };
        let window = window.borrow();
        if window.hwnd != hWnd {
            return false;
        }
        window.rect()
    };
    lpRect.write(&mut ctx.memory, rect).is_some()
}

#[win32_derive::dllexport]
pub fn GetWindowRect(ctx: &mut Context, hWnd: HWND, lpRect: Ptr<RECT>) -> bool {
    let rect = {
        let window = state().window.borrow();
        let Some(window) = window.as_ref() else {
            return false;
        };
        let window = window.borrow();
        if hWnd != window.hwnd {
            return false;
        }
        window.rect().add(POINT {
            x: window.x,
            y: window.y,
        })
    };
    lpRect.write(&mut ctx.memory, rect).is_some()
}

#[win32_derive::dllexport]
pub fn SetWindowPos(
    ctx: &mut Context,
    hWnd: HWND,
    _hWndInsertAfter: u32,
    X: i32,
    Y: i32,
    cx: i32,
    cy: i32,
    uFlags: u32,
) -> bool {
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    // SWP_NOZORDER/SWP_NOACTIVATE/SWP_SHOWWINDOW and friends change nothing
    // in a single-window model.
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    let moved = uFlags & SWP_NOMOVE == 0 && (window.x != X || window.y != Y);
    if moved {
        window.x = X;
        window.y = Y;
    }
    let (cx, cy) = (cx.max(0) as u32, cy.max(0) as u32);
    let resized = uFlags & SWP_NOSIZE == 0 && (window.width != cx || window.height != cy);
    if resized {
        window.resize(ctx, cx, cy);
        window.dirty = true;
    }
    use super::message::post_message;
    if moved {
        // WM_MOVE lParam packs the new client origin.
        post_message(
            hWnd,
            WM::MOVE as u32,
            0,
            ((Y as u16 as u32) << 16) | X as u16 as u32,
        );
    }
    if resized {
        // WM_SIZE: SIZE_RESTORED wParam, client dimensions in lParam.
        post_message(hWnd, WM::SIZE as u32, 0, (cy << 16) | cx);
    }
    true
}

const ERROR_INVALID_PARAMETER: u32 = 87;

#[win32_derive::dllexport]
pub fn SetWindowTextA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>) -> bool {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    if lpString.addr < 0x1000 {
        if let Some(teb) = kernel32::teb_mut(ctx) {
            teb.LastErrorValue = ERROR_INVALID_PARAMETER;
        }
        return false;
    }
    window.title = ctx.memory.read_str(lpString.addr).to_owned();
    true
}

#[win32_derive::dllexport]
pub fn GetWindowTextA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>, nMaxCount: i32) -> i32 {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return 0;
    };
    let window = window.borrow();
    if window.hwnd != hWnd || nMaxCount <= 0 {
        return 0;
    }
    let title = window.title.as_bytes();
    let copy = (nMaxCount as usize - 1).min(title.len());
    let end = lpString.addr as usize + copy + 1;
    if lpString.addr < 0x1000 || end > ctx.memory.bytes.len() {
        return 0;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lpString.addr as usize..)
        .and_then(|b| b.get_mut(..copy))
    {
        dst.copy_from_slice(&title[..copy]);
    }
    ctx.memory.write::<u8>(lpString.addr + copy as u32, 0);
    copy as i32
}

fn get_class_name(ctx: &mut Context, hWnd: HWND, addr: u32, nMaxCount: i32, wide: bool) -> i32 {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return 0;
    };
    if window.borrow().hwnd != hWnd || nMaxCount <= 0 {
        return 0;
    }
    let wndclass = state().wndclass.borrow();
    let name = match wndclass.as_ref().and_then(|c| c.name.as_ref()) {
        // An atom-registered class reports the "#atom" atom string.
        Some(ClassName::Atom(atom)) => format!("#{atom}"),
        Some(ClassName::Name(name)) => name.clone(),
        None => return 0,
    };
    drop(wndclass);
    if wide {
        let units: Vec<u16> = name.encode_utf16().collect();
        let copy = (nMaxCount as usize - 1).min(units.len());
        let Some(end) = addr
            .checked_add((copy as u32 + 1) * 2)
            .map(|e| e as usize <= ctx.memory.bytes.len())
        else {
            return 0;
        };
        if addr < 0x1000 || !end {
            return 0;
        }
        for (i, unit) in units[..copy].iter().enumerate() {
            ctx.memory.write::<u16>(addr + i as u32 * 2, *unit);
        }
        ctx.memory.write::<u16>(addr + copy as u32 * 2, 0);
        copy as i32
    } else {
        let bytes = name.as_bytes();
        let copy = (nMaxCount as usize - 1).min(bytes.len());
        let end = addr as usize + copy + 1;
        if addr < 0x1000 || end > ctx.memory.bytes.len() {
            return 0;
        }
        if let Some(dst) = ctx
            .memory
            .bytes
            .get_mut(addr as usize..)
            .and_then(|b| b.get_mut(..copy))
        {
            dst.copy_from_slice(&bytes[..copy]);
        }
        ctx.memory.write::<u8>(addr + copy as u32, 0);
        copy as i32
    }
}

#[win32_derive::dllexport]
pub fn GetClassNameA(ctx: &mut Context, hWnd: HWND, lpClassName: Ptr<u8>, nMaxCount: i32) -> i32 {
    get_class_name(ctx, hWnd, lpClassName.addr, nMaxCount, false)
}

#[win32_derive::dllexport]
pub fn GetClassNameW(
    ctx: &mut Context,
    hWnd: HWND,
    lpClassName: Ptr<u16>, /* WSTR */
    nMaxCount: i32,
) -> i32 {
    get_class_name(ctx, hWnd, lpClassName.addr, nMaxCount, true)
}

#[win32_derive::dllexport]
pub fn EnableWindow(_ctx: &mut Context, hWnd: HWND, bEnable: bool) -> bool {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let mut window = window.borrow_mut();
    if window.hwnd != hWnd {
        return false;
    }
    let was_enabled = std::mem::replace(&mut window.enabled, bEnable);
    drop(window);
    use super::message::{WM, post_message};
    post_message(hWnd, WM::ENABLE as u32, bEnable as u32, 0);
    // The contract: nonzero when the window was previously disabled.
    !was_enabled
}

#[win32_derive::dllexport]
pub fn IsWindowEnabled(_ctx: &mut Context, hWnd: HWND) -> bool {
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return false;
    };
    let window = window.borrow();
    window.hwnd == hWnd && window.enabled
}

#[win32_derive::dllexport]
pub fn SetCursor(_ctx: &mut Context, hCursor: u32) -> u32 {
    state().set_cursor(hCursor)
}

#[win32_derive::dllexport]
pub fn SetCursorPos(_ctx: &mut Context, X: i32, Y: i32) -> bool {
    // We can't warp the host's cursor, but tracking where the app put it keeps
    // GetCursorPos and DirectInput's relative motion consistent with each other.
    state().input.borrow_mut().mouse.warp(X, Y);
    true
}

#[win32_derive::dllexport]
pub fn GetCursorPos(ctx: &mut Context, lpPoint: Ptr<POINT>) -> bool {
    crate::user32::pump_host_input();
    let mouse = &state().input.borrow().mouse;
    lpPoint
        .write(
            &mut ctx.memory,
            POINT {
                x: mouse.x,
                y: mouse.y,
            },
        )
        .is_some()
}

#[win32_derive::dllexport]
pub fn MapWindowPoints(
    ctx: &mut Context,
    hWndFrom: HWND,
    hWndTo: HWND,
    lpPoints: Ptr<POINT>,
    cPoints: u32,
) -> i32 {
    let state = state();
    let window = state.window.borrow();
    let window_origin = |hwnd: HWND| -> POINT {
        if hwnd.is_null() {
            // A null HWND is the desktop: points are already in screen space.
            return POINT::default();
        }
        let Some(window) = window.as_ref() else {
            return POINT::default();
        };
        let window = window.borrow();
        if window.hwnd != hwnd {
            return POINT::default();
        }
        // The emulated window has no frame or caption, so the client origin
        // coincides with the window's screen position.
        POINT {
            x: window.x,
            y: window.y,
        }
    };

    let from = window_origin(hWndFrom);
    let to = window_origin(hWndTo);
    let delta = from.sub(to);

    let mut points = lpPoints;
    for _ in 0..cPoints {
        let Some(point) = points.read(&ctx.memory) else {
            return 0;
        };
        if points.write(&mut ctx.memory, point.add(delta)).is_none() {
            return 0;
        }
        points.advance();
    }

    ((delta.y as u16 as u32) << 16 | delta.x as u16 as u32) as i32
}

#[win32_derive::dllexport]
pub fn ValidateRect(_ctx: &mut Context, hWnd: HWND, _lpRect: Ptr<RECT>) -> bool {
    // The update region is a single dirty flag covering the window, so
    // validating any part of it clears the pending WM_PAINT.
    let window = state().window.borrow();
    let Some(window) = window.as_ref() else {
        return hWnd.is_null();
    };
    let mut window = window.borrow_mut();
    if !hWnd.is_null() && window.hwnd != hWnd {
        return false;
    }
    window.dirty = false;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        GetWindowTextA, HWND, RegisterClassA, SetWindowTextA, UnregisterClassA, UnregisterClassW,
        Window,
    };
    use crate::Ptr;
    use runtime::{BlockCache, CPU, ContFn, Context, Memory};
    use std::{cell::RefCell, rc::Rc};

    static BLOCKS: &[(u32, ContFn)] = &[(0x3000, Context::return_from_x86)];

    /// The registered-class slot is process-global; tests that register and
    /// unregister classes must not interleave.
    static CLASS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x30000),
            blocks: BLOCKS,
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    /// WNDCLASS fields: style@0, lpfnWndProc@4, ..., hbrBackground@0x1c,
    /// lpszMenuName@0x20, lpszClassName@0x24.
    fn write_wndclass(ctx: &mut Context, addr: u32, name: u32) {
        ctx.memory.write::<u32>(addr + 4, 0x3000); // lpfnWndProc
        ctx.memory.write::<u32>(addr + 0x24, name); // lpszClassName
    }

    // Addresses below 0x10000 read as atoms per MAKEINTRESOURCE, so the test
    // keeps its buffers above that line.
    const WNDCLASS_ADDR: u32 = 0x1_4000;
    const NAME_ADDR: u32 = 0x1_5000;
    const OTHER_NAME_ADDR: u32 = 0x1_6000;

    #[test]
    fn unregister_class_matches_name_or_atom() {
        let _guard = CLASS_LOCK.lock().unwrap();
        let mut ctx = context();
        ctx.memory[NAME_ADDR..][..10].copy_from_slice(b"TestClass\0");
        write_wndclass(&mut ctx, WNDCLASS_ADDR, NAME_ADDR);

        let atom = RegisterClassA(&mut ctx, Ptr::new(WNDCLASS_ADDR));
        assert_ne!(atom, 0);

        // A different name must not unregister the class.
        ctx.memory[NAME_ADDR..][..8].copy_from_slice(b"Other\0\0\0");
        assert!(!UnregisterClassA(&mut ctx, Ptr::new(NAME_ADDR), 0));

        // The registration atom unregisters it.
        assert!(UnregisterClassA(&mut ctx, Ptr::new(atom as u32), 0));
        // Already gone.
        assert!(!UnregisterClassA(&mut ctx, Ptr::new(atom as u32), 0));
        assert!(!UnregisterClassW(&mut ctx, Ptr::new(NAME_ADDR), 0));
    }

    #[test]
    fn unregister_class_by_name() {
        let _guard = CLASS_LOCK.lock().unwrap();
        let mut ctx = context();
        ctx.memory[NAME_ADDR..][..10].copy_from_slice(b"TestClass\0");
        write_wndclass(&mut ctx, WNDCLASS_ADDR, NAME_ADDR);

        assert_ne!(RegisterClassA(&mut ctx, Ptr::new(WNDCLASS_ADDR)), 0);
        // Case-insensitive match, like Windows.
        ctx.memory[OTHER_NAME_ADDR..][..10].copy_from_slice(b"testclass\0");
        assert!(UnregisterClassA(&mut ctx, Ptr::new(OTHER_NAME_ADDR), 0));
    }

    #[test]
    fn set_and_get_window_text_reject_null_pointers() {
        // This test manually inserts a Window to avoid the SDL single-thread
        // restriction that CreateWindowExA would otherwise trigger.
        let _guard = CLASS_LOCK.lock().unwrap();
        let mut ctx = context();

        let host_window: host::Window = unsafe { std::mem::zeroed() };
        let window = Rc::new(RefCell::new(Window {
            hwnd: HWND::from_raw(1),
            style: 0,
            ex_style: 0,
            dirty: false,
            title: "Initial".into(),
            enabled: true,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels: None,
            host: host_window,
            surface: None,
        }));
        super::state().window.borrow_mut().replace(window);
        let hwnd = HWND::from_raw(1);

        ctx.memory[0x2000..][..10].copy_from_slice(b"NewTitle\0\0");
        assert!(SetWindowTextA(&mut ctx, hwnd, Ptr::new(0x2000)));

        assert_eq!(GetWindowTextA(&mut ctx, hwnd, Ptr::new(0x3000), 16), 8);
        assert_eq!(&ctx.memory.bytes[0x3000..0x3009], b"NewTitle\0");

        assert!(!SetWindowTextA(&mut ctx, hwnd, Ptr::new(0)));
        assert!(!SetWindowTextA(&mut ctx, hwnd, Ptr::new(0x500)));
        assert_eq!(GetWindowTextA(&mut ctx, hwnd, Ptr::new(0x3000), 16), 8);
        assert_eq!(&ctx.memory.bytes[0x3000..0x3009], b"NewTitle\0");

        ctx.memory[0x4000..][..8].fill(0xAB);
        assert_eq!(GetWindowTextA(&mut ctx, hwnd, Ptr::new(0x500), 16), 0);
        assert_eq!(&ctx.memory.bytes[0x4000..0x4008], &[0xAB; 8]);

        super::state().window.borrow_mut().take();
    }
}
