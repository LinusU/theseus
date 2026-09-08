use runtime::Context;

use crate::{Ptr, kernel32::lock, stub};

const ERROR_FILE_NOT_FOUND: u32 = 2;

#[win32_derive::dllexport]
pub fn GetLastError(_ctx: &mut Context) -> u32 {
    0
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

#[win32_derive::dllexport]
pub fn Beep(_ctx: &mut Context, _dwFreq: u32, _dwDuration: u32) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn GetProcessVersion(_ctx: &mut Context, _ProcessId: u32) -> u32 {
    // Major version in the high word, as for a Windows 4.0 (95) executable.
    0x0004_0000
}

#[win32_derive::dllexport]
pub fn SetErrorMode(_ctx: &mut Context, _uMode: u32) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn SetLastError(_ctx: &mut Context, _dwErrCode: u32) {
    // GetLastError always reports success; nothing keeps the value.
}

#[win32_derive::dllexport]
pub fn IsBadReadPtr(_ctx: &mut Context, _lp: u32, _ucb: u32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn IsBadWritePtr(_ctx: &mut Context, _lp: u32, _ucb: u32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn IsBadCodePtr(_ctx: &mut Context, _lpfn: u32) -> bool {
    false
}

#[win32_derive::dllexport]
pub fn SetUnhandledExceptionFilter(_ctx: &mut Context, _lpTopLevelExceptionFilter: u32) -> u32 {
    0 // no previous filter
}

#[win32_derive::dllexport]
pub fn SetConsoleCtrlHandler(_ctx: &mut Context, _HandlerRoutine: u32, _Add: bool) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn MulDiv(_ctx: &mut Context, nNumber: i32, nNumerator: i32, nDenominator: i32) -> i32 {
    if nDenominator == 0 {
        return -1;
    }
    let product = nNumber as i64 * nNumerator as i64;
    // Rounds half away from zero.
    let half = nDenominator.unsigned_abs() as i64 / 2;
    let rounded = if (product < 0) != (nDenominator < 0) {
        product - half
    } else {
        product + half
    };
    (rounded / nDenominator as i64) as i32
}

#[repr(C)]
#[derive(zerocopy::IntoBytes, zerocopy::Immutable)]
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
    let phys = 128 << 20;
    lpBuffer.write(
        &mut ctx.memory,
        MEMORYSTATUS {
            dwLength: std::mem::size_of::<MEMORYSTATUS>() as u32,
            dwMemoryLoad: 25,
            dwTotalPhys: phys,
            dwAvailPhys: phys * 3 / 4,
            dwTotalPageFile: phys * 2,
            dwAvailPageFile: phys * 3 / 2,
            dwTotalVirtual: 0x7fff_0000,
            dwAvailVirtual: 0x7000_0000,
        },
    );
}

#[win32_derive::dllexport]
pub fn GlobalDeleteAtom(_ctx: &mut Context, _nAtom: u16) -> u16 {
    0
}

#[win32_derive::dllexport]
pub fn DuplicateHandle(
    ctx: &mut Context,
    _hSourceProcessHandle: u32,
    hSourceHandle: u32,
    _hTargetProcessHandle: u32,
    lpTargetHandle: Ptr<u32>,
    _dwDesiredAccess: u32,
    _bInheritHandle: bool,
    _dwOptions: u32,
) -> bool {
    // Handles aren't reference counted, so the duplicate is the original; a
    // CloseHandle on either closes both.
    stub!(());
    lpTargetHandle.write(&mut ctx.memory, hSourceHandle);
    true
}

#[win32_derive::dllexport]
pub fn VirtualProtect(
    ctx: &mut Context,
    _lpAddress: u32,
    _dwSize: u32,
    _flNewProtect: u32,
    lpflOldProtect: Ptr<u32>,
) -> bool {
    const PAGE_READWRITE: u32 = 0x04;
    if lpflOldProtect.addr != 0 {
        lpflOldProtect.write(&mut ctx.memory, PAGE_READWRITE);
    }
    true
}
