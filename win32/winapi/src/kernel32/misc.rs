use runtime::Context;

use crate::{
    Ptr,
    kernel32::{lock, teb, teb_mut},
    stub,
};

const ERROR_FILE_NOT_FOUND: u32 = 2;

#[win32_derive::dllexport]
pub fn GetLastError(ctx: &mut Context) -> u32 {
    teb(ctx).LastErrorValue
}

#[win32_derive::dllexport]
pub fn SetLastError(ctx: &mut Context, error: u32) {
    teb_mut(ctx).LastErrorValue = error;
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
    lpSystemInfo.write(&mut ctx.memory, info).unwrap();
}

#[win32_derive::dllexport]
pub fn GetComputerNameA(ctx: &mut Context, lpBuffer: Ptr<u8>, nSize: Ptr<u32>) -> bool {
    let name = b"THESEUS";
    let size = nSize.read(&ctx.memory).unwrap_or(0);
    if (size as usize) < name.len() + 1 {
        return false;
    }
    ctx.memory[lpBuffer.addr..][..name.len()].copy_from_slice(name);
    ctx.memory.write::<u8>(lpBuffer.addr + name.len() as u32, 0);
    nSize.write(&mut ctx.memory, name.len() as u32);
    true
}

#[win32_derive::dllexport]
pub fn SetEnvironmentVariableA(_ctx: &mut Context, _lpName: Ptr<u8>, _lpValue: Ptr<u8>) -> bool {
    stub!(true)
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
    let size = ctx.memory.read::<u32>(lpStartupInfo.addr);
    if size > 0 && size < std::mem::size_of::<STARTUPINFOA>() as u32 {
        log::error!("GetStartupInfoA: undersized buffer");
        return;
    }

    let info = STARTUPINFOA {
        ..Default::default()
    };
    lpStartupInfo.write(&mut ctx.memory, info).unwrap();
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
    lpBuffer.write(&mut ctx.memory, status).unwrap();
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

    lpSectorsPerCluster.write(&mut ctx.memory, 1).unwrap();
    lpBytesPerSector.write(&mut ctx.memory, 512).unwrap();
    lpNumberOfFreeClusters
        .write(&mut ctx.memory, 0x1_0000)
        .unwrap();
    lpTotalNumberOfClusters
        .write(&mut ctx.memory, 0x2_0000)
        .unwrap();
    true
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
    let size = ctx.memory.read::<u32>(lpVersionInformation.addr);
    if size < std::mem::size_of::<OSVERSIONINFO>() as u32 {
        log::error!("GetVersionExA undersized buffer");
        return false;
    }

    let info = OSVERSIONINFO {
        dwMajorVersion: 6, // ? pulled from debugger
        dwPlatformId: 2,   /* VER_PLATFORM_WIN32_NT */
        ..Default::default()
    };
    lpVersionInformation.write(&mut ctx.memory, info).unwrap();

    true
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
    state.unhandled_exception_filter = lpTopLevelExceptionFilter.addr;
    previous
}

#[win32_derive::dllexport]
pub fn SetConsoleCtrlHandler(_ctx: &mut Context, HandlerRoutine: Ptr<()>, Add: bool) -> bool {
    let mut state = lock();
    if Add {
        if HandlerRoutine.addr != 0 && !state.console_ctrl_handlers.contains(&HandlerRoutine.addr) {
            state.console_ctrl_handlers.push(HandlerRoutine.addr);
        }
    } else if HandlerRoutine.addr == 0 {
        state.console_ctrl_handlers.clear();
    } else {
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
            .map_or(true, |end| end > ctx.memory.bytes.len())
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
    _ctx: &mut Context,
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
    lock().mappings.alloc("VirtualAlloc".into(), dwSize)
    /*
    let memory = sys.memory_mut();
    if lpAddress != 0 {
        // Changing flags on an existing address, hopefully.
        match memory
            .mappings
            .vec()
            .iter()
            .find(|&mapping| mapping.contains(lpAddress))
        {
            None => {
                log::error!("failing VirtualAlloc({lpAddress:x}, ...) refers to unknown mapping");
                return 0;
            }
            Some(_) => {
                // adjusting flags on existing mapping, ignore.
                return lpAddress;
            }
        }
    }
    // TODO round dwSize to page boundary

    let mapping = memory
        .mappings
        .alloc(memory.imp.mem(), dwSize, "VirtualAlloc".into());
    mapping.addr
    */
}

#[win32_derive::dllexport]
pub fn VirtualFree(
    _ctx: &mut Context,
    _lpAddress: Ptr<()>,
    _dwSize: u32,
    _dwFreeType: u32, /* VIRTUAL_FREE_TYPE */
) -> bool {
    true // success
}

#[win32_derive::dllexport]
pub fn OutputDebugStringA(_ctx: &mut Context, _lpOutputString: Ptr<u8>) {
    todo!()
}

#[win32_derive::dllexport]
pub fn RtlUnwind(
    _ctx: &mut Context,
    _TargetFrame: Ptr<()>,
    _TargetIp: Ptr<()>,
    _ExceptionRecord: Ptr<()>,
    _ReturnValue: Ptr<()>,
) {
    todo!()
}

#[win32_derive::dllexport]
pub fn lstrcpyW(ctx: &mut Context, lpString1: Ptr<u16>, lpString2: Ptr<u16>) -> u32 /* WSTR */ {
    let buf = &ctx.memory[lpString2.addr..];
    let len = buf.chunks_exact(2).position(|c| c == &[0, 0]).unwrap();
    let src = lpString2.addr as usize;
    let dst = lpString1.addr as usize;
    ctx.memory
        .bytes
        .copy_within(src..src + len + 2, dst as usize);
    lpString1.addr
}

#[win32_derive::dllexport]
pub fn lstrlenW(ctx: &mut Context, lpString: Ptr<u16>) -> i32 {
    let buf = &ctx.memory[lpString.addr..];
    buf.chunks_exact(2).position(|c| c == &[0, 0]).unwrap() as i32
}

#[cfg(test)]
mod tests {
    use super::SYSTEM_INFO;

    #[test]
    fn system_info_matches_win32_abi() {
        assert_eq!(std::mem::size_of::<SYSTEM_INFO>(), 36);
    }
}

#[win32_derive::dllexport]
pub fn GetPrivateProfileIntW(
    _ctx: &mut Context,
    _lpAppName: Ptr<u16>,
    _lpKeyName: Ptr<u16>,
    nDefault: i32,
    _lpFileName: Ptr<u16>,
) -> i32 {
    stub!(nDefault)
}

#[win32_derive::dllexport]
pub fn GetPrivateProfileStringW(
    _ctx: &mut Context,
    _lpAppName: Ptr<u16>,
    _lpKeyName: Ptr<u16>,
    _lpDefault: Ptr<u16>,
    _lpReturnedString: Ptr<u16>,
    _nSize: u32,
    _lpFileName: Ptr<u16>,
) -> u32 {
    stub!(ERROR_FILE_NOT_FOUND)
}
