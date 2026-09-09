mod dialog;
mod hook;
mod input;
mod menu;
mod message;
mod misc;
mod prop;
mod rect;
mod resource;
mod window;

use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

pub use dialog::*;
pub use hook::*;
pub use input::*;
pub use menu::*;
pub use message::*;
pub use misc::*;
pub use prop::*;
pub use rect::*;
pub use resource::*;
pub use window::*;

use crate::{HANDLE, handle::Handles};

pub type HWND = HANDLE;
pub type HMENU = u32;
pub type HINSTANCE = u32;
pub type HCURSOR = u32;
pub type HICON = u32;
pub type HACCEL = u32;

pub struct State {
    /// Registered window classes; a class atom is its index here plus 0xc000.
    pub wndclasses: RefCell<Vec<WndClass>>,
    /// The one window. TODO: programs built on frameworks like MFC create
    /// several windows (frame, view, hidden helpers); model them all.
    pub window: RefCell<Option<Rc<RefCell<Window>>>>,
    message_queue: RefCell<MessageQueue>,
    pub input: RefCell<Input>,
    pub hooks: RefCell<Vec<Hook>>,
    /// Dialogs created via CreateDialogIndirectParamA, and their controls.
    /// The one real window's HWND is hardcoded to 1 (see
    /// window::create_window), so this starts at 2 to stay disjoint from it.
    pub dialog_windows: RefCell<Handles<DialogWindow>>,
}

// TODO: reuse locking pattern from kernel32
// XXX sdl is not thread-safe so we cannot put it in a Mutex anyway, argh
struct StaticState(OnceCell<State>);
unsafe impl Sync for StaticState {}

static STATE: StaticState = StaticState(OnceCell::new());

pub fn state() -> &'static State {
    STATE.0.get_or_init(|| State {
        window: Default::default(),
        wndclasses: Default::default(),
        message_queue: Default::default(),
        input: Default::default(),
        hooks: Default::default(),
        dialog_windows: RefCell::new(Handles::new(2)),
    })
}
