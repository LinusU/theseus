mod dialog;
mod input;
mod message;
mod misc;
mod rect;
mod resource;
mod window;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::OnceLock,
};

pub use dialog::*;
pub use input::*;
pub use message::*;
pub use misc::*;
pub use rect::*;
pub use resource::*;
pub use window::*;

use crate::HANDLE;

pub type HWND = HANDLE;
pub type HMENU = u32;
pub type HINSTANCE = u32;
pub type HCURSOR = u32;
pub type HICON = u32;
pub type HACCEL = u32;

pub struct State {
    pub wndclass: RefCell<Option<WndClass>>,
    pub window: RefCell<Option<Rc<RefCell<Window>>>>,
    message_queue: RefCell<MessageQueue>,
    pub input: RefCell<Input>,
    /// Cursor/icon handles the app has loaded, keyed by (instance, name)
    /// so repeated loads of one resource share a handle like the real
    /// shared system resources do.
    cursors: RefCell<HashMap<(u32, u32), HCURSOR>>,
    icons: RefCell<HashMap<(u32, u32), HICON>>,
    /// Opaque handle source for user objects, kept clear of HWND and GDI
    /// handle ranges.
    next_object_handle: Cell<u32>,
    /// The cursor most recently passed to SetCursor.
    current_cursor: Cell<HCURSOR>,
    /// ShowCursor's display counter; the cursor shows when it's >= 0.
    cursor_display: Cell<i32>,
    /// Next RegisterClass atom; class atoms live at 0xC000 and above.
    next_class_atom: Cell<u16>,
    /// The window that has captured the mouse, or 0 if none.
    capture: Cell<HWND>,
    /// The window with keyboard focus, or 0 if none.
    pub focused: Cell<HWND>,
}

// TODO: reuse locking pattern from kernel32
// XXX sdl is not thread-safe so we cannot put it in a Mutex anyway, argh
// OnceLock rather than cell::OnceCell: get_or_init can be reached from host
// threads (winmm/dsound callbacks), and cell::OnceCell::get_or_init panics
// with "reentrant init" when two threads race the initializer.
struct StaticState(OnceLock<State>);
unsafe impl Sync for StaticState {}

static STATE: StaticState = StaticState(OnceLock::new());

impl State {
    /// The handle for (instance, name), allocating a fresh opaque handle on
    /// first load and sharing it on repeats.
    fn cached_handle(
        &self,
        map: &RefCell<HashMap<(u32, u32), u32>>,
        hinstance: u32,
        name: u32,
    ) -> u32 {
        *map.borrow_mut()
            .entry((hinstance, name))
            .or_insert_with(|| {
                let handle = self.next_object_handle.get();
                self.next_object_handle.set(handle + 1);
                handle
            })
    }

    pub fn load_cursor(&self, hinstance: u32, name: u32) -> HCURSOR {
        self.cached_handle(&self.cursors, hinstance, name)
    }

    pub fn load_icon(&self, hinstance: u32, name: u32) -> HICON {
        self.cached_handle(&self.icons, hinstance, name)
    }

    /// A fresh cursor handle for CreateCursor's caller-built cursor.
    pub fn new_cursor(&self) -> HCURSOR {
        let handle = self.next_object_handle.get();
        self.next_object_handle.set(handle + 1);
        handle
    }

    pub fn set_cursor(&self, hcursor: HCURSOR) -> HCURSOR {
        self.current_cursor.replace(hcursor)
    }

    /// ShowCursor's counter: each call nudges it and returns the new value.
    pub fn show_cursor(&self, show: bool) -> i32 {
        let count = self.cursor_display.get() + if show { 1 } else { -1 };
        self.cursor_display.set(count);
        count
    }
}

pub fn state() -> &'static State {
    STATE.0.get_or_init(|| State {
        window: Default::default(),
        wndclass: Default::default(),
        message_queue: Default::default(),
        input: Default::default(),
        cursors: Default::default(),
        icons: Default::default(),
        next_object_handle: Cell::new(0xD000_0000),
        current_cursor: Cell::new(0),
        cursor_display: Cell::new(0),
        next_class_atom: Cell::new(0xC000),
        capture: Cell::new(HWND::null()),
        focused: Cell::new(HWND::null()),
    })
}

/// The window/class slots are process-global and `State` is unsafely `Sync`,
/// so tests that install into them from any module must take this lock.
#[cfg(test)]
pub(crate) static WINDOW_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
