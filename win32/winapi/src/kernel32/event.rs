use std::sync::{Arc, Condvar, Mutex};

use runtime::Context;

use crate::{HANDLE, Ptr, kernel32::lock};

pub enum Object {
    Thread,
    Event(Arc<Event>),
    Mutex,
    Mixer([u32; 2]),
    File(host::fs::File),
    FindHandle(crate::kernel32::FindHandle),
}

pub struct Event {
    _name: String,
    manual_reset: bool,
    signaled: Mutex<bool>,
    cond: Condvar,
}

#[win32_derive::dllexport]
pub fn WaitForSingleObject(_ctx: &mut Context, hHandle: HANDLE, dwMilliseconds: u32) -> u32 /* WAIT_EVENT */
{
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 0x102;
    const WAIT_FAILED: u32 = u32::MAX;

    let event = {
        let kernel32 = lock();
        match kernel32.objects.get(hHandle) {
            Some(Object::Event(event)) => event.clone(),
            Some(Object::Mutex) => return WAIT_OBJECT_0,
            Some(Object::Thread) => return WAIT_TIMEOUT,
            _ => return WAIT_FAILED,
        }
    };

    let mut signaled = event.signaled.lock().unwrap();
    while !*signaled {
        let (new_signaled, result) = event
            .cond
            .wait_timeout(
                signaled,
                std::time::Duration::from_millis(dwMilliseconds as u64),
            )
            .unwrap();
        signaled = new_signaled;
        if result.timed_out() {
            return WAIT_TIMEOUT;
        }
    }
    if !event.manual_reset {
        *signaled = false;
    }
    WAIT_OBJECT_0
}

#[win32_derive::dllexport]
pub fn CreateEventA(
    ctx: &mut Context,
    _lpEventAttributes: Ptr<()>,
    bManualReset: bool,
    bInitialState: bool,
    lpName: Ptr<u8>,
) -> HANDLE {
    // A NULL lpName creates an unnamed event.
    let name = if lpName.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_str(lpName.addr).to_string()
    };
    let event = Event {
        _name: name,
        manual_reset: bManualReset,
        signaled: Mutex::new(bInitialState),
        cond: Condvar::new(),
    };
    let mut kernel32 = lock();
    kernel32.objects.add(Object::Event(Arc::new(event)))
}

#[win32_derive::dllexport]
pub fn CreateMutexA(
    _ctx: &mut Context,
    _lpMutexAttributes: Ptr<()>,
    _bInitialOwner: bool,
    _lpName: Ptr<u8>,
) -> HANDLE {
    // Single x86 thread; no contention possible.
    lock().objects.add(Object::Mutex)
}

#[win32_derive::dllexport]
pub fn ReleaseMutex(_ctx: &mut Context, _hMutex: HANDLE) -> bool {
    true
}

enum Waitable {
    Event(Arc<Event>),
    /// Mutexes are always signaled: there is only one x86 thread.
    Always,
    /// Thread handles never signal; the emulated process has one thread that
    /// does not exit.
    Never,
}

impl Waitable {
    fn signaled(&self) -> bool {
        match self {
            Waitable::Event(event) => *event.signaled.lock().unwrap(),
            Waitable::Always => true,
            Waitable::Never => false,
        }
    }

    /// Auto-reset events are consumed by a completed wait.
    fn consume(&self) {
        if let Waitable::Event(event) = self
            && !event.manual_reset
        {
            *event.signaled.lock().unwrap() = false;
        }
    }
}

#[win32_derive::dllexport]
pub fn WaitForMultipleObjects(
    ctx: &mut Context,
    nCount: u32,
    lpHandles: Ptr<u32>,
    bWaitAll: bool,
    dwMilliseconds: u32,
) -> u32 /* WAIT_EVENT */ {
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 0x102;
    const WAIT_FAILED: u32 = u32::MAX;
    const INFINITE: u32 = u32::MAX;

    if nCount == 0 {
        return WAIT_FAILED;
    }
    let mut waitables = Vec::with_capacity(nCount as usize);
    {
        let kernel32 = lock();
        for i in 0..nCount {
            let raw = ctx.memory.read::<u32>(lpHandles.addr + i * 4);
            match kernel32.objects.get(HANDLE::from_raw(raw)) {
                Some(Object::Event(event)) => waitables.push(Waitable::Event(event.clone())),
                Some(Object::Mutex) => waitables.push(Waitable::Always),
                Some(Object::Thread) => waitables.push(Waitable::Never),
                _ => return WAIT_FAILED,
            }
        }
    }

    let deadline = (dwMilliseconds != INFINITE).then(|| {
        std::time::Instant::now() + std::time::Duration::from_millis(dwMilliseconds as u64)
    });
    loop {
        let mut any = None;
        let mut all = true;
        for (i, waitable) in waitables.iter().enumerate() {
            if waitable.signaled() {
                if any.is_none() {
                    any = Some(i);
                }
            } else {
                all = false;
            }
        }
        if (!bWaitAll && any.is_some()) || (bWaitAll && all) {
            if bWaitAll {
                for waitable in &waitables {
                    waitable.consume();
                }
                return WAIT_OBJECT_0;
            }
            let index = any.unwrap();
            waitables[index].consume();
            return WAIT_OBJECT_0 + index as u32;
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return WAIT_TIMEOUT;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Signal an event object by handle, used by timer notifications that
/// deliver TIME_CALLBACK_EVENT_SET (pulse = false) or
/// TIME_CALLBACK_EVENT_PULSE (pulse = true) instead of a function call.
pub fn signal_event(hEvent: HANDLE, pulse: bool) -> bool {
    let kernel32 = lock();
    let Some(Object::Event(event)) = kernel32.objects.get(hEvent) else {
        return false;
    };
    let mut signaled = event.signaled.lock().unwrap();
    *signaled = true;
    event.cond.notify_all();
    if pulse {
        *signaled = false;
    }
    true
}

#[win32_derive::dllexport]
pub fn SetEvent(_ctx: &mut Context, hEvent: HANDLE) -> bool {
    let kernel32 = lock();
    let Some(Object::Event(event)) = kernel32.objects.get(hEvent) else {
        return false;
    };
    *event.signaled.lock().unwrap() = true;
    // A manual-reset event stays signaled until ResetEvent, so wake every
    // waiter; an auto-reset event is consumed by the first thread that wakes.
    if event.manual_reset {
        event.cond.notify_all();
    } else {
        event.cond.notify_one();
    }
    true
}

#[win32_derive::dllexport]
pub fn ResetEvent(_ctx: &mut Context, hEvent: HANDLE) -> bool {
    let kernel32 = lock();
    let Some(Object::Event(event)) = kernel32.objects.get(hEvent) else {
        return false;
    };
    *event.signaled.lock().unwrap() = false;
    true
}
