use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::LazyLock};

use runtime::Context;

use crate::{
    POINT, Ptr,
    dllexport::win32flags,
    stub, trace,
    user32::{HACCEL, HWND, Window, char_for_key, message_vkey, state},
};

/// If THESEUS_TRACE includes "wm", log all Windows messages.
static LOG_MESSAGES: LazyLock<bool> =
    LazyLock::new(|| !matches!(trace::get_uncached("wm"), trace::Trace::None));

pub type WPARAM = u32;
pub type LPARAM = u32;

#[derive(win32_derive::ABIEnum, Debug)]
pub enum WM {
    MOVE = 0x3,
    SIZE = 0x5,
    ACTIVATE = 0x6,
    SETFOCUS = 0x7,
    KILLFOCUS = 0x8,
    ENABLE = 0xa,
    SETTEXT = 0xc,
    PAINT = 0xf,
    ERASEBKGND = 0x14,
    QUIT = 0x12,
    SHOWWINDOW = 0x18,
    ACTIVATEAPP = 0x1c,
    TIMER = 0x113,
    KEYDOWN = 0x100,
    KEYUP = 0x101,
    CHAR = 0x102,
    SYSKEYDOWN = 0x104,
    SYSKEYUP = 0x105,
    MOUSEMOVE = 0x200,
    LBUTTONDOWN = 0x201,
    LBUTTONUP = 0x202,
    RBUTTONDOWN = 0x204,
    RBUTTONUP = 0x205,
    MBUTTONDOWN = 0x207,
    MBUTTONUP = 0x208,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct MSG {
    hwnd: HWND,
    message: u32,
    wParam: WPARAM,
    lParam: LPARAM,
    time: u32,
    pt: POINT,
}

/// A SetTimer-registered timer: WM_TIMER messages are synthesized on demand
/// rather than posted, so a due timer never queues more than one message.
struct Timer {
    hwnd: HWND,
    id: u32,
    /// The SetTimer lpTimerFunc callback, or 0 for window-proc delivery.
    proc_addr: u32,
    /// Milliseconds between firings.
    elapse: u32,
    /// Host-clock time of the next firing.
    next_fire: u32,
    /// A WM_TIMER was synthesized for this timer and is still pending.
    queued: bool,
}

#[derive(Default)]
pub struct MessageQueue {
    pub window: Option<Rc<RefCell<Window>>>,
    messages: VecDeque<MSG>,
    quit: Option<MSG>,
    timers: Vec<Timer>,
    next_timer_id: u32,
}

/// Which queued messages a GetMessage/PeekMessage `hWnd` argument selects.
#[derive(Clone, Copy)]
enum HwndFilter {
    /// `hWnd == NULL`: every message queued to this thread.
    Any,
    /// `hWnd == (HWND)-1`: only thread messages (those with a null `hwnd`).
    ThreadOnly,
    /// A specific window handle: only that window's messages.
    Window(HWND),
}

/// The `(hWnd, wMsgFilterMin, wMsgFilterMax)` selection shared by GetMessage
/// and PeekMessage.
struct MsgFilter {
    hwnd: HwndFilter,
    min: u32,
    max: u32,
}

impl MsgFilter {
    fn new(hwnd: HWND, min: u32, max: u32) -> Self {
        let hwnd = if hwnd.is_null() {
            HwndFilter::Any
        } else if hwnd.is_invalid() {
            HwndFilter::ThreadOnly
        } else {
            HwndFilter::Window(hwnd)
        };
        MsgFilter { hwnd, min, max }
    }

    /// The hWnd names a window that is not ours, so GetMessage should fail
    /// rather than block forever.
    fn is_invalid_window(&self) -> bool {
        match self.hwnd {
            HwndFilter::Window(hwnd) => match state().window.borrow().as_ref() {
                Some(window) => window.borrow().hwnd != hwnd,
                None => true,
            },
            _ => false,
        }
    }

    fn matches(&self, msg: &MSG) -> bool {
        let hwnd = match self.hwnd {
            HwndFilter::Any => true,
            HwndFilter::ThreadOnly => msg.hwnd.is_null(),
            HwndFilter::Window(hwnd) => msg.hwnd == hwnd,
        };
        // A min/max of 0/0 selects every message.
        let range = (self.min == 0 && self.max == 0)
            || (msg.message >= self.min && msg.message <= self.max);
        hwnd && range
    }
}

win32flags! {
    pub struct MK {
        const LBUTTON = 0x0001;
        const RBUTTON = 0x0002;
        const SHIFT   = 0x0004;
        const CONTROL = 0x0008;
        const MBUTTON = 0x0010;
    }
}

fn mouse_button_to_wm(is_down: bool, message: &host::MouseMessage) -> WM {
    // Can't use a match here because MouseButton is a bitfield, not an enum.
    if message.button == host::MouseButton::Left {
        if is_down {
            WM::LBUTTONDOWN
        } else {
            WM::LBUTTONUP
        }
    } else if message.button == host::MouseButton::Right {
        if is_down {
            WM::RBUTTONDOWN
        } else {
            WM::RBUTTONUP
        }
    } else if message.button == host::MouseButton::Middle {
        if is_down {
            WM::MBUTTONDOWN
        } else {
            WM::MBUTTONUP
        }
    } else {
        WM::MOUSEMOVE
    }
}

fn mouse_msg(wm: WM, hwnd: HWND, message: &host::MouseMessage) -> MSG {
    let mut wParam = MK::empty();
    if message.buttons.contains(host::MouseButton::Left) {
        wParam |= MK::LBUTTON;
    }
    if message.buttons.contains(host::MouseButton::Middle) {
        wParam |= MK::MBUTTON;
    }
    if message.buttons.contains(host::MouseButton::Right) {
        wParam |= MK::RBUTTON;
    }

    // MSG.pt is the cursor position in *screen* coordinates: the client-space
    // message position plus the window's screen origin (no frame or caption is
    // modeled, so the client origin coincides with the window origin).
    let origin = state()
        .window
        .borrow()
        .as_ref()
        .map(|window| {
            let window = window.borrow();
            (window.x, window.y)
        })
        .unwrap_or_default();
    MSG {
        hwnd,
        message: wm as u32,
        wParam: wParam.bits(),
        lParam: (message.y as u16 as u32) << 16 | message.x as u16 as u32,
        time: host::host().time(),
        pt: POINT {
            x: message.x as i32 + origin.0,
            y: message.y as i32 + origin.1,
        },
    }
}

fn key_msg(hwnd: HWND, key: &host::KeyMessage, down: bool) -> MSG {
    // lParam packs the key's physical details, as documented for WM_KEYDOWN.
    let mut lParam = 1; // repeat count; the host reports repeats one at a time
    lParam |= (key.scancode as u32) << 16;
    if key.extended {
        lParam |= 1 << 24;
    }
    // Bit 29 is set while alt is held, bit 30 holds the previous key state,
    // bit 31 marks the release.
    let alt = state().input.borrow().key_down(0x12); // VK_MENU
    if alt {
        lParam |= 1 << 29;
    }
    if key.repeat || !down {
        lParam |= 1 << 30;
    }
    if !down {
        lParam |= 1 << 31;
    }

    // Keys pressed with alt held are "system" keys, as is alt itself.
    let system = alt || key.vkey == 0xa4 || key.vkey == 0xa5;
    let message = match (down, system) {
        (true, false) => WM::KEYDOWN,
        (false, false) => WM::KEYUP,
        (true, true) => WM::SYSKEYDOWN,
        (false, true) => WM::SYSKEYUP,
    };

    MSG {
        hwnd,
        message: message as u32,
        wParam: message_vkey(key.vkey) as u32,
        lParam,
        time: host::host().time(),
        pt: POINT::default(),
    }
}

/// Post a message to the application's queue (e.g. synthetic activation
/// messages from ShowWindow).
pub fn post_message(hwnd: HWND, message: u32, wParam: WPARAM, lParam: LPARAM) {
    let mut queue = state().message_queue.borrow_mut();
    queue.messages.push_back(MSG {
        hwnd,
        message,
        wParam,
        lParam,
        time: 0,
        pt: POINT::default(),
    });
}

#[win32_derive::dllexport]
pub fn WaitMessage(_ctx: &mut Context) -> bool {
    let mut queue = state().message_queue.borrow_mut();
    if queue.peek(host::host().time()).is_none() {
        queue.wait_or_poll();
    }
    true
}

impl MessageQueue {
    fn paint_msg(&self) -> Option<MSG> {
        let window = self.window.as_ref()?.borrow();
        if !window.dirty {
            return None;
        }

        Some(MSG {
            hwnd: window.hwnd,
            message: WM::PAINT as u32,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT::default(),
        })
    }

    /// WM_TIMER, like WM_PAINT, is generated when the queue is otherwise
    /// empty rather than posted: an already-synthesized timer message
    /// (`queued`) or a newly due one matches here. Removing the message
    /// reschedules the timer; peeking just marks it pending so a due timer
    /// never reports more than one waiting message.
    fn timer_msg(&mut self, remove: bool, now: u32, filter: &MsgFilter) -> Option<MSG> {
        if self.timers.is_empty() {
            return None;
        }
        let index = self.timers.iter().position(|timer| {
            if !timer.queued && now.wrapping_sub(timer.next_fire) >= 0x8000_0000 {
                return false;
            }
            let msg = MSG {
                hwnd: timer.hwnd,
                message: WM::TIMER as u32,
                wParam: timer.id,
                lParam: timer.proc_addr,
                time: now,
                pt: POINT::default(),
            };
            filter.matches(&msg)
        })?;
        let timer = &mut self.timers[index];
        let msg = MSG {
            hwnd: timer.hwnd,
            message: WM::TIMER as u32,
            wParam: timer.id,
            lParam: timer.proc_addr,
            time: now,
            pt: POINT::default(),
        };
        if remove {
            timer.queued = false;
            timer.next_fire = now.wrapping_add(timer.elapse);
        } else {
            timer.queued = true;
        }
        Some(msg)
    }

    /// `now` is the host millisecond clock, supplied by the caller so the
    /// queue itself never depends on the host and stays testable.
    fn peek_filtered(&mut self, now: u32, filter: &MsgFilter) -> Option<MSG> {
        if let Some(msg) = self.messages.iter().find(|msg| filter.matches(msg)) {
            Some(*msg)
        } else if self.quit.is_some() {
            // WM_QUIT is a thread-level flag, not a window message: it is
            // delivered regardless of the hWnd or message-range filter.
            self.quit
        } else {
            self.paint_msg()
                .filter(|msg| filter.matches(msg))
                .or_else(|| self.timer_msg(false, now, filter))
        }
    }

    fn peek(&mut self, now: u32) -> Option<MSG> {
        self.peek_filtered(now, &MsgFilter::new(HWND::null(), 0, 0))
    }

    fn pop_filtered(&mut self, now: u32, filter: &MsgFilter) -> Option<MSG> {
        if let Some(index) = self.messages.iter().position(|msg| filter.matches(msg)) {
            self.messages.remove(index)
        } else if self.quit.is_some() {
            self.quit.take()
        } else {
            self.paint_msg()
                .filter(|msg| filter.matches(msg))
                .or_else(|| self.timer_msg(true, now, filter))
        }
    }

    /// Pop one message matching the filter, waiting for a new one if
    /// necessary.
    fn read(&mut self, filter: &MsgFilter) -> MSG {
        loop {
            if let Some(msg) = self.pop_filtered(host::host().time(), filter) {
                return msg;
            }
            self.wait_or_poll();
        }
    }

    /// Block for a host event, or poll briefly when a timer could expire
    /// during the wait — timers synthesize their own messages and must wake
    /// the loop without a host event.
    fn wait_or_poll(&mut self) {
        if self.timers.is_empty() {
            self.wait_host();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.poll_host();
        }
    }

    /// Read one pending host message, if any available.
    fn poll_host(&mut self) {
        let Some(message) = host::host().poll() else {
            return;
        };
        self.enqueue_message(message);
    }

    /// Read every pending host message. DirectInput calls this to refresh
    /// input state without going through the window message queue.
    pub fn poll_host_all(&mut self) {
        while let Some(message) = host::host().poll() {
            self.enqueue_message(message);
        }
    }

    /// Wait for a new message to arrive.
    fn wait_host(&mut self) {
        let message = host::host().wait();
        self.enqueue_message(message);
    }

    fn enqueue_message(&mut self, msg: host::Message) {
        #[cfg(not(target_family = "wasm"))]
        if matches!(msg, host::Message::Paint) {
            if let Some(window) = &self.window {
                window.borrow_mut().dirty = true;
            }
            return;
        }

        // Every host input event updates the shared input state, whether or not
        // the app reads it through the message queue: DirectInput reads the
        // same state, and this is the only place host events are consumed.
        {
            let mut input = state().input.borrow_mut();
            match &msg {
                host::Message::KeyDown(key) => input.on_key(key, true),
                host::Message::KeyUp(key) => input.on_key(key, false),
                host::Message::MouseDown(mouse)
                | host::Message::MouseUp(mouse)
                | host::Message::MouseMove(mouse) => input.on_mouse(mouse),
                _ => {}
            }
        }

        let Some(msg) = self.msg_from_message(msg) else {
            return;
        };
        if *LOG_MESSAGES {
            log::info!("{:#x?}", msg);
        }

        // PAINT/TIMER/QUIT are in special queues.
        if msg.message == WM::QUIT as u32 {
            self.quit = Some(msg);
        } else {
            self.messages.push_back(msg);
        }
    }

    /// SetTimer: install or replace a timer for `(hWnd, nIDEvent)`. With a
    /// null hWnd the caller's id is ignored and a fresh one is returned.
    pub fn set_timer(
        &mut self,
        hwnd: HWND,
        id: u32,
        elapse_ms: u32,
        proc_addr: u32,
        now: u32,
    ) -> u32 {
        let id = if hwnd.is_null() {
            self.next_timer_id = self.next_timer_id.wrapping_add(1).max(1);
            self.next_timer_id
        } else {
            id
        };
        // uElapse is clamped into [USER_TIMER_MINIMUM, USER_TIMER_MAXIMUM].
        let elapse = elapse_ms.clamp(10, 0x7fff_ffff);
        let next_fire = now.wrapping_add(elapse);
        match self
            .timers
            .iter_mut()
            .find(|timer| timer.hwnd == hwnd && timer.id == id)
        {
            Some(timer) => {
                timer.proc_addr = proc_addr;
                timer.elapse = elapse;
                timer.next_fire = next_fire;
            }
            None => self.timers.push(Timer {
                hwnd,
                id,
                proc_addr,
                elapse,
                next_fire,
                queued: false,
            }),
        }
        id
    }

    /// KillTimer: remove the `(hWnd, uIDEvent)` timer if one exists.
    pub fn kill_timer(&mut self, hwnd: HWND, id: u32) -> bool {
        let before = self.timers.len();
        self.timers
            .retain(|timer| !(timer.hwnd == hwnd && timer.id == id));
        self.timers.len() != before
    }

    fn msg_from_message(&self, message: host::Message) -> Option<MSG> {
        use host::Message::*;
        // WM_QUIT is a thread message like PostQuitMessage's, not a window
        // message, and must still be recorded before a window exists.
        #[cfg(not(target_family = "wasm"))]
        if matches!(message, Quit) {
            return Some(MSG {
                hwnd: HWND::null(),
                message: WM::QUIT as u32,
                wParam: 0,
                lParam: 0,
                time: host::host().time(),
                pt: POINT::default(),
            });
        }
        // Other host events can arrive before the window exists; there is no
        // hwnd to deliver them to, but their input-state update above still
        // ran.
        let hwnd = self.window.as_ref()?.borrow().hwnd;
        Some(match message {
            MouseDown(mouse) => mouse_msg(mouse_button_to_wm(true, &mouse), hwnd, &mouse),
            MouseUp(mouse) => mouse_msg(mouse_button_to_wm(false, &mouse), hwnd, &mouse),
            MouseMove(mouse) => mouse_msg(WM::MOUSEMOVE, hwnd, &mouse),
            KeyDown(key) => key_msg(hwnd, &key, true),
            KeyUp(key) => key_msg(hwnd, &key, false),
            #[cfg(not(target_family = "wasm"))]
            // Paint is translated into a dirty flag in enqueue_message and
            // Quit is a thread message handled above; a stray event here is
            // dropped rather than panicking the host.
            Paint | Quit => return None,
        })
    }
}

#[win32_derive::dllexport]
pub fn DispatchMessageA(ctx: &mut Context, lpMsg: Ptr<MSG>) -> u32 {
    DispatchMessageW(ctx, lpMsg)
}

#[win32_derive::dllexport]
pub fn DispatchMessageW(ctx: &mut Context, lpMsg: Ptr<MSG>) -> u32 {
    let Some(msg) = lpMsg.read(&ctx.memory) else {
        return 0;
    };
    // A WM_TIMER whose lParam names a SetTimer callback goes to that
    // TIMERPROC, not to the window procedure.
    if msg.message == WM::TIMER as u32 && msg.lParam != 0 {
        ctx.call32_x86(
            ctx.indirect(msg.lParam),
            vec![
                msg.hwnd.to_raw(),
                msg.message,
                msg.wParam,
                host::host().time(),
            ],
        );
        return 0;
    }
    let wndproc = {
        let window = state().window.borrow();
        let wndclass = state().wndclass.borrow();
        match (window.as_ref(), wndclass.as_ref()) {
            (Some(window), Some(wndclass)) if window.borrow().hwnd == msg.hwnd => {
                // A SetWindowLong(GWL_WNDPROC) subclass wins over the
                // class's registered procedure.
                match window.borrow().subclass_proc {
                    Some(addr) => ctx.indirect(addr),
                    None => wndclass.wndproc,
                }
            }
            // Thread messages and messages for windows we do not model have
            // no window procedure to dispatch to.
            _ => return 0,
        }
    };
    // WNDPROC
    ctx.call32_x86(
        wndproc,
        vec![msg.hwnd.to_raw(), msg.message, msg.wParam, msg.lParam],
    );
    ctx.cpu.regs.eax
}

#[win32_derive::dllexport]
pub fn TranslateMessage(ctx: &mut Context, lpMsg: Ptr<MSG>) -> bool {
    let Some(msg) = lpMsg.read(&ctx.memory) else {
        return false;
    };
    if msg.message != WM::KEYDOWN as u32 {
        return false;
    }
    let Some(ch) = char_for_key(msg.wParam as u8) else {
        return false;
    };
    // The character message follows the key message in the queue, so the app
    // sees it on its next pump.
    post_message(msg.hwnd, WM::CHAR as u32, ch as u32, msg.lParam);
    true
}

#[win32_derive::dllexport]
pub fn PeekMessageA(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
    wRemoveMsg: u32, /* PEEK_MESSAGE_REMOVE_TYPE */
) -> bool {
    // PM_REMOVE is bit 0; the remaining bits (PM_NOYIELD, PM_QS_*) are
    // filtering/scheduling hints that change nothing in this emulated queue.
    let remove = wRemoveMsg & 1 != 0;
    // Games poll for messages every frame; keep the audio mixer fed from here
    // too, in case the app renders without flipping.
    crate::dsound::pump(ctx);

    let filter = MsgFilter::new(hWnd, wMsgFilterMin, wMsgFilterMax);
    let mut queue = state().message_queue.borrow_mut();
    queue.poll_host();
    let now = host::host().time();
    let Some(msg) = queue.peek_filtered(now, &filter) else {
        return false;
    };

    if lpMsg.write(&mut ctx.memory, msg).is_none() {
        return false;
    }
    if remove {
        queue.pop_filtered(now, &filter);
    }
    true
}

#[win32_derive::dllexport]
pub fn PeekMessageW(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
    wRemoveMsg: u32, /* PEEK_MESSAGE_REMOVE_TYPE */
) -> bool {
    PeekMessageA(ctx, lpMsg, hWnd, wMsgFilterMin, wMsgFilterMax, wRemoveMsg)
}

#[win32_derive::dllexport]
pub fn GetMessageA(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
) -> i32 {
    GetMessageW(ctx, lpMsg, hWnd, wMsgFilterMin, wMsgFilterMax)
}

#[win32_derive::dllexport]
pub fn GetMessageW(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
) -> i32 {
    let filter = MsgFilter::new(hWnd, wMsgFilterMin, wMsgFilterMax);
    if filter.is_invalid_window() {
        return -1;
    }
    let msg = state().message_queue.borrow_mut().read(&filter);
    if lpMsg.write(&mut ctx.memory, msg).is_none() {
        return -1; // error
    }
    if msg.message == WM::QUIT as u32 {
        return 0;
    }

    1 // no error, no WM_QUIT
}

#[win32_derive::dllexport]
pub fn TranslateAcceleratorA(
    _ctx: &mut Context,
    _hWnd: HWND,
    _hAccTable: HACCEL,
    _lpMsg: Ptr<MSG>,
) -> i32 {
    stub!(0) // no translation
}

#[win32_derive::dllexport]
pub fn TranslateAcceleratorW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _hAccTable: HACCEL,
    _lpMsg: Ptr<MSG>,
) -> i32 {
    stub!(0) // no translation
}

#[win32_derive::dllexport]
pub fn PostQuitMessage(_ctx: &mut Context, nExitCode: i32) {
    let mut queue = state().message_queue.borrow_mut();
    queue.quit = Some(MSG {
        hwnd: HWND::null(),
        message: WM::QUIT as u32,
        wParam: nExitCode as u32,
        lParam: 0,
        time: 0,
        pt: POINT::default(),
    });
}

#[win32_derive::dllexport]
pub fn PostMessageW(
    _ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> bool {
    post_message(hWnd, Msg, wParam, lParam);
    true
}

#[win32_derive::dllexport]
pub fn PostMessageA(
    _ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> bool {
    post_message(hWnd, Msg, wParam, lParam);
    true
}

#[win32_derive::dllexport]
pub fn SendMessageA(
    ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> u32 {
    SendMessageW(ctx, hWnd, Msg, wParam, lParam)
}

#[win32_derive::dllexport]
pub fn SendMessageW(
    ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> u32 {
    let wndproc = {
        let window = state().window.borrow();
        let Some(window) = window.as_ref() else {
            return 0;
        };
        let subclass = {
            let window = window.borrow();
            if window.hwnd != hWnd {
                return 0;
            }
            window.subclass_proc
        };
        // A SetWindowLong(GWL_WNDPROC) subclass wins over the class's
        // registered procedure.
        match subclass {
            Some(addr) => ctx.indirect(addr),
            None => {
                let wndclass = state().wndclass.borrow();
                let Some(wndclass) = wndclass.as_ref() else {
                    return 0;
                };
                wndclass.wndproc
            }
        }
    };
    ctx.call32_x86(wndproc, vec![hWnd.to_raw(), Msg, wParam, lParam]);
    ctx.cpu.regs.eax
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(hwnd: u32, message: u32) -> MSG {
        MSG {
            hwnd: HWND::from_raw(hwnd),
            message,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT::default(),
        }
    }

    #[test]
    fn queue_filters_by_window_and_message_range() {
        let mut queue = MessageQueue::default();
        queue.messages.push_back(msg(7, WM::KEYDOWN as u32));
        queue.messages.push_back(msg(0, WM::CHAR as u32));
        queue.messages.push_back(msg(7, WM::LBUTTONDOWN as u32));

        // A specific window only sees its own messages; a null-hwnd filter
        // sees everything in posted order.
        let window = MsgFilter::new(HWND::from_raw(7), 0, 0);
        assert_eq!(
            queue.peek_filtered(0, &window).unwrap().message,
            WM::KEYDOWN as u32
        );
        assert_eq!(
            queue.pop_filtered(0, &window).unwrap().message,
            WM::KEYDOWN as u32
        );
        assert_eq!(
            queue.peek_filtered(0, &window).unwrap().message,
            WM::LBUTTONDOWN as u32
        );

        // (HWND)-1 selects only thread messages: the null-hwnd WM_CHAR.
        let thread = MsgFilter::new(HWND::invalid(), 0, 0);
        assert_eq!(
            queue.peek_filtered(0, &thread).unwrap().message,
            WM::CHAR as u32
        );

        // Message-range filtering applies on top of the hWnd selection.
        let keys = MsgFilter::new(HWND::from_raw(7), WM::KEYDOWN as u32, WM::KEYUP as u32);
        assert!(queue.peek_filtered(0, &keys).is_none());
        let mouse = MsgFilter::new(
            HWND::from_raw(7),
            WM::LBUTTONDOWN as u32,
            WM::LBUTTONUP as u32,
        );
        assert_eq!(
            queue.pop_filtered(0, &mouse).unwrap().message,
            WM::LBUTTONDOWN as u32
        );
    }

    #[test]
    fn quit_message_survives_window_filtering() {
        let mut queue = MessageQueue::default();
        queue.messages.push_back(msg(7, WM::KEYDOWN as u32));
        queue.quit = Some(msg(0, WM::QUIT as u32));

        // Posted window messages still win over the quit flag; once no
        // matching message remains the quit is delivered even to a
        // window-filtered read.
        let window = MsgFilter::new(HWND::from_raw(7), 0, 0);
        assert_eq!(
            queue.pop_filtered(0, &window).unwrap().message,
            WM::KEYDOWN as u32
        );
        assert_eq!(
            queue.pop_filtered(0, &window).unwrap().message,
            WM::QUIT as u32
        );
        assert!(queue.quit.is_none());
    }

    #[test]
    fn timers_fire_once_and_reschedule_on_removal() {
        let mut queue = MessageQueue::default();
        let any = MsgFilter::new(HWND::null(), 0, 0);
        let id = queue.set_timer(HWND::from_raw(7), 4, 50, 0x1234, 1000);
        assert_eq!(id, 4);

        // Not due yet, then due at next_fire.
        assert!(queue.pop_filtered(1049, &any).is_none());
        let timer = queue.timer_msg(false, 1051, &any).unwrap();
        assert_eq!(timer.message, WM::TIMER as u32);
        assert_eq!(timer.hwnd, HWND::from_raw(7));
        assert_eq!(timer.wParam, 4);
        assert_eq!(timer.lParam, 0x1234);

        // A due timer coalesces: repeated peeks report the same pending
        // message instead of queueing more.
        let again = queue.timer_msg(false, 1060, &any).unwrap();
        assert_eq!(again.wParam, 4);
        // Removal reschedules; the timer is no longer pending or due.
        let removed = queue.pop_filtered(1100, &any).unwrap();
        assert_eq!(removed.message, WM::TIMER as u32);
        assert!(queue.timer_msg(false, 1149, &any).is_none());
        assert_eq!(
            queue.timer_msg(true, 1150, &any).unwrap().message,
            WM::TIMER as u32
        );

        // KillTimer drops the timer entirely.
        assert!(queue.kill_timer(HWND::from_raw(7), 4));
        assert!(!queue.kill_timer(HWND::from_raw(7), 4));
    }

    #[test]
    fn null_hwnd_timers_get_allocated_ids() {
        let mut queue = MessageQueue::default();
        let first = queue.set_timer(HWND::null(), 0, 20, 0, 0);
        let second = queue.set_timer(HWND::null(), 0, 20, 0, 0);
        assert!(first != 0 && second != 0 && first != second);
        // A window filter does not see a null-hwnd timer's message.
        let window = MsgFilter::new(HWND::from_raw(7), 0, 0);
        assert!(queue.timer_msg(false, 100, &window).is_none());
        let any = MsgFilter::new(HWND::null(), 0, 0);
        assert_eq!(
            queue.timer_msg(false, 100, &any).unwrap().message,
            WM::TIMER as u32
        );
    }
}
