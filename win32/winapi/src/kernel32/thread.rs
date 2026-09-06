use std::{
    collections::HashMap,
    sync::{Arc, Condvar, LazyLock, Mutex},
};

use runtime::Context;

use crate::{
    HANDLE, Ptr,
    kernel32::{self, Object},
};

struct CriticalSection {
    state: Mutex<CriticalSectionState>,
    available: Condvar,
}

#[derive(Default)]
struct CriticalSectionState {
    owner: Option<std::thread::ThreadId>,
    depth: u32,
}

static CRITICAL_SECTIONS: LazyLock<Mutex<HashMap<u32, Arc<CriticalSection>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn critical_section(addr: u32) -> Arc<CriticalSection> {
    let mut sections = CRITICAL_SECTIONS.lock().unwrap();
    sections
        .entry(addr)
        .or_insert_with(|| {
            Arc::new(CriticalSection {
                state: Mutex::new(CriticalSectionState::default()),
                available: Condvar::new(),
            })
        })
        .clone()
}

#[repr(C)]
#[derive(zerocopy::FromBytes, zerocopy::Immutable, zerocopy::IntoBytes)]
pub struct NT_TIB {
    ExceptionList: u32,
    StackBase: u32,
    StackLimit: u32,
    SubSystemTib: u32,
    FiberData: u32,
    ArbitraryUserPointer: u32,
    _Self: u32,
}

#[repr(C)]
#[derive(zerocopy::FromBytes, zerocopy::Immutable, zerocopy::IntoBytes, zerocopy::KnownLayout)]
pub struct TEB {
    pub Tib: NT_TIB,
    pub EnvironmentPointer: u32,
    pub ClientId_UniqueProcess: u32,
    pub ClientId_UniqueThread: u32,
    pub ActiveRpcHandle: u32,
    pub ThreadLocalStoragePointer: u32,
    pub Peb: u32,
    pub LastErrorValue: u32,
    pub CountOfOwnedCriticalSections: u32,
    pub CsrClientThread: u32,
    pub Win32ThreadInfo: u32,
    pub User32Reserved: [u32; 26],
    pub UserReserved: [u32; 5],
    pub WOW32Reserved: u32,
    pub CurrentLocale: u32,
    // TODO: ... there are many more fields here
    pub padding: [u32; 20],

    // This is at the wrong offset, but it shouldn't matter.
    pub TlsSlots: [u32; 64],
}

#[allow(unused)]
pub fn teb(ctx: &mut Context) -> &TEB {
    let teb_ptr = Ptr::<TEB>::new(ctx.cpu.regs.fs_base);
    teb_ptr.aligned_ref(&ctx.memory).unwrap()
}

#[allow(unused)]
pub fn teb_mut(ctx: &mut Context) -> &mut TEB {
    let teb_ptr = Ptr::<TEB>::new(ctx.cpu.regs.fs_base);
    teb_ptr.aligned_mut(&mut ctx.memory).unwrap()
}

impl kernel32::State {
    /// Spawn a host thread running `proc` against a fresh guest context.
    /// Returns the thread's object handle and its thread id — the two are
    /// different namespaces, and callers that only need success/failure can
    /// check `.is_some()`.
    pub fn create_thread(
        &mut self,
        ctx: &mut Context,
        name: String,
        proc: impl FnOnce(&mut Context) + Send + 'static,
    ) -> Option<(HANDLE, u32)> {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle = self.objects.add(Object::Thread(done.clone()));
        let thread_id = self.next_thread_id;
        let mut new_ctx = Context {
            cpu: runtime::CPU::default(),
            thread_handle: handle.to_raw(),
            thread_id,
            // See docstring on Memory about the unsafety of sharing memory in this way.
            memory: ctx.memory.unsafe_clone(),
            blocks: ctx.blocks,
            cache: Default::default(),
            recent: [Context::return_from_x86; 4],
        };
        self.next_thread_id += 1;
        // Mapping or spawn exhaustion turns into a null handle rather
        // than a host panic: a guest that spams CreateThread only DoSes
        // itself.
        if !self.init_thread(&mut new_ctx, teb(ctx).Peb) {
            self.objects.remove(handle);
            return None;
        }
        if let Err(err) = std::thread::Builder::new().name(name).spawn(move || {
            proc(&mut new_ctx);
            // Signal the thread handle so WaitForSingleObject and
            // WaitForMultipleObjects observe the thread as exited.
            done.store(true, std::sync::atomic::Ordering::Release);
        }) {
            log::warn!("create_thread: host spawn failed: {err}");
            self.objects.remove(handle);
            return None;
        }
        Some((handle, thread_id))
    }

    // shared between the process initial thread and create_thread
    pub fn init_thread(&mut self, ctx: &mut Context, peb_addr: u32) -> bool {
        let memory_size = ctx.memory.bytes.len() as u32;
        let Some(teb_addr) = self.mappings.try_alloc(
            format!("thread {} TEB", ctx.thread_id),
            std::mem::size_of::<TEB>() as u32,
            memory_size,
        ) else {
            log::warn!("thread TEB mapping could not be allocated");
            return false;
        };
        let Some(teb) = Ptr::<TEB>::new(teb_addr).aligned_mut(&mut ctx.memory) else {
            return false;
        };
        teb.Peb = peb_addr;
        teb.Tib._Self = teb_addr;
        ctx.cpu.regs.fs_base = teb_addr;

        let stack_size = 64 << 10;
        let Some(stack_addr) = self.mappings.try_alloc(
            format!("thread {} stack", ctx.thread_id),
            stack_size,
            memory_size,
        ) else {
            log::warn!("thread stack mapping could not be allocated");
            return false;
        };
        let stack_pointer = stack_addr + stack_size;
        ctx.cpu.regs.esp = stack_pointer;
        ctx.cpu.regs.ebp = stack_pointer;
        true
    }
}

#[win32_derive::dllexport]
pub fn CreateThread(
    ctx: &mut Context,
    _lpThreadAttributes: Ptr<()>,
    _dwStackSize: u32,
    lpStartAddress: Ptr<()>,
    lpParameter: Ptr<()>,
    _dwCreationFlags: u32, /* THREAD_CREATION_FLAGS */
    lpThreadId: Ptr<u32>,
) -> HANDLE {
    let mut lock = kernel32::lock();
    let name = format!("thread {}@{:x}", lock.next_thread_id, lpStartAddress.addr);
    let Some((handle, thread_id)) = lock.create_thread(ctx, name, move |ctx| {
        let f = ctx.indirect(lpStartAddress.addr);
        ctx.call32_x86(f, vec![lpParameter.addr]);
    }) else {
        return HANDLE::null();
    };
    if lpThreadId.addr != 0 {
        let _ = lpThreadId.write(&mut ctx.memory, thread_id);
    }
    // The caller gets the object handle; returning the thread id here would
    // hand back a value WaitForSingleObject/CloseHandle can't resolve.
    handle
}

#[win32_derive::dllexport]
pub fn GetCurrentThread(ctx: &mut Context) -> HANDLE {
    HANDLE::from_raw(ctx.thread_handle)
}

#[win32_derive::dllexport]
pub fn GetCurrentThreadId(ctx: &mut Context) -> u32 {
    ctx.thread_id
}

const TLS_OUT_OF_INDEXES: u32 = 0xffff_ffff;
const ERROR_INVALID_PARAMETER: u32 = 87;

#[win32_derive::dllexport]
pub fn TlsAlloc(_ctx: &mut Context) -> u32 {
    let mut state = kernel32::lock();
    // The lowest clear bit is the lowest available TLS index.
    let index = state.tls_allocated.trailing_ones();
    if index >= 64 {
        return TLS_OUT_OF_INDEXES;
    }
    state.tls_allocated |= 1 << index;
    index
}

#[win32_derive::dllexport]
pub fn TlsGetValue(ctx: &mut Context, dwTlsIndex: u32) -> u32 {
    if dwTlsIndex >= 64 {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return 0;
    }
    teb(ctx).TlsSlots[dwTlsIndex as usize]
}

#[win32_derive::dllexport]
pub fn TlsSetValue(ctx: &mut Context, dwTlsIndex: u32, lpTlsValue: u32) -> bool {
    if dwTlsIndex >= 64 {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return false;
    }
    teb_mut(ctx).TlsSlots[dwTlsIndex as usize] = lpTlsValue;
    true
}

#[win32_derive::dllexport]
pub fn TlsFree(ctx: &mut Context, dwTlsIndex: u32) -> bool {
    if dwTlsIndex >= 64 {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return false;
    }
    let allocated = {
        let mut state = kernel32::lock();
        let bit = 1 << dwTlsIndex;
        let allocated = state.tls_allocated & bit != 0;
        state.tls_allocated &= !bit;
        allocated
    };
    if !allocated {
        teb_mut(ctx).LastErrorValue = ERROR_INVALID_PARAMETER;
        return false;
    }
    // Windows zeroes the slot of the calling thread on free.
    teb_mut(ctx).TlsSlots[dwTlsIndex as usize] = 0;
    true
}

#[win32_derive::dllexport]
pub fn InitializeCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    let _ = critical_section(lpCriticalSection.addr);
}

#[win32_derive::dllexport]
pub fn DeleteCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    CRITICAL_SECTIONS
        .lock()
        .unwrap()
        .remove(&lpCriticalSection.addr);
}

#[win32_derive::dllexport]
pub fn EnterCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    let section = critical_section(lpCriticalSection.addr);
    let current = std::thread::current().id();
    let mut state = section.state.lock().unwrap();
    loop {
        match state.owner {
            None => {
                state.owner = Some(current);
                state.depth = 1;
                return;
            }
            Some(owner) if owner == current => {
                state.depth += 1;
                return;
            }
            Some(_) => state = section.available.wait(state).unwrap(),
        }
    }
}

#[win32_derive::dllexport]
pub fn LeaveCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    let Some(section) = CRITICAL_SECTIONS
        .lock()
        .unwrap()
        .get(&lpCriticalSection.addr)
        .cloned()
    else {
        return;
    };
    let current = std::thread::current().id();
    let mut state = section.state.lock().unwrap();
    if state.owner != Some(current) {
        return;
    }
    state.depth -= 1;
    if state.depth == 0 {
        state.owner = None;
        section.available.notify_one();
    }
}

#[win32_derive::dllexport]
pub fn InterlockedIncrement(ctx: &mut Context, Addend: Ptr<i32>) -> i32 {
    let value = Addend.read(&ctx.memory).unwrap_or_default().wrapping_add(1);
    let _ = Addend.write(&mut ctx.memory, value);
    value
}

#[win32_derive::dllexport]
pub fn InterlockedDecrement(ctx: &mut Context, Addend: Ptr<i32>) -> i32 {
    let value = Addend.read(&ctx.memory).unwrap_or_default().wrapping_sub(1);
    let _ = Addend.write(&mut ctx.memory, value);
    value
}

#[win32_derive::dllexport]
pub fn GetThreadPriority(_ctx: &mut Context, hThread: HANDLE) -> i32 {
    let state = kernel32::lock();
    if !matches!(state.objects.get(hThread), Some(Object::Thread(_))) {
        return -1;
    }
    state.thread_priorities.get(&hThread).copied().unwrap_or(0)
}

#[win32_derive::dllexport]
pub fn SetThreadPriority(
    _ctx: &mut Context,
    hThread: HANDLE,
    nPriority: i32, /* THREAD_PRIORITY */
) -> bool {
    if !(-15..=15).contains(&nPriority) {
        return false;
    }
    let mut state = kernel32::lock();
    if !matches!(state.objects.get(hThread), Some(Object::Thread(_))) {
        return false;
    }
    state.thread_priorities.insert(hThread, nPriority);
    true
}

#[cfg(test)]
mod tests {
    use super::{TlsAlloc, TlsFree, TlsGetValue, TlsSetValue};
    use crate::kernel32::ensure_test_state;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        ensure_test_state();
        let mut ctx = Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        };
        ctx.cpu.regs.fs_base = 0x2000;
        ctx
    }

    #[test]
    fn tls_slots_round_trip_and_reject_bad_indices() {
        let mut ctx = context();
        let index = TlsAlloc(&mut ctx);
        assert!(index < 64);
        assert!(TlsSetValue(&mut ctx, index, 0x1234_5678));
        assert_eq!(TlsGetValue(&mut ctx, index), 0x1234_5678);
        // Out-of-range indices fail with ERROR_INVALID_PARAMETER, not a panic.
        assert!(!TlsSetValue(&mut ctx, 64, 1));
        assert_eq!(teb_last_error(&mut ctx), 87);
        assert_eq!(TlsGetValue(&mut ctx, 64), 0);
        assert_eq!(teb_last_error(&mut ctx), 87);
        // Freeing releases the slot for reuse and zeroes this thread's value.
        assert!(TlsFree(&mut ctx, index));
        assert_eq!(TlsGetValue(&mut ctx, index), 0);
        let reallocated = TlsAlloc(&mut ctx);
        assert_eq!(reallocated, index);
        assert!(TlsFree(&mut ctx, reallocated));
        // Freeing an out-of-range index fails without a panic.
        assert!(!TlsFree(&mut ctx, u32::MAX));
        assert_eq!(teb_last_error(&mut ctx), 87);
    }

    fn teb_last_error(ctx: &mut Context) -> u32 {
        crate::Ptr::<crate::kernel32::thread::TEB>::new(ctx.cpu.regs.fs_base)
            .aligned_ref(&ctx.memory)
            .unwrap()
            .LastErrorValue
    }
}
