use runtime::Context;

use crate::{
    Ptr,
    kernel32::{lock, teb, teb_mut},
};

#[win32_derive::dllexport]
pub fn GetLastError(ctx: &mut Context) -> u32 {
    teb(ctx).map_or(0, |teb| teb.LastErrorValue)
}

#[win32_derive::dllexport]
pub fn SetLastError(ctx: &mut Context, error: u32) {
    if let Some(teb) = teb_mut(ctx) {
        teb.LastErrorValue = error;
    }
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct SYSTEM_INFO {
    pub dwOemId: u32,
    pub dwPageSize: u32,
    pub lpMinimumApplicationAddress: u32,
    pub lpMaximumApplicationAddress: u32,
    pub dwActiveProcessorMask: u32,
    pub dwNumberOfProcessors: u32,
    pub dwProcessorType: u32,
    pub dwAllocationGranularity: u32,
    pub wProcessorLevel: u16,
    pub wProcessorRevision: u16,
}

#[win32_derive::dllexport]
pub fn GetSystemInfo(ctx: &mut Context, lpSystemInfo: Ptr<SYSTEM_INFO>) {
    if !crate::ddraw::guest_range(
        ctx,
        lpSystemInfo.addr,
        std::mem::size_of::<SYSTEM_INFO>() as u32,
    ) {
        return;
    }
    let info = SYSTEM_INFO {
        dwPageSize: 0x1000,
        lpMinimumApplicationAddress: 0x10000,
        lpMaximumApplicationAddress: 0x7fff0000,
        dwActiveProcessorMask: 1,
        dwNumberOfProcessors: 1,
        dwProcessorType: 586,
        dwAllocationGranularity: 0x10000,
        wProcessorLevel: 6,
        ..Default::default()
    };
    let _ = lpSystemInfo.write(&mut ctx.memory, info);
}

fn processor_feature_present(feature: u32) -> bool {
    matches!(feature, 3 | 8)
}

#[win32_derive::dllexport]
pub fn IsProcessorFeaturePresent(_ctx: &mut Context, ProcessorFeature: u32) -> bool {
    processor_feature_present(ProcessorFeature)
}

#[win32_derive::dllexport]
pub fn GetComputerNameA(ctx: &mut Context, lpBuffer: Ptr<u8>, nSize: Ptr<u32>) -> bool {
    let name = b"THESEUS";
    if nSize.addr < 0x1000 || lpBuffer.addr < 0x1000 {
        return false;
    }
    let Some(size) = nSize.read(&ctx.memory) else {
        return false;
    };
    let output_len = name.len() + 1;
    if (size as usize) < output_len
        || (lpBuffer.addr as usize)
            .checked_add(output_len)
            .is_none_or(|end| end > ctx.memory.bytes.len())
    {
        return false;
    }
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(lpBuffer.addr as usize..)
        .and_then(|b| b.get_mut(..name.len()))
    {
        dst.copy_from_slice(name);
    }
    if Ptr::<u8>::new(lpBuffer.addr + name.len() as u32)
        .write(&mut ctx.memory, 0)
        .is_none()
        || nSize.write(&mut ctx.memory, name.len() as u32).is_none()
    {
        return false;
    }
    true
}

#[win32_derive::dllexport]
pub fn ExitThread(_ctx: &mut Context, dwExitCode: u32) {
    // Wrong for a thread from CreateThread, which should end just that thread.
    // Loud because getting here from a worker kills the process.
    log::warn!("ExitThread({dwExitCode}): exiting the whole process");
    std::process::exit(dwExitCode as i32);
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct STARTUPINFOA {
    cb: u32,
    lpReserved: u32,
    lpDesktop: u32,
    lpTitle: u32,
    dwX: u32,
    dwY: u32,
    dwXSize: u32,
    dwYSize: u32,
    dwXCountChars: u32,
    dwYCountChars: u32,
    dwFillAttribute: u32,
    dwFlags: u32,
    wShowWindow: u16,
    cbReserved2: u16,
    lpReserved2: u32,
    hStdInput: u32,
    hStdOutput: u32,
    hStdError: u32,
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct PROCESS_INFORMATION {
    pub hProcess: u32,
    pub hThread: u32,
    pub dwProcessId: u32,
    pub dwThreadId: u32,
}

#[win32_derive::dllexport]
pub fn CreateProcessA(
    _ctx: &mut Context,
    _lpApplicationName: Ptr<u8>,
    _lpCommandLine: Ptr<u8>,
    _lpProcessAttributes: Ptr<()>,
    _lpThreadAttributes: Ptr<()>,
    _bInheritHandles: bool,
    _dwCreationFlags: u32,
    _lpEnvironment: Ptr<()>,
    _lpCurrentDirectory: Ptr<u8>,
    _lpStartupInfo: Ptr<STARTUPINFOA>,
    _lpProcessInformation: Ptr<PROCESS_INFORMATION>,
) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn GetStartupInfoA(ctx: &mut Context, lpStartupInfo: Ptr<STARTUPINFOA>) {
    let Some(size) = crate::Ptr::<u32>::new(lpStartupInfo.addr).read(&ctx.memory) else {
        return;
    };
    if size > 0 && size < std::mem::size_of::<STARTUPINFOA>() as u32 {
        log::error!("GetStartupInfoA: undersized buffer");
        return;
    }

    let info = STARTUPINFOA {
        ..Default::default()
    };
    let _ = lpStartupInfo.write(&mut ctx.memory, info);
}

#[win32_derive::dllexport]
pub fn GetVersion(_ctx: &mut Context) -> u32 {
    // Win95, version 4.0.
    (1 << 31) | 0x4
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct MEMORYSTATUS {
    dwLength: u32,
    dwMemoryLoad: u32,
    dwTotalPhys: u32,
    dwAvailPhys: u32,
    dwTotalPageFile: u32,
    dwAvailPageFile: u32,
    dwTotalVirtual: u32,
    dwAvailVirtual: u32,
}

#[win32_derive::dllexport]
pub fn GlobalMemoryStatus(ctx: &mut Context, lpBuffer: Ptr<MEMORYSTATUS>) {
    let capacity = ctx.memory.bytes.len() as u32;
    let status = MEMORYSTATUS {
        dwLength: std::mem::size_of::<MEMORYSTATUS>() as u32,
        dwTotalPhys: capacity,
        dwAvailPhys: capacity,
        dwTotalPageFile: capacity,
        dwAvailPageFile: capacity,
        dwTotalVirtual: capacity,
        dwAvailVirtual: capacity,
        ..Default::default()
    };
    let _ = lpBuffer.write(&mut ctx.memory, status);
}

#[win32_derive::dllexport]
pub fn GetDiskFreeSpaceA(
    ctx: &mut Context,
    _lpRootPathName: Ptr<u8>,
    lpSectorsPerCluster: Ptr<u32>,
    lpBytesPerSector: Ptr<u32>,
    lpNumberOfFreeClusters: Ptr<u32>,
    lpTotalNumberOfClusters: Ptr<u32>,
) -> bool {
    if [
        lpSectorsPerCluster.addr,
        lpBytesPerSector.addr,
        lpNumberOfFreeClusters.addr,
        lpTotalNumberOfClusters.addr,
    ]
    .into_iter()
    .any(|addr| addr < 0x1000)
    {
        return false;
    }

    lpSectorsPerCluster.write(&mut ctx.memory, 1).is_some()
        && lpBytesPerSector.write(&mut ctx.memory, 512).is_some()
        && lpNumberOfFreeClusters
            .write(&mut ctx.memory, 0x1_0000)
            .is_some()
        && lpTotalNumberOfClusters
            .write(&mut ctx.memory, 0x2_0000)
            .is_some()
}

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct OSVERSIONINFO {
    dwOSVersionInfoSize: u32,
    dwMajorVersion: u32,
    dwMinorVersion: u32,
    dwBuildNumber: u32,
    dwPlatformId: u32,
    //szCSDVersion: [u8; 128],
}

#[win32_derive::dllexport]
pub fn GetVersionExA(ctx: &mut Context, lpVersionInformation: Ptr<OSVERSIONINFO>) -> bool {
    let Some(size) = Ptr::<u32>::new(lpVersionInformation.addr).read(&ctx.memory) else {
        return false;
    };
    if size < std::mem::size_of::<OSVERSIONINFO>() as u32 {
        log::error!("GetVersionExA undersized buffer");
        return false;
    }

    let info = OSVERSIONINFO {
        dwMajorVersion: 6, // ? pulled from debugger
        dwPlatformId: 2,   /* VER_PLATFORM_WIN32_NT */
        ..Default::default()
    };
    lpVersionInformation.write(&mut ctx.memory, info).is_some()
}

#[win32_derive::dllexport]
pub fn UnhandledExceptionFilter(_ctx: &mut Context, _ExceptionInfo: Ptr<()>) -> i32 {
    // "The process is being debugged, so the exception should be passed (as second chance) to the application's debugger."
    0 // EXCEPTION_CONTINUE_SEARCH
}

#[win32_derive::dllexport]
pub fn SetUnhandledExceptionFilter(_ctx: &mut Context, lpTopLevelExceptionFilter: Ptr<()>) -> u32 {
    let mut state = lock();
    let previous = state.unhandled_exception_filter;
    state.unhandled_exception_filter = if lpTopLevelExceptionFilter.addr >= 0x1000 {
        lpTopLevelExceptionFilter.addr
    } else {
        0
    };
    previous
}

#[win32_derive::dllexport]
pub fn SetConsoleCtrlHandler(_ctx: &mut Context, HandlerRoutine: Ptr<()>, Add: bool) -> bool {
    let mut state = lock();
    if Add {
        if HandlerRoutine.addr >= 0x1000
            && !state.console_ctrl_handlers.contains(&HandlerRoutine.addr)
        {
            state.console_ctrl_handlers.push(HandlerRoutine.addr);
        }
    } else if HandlerRoutine.addr == 0 {
        state.console_ctrl_handlers.clear();
    } else if HandlerRoutine.addr >= 0x1000 {
        state
            .console_ctrl_handlers
            .retain(|handler| *handler != HandlerRoutine.addr);
    }
    true
}

#[win32_derive::dllexport]
pub fn DebugBreak(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn RaiseException(
    _ctx: &mut Context,
    code: u32,
    flags: u32,
    argument_count: u32,
    _arguments: Ptr<u32>,
) {
    log::debug!("RaiseException({code:#x}, {flags:#x}, {argument_count})");
}

#[win32_derive::dllexport]
pub fn IsBadReadPtr(ctx: &mut Context, lp: Ptr<()>, ucb: u32) -> bool {
    let start = lp.addr as usize;
    start < 0x1000
        || start
            .checked_add(ucb as usize)
            .is_none_or(|end| end > ctx.memory.bytes.len())
}

#[win32_derive::dllexport]
pub fn IsBadWritePtr(ctx: &mut Context, lp: Ptr<()>, ucb: u32) -> bool {
    IsBadReadPtr(ctx, lp, ucb)
}

#[win32_derive::dllexport]
pub fn IsBadCodePtr(ctx: &mut Context, lp: Ptr<()>) -> bool {
    IsBadReadPtr(ctx, lp, 1)
}

#[win32_derive::dllexport]
pub fn VirtualAlloc(
    ctx: &mut Context,
    lpAddress: Ptr<()>,
    dwSize: u32,
    _flAllocationType: u32, /* VIRTUAL_ALLOCATION_TYPE */
    _flProtect: u32,        /* PAGE_PROTECTION_FLAGS */
) -> u32 {
    if lpAddress.addr != 0 {
        // Committing (or re-protecting) part of an earlier reservation; all our
        // memory is always committed, so just say yes.
        return lpAddress.addr;
    }
    // A reservation has to fit inside emulated memory; failing with NULL
    // beats handing out an address no access can reach.
    let limit = u32::try_from(ctx.memory.bytes.len()).unwrap_or(u32::MAX);
    lock()
        .mappings
        .try_alloc("VirtualAlloc".into(), dwSize, limit)
        .unwrap_or(0)
}

#[win32_derive::dllexport]
pub fn VirtualFree(
    _ctx: &mut Context,
    lpAddress: Ptr<()>,
    dwSize: u32,
    dwFreeType: u32, /* VIRTUAL_FREE_TYPE */
) -> bool {
    const MEM_DECOMMIT: u32 = 0x4000;
    const MEM_RELEASE: u32 = 0x8000;
    match dwFreeType {
        // All emulated memory is always committed; decommit is a no-op.
        MEM_DECOMMIT => true,
        // MEM_RELEASE releases the whole region and requires dwSize == 0.
        MEM_RELEASE => dwSize == 0 && lpAddress.addr != 0 && lock().mappings.free(lpAddress.addr),
        _ => false,
    }
}

#[win32_derive::dllexport]
pub fn OutputDebugStringA(ctx: &mut Context, lpOutputString: Ptr<u8>) {
    if lpOutputString.addr >= 0x1000 {
        log::debug!(
            "OutputDebugStringA: {}",
            ctx.memory.read_str(lpOutputString.addr)
        );
    }
}

#[win32_derive::dllexport]
pub fn RtlUnwind(
    _ctx: &mut Context,
    TargetFrame: Ptr<()>,
    TargetIp: Ptr<()>,
    ExceptionRecord: Ptr<()>,
    ReturnValue: Ptr<()>,
) {
    // No SEH dispatcher exists; match the RaiseException policy of logging the
    // request and continuing rather than fabricating an unwind.
    log::debug!(
        "RtlUnwind(frame={:#x}, ip={:#x}, record={:#x}, retval={:#x})",
        TargetFrame.addr,
        TargetIp.addr,
        ExceptionRecord.addr,
        ReturnValue.addr
    );
}

#[win32_derive::dllexport]
pub fn lstrcpyW(ctx: &mut Context, lpString1: Ptr<u16>, lpString2: Ptr<u16>) -> u32 /* WSTR */ {
    let Some(buf) = ctx.memory.bytes.get(lpString2.addr as usize..) else {
        return 0;
    };
    let Some(len) = buf.chunks_exact(2).position(|c| c == [0, 0]) else {
        log::error!("lstrcpyW: unterminated source string");
        return 0;
    };
    let src = lpString2.addr as usize;
    let dst = lpString1.addr as usize;
    let Some(bytes) = len.checked_add(1).and_then(|len| len.checked_mul(2)) else {
        return 0;
    };
    let Some(src_end) = src.checked_add(bytes) else {
        return 0;
    };
    let Some(dst_end) = dst.checked_add(bytes) else {
        return 0;
    };
    if src_end > ctx.memory.bytes.len() || dst_end > ctx.memory.bytes.len() {
        return 0;
    }
    ctx.memory.bytes.copy_within(src..src_end, dst);
    lpString1.addr
}

#[win32_derive::dllexport]
pub fn lstrlenW(ctx: &mut Context, lpString: Ptr<u16>) -> i32 {
    let Some(buf) = ctx.memory.bytes.get(lpString.addr as usize..) else {
        return 0;
    };
    let Some(len) = buf.chunks_exact(2).position(|c| c == [0, 0]) else {
        log::error!("lstrlenW: unterminated string");
        return 0;
    };
    len as i32
}

#[cfg(test)]
mod tests {
    use super::{
        GetComputerNameA, GetPrivateProfileStringW, GetSystemInfo, SYSTEM_INFO,
        SetConsoleCtrlHandler, SetUnhandledExceptionFilter, lstrcpyW, lstrlenW,
        processor_feature_present,
    };
    use crate::Ptr;
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
    fn system_info_matches_win32_abi() {
        assert_eq!(std::mem::size_of::<SYSTEM_INFO>(), 36);
    }

    #[test]
    fn system_info_rejects_bad_output_pointers() {
        let mut ctx = context();
        // Low and far out-of-range output pointers are rejected without panic.
        GetSystemInfo(&mut ctx, Ptr::new(0x500));
        GetSystemInfo(&mut ctx, Ptr::new(0xffff_fff0));
        // A valid pointer writes the page size at offset 4.
        GetSystemInfo(&mut ctx, Ptr::new(0x1000));
        assert_eq!(ctx.memory.read::<u32>(0x1000 + 4), 0x1000);
    }

    #[test]
    fn processor_feature_model_matches_emulated_cpu() {
        assert!(processor_feature_present(3));
        assert!(processor_feature_present(8));
        assert!(!processor_feature_present(6));
        assert!(!processor_feature_present(u32::MAX));
    }

    #[test]
    fn computer_name_rejects_truncated_and_bad_pointers() {
        let mut ctx = context();
        ctx.memory.write::<u32>(0x1000, 8);

        // Buffer too small for "THESEUS\0" (needs 8 bytes).
        assert!(!GetComputerNameA(
            &mut ctx,
            Ptr::new(0x3fff),
            Ptr::new(0x1000),
        ));

        // A non-null but out-of-range size pointer fails.
        assert!(!GetComputerNameA(
            &mut ctx,
            Ptr::new(0x2000),
            Ptr::new(0xffff_ff00),
        ));

        // A non-null size pointer in the null page fails without a log.
        assert!(!GetComputerNameA(
            &mut ctx,
            Ptr::new(0x2000),
            Ptr::new(0x500)
        ));

        // Success path writes the name, null terminator, and length.
        assert!(GetComputerNameA(
            &mut ctx,
            Ptr::new(0x2000),
            Ptr::new(0x1000)
        ));
        assert_eq!(&ctx.memory.bytes[0x2000..0x2008], b"THESEUS\0");
        assert_eq!(ctx.memory.read::<u32>(0x1000), 7);
    }

    #[test]
    fn wide_string_copy_preserves_all_utf16_bytes() {
        let mut ctx = context();
        ctx.memory.write::<u16>(0x1000, b'A' as u16);
        ctx.memory.write::<u16>(0x1002, 0x03b2);
        ctx.memory.write::<u16>(0x1004, 0);

        assert_eq!(lstrlenW(&mut ctx, Ptr::new(0x1000)), 2);
        assert_eq!(
            lstrcpyW(&mut ctx, Ptr::new(0x1200), Ptr::new(0x1000)),
            0x1200
        );
        assert_eq!(ctx.memory.read::<u16>(0x1200), b'A' as u16);
        assert_eq!(ctx.memory.read::<u16>(0x1202), 0x03b2);
        assert_eq!(ctx.memory.read::<u16>(0x1204), 0);
    }

    #[test]
    fn wide_string_copy_rejects_truncated_destination() {
        let mut ctx = context();
        ctx.memory.write::<u16>(0x1000, b'A' as u16);
        ctx.memory.write::<u16>(0x1002, 0);

        assert_eq!(lstrcpyW(&mut ctx, Ptr::new(0x3fff), Ptr::new(0x1000)), 0);
    }

    #[test]
    fn wide_string_functions_report_missing_terminators() {
        let mut ctx = context();
        ctx.memory[0x3ff0..].fill(0xff);

        assert_eq!(lstrlenW(&mut ctx, Ptr::new(0x3ff0)), 0);
        assert_eq!(lstrcpyW(&mut ctx, Ptr::new(0x1200), Ptr::new(0x3ff0)), 0);
    }

    fn wstr(ctx: &mut Context, addr: u32, s: &str) {
        for (i, c) in s.encode_utf16().chain(std::iter::once(0)).enumerate() {
            ctx.memory.write::<u16>(addr + i as u32 * 2, c);
        }
    }

    #[test]
    fn private_profile_string_copies_default_and_reports_length() {
        let mut ctx = context();
        wstr(&mut ctx, 0x1000, "App");
        wstr(&mut ctx, 0x1100, "Key");
        wstr(&mut ctx, 0x1200, "fallback");
        wstr(&mut ctx, 0x1300, "no\\such\\file.ini");

        // Missing file: the default is copied and its length returned.
        let got = GetPrivateProfileStringW(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x1100),
            Ptr::new(0x1200),
            Ptr::new(0x2000),
            32,
            Ptr::new(0x1300),
        );
        assert_eq!(got, 8);
        assert_eq!(&ctx.memory[0x2000..0x2012], b"f\0a\0l\0l\0b\0a\0c\0k\0\0\0");

        // A too-small buffer truncates to nSize - 1 and stays NUL-terminated.
        ctx.memory[0x2000..0x2010].fill(0xff);
        let got = GetPrivateProfileStringW(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x1100),
            Ptr::new(0x1200),
            Ptr::new(0x2000),
            4,
            Ptr::new(0x1300),
        );
        assert_eq!(got, 3);
        assert_eq!(&ctx.memory[0x2000..0x2008], b"f\0a\0l\0\0\0");
    }

    #[test]
    fn low_pointer_exception_and_console_handlers_are_ignored() {
        crate::kernel32::ensure_test_state();
        let mut ctx = context();
        assert_eq!(SetUnhandledExceptionFilter(&mut ctx, Ptr::new(0x2000)), 0);
        assert_eq!(
            SetUnhandledExceptionFilter(&mut ctx, Ptr::new(0x500)),
            0x2000
        );
        assert_eq!(crate::kernel32::lock().unhandled_exception_filter, 0);
        SetUnhandledExceptionFilter(&mut ctx, Ptr::new(0x3000));
        assert_eq!(crate::kernel32::lock().unhandled_exception_filter, 0x3000);

        assert!(SetConsoleCtrlHandler(&mut ctx, Ptr::new(0x4000), true));
        assert!(SetConsoleCtrlHandler(&mut ctx, Ptr::new(0x500), true));
        assert!(SetConsoleCtrlHandler(&mut ctx, Ptr::new(0x4000), false));
        assert!(SetConsoleCtrlHandler(&mut ctx, Ptr::new(0x500), false));
        assert!(crate::kernel32::lock().console_ctrl_handlers.is_empty());
    }
}

/// Read a NUL-terminated UTF-16 string from guest memory.
fn read_wstr(ctx: &Context, addr: u32) -> String {
    if addr < 0x1000 {
        return String::new();
    }
    let mut buf = Vec::new();
    let mut ofs = addr;
    while (ofs as usize) < ctx.memory.bytes.len() {
        let c = ctx.memory.read::<u16>(ofs);
        if c == 0 {
            break;
        }
        buf.push(c);
        ofs += 2;
    }
    String::from_utf16_lossy(&buf)
}

/// Parse a .ini file into (section, key, value) rows. Keys and section names
/// compare case-insensitively per the profile API.
fn read_ini(path: &str) -> Option<Vec<(String, String, String)>> {
    let bytes = host::fs::read(crate::kernel32::resolve_path(path)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut rows = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = name.trim().to_owned();
        } else if !section.is_empty()
            && !line.starts_with(';')
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
        {
            rows.push((
                section.clone(),
                key.trim().to_owned(),
                value.trim().to_owned(),
            ));
        }
    }
    Some(rows)
}

/// Write a UTF-16 string into a caller buffer with the profile-API contract:
/// truncated to `nSize - 1`, NUL-terminated, returns the count excluding NUL.
fn write_wstr(ctx: &mut Context, addr: u32, value: &str, nSize: u32) -> u32 {
    if nSize == 0 || addr < 0x1000 {
        return 0;
    }
    let units: Vec<u16> = value.encode_utf16().collect();
    let copy = (nSize as usize - 1).min(units.len());
    let Some(end) = (addr as usize).checked_add((copy + 1) * 2) else {
        return 0;
    };
    if end > ctx.memory.bytes.len() {
        return 0;
    }
    for (i, c) in units[..copy].iter().enumerate() {
        ctx.memory.write::<u16>(addr + i as u32 * 2, *c);
    }
    ctx.memory.write::<u16>(addr + copy as u32 * 2, 0);
    copy as u32
}

/// Write a NUL-separated multi-string, returning chars copied excluding the
/// final extra NUL (nSize - 2 on overflow, per the API contract).
fn write_wstr_multi(ctx: &mut Context, addr: u32, entries: &[String], nSize: u32) -> u32 {
    if nSize < 2 || addr == 0 {
        return 0;
    }
    let mut out: Vec<u16> = Vec::new();
    for entry in entries {
        out.extend(entry.encode_utf16());
        out.push(0);
    }
    out.push(0);
    // Both branches below write at most this many u16 units at `addr`.
    let units = out.len().min(nSize as usize);
    let Some(end) = (addr as usize).checked_add(units * 2) else {
        return 0;
    };
    if addr < 0x1000 || end > ctx.memory.bytes.len() {
        return 0;
    }
    if out.len() <= nSize as usize {
        for (i, c) in out.iter().enumerate() {
            ctx.memory.write::<u16>(addr + i as u32 * 2, *c);
        }
        return (out.len() - 1) as u32;
    }
    let copy = nSize as usize - 2;
    for (i, c) in out[..copy].iter().enumerate() {
        ctx.memory.write::<u16>(addr + i as u32 * 2, *c);
    }
    ctx.memory.write::<u16>(addr + copy as u32 * 2, 0);
    ctx.memory.write::<u16>(addr + (copy + 1) as u32 * 2, 0);
    copy as u32
}

/// atoi-style leading-integer parse, matching the profile-API coercion.
fn atoi(s: &str) -> Option<i32> {
    let s = s.trim_start();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|v| (v * sign) as i32)
}

#[win32_derive::dllexport]
pub fn GetPrivateProfileIntW(
    ctx: &mut Context,
    lpAppName: Ptr<u16>,
    lpKeyName: Ptr<u16>,
    nDefault: i32,
    lpFileName: Ptr<u16>,
) -> i32 {
    let section = read_wstr(ctx, lpAppName.addr);
    let key = read_wstr(ctx, lpKeyName.addr);
    let path = read_wstr(ctx, lpFileName.addr);
    read_ini(&path)
        .and_then(|rows| {
            rows.iter().find_map(|(s, k, v)| {
                (s.eq_ignore_ascii_case(&section) && k.eq_ignore_ascii_case(&key))
                    .then(|| v.clone())
            })
        })
        .and_then(|v| atoi(&v))
        .unwrap_or(nDefault)
}

#[win32_derive::dllexport]
pub fn GetPrivateProfileStringW(
    ctx: &mut Context,
    lpAppName: Ptr<u16>,
    lpKeyName: Ptr<u16>,
    lpDefault: Ptr<u16>,
    lpReturnedString: Ptr<u16>,
    nSize: u32,
    lpFileName: Ptr<u16>,
) -> u32 {
    let section = read_wstr(ctx, lpAppName.addr);
    let key = read_wstr(ctx, lpKeyName.addr);
    let path = read_wstr(ctx, lpFileName.addr);

    // A null app or key name enumerates section or key names as a
    // multi-string instead of a single value.
    if lpAppName.addr == 0 || lpKeyName.addr == 0 {
        let rows = read_ini(&path).unwrap_or_default();
        let entries: Vec<String> = if lpAppName.addr == 0 {
            let mut seen: Vec<String> = Vec::new();
            for (s, _, _) in &rows {
                if !seen.iter().any(|e| e.eq_ignore_ascii_case(s)) {
                    seen.push(s.clone());
                }
            }
            seen
        } else {
            rows.iter()
                .filter(|(s, _, _)| s.eq_ignore_ascii_case(&section))
                .map(|(_, k, _)| k.clone())
                .collect()
        };
        return write_wstr_multi(ctx, lpReturnedString.addr, &entries, nSize);
    }

    let value = read_ini(&path).and_then(|rows| {
        rows.iter().find_map(|(s, k, v)| {
            (s.eq_ignore_ascii_case(&section) && k.eq_ignore_ascii_case(&key)).then(|| v.clone())
        })
    });
    let value = value.unwrap_or_else(|| {
        if lpDefault.addr != 0 {
            read_wstr(ctx, lpDefault.addr)
        } else {
            String::new()
        }
    });
    write_wstr(ctx, lpReturnedString.addr, &value, nSize)
}
