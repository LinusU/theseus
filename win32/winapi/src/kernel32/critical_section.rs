use std::sync::{Arc, Condvar, Mutex};

use runtime::Context;

use crate::{Ptr, kernel32::lock};

#[derive(Default)]
pub(crate) struct CriticalSection {
    owner: Mutex<Owner>,
    wake: Condvar,
}

#[derive(Default)]
struct Owner {
    thread_id: Option<u32>,
    recursion: u32,
}

impl CriticalSection {
    fn enter(&self, thread_id: u32) {
        let mut owner = self.owner.lock().unwrap();
        while owner.thread_id.is_some_and(|owner| owner != thread_id) {
            owner = self.wake.wait(owner).unwrap();
        }
        owner.thread_id = Some(thread_id);
        owner.recursion += 1;
    }

    fn leave(&self, thread_id: u32) {
        let mut owner = self.owner.lock().unwrap();
        assert_eq!(owner.thread_id, Some(thread_id));
        owner.recursion -= 1;
        if owner.recursion == 0 {
            owner.thread_id = None;
            self.wake.notify_one();
        }
    }
}

fn get_or_create(addr: u32) -> Arc<CriticalSection> {
    lock().critical_sections.entry(addr).or_default().clone()
}

#[win32_derive::dllexport]
pub fn InitializeCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    get_or_create(lpCriticalSection.addr);
}

#[win32_derive::dllexport]
pub fn EnterCriticalSection(ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    get_or_create(lpCriticalSection.addr).enter(ctx.thread_id);
}

#[win32_derive::dllexport]
pub fn LeaveCriticalSection(ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    let critical_section = {
        lock()
            .critical_sections
            .get(&lpCriticalSection.addr)
            .cloned()
    };
    critical_section
        .expect("leaving an uninitialized critical section")
        .leave(ctx.thread_id);
}

#[win32_derive::dllexport]
pub fn DeleteCriticalSection(_ctx: &mut Context, lpCriticalSection: Ptr<()>) {
    lock().critical_sections.remove(&lpCriticalSection.addr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_sections_are_recursive() {
        let critical_section = CriticalSection::default();

        critical_section.enter(1);
        critical_section.enter(1);
        critical_section.leave(1);
        critical_section.leave(1);

        let owner = critical_section.owner.lock().unwrap();
        assert_eq!(owner.thread_id, None);
        assert_eq!(owner.recursion, 0);
    }

    #[test]
    fn critical_sections_exclude_other_threads() {
        let critical_section = Arc::new(CriticalSection::default());
        critical_section.enter(1);

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let other = critical_section.clone();
        let thread = std::thread::spawn(move || {
            other.enter(2);
            entered_tx.send(()).unwrap();
            other.leave(2);
        });

        assert!(
            entered_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        critical_section.leave(1);
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        thread.join().unwrap();
    }
}
