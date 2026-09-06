use runtime::Context;

use super::HWND;
use crate::{POINT, Ptr, RECT};

#[win32_derive::dllexport]
pub fn OffsetRect(ctx: &mut Context, lprc: Ptr<RECT>, dx: i32, dy: i32) -> bool {
    let Some(mut rect) = lprc.read(&ctx.memory) else {
        return false;
    };
    rect.left = rect.left.wrapping_add(dx);
    rect.right = rect.right.wrapping_add(dx);
    rect.top = rect.top.wrapping_add(dy);
    rect.bottom = rect.bottom.wrapping_add(dy);
    lprc.write(&mut ctx.memory, rect).is_some()
}

#[win32_derive::dllexport]
pub fn ClientToScreen(ctx: &mut Context, hWnd: HWND, lpPoint: Ptr<POINT>) -> bool {
    let Some(mut point) = lpPoint.read(&ctx.memory) else {
        return false;
    };
    // A null HWND is the desktop (see GetDesktopWindow): the point is
    // already in screen space. Otherwise the window's (x, y) is the client
    // origin — the model has no frame or caption — matching
    // MapWindowPoints.
    if !hWnd.is_null() {
        let window = super::state().window.borrow();
        let Some(window) = window.as_ref() else {
            return false;
        };
        let window = window.borrow();
        if window.hwnd != hWnd {
            return false;
        }
        point = point.add(POINT {
            x: window.x,
            y: window.y,
        });
    }
    lpPoint.write(&mut ctx.memory, point).is_some()
}

#[win32_derive::dllexport]
pub fn PtInRect(ctx: &mut Context, lprc: Ptr<RECT>, x: i32, y: i32) -> bool {
    let Some(rect) = lprc.read(&ctx.memory) else {
        return false;
    };
    let point = POINT { x, y };
    rect.contains(point)
}

#[win32_derive::dllexport]
pub fn SetRect(
    ctx: &mut Context,
    lprc: Ptr<RECT>,
    xLeft: i32,
    yTop: i32,
    xRight: i32,
    yBottom: i32,
) -> bool {
    lprc.write(
        &mut ctx.memory,
        RECT {
            left: xLeft,
            top: yTop,
            right: xRight,
            bottom: yBottom,
        },
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::{ClientToScreen, HWND, POINT, Ptr};
    use runtime::{BlockCache, CPU, Context, Memory};
    use std::{cell::RefCell, rc::Rc};

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

    #[test]
    fn client_to_screen_offsets_by_the_window_origin() {
        // The shared window slot is also exercised by `user32::window`'s
        // tests; serialize against them.
        let _guard = crate::user32::WINDOW_STATE_LOCK.lock().unwrap();
        let mut ctx = context();
        let host_window: host::Window = unsafe { std::mem::zeroed() };
        let window = Rc::new(RefCell::new(crate::user32::Window {
            hwnd: HWND::from_raw(1),
            style: 0,
            ex_style: 0,
            dirty: false,
            title: "Test".into(),
            enabled: true,
            visible: false,
            user_data: 0,
            hinstance: 0,
            id: 0,
            subclass_proc: None,
            paint_dc: None,
            x: 10,
            y: 20,
            width: 1,
            height: 1,
            pixels: None,
            host: host_window,
            surface: None,
        }));
        crate::user32::state().window.borrow_mut().replace(window);

        ctx.memory.write(0x1000, POINT { x: 1, y: 2 });
        assert!(ClientToScreen(
            &mut ctx,
            HWND::from_raw(1),
            Ptr::new(0x1000)
        ));
        assert_eq!(ctx.memory.read::<POINT>(0x1000), POINT { x: 11, y: 22 });

        // A null hwnd is the desktop: the point passes through unchanged.
        assert!(ClientToScreen(&mut ctx, HWND::null(), Ptr::new(0x1000)));
        assert_eq!(ctx.memory.read::<POINT>(0x1000), POINT { x: 11, y: 22 });

        // A foreign hwnd and an unreadable out-pointer both fail.
        assert!(!ClientToScreen(
            &mut ctx,
            HWND::from_raw(2),
            Ptr::new(0x1000)
        ));
        assert!(!ClientToScreen(&mut ctx, HWND::from_raw(1), Ptr::new(0)));

        crate::user32::state().window.borrow_mut().take();
    }
}
