use runtime::Context;

use crate::{
    Ptr,
    user32::{HICON, HINSTANCE, HWND},
};

#[win32_derive::dllexport]
pub fn ShellExecuteA(
    ctx: &mut Context,
    _hWnd: HWND,
    lpOperation: Ptr<u8>,
    lpFile: Ptr<u8>,
    lpParameters: Ptr<u8>,
    lpDirectory: Ptr<u8>,
    _nShowCmd: i32,
) -> HINSTANCE {
    const SE_ERR_FNF: HINSTANCE = 2;
    const SE_ERR_NOASSOC: HINSTANCE = 31;

    let Some(file) = (lpFile.addr >= 0x1000).then(|| ctx.memory.read_str(lpFile.addr)) else {
        return SE_ERR_FNF;
    };
    let read_opt = |addr: u32| (addr != 0 && addr >= 0x1000).then(|| ctx.memory.read_str(addr));
    let operation = read_opt(lpOperation.addr);
    let parameters = read_opt(lpParameters.addr);
    let directory = read_opt(lpDirectory.addr);
    log::warn!(
        "ShellExecuteA is unavailable: operation={operation:?}, file={file:?}, parameters={parameters:?}, directory={directory:?}"
    );
    SE_ERR_NOASSOC
}

#[win32_derive::dllexport]
pub fn ShellAboutW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _szApp: u32,        /* WSTR */
    _szOtherStuff: u32, /* WSTR */
    _hIcon: HICON,
) -> i32 {
    // No about-dialog UI is modeled; report failure.
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Context, Memory};

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
    fn shell_execute_rejects_null_page_strings() {
        let mut ctx = context();
        assert_eq!(
            ShellExecuteA(
                &mut ctx,
                HWND::from_raw(0),
                Ptr::new(0),
                Ptr::new(0x500),
                Ptr::new(0),
                Ptr::new(0),
                0,
            ),
            2
        );
    }
}
