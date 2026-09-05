use std::{cell::RefCell, rc::Rc};

use runtime::Context;

use crate::{
    FromABIParam, POINT, Ptr, RECT,
    gdi32::{self, Brush, COLORREF, DC, HBRUSH, HDC},
    kernel32, stub,
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

impl Window {
    pub fn resize(&mut self, ctx: &mut Context, width: u32, height: u32) {
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

    pub fn ensure_pixels(&mut self, ctx: &mut Context) -> u32 {
        *self.pixels.get_or_insert_with(|| {
            kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, self.width * self.height * 4)
        })
    }

    pub fn flush(&mut self, ctx: &mut Context) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let stride = self.width * 4;
        let pixels = self.pixels.unwrap();
        let pixels = &mut ctx.memory[pixels..][..(self.height * stride) as usize];
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
        let width = args.width.unwrap_or(640);
        let height = args.height.unwrap_or(480);

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
    _hWnd: HWND,
    X: i32,
    Y: i32,
    nWidth: i32,
    nHeight: i32,
    bRepaint: bool,
) -> bool {
    let state = state();
    let window = state.window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();
    window.x = X;
    window.y = Y;
    window.resize(ctx, nWidth as u32, nHeight as u32);
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
    _ctx: &mut Context,
    _hWnd: HWND,
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

    let window = state().window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();

    if let WM::PAINT = msg {
        window.dirty = false;
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

pub struct WndClass {
    pub wndproc: runtime::Cont,
    pub background: Option<gdi32::Brush>,
}

impl State {
    pub fn register_class(&self, wnd_class: WndClass) -> u16 {
        *self.wndclass.borrow_mut() = Some(wnd_class);
        let atom = self.next_class_atom.get();
        self.next_class_atom.set(atom + 1);
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
    RegisterClassW(ctx, lpWndClass)
}

#[win32_derive::dllexport]
pub fn RegisterClassW(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>) -> u16 {
    let wndclass = lpWndClass.read(&ctx.memory).unwrap();
    let background = if wndclass.hbrBackground.is_null() {
        None
    } else if wndclass.hbrBackground.to_raw() < 32 {
        // An out-of-range system color index can't be mapped to a brush;
        // treat it like a null background rather than rejecting the class.
        COLOR::try_from(wndclass.hbrBackground.to_raw())
            .ok()
            .map(|color| Brush(Some(color.to_colorref())))
    } else {
        Some(
            gdi32::lock()
                .objects
                .get(wndclass.hbrBackground)
                .unwrap()
                .unwrap_brush(),
        )
    };
    state().register_class(WndClass {
        wndproc: ctx.indirect(wndclass.lpfnWndProc),
        background,
    })
}

#[win32_derive::dllexport]
pub fn UnregisterClassA(_ctx: &mut Context, _lpClassName: Ptr<u8>, _hInstance: HINSTANCE) -> bool {
    state().wndclass.borrow_mut().take().is_some()
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
    let mut window = window.as_ref().unwrap().borrow_mut();

    let wndclass = state().wndclass.borrow();
    let wndclass = wndclass.as_ref().unwrap();
    if let Some(background) = &wndclass.background {
        // TODO: send WM_ERASEBKGND, let DefWindowProc handle it
        let pixels = window.ensure_pixels(ctx);
        let pixel_count = (window.width * (window.height)) as usize;
        use zerocopy::FromBytes;
        let pixels = <[[u8; 4]]>::mut_from_bytes_with_elems(
            &mut ctx.memory[pixels..][..pixel_count * 4],
            pixel_count,
        )
        .unwrap();
        if let Some(color) = background.0 {
            pixels.fill(color.to_pixel());
        }
    };
    let rcPaint = window.rect();
    drop(window);

    let hdc = GetDC(ctx, hWnd);
    lpPaint
        .write(
            &mut ctx.memory,
            PAINTSTRUCT {
                hdc,
                fErase: wndclass.background.is_none() as u32,
                rcPaint,
                reserved: [0; 10],
            },
        )
        .unwrap();
    hdc
}

#[win32_derive::dllexport]
pub fn EndPaint(ctx: &mut Context, _hWnd: HWND, lpPaint: Ptr<PAINTSTRUCT>) -> bool {
    let window = state().window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();
    let paint = lpPaint.read(&ctx.memory).unwrap();
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
        let pixels = kernel32::lock()
            .process_heap
            .alloc(&mut ctx.memory, 640 * 480 * 4);
        let bitmap = gdi32::Bitmap::new_simple(640, 480, pixels);
        return gdi32::lock().new_memory_dc(bitmap);
    }

    let state = state();
    let window = state.window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();

    let pixels = window.ensure_pixels(ctx);
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
        let mut window = window.as_ref().unwrap().borrow_mut();
        window.flush(ctx);
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
    stub!(HWND::null())
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
    if end > ctx.memory.bytes.len() {
        return 0;
    }
    ctx.memory[lpString.addr..][..copy].copy_from_slice(&title[..copy]);
    ctx.memory[lpString.addr + copy as u32] = 0;
    copy as i32
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
        let point = points.read(&ctx.memory).unwrap();
        points.write(&mut ctx.memory, point.add(delta)).unwrap();
        points.advance();
    }

    ((delta.y as u16 as u32) << 16 | delta.x as u16 as u32) as i32
}

#[win32_derive::dllexport]
pub fn ValidateRect(_ctx: &mut Context, _hWnd: HWND, _lpRect: Ptr<RECT>) -> bool {
    stub!(true)
}
