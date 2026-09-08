//! Implementation of host interfaces using SDL.

use std::{ffi::CString, mem::MaybeUninit};

use sdl3_sys as sdl;

use crate::{self as host, SingleThreader};

fn sdl_error() -> String {
    unsafe {
        std::ffi::CStr::from_ptr(sdl::error::SDL_GetError())
            .to_string_lossy()
            .into_owned()
    }
}

pub struct MainThread {
    headless: bool,
    /// Mouse buttons currently held. Button events carry no mask of their own,
    /// so it is tracked as they arrive.
    buttons: std::cell::Cell<host::MouseButton>,
    /// The one guest window's SDL handle; null in headless mode or before the
    /// guest creates it. Input mapping and the fullscreen toggle need it.
    window: std::cell::Cell<*mut sdl::video::SDL_Window>,
    /// The guest's logical client size. The SDL window may be resized or made
    /// fullscreen without changing it; the frame letterboxes to fit.
    logical: std::cell::Cell<(u32, u32)>,
}

struct ClickInject {
    /// (x, y, absolute-time override): when the override is set the click
    /// fires at that millisecond instead of the MS + i*GAP schedule.
    clicks: Vec<(u32, u32, Option<u32>)>,
    at_ms: u32,
    gap: u32,
}

pub struct Host {
    pub main_thread: SingleThreader<MainThread>,
}

impl Host {
    pub fn new() -> Self {
        let headless = std::env::var("THESEUS_HEADLESS").unwrap_or_default() != "";
        Self {
            main_thread: SingleThreader::new(MainThread::new(headless)),
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

fn mouse_buttons_from_sdl(state: sdl::mouse::SDL_MouseButtonFlags) -> host::MouseButton {
    let mut buttons = host::MouseButton::empty();
    if state.0 & sdl::mouse::SDL_BUTTON_LMASK.0 != 0 {
        buttons.insert(host::MouseButton::Left);
    }
    if state.0 & sdl::mouse::SDL_BUTTON_MMASK.0 != 0 {
        buttons.insert(host::MouseButton::Middle);
    }
    if state.0 & sdl::mouse::SDL_BUTTON_RMASK.0 != 0 {
        buttons.insert(host::MouseButton::Right);
    }
    buttons
}

/// SDL scan code -> (PC set-1 scan code, Windows VK_*, extended key).
///
/// SDL scan codes are USB HID usages, while Windows apps (and DirectInput's
/// DIK_* constants) speak PC/AT set 1, so the two need an explicit table.
/// Keys with no PC/AT equivalent are simply absent.
#[rustfmt::skip]
const KEY_MAP: &[(sdl::scancode::SDL_Scancode, u8, u8, bool)] = {
    use sdl::scancode::SDL_Scancode as SC;
    &[
        (SC::ESCAPE, 0x01, 0x1b, false),
        (SC::_1, 0x02, b'1', false),
        (SC::_2, 0x03, b'2', false),
        (SC::_3, 0x04, b'3', false),
        (SC::_4, 0x05, b'4', false),
        (SC::_5, 0x06, b'5', false),
        (SC::_6, 0x07, b'6', false),
        (SC::_7, 0x08, b'7', false),
        (SC::_8, 0x09, b'8', false),
        (SC::_9, 0x0a, b'9', false),
        (SC::_0, 0x0b, b'0', false),
        (SC::MINUS, 0x0c, 0xbd, false),        // VK_OEM_MINUS
        (SC::EQUALS, 0x0d, 0xbb, false),       // VK_OEM_PLUS
        (SC::BACKSPACE, 0x0e, 0x08, false),
        (SC::TAB, 0x0f, 0x09, false),
        (SC::Q, 0x10, b'Q', false),
        (SC::W, 0x11, b'W', false),
        (SC::E, 0x12, b'E', false),
        (SC::R, 0x13, b'R', false),
        (SC::T, 0x14, b'T', false),
        (SC::Y, 0x15, b'Y', false),
        (SC::U, 0x16, b'U', false),
        (SC::I, 0x17, b'I', false),
        (SC::O, 0x18, b'O', false),
        (SC::P, 0x19, b'P', false),
        (SC::LEFTBRACKET, 0x1a, 0xdb, false),  // VK_OEM_4
        (SC::RIGHTBRACKET, 0x1b, 0xdd, false), // VK_OEM_6
        (SC::RETURN, 0x1c, 0x0d, false),
        (SC::LCTRL, 0x1d, 0xa2, false),        // VK_LCONTROL
        (SC::A, 0x1e, b'A', false),
        (SC::S, 0x1f, b'S', false),
        (SC::D, 0x20, b'D', false),
        (SC::F, 0x21, b'F', false),
        (SC::G, 0x22, b'G', false),
        (SC::H, 0x23, b'H', false),
        (SC::J, 0x24, b'J', false),
        (SC::K, 0x25, b'K', false),
        (SC::L, 0x26, b'L', false),
        (SC::SEMICOLON, 0x27, 0xba, false),    // VK_OEM_1
        (SC::APOSTROPHE, 0x28, 0xde, false),   // VK_OEM_7
        (SC::GRAVE, 0x29, 0xc0, false),        // VK_OEM_3
        (SC::LSHIFT, 0x2a, 0xa0, false),       // VK_LSHIFT
        (SC::BACKSLASH, 0x2b, 0xdc, false),    // VK_OEM_5
        (SC::Z, 0x2c, b'Z', false),
        (SC::X, 0x2d, b'X', false),
        (SC::C, 0x2e, b'C', false),
        (SC::V, 0x2f, b'V', false),
        (SC::B, 0x30, b'B', false),
        (SC::N, 0x31, b'N', false),
        (SC::M, 0x32, b'M', false),
        (SC::COMMA, 0x33, 0xbc, false),        // VK_OEM_COMMA
        (SC::PERIOD, 0x34, 0xbe, false),       // VK_OEM_PERIOD
        (SC::SLASH, 0x35, 0xbf, false),        // VK_OEM_2
        (SC::RSHIFT, 0x36, 0xa1, false),       // VK_RSHIFT
        (SC::KP_MULTIPLY, 0x37, 0x6a, false),  // VK_MULTIPLY
        (SC::LALT, 0x38, 0xa4, false),         // VK_LMENU
        (SC::SPACE, 0x39, 0x20, false),
        (SC::CAPSLOCK, 0x3a, 0x14, false),
        (SC::F1, 0x3b, 0x70, false),
        (SC::F2, 0x3c, 0x71, false),
        (SC::F3, 0x3d, 0x72, false),
        (SC::F4, 0x3e, 0x73, false),
        (SC::F5, 0x3f, 0x74, false),
        (SC::F6, 0x40, 0x75, false),
        (SC::F7, 0x41, 0x76, false),
        (SC::F8, 0x42, 0x77, false),
        (SC::F9, 0x43, 0x78, false),
        (SC::F10, 0x44, 0x79, false),
        (SC::NUMLOCKCLEAR, 0x45, 0x90, false),
        (SC::SCROLLLOCK, 0x46, 0x91, false),
        (SC::KP_7, 0x47, 0x67, false),
        (SC::KP_8, 0x48, 0x68, false),
        (SC::KP_9, 0x49, 0x69, false),
        (SC::KP_MINUS, 0x4a, 0x6d, false),
        (SC::KP_4, 0x4b, 0x64, false),
        (SC::KP_5, 0x4c, 0x65, false),
        (SC::KP_6, 0x4d, 0x66, false),
        (SC::KP_PLUS, 0x4e, 0x6b, false),
        (SC::KP_1, 0x4f, 0x61, false),
        (SC::KP_2, 0x50, 0x62, false),
        (SC::KP_3, 0x51, 0x63, false),
        (SC::KP_0, 0x52, 0x60, false),
        (SC::KP_PERIOD, 0x53, 0x6e, false),
        (SC::F11, 0x57, 0x7a, false),
        (SC::F12, 0x58, 0x7b, false),
        // Extended keys: same scan code as their non-extended twin, but
        // prefixed with 0xe0 on the wire.
        (SC::KP_ENTER, 0x1c, 0x0d, true),
        (SC::RCTRL, 0x1d, 0xa3, true),         // VK_RCONTROL
        (SC::KP_DIVIDE, 0x35, 0x6f, true),     // VK_DIVIDE
        (SC::RALT, 0x38, 0xa5, true),          // VK_RMENU
        (SC::HOME, 0x47, 0x24, true),
        (SC::UP, 0x48, 0x26, true),
        (SC::PAGEUP, 0x49, 0x21, true),
        (SC::LEFT, 0x4b, 0x25, true),
        (SC::RIGHT, 0x4d, 0x27, true),
        (SC::END, 0x4f, 0x23, true),
        (SC::DOWN, 0x50, 0x28, true),
        (SC::PAGEDOWN, 0x51, 0x22, true),
        (SC::INSERT, 0x52, 0x2d, true),
        (SC::DELETE, 0x53, 0x2e, true),
        (SC::LGUI, 0x5b, 0x5b, true),          // VK_LWIN
        (SC::RGUI, 0x5c, 0x5c, true),          // VK_RWIN
        (SC::APPLICATION, 0x5d, 0x5d, true),   // VK_APPS
    ]
};

fn key_from_sdl(event: &sdl::events::SDL_KeyboardEvent) -> Option<host::KeyMessage> {
    let &(_, scancode, vkey, extended) = KEY_MAP.iter().find(|key| key.0 == event.scancode)?;
    Some(host::KeyMessage {
        scancode,
        vkey,
        extended,
        repeat: event.repeat,
    })
}

/// The centered, aspect-preserving rect that fits a `guest`-sized frame into
/// an `out`-sized area, as (x, y, w, h) in `out` units. Present uses it in
/// render-output pixels; input mapping uses it in window points.
fn letterbox(out_w: f32, out_h: f32, guest_w: f32, guest_h: f32) -> (f32, f32, f32, f32) {
    if out_w <= 0.0 || out_h <= 0.0 || guest_w <= 0.0 || guest_h <= 0.0 {
        return (0.0, 0.0, out_w.max(0.0), out_h.max(0.0));
    }
    let scale = (out_w / guest_w).min(out_h / guest_h);
    let w = guest_w * scale;
    let h = guest_h * scale;
    ((out_w - w) / 2.0, (out_h - h) / 2.0, w, h)
}

/// A mouse position in window points -> guest logical coordinates, inverting
/// the letterbox that `render` applies. Positions in the bars clamp to the
/// nearest frame edge because `MouseMessage` cannot express out-of-range
/// coordinates.
fn map_to_guest(win_w: i32, win_h: i32, guest_w: u32, guest_h: u32, x: f32, y: f32) -> (u32, u32) {
    if win_w <= 0 || win_h <= 0 || guest_w == 0 || guest_h == 0 {
        return (x.max(0.0) as u32, y.max(0.0) as u32);
    }
    let (rx, ry, rw, rh) = letterbox(win_w as f32, win_h as f32, guest_w as f32, guest_h as f32);
    let gx = (x - rx) * guest_w as f32 / rw;
    let gy = (y - ry) * guest_h as f32 / rh;
    (
        gx.clamp(0.0, guest_w as f32 - 1.0) as u32,
        gy.clamp(0.0, guest_h as f32 - 1.0) as u32,
    )
}

/// Alt+Enter is the host-level fullscreen chord: the guest keeps its logical
/// mode and never sees it, so both the down and up edges are consumed.
fn is_fullscreen_chord(event: &sdl::events::SDL_KeyboardEvent) -> bool {
    event.scancode == sdl::scancode::SDL_Scancode::RETURN
        && (event.r#mod & sdl::keycode::SDL_KMOD_ALT).0 != 0
}

impl MainThread {
    /// Window-point mouse position -> guest logical coordinates. Headless and
    /// pre-window events pass through unchanged (they are already guest
    /// coordinates, like the injected ones).
    fn guest_point(&self, x: f32, y: f32) -> (u32, u32) {
        let window = self.window.get();
        if window.is_null() {
            return (x.max(0.0) as u32, y.max(0.0) as u32);
        }
        let (mut w, mut h) = (0, 0);
        unsafe {
            if !sdl::video::SDL_GetWindowSize(window, &mut w, &mut h) {
                log::warn!(
                    "SDL_GetWindowSize failed ({}); using raw point",
                    sdl_error()
                );
                return (x.max(0.0) as u32, y.max(0.0) as u32);
            }
        }
        let (gw, gh) = self.logical.get();
        map_to_guest(w, h, gw, gh, x, y)
    }

    fn msg_from_event(&self, event: &sdl::events::SDL_Event) -> Option<host::Message> {
        unsafe {
            use sdl::events::SDL_EventType;
            let typ: sdl::events::SDL_EventType = std::mem::transmute(event.r#type);
            match typ {
                SDL_EventType::WINDOW_EXPOSED
                | SDL_EventType::WINDOW_RESIZED
                | SDL_EventType::WINDOW_PIXEL_SIZE_CHANGED => {
                    // A resize changes the letterbox area; ask for a repaint
                    // so a guest that is not presenting every frame does not
                    // leave stale content under the new window shape.
                    return Some(host::Message::Paint);
                }
                SDL_EventType::MOUSE_MOTION => {
                    let event = &event.motion;
                    // Motion events do carry the mask, so resync from them.
                    self.buttons.set(mouse_buttons_from_sdl(event.state));
                    let (x, y) = self.guest_point(event.x, event.y);
                    return Some(host::Message::MouseMove(host::MouseMessage {
                        x,
                        y,
                        button: host::MouseButton::empty(),
                        buttons: mouse_buttons_from_sdl(event.state),
                    }));
                }
                SDL_EventType::MOUSE_BUTTON_DOWN | SDL_EventType::MOUSE_BUTTON_UP => {
                    let event = &event.button;
                    let button = match event.button as _ {
                        sdl::mouse::SDL_BUTTON_LEFT => host::MouseButton::Left,
                        sdl::mouse::SDL_BUTTON_MIDDLE => host::MouseButton::Middle,
                        sdl::mouse::SDL_BUTTON_RIGHT => host::MouseButton::Right,
                        _ => return None,
                    };
                    // `buttons` has to be the state right after this event: a
                    // release that reported its button as still held would leave
                    // it stuck down. Button events carry no mask, and the live
                    // state is the state now rather than when the event happened,
                    // so track it as events arrive.
                    let mut buttons = self.buttons.get();
                    if typ == SDL_EventType::MOUSE_BUTTON_DOWN {
                        buttons.insert(button);
                    } else {
                        buttons.remove(button);
                    }
                    self.buttons.set(buttons);
                    let (x, y) = self.guest_point(event.x, event.y);
                    let message = host::MouseMessage {
                        x,
                        y,
                        button,
                        buttons,
                    };
                    if typ == SDL_EventType::MOUSE_BUTTON_DOWN {
                        return Some(host::Message::MouseDown(message));
                    } else {
                        return Some(host::Message::MouseUp(message));
                    }
                }
                SDL_EventType::KEY_DOWN | SDL_EventType::KEY_UP => {
                    let event = &event.key;
                    let window = self.window.get();
                    if is_fullscreen_chord(event) && !window.is_null() {
                        if typ == SDL_EventType::KEY_DOWN && !event.repeat {
                            let fullscreen = (sdl::video::SDL_GetWindowFlags(window)
                                & sdl::video::SDL_WindowFlags::FULLSCREEN)
                                .0
                                != 0;
                            if !sdl::video::SDL_SetWindowFullscreen(window, !fullscreen) {
                                log::warn!(
                                    "SDL_SetWindowFullscreen({}) failed: {}",
                                    !fullscreen,
                                    sdl_error()
                                );
                            }
                        }
                        return None;
                    }
                    let key = key_from_sdl(event)?;
                    if typ == SDL_EventType::KEY_DOWN {
                        return Some(host::Message::KeyDown(key));
                    } else {
                        return Some(host::Message::KeyUp(key));
                    }
                }
                SDL_EventType::QUIT => {
                    return Some(host::Message::Quit);
                }
                SDL_EventType::WINDOW_CLOSE_REQUESTED => {
                    return Some(host::Message::Close);
                }
                SDL_EventType::WINDOW_FOCUS_LOST => {
                    return Some(host::Message::FocusLost);
                }
                SDL_EventType::WINDOW_FOCUS_GAINED => {
                    return Some(host::Message::FocusGained);
                }
                _ => {}
            }
            //log::warn!("todo: handle sdl event: {:#x?}", typ);
        }
        None
    }
}

impl MainThread {
    fn new(mut headless: bool) -> Self {
        unsafe {
            if !sdl::hints::SDL_SetHint(sdl::hints::SDL_HINT_NO_SIGNAL_HANDLERS, c"1".as_ptr()) {
                log::warn!("SDL_SetHint(NO_SIGNAL_HANDLERS) failed: {}", sdl_error());
            }
            // On macOS 14+ SDL no longer activates a non-bundled process at
            // launch (the hint defaults to "1" there), and SDL_RaiseWindow's
            // deprecated activation call is ignored, so the game window came
            // up behind the terminal and never received a single key event.
            // Ask for foreground activation explicitly; this must precede
            // SDL_Init to take effect.
            if !sdl::hints::SDL_SetHint(sdl::hints::SDL_HINT_MAC_BACKGROUND_APP, c"0".as_ptr()) {
                log::warn!("SDL_SetHint(MAC_BACKGROUND_APP) failed: {}", sdl_error());
            }
            // SDL's default is to swallow the click that focuses an
            // unfocused window, which makes the first click on the game do
            // nothing; deliver it like a normal click instead.
            if !sdl::hints::SDL_SetHint(
                sdl::hints::SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH,
                c"1".as_ptr(),
            ) {
                log::warn!(
                    "SDL_SetHint(MOUSE_FOCUS_CLICKTHROUGH) failed: {}",
                    sdl_error()
                );
            }
            if !sdl::hints::SDL_SetHint(sdl::hints::SDL_HINT_RENDER_VSYNC, c"1".as_ptr()) {
                log::warn!("SDL_SetHint(RENDER_VSYNC) failed: {}", sdl_error());
            }
            // The event subsystem is always needed, even in headless mode, for
            // injected input and for tests that drive the message queue.
            if !sdl::init::SDL_Init(sdl::init::SDL_INIT_EVENTS) {
                log::warn!(
                    "SDL_Init(EVENTS) failed: {}; continuing without events",
                    sdl_error()
                );
            }
            if !headless
                && !sdl::init::SDL_Init(sdl::init::SDL_INIT_VIDEO | sdl::init::SDL_INIT_AUDIO)
            {
                log::warn!(
                    "SDL video+audio init failed ({}); falling back to headless",
                    sdl_error()
                );
                headless = true;
            }
        }
        Self {
            headless,
            buttons: Default::default(),
            window: Default::default(),
            logical: Default::default(),
        }
    }

    pub fn poll(&self) -> Option<host::Message> {
        let event = unsafe {
            let mut event = MaybeUninit::uninit();
            if !sdl::events::SDL_PollEvent(event.as_mut_ptr()) {
                return None;
            };
            event.assume_init()
        };
        let msg = self.msg_from_event(&event)?;
        Some(msg)
    }

    pub fn wait(&self) -> host::Message {
        loop {
            let event = unsafe {
                let mut event = MaybeUninit::uninit();
                if !sdl::events::SDL_WaitEvent(event.as_mut_ptr()) {
                    log::warn!("SDL_WaitEvent failed: {}; retrying", sdl_error());
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                event.assume_init()
            };
            if let Some(msg) = self.msg_from_event(&event) {
                return msg;
            }
        }
    }
}

pub struct Surface {
    /// null when running in headless mode
    texture: *mut sdl::render::SDL_Texture,
    width: u32,
    height: u32,
    /// Most recent `set_pixels` upload, kept so `render` can dump the
    /// presented frame even without a window.
    last: Vec<u8>,
    last_stride: u32,
}

impl Surface {
    /// pixels are RGBA in memory
    pub fn set_pixels(&mut self, pixels: &[u8], stride: u32) {
        self.last.clear();
        self.last.extend_from_slice(pixels);
        self.last_stride = stride;
        if self.texture.is_null() {
            return;
        }
        unsafe {
            if !sdl::render::SDL_UpdateTexture(
                self.texture,
                std::ptr::null(),
                pixels.as_ptr() as *const _,
                stride as i32,
            ) {
                log::warn!("SDL_UpdateTexture failed ({}); ignoring", sdl_error());
            }
        }
    }

    /// Write the last uploaded frame to `path` as a binary PPM.
    fn dump(&self, path: &str) {
        use std::io::Write;
        // Widen to usize before multiplying: u32 row arithmetic overflows
        // for very large surfaces.
        let need = self.height as usize * self.last_stride as usize;
        if (self.last_stride as usize) < self.width as usize * 4 || self.last.len() < need {
            // Rows are 4-byte pixels; a narrower stride (or nothing
            // presented yet) cannot produce a valid dump.
            return;
        }
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 3);
        out.extend_from_slice(format!("P6\n{} {}\n255\n", self.width, self.height).as_bytes());
        for y in 0..self.height as usize {
            let row = &self.last[y * self.last_stride as usize..][..self.width as usize * 4];
            for px in row.chunks_exact(4) {
                out.extend_from_slice(&px[..3]);
            }
        }
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::File::create(path).and_then(|mut f| f.write_all(&out)) {
            log::warn!("frame dump to {path} failed: {e}");
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if !self.texture.is_null() {
            unsafe { sdl::render::SDL_DestroyTexture(self.texture) };
        }
    }
}

pub struct Window {
    /// null when running in headless mode
    window: *mut sdl::video::SDL_Window,
    /// null when running in headless mode
    renderer: *mut sdl::render::SDL_Renderer,
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.window.is_null() {
            return;
        }
        unsafe {
            // The renderer references the window; destroy it first.
            if !self.renderer.is_null() {
                sdl::render::SDL_DestroyRenderer(self.renderer);
            }
            sdl::video::SDL_DestroyWindow(self.window);
        }
        // Input mapping and the fullscreen chord keep this window's handle;
        // clear it so neither queries a destroyed window.
        if let Some(main) = crate::host().main_thread.try_get()
            && main.window.get() == self.window
        {
            main.window.set(std::ptr::null_mut());
        }
    }
}

impl Window {
    /// The guest destroyed this window. Hide it rather than free it: a
    /// DirectDraw object bound to it may still hold surfaces on its
    /// renderer, but a hidden window can no longer take focus or clicks
    /// away from the window the guest creates next.
    pub fn close(&mut self) {
        if self.window.is_null() {
            return;
        }
        unsafe {
            if !sdl::video::SDL_HideWindow(self.window) {
                log::warn!("SDL_HideWindow failed ({}); continuing", sdl_error());
            }
        }
    }

    pub fn create_surface(&mut self, width: u32, height: u32) -> Surface {
        if self.window.is_null() {
            return Surface {
                texture: std::ptr::null_mut(),
                width,
                height,
                last: Vec::new(),
                last_stride: 0,
            };
        }
        unsafe {
            let texture = sdl::render::SDL_CreateTexture(
                self.renderer,
                // this means RGBA in memory order
                sdl::pixels::SDL_PIXELFORMAT_ABGR8888,
                sdl::render::SDL_TEXTUREACCESS_TARGET,
                width as i32,
                height as i32,
            );
            if texture.is_null() {
                // A surface the renderer refuses (oversized dimensions,
                // memory pressure) presents as headless rather than
                // panicking the host.
                log::warn!(
                    "SDL_CreateTexture({width}x{height}) failed ({}); continuing headless",
                    sdl_error()
                );
            }
            Surface {
                texture,
                width,
                height,
                last: Vec::new(),
                last_stride: 0,
            }
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if self.window.is_null() {
            return;
        }
        // The guest changed its logical client size (a display-mode change);
        // the letterbox transform follows it.
        crate::host().main_thread.get().logical.set((width, height));
        unsafe {
            if !sdl::video::SDL_SetWindowSize(self.window, width as i32, height as i32) {
                log::warn!(
                    "SDL_SetWindowSize({width}x{height}) failed ({}); ignoring",
                    sdl_error()
                );
            }
        }
    }

    pub fn render(&mut self, surface: &mut Surface) {
        static DUMP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        static DUMP_EVERY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        static FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if let Some(path) = DUMP
            .get_or_init(|| {
                std::env::var("THESEUS_FRAME_DUMP")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .as_deref()
        {
            // THESEUS_FRAME_DUMP_EVERY=<n> writes a numbered film strip
            // (path.NNNNN.ppm) every n frames instead of overwriting `path`
            // on each present.
            let every = *DUMP_EVERY.get_or_init(|| {
                std::env::var("THESEUS_FRAME_DUMP_EVERY")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
            });
            let frame = FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if every == 0 {
                surface.dump(path);
            } else if frame.is_multiple_of(every) {
                surface.dump(&format!("{path}.{frame:05}.ppm"));
            }
        }
        if self.window.is_null() {
            return;
        }
        if surface.texture.is_null() {
            log::warn!("render: surface has no texture; skipping");
            return;
        }
        unsafe {
            // For debugging, can verify that the flip covers the entire canvas by starting with red:
            // check(sdl::render::SDL_SetRenderDrawColor(
            //     self.renderer,
            //     255,
            //     0,
            //     0,
            //     255,
            // ));
            // check(sdl::render::SDL_RenderClear(self.renderer));

            // Ignore any alpha in the input when doing the final render copy.
            if !sdl::render::SDL_SetTextureBlendMode(
                surface.texture,
                sdl::blendmode::SDL_BlendMode::NONE,
            ) {
                log::warn!(
                    "SDL_SetTextureBlendMode failed ({}); skipping render",
                    sdl_error()
                );
                return;
            }
            // The SDL window can outgrow the guest's logical size (user
            // resize or fullscreen), so the frame letterboxes into the render
            // output. Clear first so the bars are black, not stale pixels.
            let (mut out_w, mut out_h) = (0, 0);
            if !sdl::render::SDL_GetRenderOutputSize(self.renderer, &mut out_w, &mut out_h) {
                log::warn!(
                    "SDL_GetRenderOutputSize failed ({}); assuming surface size",
                    sdl_error()
                );
                out_w = surface.width as i32;
                out_h = surface.height as i32;
            }
            let (x, y, w, h) = letterbox(
                out_w as f32,
                out_h as f32,
                surface.width as f32,
                surface.height as f32,
            );
            let dst = sdl::rect::SDL_FRect { x, y, w, h };
            if !sdl::render::SDL_SetRenderDrawColor(self.renderer, 0, 0, 0, 255) {
                log::warn!("SDL_SetRenderDrawColor failed ({}); ignoring", sdl_error());
            }
            if !sdl::render::SDL_RenderClear(self.renderer) {
                log::warn!("SDL_RenderClear failed ({}); ignoring", sdl_error());
            }
            if !sdl::render::SDL_RenderTexture(
                self.renderer,
                surface.texture,
                std::ptr::null(),
                &dst,
            ) {
                log::warn!(
                    "SDL_RenderTexture failed ({}); skipping present",
                    sdl_error()
                );
                return;
            }
            if !sdl::render::SDL_RenderPresent(self.renderer) {
                log::warn!("SDL_RenderPresent failed ({}); ignoring", sdl_error());
            }
        }
    }
}

impl MainThread {
    pub fn create_window(&self, title: &str, width: u32, height: u32) -> Window {
        // Track the guest's logical size so physical mouse coordinates can
        // be mapped back through the letterbox `render` applies.
        self.logical.set((width, height));
        if self.headless {
            return Window {
                window: std::ptr::null_mut(),
                renderer: std::ptr::null_mut(),
            };
        }
        unsafe {
            // The title comes from the guest; an interior NUL gets an
            // empty title rather than a host panic.
            let title = CString::new(title).unwrap_or_default();
            // RESIZABLE lets the user pick any host-side size; the guest's
            // logical coordinate system is unaffected because present
            // letterboxes and input maps back through it.
            let window = sdl::video::SDL_CreateWindow(
                title.as_ptr(),
                width as i32,
                height as i32,
                sdl::video::SDL_WindowFlags::HIGH_PIXEL_DENSITY
                    | sdl::video::SDL_WindowFlags::RESIZABLE,
            );
            if window.is_null() {
                log::warn!(
                    "SDL_CreateWindow({width}x{height}) failed ({}); continuing headless",
                    sdl_error()
                );
                self.window.set(std::ptr::null_mut());
                return Window {
                    window: std::ptr::null_mut(),
                    renderer: std::ptr::null_mut(),
                };
            }
            self.window.set(window);
            // Raise the window so it takes keyboard focus: an unfocused
            // window gets no key events at all, so in-race driving keys
            // would stay dead until the user clicked into the window.
            if !sdl::video::SDL_RaiseWindow(window) {
                log::warn!("SDL_RaiseWindow failed ({}); continuing", sdl_error());
            }
            // THESEUS_FULLSCREEN=1 starts in native fullscreen; the guest's
            // logical mode is unchanged (present letterboxes, input maps
            // back through it). Alt+Enter toggles it at runtime.
            if std::env::var("THESEUS_FULLSCREEN").unwrap_or_default() != ""
                && !sdl::video::SDL_SetWindowFullscreen(window, true)
            {
                log::warn!(
                    "SDL_SetWindowFullscreen(true) failed ({}); staying windowed",
                    sdl_error()
                );
            }
            let renderer = sdl::render::SDL_CreateRenderer(window, std::ptr::null());
            if renderer.is_null() {
                log::warn!(
                    "SDL_CreateRenderer failed ({}); destroying window and continuing headless",
                    sdl_error()
                );
                sdl::video::SDL_DestroyWindow(window);
                self.window.set(std::ptr::null_mut());
                return Window {
                    window: std::ptr::null_mut(),
                    renderer: std::ptr::null_mut(),
                };
            }
            if !sdl::render::SDL_RenderClear(renderer) {
                log::warn!(
                    "SDL_RenderClear failed ({}); destroying window and continuing headless",
                    sdl_error()
                );
                sdl::render::SDL_DestroyRenderer(renderer);
                sdl::video::SDL_DestroyWindow(window);
                self.window.set(std::ptr::null_mut());
                return Window {
                    window: std::ptr::null_mut(),
                    renderer: std::ptr::null_mut(),
                };
            }
            if !sdl::render::SDL_SetDefaultTextureScaleMode(
                renderer,
                sdl::surface::SDL_ScaleMode::NEAREST,
            ) {
                log::warn!(
                    "SDL_SetDefaultTextureScaleMode failed ({}); destroying window and continuing headless",
                    sdl_error()
                );
                sdl::render::SDL_DestroyRenderer(renderer);
                sdl::video::SDL_DestroyWindow(window);
                self.window.set(std::ptr::null_mut());
                return Window {
                    window: std::ptr::null_mut(),
                    renderer: std::ptr::null_mut(),
                };
            }
            Window { window, renderer }
        }
    }
}

impl Host {
    #[allow(unused)] // todo
    pub fn print(&self, text: &[u8]) {
        use std::io::Write;
        // A broken pipe or closed stdout shouldn't kill the emulator.
        let _ = std::io::stdout().write_all(text);
    }
}

/// An audio output stream. Null when the host has no usable audio device, in
/// which case writes are discarded — a machine without sound shouldn't stop a
/// program from running.
pub struct AudioStream(*mut sdl::audio::SDL_AudioStream);
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// False when the host has no audio device. Callers should skip producing
    /// audio entirely rather than mixing into nothing.
    pub fn is_open(&self) -> bool {
        !self.0.is_null()
    }

    pub fn queued_bytes(&self) -> u32 {
        if self.0.is_null() {
            // Nothing is queued, because writes are discarded. Saying anything
            // else strands callers that wait for the queue to drain.
            return 0;
        }
        let queued = unsafe { sdl::audio::SDL_GetAudioStreamQueued(self.0) };
        if queued < 0 {
            // Report an empty queue on failure so callers waiting for the
            // stream to drain are not stuck on an unrecoverable SDL error.
            log::warn!("SDL_GetAudioStreamQueued failed: {}", sdl_error());
            return 0;
        }
        queued as u32
    }

    pub fn put_data(&self, data: &[u8]) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            if !sdl::audio::SDL_PutAudioStreamData(
                self.0,
                data.as_ptr() as *const _,
                data.len() as i32,
            ) {
                log::warn!("SDL_PutAudioStreamData failed: {}", sdl_error());
            }
        }
    }

    pub fn resume(&self) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            if !sdl::audio::SDL_ResumeAudioStreamDevice(self.0) {
                log::warn!("SDL_ResumeAudioStreamDevice failed: {}", sdl_error());
            }
        }
    }

    /// Discard every queued byte without playing it.
    pub fn clear(&self) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            if !sdl::audio::SDL_ClearAudioStream(self.0) {
                log::warn!("SDL_ClearAudioStream failed: {}", sdl_error());
            }
        }
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { sdl::audio::SDL_DestroyAudioStream(self.0) };
        }
    }
}

/// Windows VK_* code -> (PC set-1 scan code, extended flag) for the keys a
/// menu or text field needs: escape, enter, space, backspace, tab, the
/// arrows, letters, and digits.
fn inject_vkey(vkey: u8) -> Option<host::KeyMessage> {
    // PC set-1 scancodes for A-Z and the top-row digits 0-9.
    const LETTER_SCAN: [u8; 26] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    const DIGIT_SCAN: [u8; 10] = [0x0b, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a];
    let (scancode, extended) = match vkey {
        0x1b => (0x01, false), // VK_ESCAPE
        0x0d => (0x1c, false), // VK_RETURN
        0x20 => (0x39, false), // VK_SPACE
        0x08 => (0x0e, false), // VK_BACK
        0x09 => (0x0f, false), // VK_TAB
        0x26 => (0x48, true),  // VK_UP
        0x28 => (0x50, true),  // VK_DOWN
        0x25 => (0x4b, true),  // VK_LEFT
        0x27 => (0x4d, true),  // VK_RIGHT
        b'A'..=b'Z' => (LETTER_SCAN[(vkey - b'A') as usize], false),
        b'0'..=b'9' => (DIGIT_SCAN[(vkey - b'0') as usize], false),
        _ => return None,
    };
    Some(host::KeyMessage {
        scancode,
        vkey,
        extended,
        repeat: false,
    })
}

impl Host {
    pub fn poll(&self) -> Option<host::Message> {
        // Debug aid: synthesize key presses so scripted or headless runs can
        // exercise the target's input path. THESEUS_INJECT_VKEY is a
        // comma-separated list of hex VK_* codes, THESEUS_INJECT_AT_MS the
        // delay before the first press; keys are tapped 300ms apart.
        static INJECT: std::sync::OnceLock<Option<(Vec<u8>, u32)>> = std::sync::OnceLock::new();
        // u16 like the other injectors: a u8 wraps (or overflows in debug
        // builds) past 255 phases, replaying a long vkey list forever.
        static PHASE: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
        if let Some((vkeys, at_ms)) = INJECT.get_or_init(|| {
            let vkeys = std::env::var("THESEUS_INJECT_VKEY").ok()?;
            let vkeys = vkeys
                .split(',')
                .map(|s| u8::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
                .collect::<Option<Vec<u8>>>()?
                .into_iter()
                .filter(|v| inject_vkey(*v).is_some())
                .collect::<Vec<u8>>();
            let at_ms = std::env::var("THESEUS_INJECT_AT_MS").ok()?.parse().ok()?;
            Some((vkeys, at_ms))
        }) {
            use std::sync::atomic::Ordering::Relaxed;
            let phase = PHASE.load(Relaxed) as usize;
            let key = phase / 2;
            let down = phase.is_multiple_of(2);
            if key < vkeys.len() {
                let at = at_ms + key as u32 * 300 + if down { 0 } else { 100 };
                if self.time() >= at {
                    PHASE.store(phase as u16 + 1, Relaxed);
                    let msg = inject_vkey(vkeys[key])?;
                    return Some(if down {
                        host::Message::KeyDown(msg)
                    } else {
                        host::Message::KeyUp(msg)
                    });
                }
            }
        }

        // Debug aid: synthesize one or more left mouse clicks for headless/
        // scripted runs. THESEUS_INJECT_CLICK is a ';'-separated list of
        // "x,y" positions with an optional "@ms" absolute-time suffix,
        // THESEUS_INJECT_CLICK_MS is the delay before the first move, and
        // THESEUS_INJECT_CLICK_GAP (default 500ms) is the pause between clicks.
        // A "@ms" suffix overrides the MS + i*GAP schedule for that click,
        // which is how a script reaches a dialog that appears only after a
        // long wait. For each click the host emits move, left down, and left
        // up 50ms apart.
        static CLICK_INJECT: std::sync::OnceLock<Option<ClickInject>> = std::sync::OnceLock::new();
        static CLICK_PHASE: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
        if let Some(ClickInject { clicks, at_ms, gap }) = CLICK_INJECT.get_or_init(|| {
            let s = std::env::var("THESEUS_INJECT_CLICK").ok()?;
            let clicks: Vec<(u32, u32, Option<u32>)> = s
                .split(';')
                .map(|part| {
                    let (pos, at) = match part.trim().split_once('@') {
                        Some((pos, at)) => (pos, Some(at.trim().parse().ok()?)),
                        None => (part.trim(), None),
                    };
                    let (x, y) = pos.split_once(',')?;
                    let x = x.trim().parse().ok()?;
                    let y = y.trim().parse().ok()?;
                    Some((x, y, at))
                })
                .collect::<Option<Vec<_>>>()?;
            if clicks.is_empty() {
                return None;
            }
            let at_ms = std::env::var("THESEUS_INJECT_CLICK_MS")
                .ok()?
                .parse()
                .ok()?;
            let gap = std::env::var("THESEUS_INJECT_CLICK_GAP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500);
            Some(ClickInject { clicks, at_ms, gap })
        }) {
            use std::sync::atomic::Ordering::Relaxed;
            let total = clicks.len() as u32 * 3;
            let phase = CLICK_PHASE.load(Relaxed) as u32;
            if phase < total {
                let click = phase / 3;
                let sub = phase % 3;
                let click_start = clicks[click as usize]
                    .2
                    .unwrap_or_else(|| at_ms + click * (150 + gap));
                let at = click_start + sub * 50;
                if self.time() >= at {
                    CLICK_PHASE.store((phase + 1) as u16, Relaxed);
                    let (x, y, _) = clicks[click as usize];
                    let (button, buttons) = match sub {
                        1 => (host::MouseButton::Left, host::MouseButton::Left),
                        2 => (host::MouseButton::Left, host::MouseButton::empty()),
                        _ => (host::MouseButton::empty(), host::MouseButton::empty()),
                    };
                    let message = host::MouseMessage {
                        x,
                        y,
                        button,
                        buttons,
                    };
                    return Some(match sub {
                        0 => host::Message::MouseMove(message),
                        1 => host::Message::MouseDown(message),
                        _ => host::Message::MouseUp(message),
                    });
                }
            }
        }

        // Debug aid: hold keys down for a span so scripted runs can exercise
        // gameplay controls like a held accelerator, which taps cannot reach.
        // THESEUS_INJECT_HOLD is a ';'-separated list of "vkey@down_ms[+up_ms]"
        // entries: the key goes down at down_ms and, when up_ms is present,
        // comes back up at up_ms; without it the key stays held.
        static HOLD_INJECT: std::sync::OnceLock<Option<Vec<(u32, u8, bool)>>> =
            std::sync::OnceLock::new();
        static HOLD_PHASE: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
        if let Some(events) = HOLD_INJECT.get_or_init(|| {
            let s = std::env::var("THESEUS_INJECT_HOLD").ok()?;
            let mut events = Vec::new();
            for part in s.split(';') {
                let (key, span) = part.trim().split_once('@')?;
                let vkey = u8::from_str_radix(key.trim().trim_start_matches("0x"), 16).ok()?;
                inject_vkey(vkey)?;
                let (down, up) = match span.split_once('+') {
                    Some((down, up)) => (down.trim().parse().ok()?, Some(up.trim().parse().ok()?)),
                    None => (span.trim().parse().ok()?, None),
                };
                events.push((down, vkey, true));
                if let Some(up) = up {
                    events.push((up, vkey, false));
                }
            }
            if events.is_empty() {
                return None;
            }
            events.sort_by_key(|&(at, _, _)| at);
            Some(events)
        }) {
            use std::sync::atomic::Ordering::Relaxed;
            let phase = HOLD_PHASE.load(Relaxed) as usize;
            if phase < events.len() {
                let (at, vkey, down) = events[phase];
                if self.time() >= at {
                    HOLD_PHASE.store((phase + 1) as u16, Relaxed);
                    let msg = inject_vkey(vkey)?;
                    return Some(if down {
                        host::Message::KeyDown(msg)
                    } else {
                        host::Message::KeyUp(msg)
                    });
                }
            }
        }

        self.main_thread.get().poll()
    }
    pub fn wait(&self) -> host::Message {
        self.main_thread.get().wait()
    }
    pub fn create_window(&self, title: &str, width: u32, height: u32) -> Window {
        self.main_thread.get().create_window(title, width, height)
    }

    pub fn create_audio_stream(&self, spec: host::AudioSpec) -> AudioStream {
        unsafe {
            let stream = sdl::audio::SDL_OpenAudioDeviceStream(
                sdl::audio::SDL_AudioDeviceID::DEFAULT_PLAYBACK,
                &sdl::audio::SDL_AudioSpec {
                    freq: spec.sample_rate as i32,
                    channels: spec.channels as i32,
                    format: sdl::audio::SDL_AudioFormat::S16LE,
                },
                None,                 // no callback
                std::ptr::null_mut(), // no userdata
            );
            if stream.is_null() {
                let err = std::ffi::CStr::from_ptr(sdl::error::SDL_GetError());
                log::warn!("no audio output: {}", err.to_string_lossy());
            }
            AudioStream(stream)
        }
    }

    pub fn time(&self) -> u32 {
        unsafe { sdl::timer::SDL_GetTicks() as u32 }
    }

    pub fn console_write(&self, text: &[u8]) {
        use std::io::Write;
        // A broken pipe or closed stdout shouldn't kill the emulator.
        let _ = std::io::stdout().write_all(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_thread() -> MainThread {
        // Tests drive `msg_from_event` directly, so no SDL subsystem is
        // needed; build the state without touching SDL_Init.
        MainThread {
            headless: true,
            buttons: Default::default(),
            window: Default::default(),
            logical: Default::default(),
        }
    }

    fn motion(x: f32, y: f32, state: sdl::mouse::SDL_MouseButtonFlags) -> sdl::events::SDL_Event {
        let mut event = sdl::events::SDL_Event::default();
        event.motion = sdl::events::SDL_MouseMotionEvent {
            r#type: sdl::events::SDL_EventType::MOUSE_MOTION,
            state,
            x,
            y,
            ..Default::default()
        };
        event
    }

    fn button(
        typ: sdl::events::SDL_EventType,
        button: u8,
        x: f32,
        y: f32,
    ) -> sdl::events::SDL_Event {
        let mut event = sdl::events::SDL_Event::default();
        event.button = sdl::events::SDL_MouseButtonEvent {
            r#type: typ,
            button,
            down: typ == sdl::events::SDL_EventType::MOUSE_BUTTON_DOWN,
            x,
            y,
            ..Default::default()
        };
        event
    }

    /// A physical left click is a motion, a down, and an up event; each must
    /// translate to the matching host message with the button mask reflecting
    /// the state right after that event.
    #[test]
    fn single_click_translates_to_down_then_up() {
        let main = main_thread();
        let down = sdl::events::SDL_EventType::MOUSE_BUTTON_DOWN;
        let up = sdl::events::SDL_EventType::MOUSE_BUTTON_UP;
        let left = sdl::mouse::SDL_BUTTON_LEFT as u8;

        let Some(host::Message::MouseMove(msg)) = main.msg_from_event(&motion(
            10.0,
            20.0,
            sdl::mouse::SDL_MouseButtonFlags::default(),
        )) else {
            panic!("motion did not produce a mouse-move message")
        };
        assert_eq!((msg.x, msg.y), (10, 20));
        assert!(msg.buttons.is_empty());

        let Some(host::Message::MouseDown(msg)) =
            main.msg_from_event(&button(down, left, 10.0, 20.0))
        else {
            panic!("button-down did not produce a mouse-down message")
        };
        assert_eq!((msg.x, msg.y), (10, 20));
        assert_eq!(msg.button, host::MouseButton::Left);
        assert_eq!(msg.buttons, host::MouseButton::Left);

        let Some(host::Message::MouseUp(msg)) = main.msg_from_event(&button(up, left, 10.0, 20.0))
        else {
            panic!("button-up did not produce a mouse-up message")
        };
        assert_eq!(msg.button, host::MouseButton::Left);
        assert!(msg.buttons.is_empty());
    }

    /// A down+up pair closer than one frame still produces both edges, and a
    /// second click is not swallowed by stale tracked state.
    #[test]
    fn back_to_back_clicks_repeat() {
        let main = main_thread();
        let down = sdl::events::SDL_EventType::MOUSE_BUTTON_DOWN;
        let up = sdl::events::SDL_EventType::MOUSE_BUTTON_UP;
        let left = sdl::mouse::SDL_BUTTON_LEFT as u8;
        for _ in 0..2 {
            assert!(matches!(
                main.msg_from_event(&button(down, left, 5.0, 5.0)),
                Some(host::Message::MouseDown(_))
            ));
            assert!(matches!(
                main.msg_from_event(&button(up, left, 5.0, 5.0)),
                Some(host::Message::MouseUp(_))
            ));
        }
    }

    /// Motion events resync the tracked mask, so a state the button events
    /// missed (focus churn, an off-window release) cannot wedge the next click.
    #[test]
    fn motion_resyncs_the_tracked_button_mask() {
        let main = main_thread();
        let down = sdl::events::SDL_EventType::MOUSE_BUTTON_DOWN;
        let left = sdl::mouse::SDL_BUTTON_LEFT as u8;
        assert!(matches!(
            main.msg_from_event(&button(down, left, 5.0, 5.0)),
            Some(host::Message::MouseDown(_))
        ));
        // The release was never delivered; the next motion reports no buttons.
        let Some(host::Message::MouseMove(msg)) = main.msg_from_event(&motion(
            6.0,
            6.0,
            sdl::mouse::SDL_MouseButtonFlags::default(),
        )) else {
            panic!("motion did not produce a mouse-move message")
        };
        assert!(msg.buttons.is_empty());
        // A following click is a fresh down edge, not a stuck-button repeat.
        let Some(host::Message::MouseDown(msg)) =
            main.msg_from_event(&button(down, left, 6.0, 6.0))
        else {
            panic!("button-down did not produce a mouse-down message")
        };
        assert_eq!(msg.buttons, host::MouseButton::Left);
    }

    fn key(scancode: sdl::scancode::SDL_Scancode, repeat: bool) -> sdl::events::SDL_Event {
        let mut event = sdl::events::SDL_Event::default();
        event.key = sdl::events::SDL_KeyboardEvent {
            r#type: sdl::events::SDL_EventType::KEY_DOWN,
            scancode,
            repeat,
            ..Default::default()
        };
        event
    }

    /// The driving keys are extended PC keys: each must map to its set-1
    /// scancode, VK_*, and the extended flag DirectInput's DIK_* codes need.
    #[test]
    fn arrow_keys_translate_to_extended_set1_scancodes() {
        let main = main_thread();
        for (sc, want_scan, want_vk) in [
            (sdl::scancode::SDL_Scancode::UP, 0x48u8, 0x26u8),
            (sdl::scancode::SDL_Scancode::DOWN, 0x50, 0x28),
            (sdl::scancode::SDL_Scancode::LEFT, 0x4b, 0x25),
            (sdl::scancode::SDL_Scancode::RIGHT, 0x4d, 0x27),
        ] {
            let Some(host::Message::KeyDown(msg)) = main.msg_from_event(&key(sc, false)) else {
                panic!("arrow key {sc:?} did not produce a key-down message")
            };
            assert_eq!(
                (msg.scancode, msg.vkey, msg.extended),
                (want_scan, want_vk, true),
                "arrow key {sc:?} mapped wrong"
            );
        }
    }

    /// Held keys arrive as auto-repeat KEY_DOWNs; the repeat flag must reach
    /// the message so the game does not treat repeats as fresh presses.
    #[test]
    fn key_repeat_flag_is_preserved() {
        let main = main_thread();
        let Some(host::Message::KeyDown(msg)) =
            main.msg_from_event(&key(sdl::scancode::SDL_Scancode::UP, true))
        else {
            panic!("repeat key-down did not produce a key-down message")
        };
        assert!(msg.repeat);
    }

    /// A same-size output is the identity rect; wider outputs pillarbox and
    /// taller outputs letterbox, always centered and aspect-preserving.
    #[test]
    fn letterbox_centers_the_frame() {
        assert_eq!(
            letterbox(640.0, 480.0, 640.0, 480.0),
            (0.0, 0.0, 640.0, 480.0)
        );
        // 1920x1080 output, 640x480 guest: scale 2.25, pillarboxed.
        assert_eq!(
            letterbox(1920.0, 1080.0, 640.0, 480.0),
            (240.0, 0.0, 1440.0, 1080.0)
        );
        // 640x1200 output, 640x480 guest: scale 1.0, letterboxed.
        assert_eq!(
            letterbox(640.0, 1200.0, 640.0, 480.0),
            (0.0, 360.0, 640.0, 480.0)
        );
        // Degenerate inputs produce a degenerate rect, not a divide by zero.
        assert_eq!(letterbox(0.0, 0.0, 640.0, 480.0), (0.0, 0.0, 0.0, 0.0));
        assert_eq!(
            letterbox(640.0, 480.0, 0.0, 480.0),
            (0.0, 0.0, 640.0, 480.0)
        );
    }

    /// Window resize and expose events request a repaint so a rescaled or
    /// letterboxed frame does not stay stale under the new window shape.
    #[test]
    fn window_resize_requests_a_repaint() {
        let main = main_thread();
        for typ in [
            sdl::events::SDL_EventType::WINDOW_RESIZED,
            sdl::events::SDL_EventType::WINDOW_PIXEL_SIZE_CHANGED,
            sdl::events::SDL_EventType::WINDOW_EXPOSED,
        ] {
            let mut event = sdl::events::SDL_Event::default();
            event.window = sdl::events::SDL_WindowEvent {
                r#type: typ,
                ..Default::default()
            };
            assert!(matches!(
                main.msg_from_event(&event),
                Some(host::Message::Paint)
            ));
        }
    }

    /// Both focus edges reach the guest: losing focus releases held input
    /// and gaining it back re-activates, so neither may be dropped.
    #[test]
    fn focus_edges_translate_to_focus_messages() {
        let main = main_thread();
        for (typ, gained) in [
            (sdl::events::SDL_EventType::WINDOW_FOCUS_LOST, false),
            (sdl::events::SDL_EventType::WINDOW_FOCUS_GAINED, true),
        ] {
            let mut event = sdl::events::SDL_Event::default();
            event.window = sdl::events::SDL_WindowEvent {
                r#type: typ,
                ..Default::default()
            };
            let msg = main.msg_from_event(&event);
            assert_eq!(
                matches!(msg, Some(host::Message::FocusGained)),
                gained,
                "{typ:?}"
            );
            assert_eq!(
                matches!(msg, Some(host::Message::FocusLost)),
                !gained,
                "{typ:?}"
            );
        }
    }

    /// The fullscreen chord is Return with an Alt modifier; plain Return and
    /// Alt+other keys are ordinary input the guest must still see.
    #[test]
    fn alt_enter_is_the_fullscreen_chord() {
        let key_event = |scancode, mods: u16| sdl::events::SDL_KeyboardEvent {
            r#type: sdl::events::SDL_EventType::KEY_DOWN,
            scancode,
            r#mod: sdl::keycode::SDL_Keymod(mods),
            ..Default::default()
        };
        use sdl::scancode::SDL_Scancode as SC;
        assert!(is_fullscreen_chord(&key_event(
            SC::RETURN,
            sdl::keycode::SDL_KMOD_LALT.0
        )));
        assert!(is_fullscreen_chord(&key_event(
            SC::RETURN,
            sdl::keycode::SDL_KMOD_RALT.0
        )));
        assert!(!is_fullscreen_chord(&key_event(SC::RETURN, 0)));
        assert!(!is_fullscreen_chord(&key_event(
            SC::A,
            sdl::keycode::SDL_KMOD_ALT.0
        )));
    }

    /// The inverse mapping recovers guest coordinates anywhere in the frame
    /// and clamps positions inside the bars to the nearest frame edge.
    #[test]
    fn map_to_guest_inverts_the_letterbox() {
        // Identity when the window matches the guest size.
        assert_eq!(map_to_guest(640, 480, 640, 480, 320.0, 240.0), (320, 240));
        // 1920x1080 window, 640x480 guest: the frame occupies x in [240,1680).
        assert_eq!(map_to_guest(1920, 1080, 640, 480, 240.0, 0.0), (0, 0));
        assert_eq!(map_to_guest(1920, 1080, 640, 480, 960.0, 540.0), (320, 240));
        assert_eq!(
            map_to_guest(1920, 1080, 640, 480, 1919.0, 1079.0),
            (639, 479)
        );
        // A point in the left bar clamps to the frame's left column.
        assert_eq!(map_to_guest(1920, 1080, 640, 480, 100.0, 540.0), (0, 240));
        // No window or no guest frame: the raw point passes through clamped.
        assert_eq!(map_to_guest(0, 0, 640, 480, 12.0, 8.0), (12, 8));
        assert_eq!(map_to_guest(640, 480, 0, 0, 12.0, 8.0), (12, 8));
    }
}
