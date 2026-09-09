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
    pub class: String,
    pub title: String,
    /// x86 address of the window procedure. Starts as the class's, and changes
    /// when the program subclasses the window with SetWindowLong(GWL_WNDPROC).
    pub wndproc: u32,
    pub style: u32,
    pub ex_style: u32,
    /// GWL_USERDATA
    pub user_data: u32,
    pub dirty: bool, // triggers WM_PAINT
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
            let addr = kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, self.width * self.height * 4);
            addr
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
    class: String,
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
    /// Look up a registered class by name, or by atom when the "name" is one.
    pub fn find_class(&self, name: &str) -> Option<usize> {
        let classes = self.wndclasses.borrow();
        if let Some(atom) = name.strip_prefix('#') {
            let atom: usize = atom.parse().ok()?;
            return atom.checked_sub(0xc000).filter(|&i| i < classes.len());
        }
        classes
            .iter()
            .position(|class| class.name.eq_ignore_ascii_case(name))
    }

    fn create_window(&self, args: CreateWindowArgs) -> HWND {
        let width = args.width.unwrap_or(640);
        let height = args.height.unwrap_or(480);

        let wndproc = match self.find_class(&args.class) {
            Some(index) => self.wndclasses.borrow()[index].wndproc_addr,
            None => {
                // TODO: system classes (BUTTON, STATIC, ...) have no procedure here.
                log::warn!("CreateWindow: unknown class {:?}", args.class);
                0
            }
        };

        if self.window.borrow().is_some() {
            log::warn!(
                "CreateWindow({:?}, {:?}): replacing the existing window; only one is modelled",
                args.class,
                args.name
            );
        }

        let hwnd = HWND::from_raw(1);
        let window = Rc::new(RefCell::new(Window {
            hwnd,
            class: args.class,
            title: args.name.clone(),
            wndproc,
            style: args.style,
            ex_style: args.ex_style,
            user_data: 0,
            dirty: true,
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
        stub!(hwnd)
    }
}

#[win32_derive::dllexport]
pub fn CreateWindowExA(
    ctx: &mut Context,
    dwExStyle: u32, /* WINDOW_EX_STYLE */
    lpClassName: Ptr<u8>,
    lpWindowName: Ptr<u8>,
    dwStyle: u32, /* WINDOW_STYLE */
    X: i32,
    Y: i32,
    nWidth: CW,
    nHeight: CW,
    hWndParent: HWND,
    hMenu: HMENU,
    hInstance: HINSTANCE,
    lpParam: Ptr<()>,
) -> HWND {
    let class = class_name(ctx, lpClassName.addr);
    let name = if lpWindowName.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_str(lpWindowName.addr).to_string()
    };
    let hwnd = state().create_window(CreateWindowArgs {
        class,
        name,
        style: dwStyle,
        ex_style: dwExStyle,
        x: X,
        y: Y,
        width: nWidth.value(),
        height: nHeight.value(),
    });
    let cs = super::CREATESTRUCTA {
        lpCreateParams: lpParam.addr,
        hInstance,
        hMenu,
        hwndParent: hWndParent.to_raw(),
        cy: nHeight.0 as i32,
        cx: nWidth.0 as i32,
        y: Y,
        x: X,
        style: dwStyle,
        lpszName: lpWindowName.addr,
        lpszClass: lpClassName.addr,
        dwExStyle,
    };
    super::cbt_create_wnd(ctx, hwnd, &cs);
    // TODO: send WM_NCCREATE / WM_CREATE to the window procedure.
    hwnd
}

/// The class name argument of CreateWindow, which may be an atom.
fn class_name(ctx: &Context, lpClassName: u32) -> String {
    if lpClassName < 0x10000 {
        format!("#{lpClassName}")
    } else {
        ctx.memory.read_str(lpClassName).to_string()
    }
}

#[win32_derive::dllexport]
pub fn CreateWindowExW(
    ctx: &mut Context,
    dwExStyle: u32,         /* WINDOW_EX_STYLE */
    lpClassName: Ptr<u16>,  /* WSTR */
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
    let class = if lpClassName.addr < 0x10000 {
        format!("#{}", lpClassName.addr)
    } else {
        ctx.memory.read_wstr(lpClassName.addr).to_string_lossy()
    };
    let name = ctx.memory.read_wstr(lpWindowName.addr);
    state().create_window(CreateWindowArgs {
        class,
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
pub fn DestroyWindow(_ctx: &mut Context, _hWnd: HWND) -> bool {
    stub!(true)
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
pub fn UpdateWindow(_ctx: &mut Context, _hWnd: HWND) -> bool {
    stub!(true)
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

    match msg {
        WM::PAINT => {
            window.dirty = false;
        }
        _ => {}
    }
    0
}

#[win32_derive::dllexport]
pub fn SetFocus(_ctx: &mut Context, _hWnd: HWND) -> HWND {
    stub!(HWND::null())
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
    pub name: String,
    /// x86 address of the class's window procedure.
    pub wndproc_addr: u32,
    pub background: Option<gdi32::Brush>,
}

impl State {
    /// Returns the class atom.
    pub fn register_class(&self, wnd_class: WndClass) -> u16 {
        let mut classes = self.wndclasses.borrow_mut();
        if let Some(index) = classes
            .iter()
            .position(|class| class.name.eq_ignore_ascii_case(&wnd_class.name))
        {
            log::warn!(
                "RegisterClass({:?}): replacing existing class",
                wnd_class.name
            );
            classes[index] = wnd_class;
            return 0xc000 + index as u16;
        }
        classes.push(wnd_class);
        0xc000 + (classes.len() - 1) as u16
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
    fn to_colorref(&self) -> COLORREF {
        use COLOR::*;
        match self {
            // The Windows 95 default color scheme.
            SCROLLBAR => COLORREF::from_rgb(0xc0, 0xc0, 0xc0),
            BACKGROUND => COLORREF::from_rgb(0x00, 0x80, 0x80),
            ACTIVECAPTION => COLORREF::from_rgb(0x00, 0x00, 0x80),
            INACTIVECAPTION => COLORREF::from_rgb(0x80, 0x80, 0x80),
            WINDOW => COLORREF::from_rgb(0xff, 0xff, 0xff),
            WINDOWFRAME | MENUTEXT | WINDOWTEXT | BTNTEXT => COLORREF::from_rgb(0, 0, 0),
            MENU | BTNFACE | ACTIVEBORDER | INACTIVEBORDER => COLORREF::from_rgb(0xc0, 0xc0, 0xc0),
            CAPTIONTEXT | HIGHLIGHTTEXT | BTNHIGHLIGHT => COLORREF::from_rgb(0xff, 0xff, 0xff),
            APPWORKSPACE | BTNSHADOW | GRAYTEXT => COLORREF::from_rgb(0x80, 0x80, 0x80),
            HIGHLIGHT => COLORREF::from_rgb(0x00, 0x00, 0x80),
            INACTIVECAPTIONTEXT => COLORREF::from_rgb(0xc0, 0xc0, 0xc0),
        }
    }
}

#[win32_derive::dllexport]
pub fn RegisterClassA(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>) -> u16 {
    let wndclass = lpWndClass.read(&ctx.memory).unwrap();
    let name = ctx.memory.read_str(wndclass.lpszClassName).to_string();
    register_class(ctx, &wndclass, name)
}

#[win32_derive::dllexport]
pub fn RegisterClassW(ctx: &mut Context, lpWndClass: Ptr<WNDCLASS>) -> u16 {
    let wndclass = lpWndClass.read(&ctx.memory).unwrap();
    let name = ctx
        .memory
        .read_wstr(wndclass.lpszClassName)
        .to_string_lossy();
    register_class(ctx, &wndclass, name)
}

fn register_class(_ctx: &mut Context, wndclass: &WNDCLASS, name: String) -> u16 {
    let background = if wndclass.hbrBackground.is_null() {
        None
    } else if wndclass.hbrBackground.to_raw() < 32 {
        let color = COLOR::from_abi(wndclass.hbrBackground.to_raw());
        Some(Brush(color.to_colorref()))
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
        name,
        wndproc_addr: wndclass.lpfnWndProc,
        background,
    })
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

    let background = state()
        .find_class(&window.class)
        .and_then(|index| state().wndclasses.borrow()[index].background.clone());
    if let Some(background) = &background {
        // TODO: send WM_ERASEBKGND, let DefWindowProc handle it
        let pixels = window.ensure_pixels(ctx);
        let pixel_count = (window.width * (window.height)) as usize;
        use zerocopy::FromBytes;
        let pixels = <[[u8; 4]]>::mut_from_bytes_with_elems(
            &mut ctx.memory[pixels..][..pixel_count * 4],
            pixel_count,
        )
        .unwrap();
        pixels.fill(background.0.to_pixel());
    };
    let rcPaint = window.rect();
    drop(window);

    let hdc = GetDC(ctx, hWnd);
    lpPaint
        .write(
            &mut ctx.memory,
            PAINTSTRUCT {
                hdc,
                fErase: background.is_none() as u32,
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
        // desktop window
        return stub!(HDC::null());
    }

    let state = state();
    let window = state.window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();

    let pixels = window.ensure_pixels(ctx);
    let bitmap = gdi32::Bitmap::new_simple(window.width, window.height, pixels);

    let mut lock = gdi32::lock();
    let (hbitmap, bitmap) = lock.new_bitmap_handle(bitmap);
    let dc = DC::new(hbitmap, bitmap);
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
    assert!(!hWnd.is_null()); // todo
    let window = user32::state().window.borrow();
    let mut window = window.as_ref().unwrap().borrow_mut();
    window.dirty = true;
    true
}

#[win32_derive::dllexport]
pub fn GetDesktopWindow(_ctx: &mut Context) -> HWND {
    stub!(HWND::null())
}

#[win32_derive::dllexport]
pub fn GetClientRect(ctx: &mut Context, _hWnd: HWND, lpRect: Ptr<RECT>) -> bool {
    let rect = {
        let window = state().window.borrow();
        let window = window.as_ref().unwrap().borrow();
        window.rect()
    };
    lpRect.write(&mut ctx.memory, rect).is_some()
}

#[win32_derive::dllexport]
pub fn SetWindowPos(
    _ctx: &mut Context,
    _hWnd: HWND,
    _hWndInsertAfter: u32,
    _X: i32,
    _Y: i32,
    _cx: i32,
    _cy: i32,
    _uFlags: u32,
) -> bool {
    stub!(true)
}

#[win32_derive::dllexport]
pub fn SetWindowTextA(_ctx: &mut Context, _hWnd: HWND, _lpString: Ptr<u8>) -> bool {
    stub!(true)
}

#[win32_derive::dllexport]
pub fn EnableWindow(_ctx: &mut Context, _hWnd: HWND, _bEnable: bool) -> bool {
    stub!(false)
}

#[win32_derive::dllexport]
pub fn SetCursor(_ctx: &mut Context, _hCursor: u32) -> u32 {
    stub!(0)
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
            return POINT::default();
        }

        let _window = window.as_ref().unwrap().borrow();
        POINT {
            x: 0,
            y: 0,
            // TODO: screen coordinates, need MSG.point to translate as well
            // x: window.x,
            // y: window.y,
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

/// The window for an HWND, if it is the one window we model.
fn window(hwnd: HWND) -> Option<Rc<RefCell<Window>>> {
    let window = state().window.borrow();
    let window = window.as_ref()?;
    if window.borrow().hwnd != hwnd {
        return None;
    }
    Some(window.clone())
}

#[win32_derive::dllexport]
pub fn GetSysColor(_ctx: &mut Context, nIndex: COLOR) -> u32 {
    nIndex.to_colorref().as_win32()
}

#[win32_derive::dllexport]
pub fn GetSysColorBrush(_ctx: &mut Context, nIndex: COLOR) -> HBRUSH {
    // A new handle per call; Windows shares them, but nobody frees these.
    gdi32::lock()
        .objects
        .add(gdi32::Object::Brush(Brush(nIndex.to_colorref())))
}

const GWL_WNDPROC: i32 = -4;
const GWL_HINSTANCE: i32 = -6;
const GWL_HWNDPARENT: i32 = -8;
const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;
const GWL_USERDATA: i32 = -21;
const GWL_ID: i32 = -12;

#[win32_derive::dllexport]
pub fn GetWindowLongA(_ctx: &mut Context, hWnd: HWND, nIndex: i32) -> u32 {
    let Some(window) = window(hWnd) else {
        log::warn!("GetWindowLongA({hWnd:?}): unknown window");
        return 0;
    };
    let window = window.borrow();
    match nIndex {
        GWL_WNDPROC => window.wndproc,
        GWL_HINSTANCE => kernel32::lock().image_base,
        GWL_HWNDPARENT | GWL_ID => 0,
        GWL_STYLE => window.style,
        GWL_EXSTYLE => window.ex_style,
        GWL_USERDATA => window.user_data,
        _ => {
            log::warn!("GetWindowLongA: unsupported index {nIndex} (cbWndExtra?)");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn SetWindowLongA(_ctx: &mut Context, hWnd: HWND, nIndex: i32, dwNewLong: u32) -> u32 {
    let Some(window) = window(hWnd) else {
        log::warn!("SetWindowLongA({hWnd:?}): unknown window");
        return 0;
    };
    let mut window = window.borrow_mut();
    let slot = match nIndex {
        GWL_WNDPROC => &mut window.wndproc,
        GWL_STYLE => &mut window.style,
        GWL_EXSTYLE => &mut window.ex_style,
        GWL_USERDATA => &mut window.user_data,
        _ => {
            log::warn!("SetWindowLongA: unsupported index {nIndex} (cbWndExtra?)");
            return 0;
        }
    };
    std::mem::replace(slot, dwNewLong)
}

#[win32_derive::dllexport]
pub fn GetWindowRect(ctx: &mut Context, hWnd: HWND, lpRect: Ptr<RECT>) -> bool {
    let Some(window) = window(hWnd) else {
        return false;
    };
    let window = window.borrow();
    lpRect.write(
        &mut ctx.memory,
        RECT {
            left: window.x,
            top: window.y,
            right: window.x + window.width as i32,
            bottom: window.y + window.height as i32,
        },
    );
    true
}

#[win32_derive::dllexport]
pub fn ScreenToClient(ctx: &mut Context, hWnd: HWND, lpPoint: Ptr<POINT>) -> bool {
    let Some(window) = window(hWnd) else {
        return false;
    };
    let window = window.borrow();
    let mut point = lpPoint.read(&ctx.memory).unwrap();
    point.x -= window.x;
    point.y -= window.y;
    lpPoint.write(&mut ctx.memory, point);
    true
}

#[win32_derive::dllexport]
pub fn AdjustWindowRectEx(
    _ctx: &mut Context,
    _lpRect: Ptr<RECT>,
    _dwStyle: u32,
    _bMenu: bool,
    _dwExStyle: u32,
) -> bool {
    // Our windows have no frame, so the client rectangle is the window rectangle.
    true
}

#[win32_derive::dllexport]
pub fn IsWindow(_ctx: &mut Context, hWnd: HWND) -> bool {
    window(hWnd).is_some()
}

#[win32_derive::dllexport]
pub fn IsWindowVisible(_ctx: &mut Context, hWnd: HWND) -> bool {
    window(hWnd).is_some()
}

#[win32_derive::dllexport]
pub fn IsWindowEnabled(_ctx: &mut Context, hWnd: HWND) -> bool {
    window(hWnd).is_some()
}

#[win32_derive::dllexport]
pub fn GetParent(_ctx: &mut Context, _hWnd: HWND) -> HWND {
    HWND::null()
}

#[win32_derive::dllexport]
pub fn GetWindow(_ctx: &mut Context, _hWnd: HWND, _uCmd: u32) -> HWND {
    HWND::null() // no siblings, children or owners
}

#[win32_derive::dllexport]
pub fn GetTopWindow(_ctx: &mut Context, _hWnd: HWND) -> HWND {
    HWND::null()
}

fn the_window() -> HWND {
    match state().window.borrow().as_ref() {
        Some(window) => window.borrow().hwnd,
        None => HWND::null(),
    }
}

#[win32_derive::dllexport]
pub fn GetForegroundWindow(_ctx: &mut Context) -> HWND {
    the_window()
}

#[win32_derive::dllexport]
pub fn SetForegroundWindow(_ctx: &mut Context, _hWnd: HWND) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn SetActiveWindow(_ctx: &mut Context, _hWnd: HWND) -> HWND {
    the_window()
}

#[win32_derive::dllexport]
pub fn GetFocus(_ctx: &mut Context) -> HWND {
    the_window()
}

#[win32_derive::dllexport]
pub fn GetClassNameA(ctx: &mut Context, hWnd: HWND, lpClassName: Ptr<u8>, nMaxCount: i32) -> i32 {
    let Some(window) = window(hWnd) else {
        return 0;
    };
    let class = window.borrow().class.clone();
    kernel32::write_cstr(ctx, lpClassName, nMaxCount.max(0) as u32, class.as_bytes()) as i32
}

#[win32_derive::dllexport]
pub fn GetClassInfoA(
    ctx: &mut Context,
    _hInstance: HINSTANCE,
    lpClassName: Ptr<u8>,
    _lpWndClass: Ptr<WNDCLASS>,
) -> bool {
    // Reporting no class makes frameworks register their own, which we can model.
    let name = class_name(ctx, lpClassName.addr);
    log::info!("GetClassInfoA({name:?}): reporting not found");
    false
}

#[win32_derive::dllexport]
pub fn GetWindowTextA(ctx: &mut Context, hWnd: HWND, lpString: Ptr<u8>, nMaxCount: i32) -> i32 {
    let Some(window) = window(hWnd) else {
        return 0;
    };
    let title = window.borrow().title.clone();
    kernel32::write_cstr(ctx, lpString, nMaxCount.max(0) as u32, title.as_bytes()) as i32
}

#[win32_derive::dllexport]
pub fn GetWindowTextLengthA(_ctx: &mut Context, hWnd: HWND) -> i32 {
    match window(hWnd) {
        Some(window) => window.borrow().title.len() as i32,
        None => 0,
    }
}

#[win32_derive::dllexport]
pub fn WindowFromPoint(_ctx: &mut Context, _x: i32, _y: i32) -> HWND {
    // POINT is passed by value as two dwords.
    the_window()
}
