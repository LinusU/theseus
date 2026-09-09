use std::sync::{Arc, Condvar, Mutex};

use runtime::Context;

use crate::{HANDLE, Ptr, kernel32::lock};

pub enum Object {
    Thread,
    Event(Arc<Event>),
    Mutex,
    File(host::fs::File),
    FindHandle(crate::kernel32::FindHandle),
}

pub struct Event {
    _name: String,
    manual_reset: bool,
    signaled: Mutex<bool>,
    cond: Condvar,
}

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x102;
const WAIT_FAILED: u32 = 0xFFFFFFFF;

#[win32_derive::dllexport]
pub fn WaitForSingleObject(_ctx: &mut Context, hHandle: HANDLE, dwMilliseconds: u32) -> u32 /* WAIT_EVENT */
{
    let event = {
        let kernel32 = lock();
        let Some(Object::Event(event)) = kernel32.objects.get(hHandle) else {
            return WAIT_FAILED;
        };
        event.clone()
    };

    let mut signaled = event.signaled.lock().unwrap();
    if *signaled {
        if !event.manual_reset {
            *signaled = false;
        }
        return WAIT_OBJECT_0;
    }

    // Already unsignaled. If the caller just wants to poll, return immediately.
    if dwMilliseconds == 0 {
        return WAIT_TIMEOUT;
    }

    // INFINITE: block until the event is set.
    if dwMilliseconds == 0xFFFFFFFF {
        while !*signaled {
            signaled = event.cond.wait(signaled).unwrap();
        }
        if !event.manual_reset {
            *signaled = false;
        }
        return WAIT_OBJECT_0;
    }

    let mut result = event
        .cond
        .wait_timeout(
            signaled,
            std::time::Duration::from_millis(dwMilliseconds as u64),
        )
        .unwrap();
    if result.1.timed_out() {
        WAIT_TIMEOUT
    } else {
        if !event.manual_reset {
            *result.0 = false;
        }
        WAIT_OBJECT_0
    }
}

#[win32_derive::dllexport]
pub fn CreateEventA(
    ctx: &mut Context,
    _lpEventAttributes: Ptr<()>,
    bManualReset: bool,
    bInitialState: bool,
    lpName: Ptr<u8>,
) -> HANDLE {
    let name = if lpName.addr != 0 {
        ctx.memory.read_str(lpName.addr).to_string()
    } else {
        String::new()
    };
    let event = Event {
        _name: name.to_string(),
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

#[win32_derive::dllexport]
pub fn WaitForMultipleObjects(
    _ctx: &mut Context,
    _nCount: u32,
    _lpHandles: Ptr<u32>,
    _bWaitAll: bool,
    _dwMilliseconds: u32,
) -> u32 /* WAIT_EVENT */ {
    crate::stub!(0) // WAIT_OBJECT_0
}

#[win32_derive::dllexport]
pub fn SetEvent(_ctx: &mut Context, hEvent: HANDLE) -> bool {
    let kernel32 = lock();
    let Some(Object::Event(event)) = kernel32.objects.get(hEvent) else {
        return false;
    };
    *event.signaled.lock().unwrap() = true;
    // TODO: the number of threads notified are different between manual reset and auto reset events!
    assert!(!event.manual_reset);
    event.cond.notify_one();
    true
}
