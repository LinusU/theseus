use std::sync::Mutex;

use crate::{ABIReturn, FromABIParam, HANDLE, handle::Handles, locked_state::LockedState};

mod bitmap;
pub use bitmap::*;
mod dc;
pub use dc::*;
mod misc;
pub use misc::*;
mod object;
pub use object::*;

pub type HGDIOBJ = HANDLE;
pub type HBRUSH = HGDIOBJ;
pub type HPEN = HGDIOBJ;
pub type HFONT = HGDIOBJ;

pub struct State {
    pub dcs: Handles<DC>,
    pub objects: Handles<Object>,
    /// Pixel addresses gdi32 heap-allocated for bitmap objects
    /// (CreateCompatibleBitmap); DeleteObject frees the matching block.
    /// Bitmap pixels pointing into guest- or window-owned memory are never
    /// in this set.
    pub heap_bitmap_pixels: std::collections::HashSet<u32>,
    /// Stock-object handles, keyed by GetStockObjectArg and created lazily
    /// on first use. Stock objects are shared singletons: GetStockObject
    /// returns the same handle every call, a fresh DC's initial pen/brush/
    /// font are these handles, and DeleteObject must not remove them.
    stock: std::collections::HashMap<u32, HGDIOBJ>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub type Lock = LockedState<State>;
pub fn lock() -> Lock {
    LockedState::from_or_init(&STATE, || State {
        // avoid low-numbered object handles to avoid conflicting with COLOR_* constants for HBRUSH
        objects: Handles::new(0x1000),
        dcs: Default::default(),
        heap_bitmap_pixels: Default::default(),
        stock: Default::default(),
    })
}

#[derive(Debug, Copy, Clone, Default)]
pub struct COLORREF(u32);

impl FromABIParam for COLORREF {
    fn from_abi(val: u32) -> Self {
        Self(val)
    }
}

impl From<COLORREF> for ABIReturn {
    fn from(val: COLORREF) -> ABIReturn {
        ABIReturn::from(val.0)
    }
}

impl COLORREF {
    pub fn to_pixel(&self) -> [u8; 4] {
        let [r, g, b] = self.to_rgb();
        [r, g, b, 0xff]
    }

    pub fn as_win32(&self) -> u32 {
        self.0
    }

    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(u32::from_le_bytes([r, g, b, 0]))
    }

    pub fn to_rgb(&self) -> [u8; 3] {
        let [r, g, b, _] = self.0.to_le_bytes();
        [r, g, b]
    }
}
