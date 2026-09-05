use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, Sender, TryRecvError},
    },
};

use runtime::{Cont, Context};
use zerocopy::FromBytes;

use crate::{dllexport::win32flags, kernel32, winmm::state};

const MMSYSERR_NOERROR: u32 = 0;
const MMSYSERR_INVALHANDLE: u32 = 5;
const WAVERR_STILLPLAYING: u32 = 33;

/// What the waveOut API surface hands to the feeder thread.
enum WaveMsg {
    /// waveOutWrite submitted a prepared WAVEHDR address.
    Block(u32),
    /// waveOutReset: stop playback and return every pending block as done.
    Reset,
}

pub struct State {
    sender: Sender<WaveMsg>,
    /// WAVEHDRs written but not yet returned as done; lets waveOutClose
    /// report WAVERR_STILLPLAYING.
    pending: Arc<AtomicU32>,
}

#[win32_derive::dllexport]
pub fn waveOutGetNumDevs(_ctx: &mut Context) -> u32 {
    1
}

#[repr(C)]
#[derive(Debug, zerocopy::Immutable, zerocopy::IntoBytes)]
pub struct WAVEOUTCAPS {
    pub wMid: u16,
    pub wPid: u16,
    pub vDriverVersion: u32,
    // TODO: TCHAR, could this be unicode based on cbwoc param?
    pub szPname: [u8; 32],
    pub dwFormats: u32,
    pub wChannels: u16,
    pub wReserved1: u16,
    pub dwSupport: u32,
}

enum WAVE_FORMAT {
    _4M16 = 0x0000_0400,
}

#[win32_derive::dllexport]
pub fn waveOutGetDevCapsA(ctx: &mut Context, _uDeviceID: u32, pwoc: u32, cbwoc: u32) -> u32 {
    assert_eq!(cbwoc, std::mem::size_of::<WAVEOUTCAPS>() as u32);

    ctx.memory.write(
        pwoc,
        WAVEOUTCAPS {
            wMid: 0,
            wPid: 0,
            vDriverVersion: 1,
            szPname: [0; 32],
            dwFormats: WAVE_FORMAT::_4M16 as u32,
            wChannels: 1, // mono
            wReserved1: 0,
            dwSupport: 0, // no features
        },
    );
    MMSYSERR_NOERROR
}

#[repr(C)]
#[derive(Debug, zerocopy::FromBytes)]
pub struct WAVEFORMATEX {
    pub wFormatTag: u16,
    pub nChannels: u16,
    pub nSamplesPerSec: u32,
    pub nAvgBytesPerSec: u32,
    pub nBlockAlign: u16,
    pub wBitsPerSample: u16,
    pub cbSize: u16,
}

/// The types of callbacks that can be used with waveOutOpen.
#[derive(Debug, PartialEq, Eq, win32_derive::ABIEnum)]
pub enum CALLBACK {
    NULL = 0x00000000,
    WINDOW = 0x00010000,
    TASK = 0x00020000,
    FUNCTION = 0x00030000,
    EVENT = 0x00050000,
}

enum MM_WOM {
    // OPEN = 0x3BB,
    // CLOSE = 0x3BC,
    DONE = 0x3BD,
}

struct QueuedBlock {
    addr: u32,
    len: u32,
}

/// waveOutReset: stop playback and return every pending block — those fed to
/// the stream, stashed behind a full queue, or still in the channel — marked
/// WHDR_DONE with a WOM_DONE callback each.
fn reset(
    ctx: &mut Context,
    stream: &host::AudioStream,
    receiver: &Receiver<WaveMsg>,
    incoming: &mut VecDeque<u32>,
    queued_blocks: &mut VecDeque<QueuedBlock>,
    total_pending: &mut u32,
    callback: Option<Cont>,
    callback_data: u32,
    pending: &AtomicU32,
) {
    stream.clear();
    *total_pending = 0;
    let mut done: Vec<u32> = queued_blocks.drain(..).map(|block| block.addr).collect();
    done.extend(incoming.drain(..));
    while let Ok(WaveMsg::Block(addr)) = receiver.try_recv() {
        done.push(addr);
    }
    for addr in done {
        let header = <WAVEHDR>::mut_from_prefix(&mut ctx.memory[addr..])
            .unwrap()
            .0;
        header.dwFlags.insert(WHDR::DONE);
        if let Some(f) = callback {
            ctx.call32_x86(f, vec![1, MM_WOM::DONE as u32, callback_data, addr, 0]);
        }
    }
    pending.store(0, Ordering::SeqCst);
}

/// Thread procedure to pass data from wave APIs to SDL.
/// We use a win32 thread here because the callbacks to the executable
/// that indicate more data is needed come from a thread.
///
/// Strategy: waveOutWrite pushes the pointer to the buffer to the queue,
/// and we pull it out here and pass it to SDL.
fn thread_proc(
    ctx: &mut Context,
    stream: host::AudioStream,
    receiver: Receiver<WaveMsg>,
    callback: u32,
    callback_data: u32,
    pending: Arc<AtomicU32>,
) {
    // We need to notify when a queued block is done, but SDL doesn't make that easy.
    // We keep track of how much data we have passed to SDL, then compare it against
    // how much data SDL says is yet to be processed, and use the difference to decide
    // whether a given block has been fully processed.
    // total_pending is the total number of bytes that have been submitted to SDL,
    // and is the sum of the lengths of all queued blocks.
    let mut total_pending = 0u32;
    let mut queued_blocks: VecDeque<QueuedBlock> = VecDeque::new();
    // Block submissions pulled while the SDL queue was full; they are fed to
    // the stream before anything still in the channel, preserving order.
    let mut incoming: VecDeque<u32> = VecDeque::new();
    let callback = (callback != 0).then(|| ctx.indirect(callback));

    loop {
        while stream.queued_bytes() < 8 << 10 {
            let msg = if let Some(addr) = incoming.pop_front() {
                WaveMsg::Block(addr)
            } else {
                match receiver.recv() {
                    Ok(msg) => msg,
                    // waveOutClose dropped the channel: shut the stream down.
                    Err(_) => return,
                }
            };
            match msg {
                WaveMsg::Block(addr) => {
                    let header = <WAVEHDR>::ref_from_prefix(&ctx.memory[addr..]).unwrap().0;
                    let buf = &ctx.memory[header.lpData..][..header.dwBufferLength as usize];
                    stream.put_data(buf);
                    total_pending += header.dwBufferLength;
                    queued_blocks.push_back(QueuedBlock {
                        addr,
                        len: header.dwBufferLength,
                    });
                }
                WaveMsg::Reset => reset(
                    ctx,
                    &stream,
                    &receiver,
                    &mut incoming,
                    &mut queued_blocks,
                    &mut total_pending,
                    callback,
                    callback_data,
                    &pending,
                ),
            }
        }

        if let Some(block) = queued_blocks.front() {
            let queued_bytes = stream.queued_bytes();
            let consumed = total_pending - queued_bytes;
            if consumed >= block.len {
                let QueuedBlock { addr, len } = *block;
                total_pending -= len;
                queued_blocks.pop_front();
                pending.fetch_sub(1, Ordering::SeqCst);

                let header = <WAVEHDR>::mut_from_prefix(&mut ctx.memory[addr..])
                    .unwrap()
                    .0;
                header.dwFlags.insert(WHDR::DONE);

                if let Some(f) = callback {
                    let hwo = 1u32; // XXX
                    let uMsg = MM_WOM::DONE as u32;
                    // waveOutProc, WOM_DONE message
                    ctx.call32_x86(f, vec![hwo, uMsg, callback_data, addr, 0]);
                }
            }
        }

        // While the SDL queue is full, still service control messages; block
        // submissions are stashed so channel order is preserved.
        loop {
            match receiver.try_recv() {
                Ok(WaveMsg::Block(addr)) => incoming.push_back(addr),
                Ok(WaveMsg::Reset) => reset(
                    ctx,
                    &stream,
                    &receiver,
                    &mut incoming,
                    &mut queued_blocks,
                    &mut total_pending,
                    callback,
                    callback_data,
                    &pending,
                ),
                Err(TryRecvError::Empty) => break,
                // waveOutClose dropped the channel.
                Err(TryRecvError::Disconnected) => return,
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[win32_derive::dllexport]
pub fn waveOutOpen(
    ctx: &mut Context,
    phwo: u32,
    _uDeviceID: u32,
    pwfx: u32,
    dwCallback: u32,
    dwInstance: u32,
    fdwOpen: u32,
) -> u32 {
    const WAVERR_BADFORMAT: u32 = 32;
    const MMSYSERR_NOTSUPPORTED: u32 = 8;
    const MMSYSERR_INVALFLAG: u32 = 10;

    let fmt = <WAVEFORMATEX>::read_from_prefix(&ctx.memory[pwfx..])
        .unwrap()
        .0;
    if fmt.wFormatTag != 1 || fmt.wBitsPerSample != 16 {
        // The emulated device only supports 16-bit PCM.
        return WAVERR_BADFORMAT;
    }

    // WAVE_FORMAT_QUERY (also part of WAVE_FORMAT_DIRECT_QUERY) asks
    // whether the format is supported without opening the device.
    if fdwOpen & 0x0001 != 0 {
        return MMSYSERR_NOERROR;
    }

    // The remaining known flags (WAVE_ALLOWSYNC, WAVE_FORMAT_DIRECT,
    // WAVE_MAPPED) don't affect the emulated stream.
    if fdwOpen & !0x000F_001B != 0 {
        return MMSYSERR_INVALFLAG;
    }
    let Ok(callback) = CALLBACK::try_from(fdwOpen & 0x000F_0000) else {
        return MMSYSERR_INVALFLAG;
    };
    if matches!(callback, CALLBACK::WINDOW | CALLBACK::EVENT) {
        // The emulated stream only delivers function callbacks.
        return MMSYSERR_NOTSUPPORTED;
    }

    let stream = host::host().create_audio_stream(host::AudioSpec {
        channels: fmt.nChannels as u32,
        sample_rate: fmt.nSamplesPerSec,
    });
    stream.resume();

    let (sender, receiver) = std::sync::mpsc::channel::<WaveMsg>();
    let pending = Arc::new(AtomicU32::new(0));
    state().wave = Some(State {
        sender,
        pending: pending.clone(),
    });

    // The feeder thread consumes blocks for every callback style: an app
    // with no callback still expects playback and the WHDR_DONE flag.
    kernel32::lock().create_thread(ctx, "winmm thread".to_string(), move |ctx| {
        thread_proc(ctx, stream, receiver, dwCallback, dwInstance, pending)
    });

    ctx.memory.write::<u32>(phwo, 1);

    MMSYSERR_NOERROR
}

/// The one emulated output handle waveOutOpen vends.
fn open_wave(hwo: u32) -> bool {
    hwo == 1 && state().wave.is_some()
}

#[win32_derive::dllexport]
pub fn waveOutReset(_ctx: &mut Context, hwo: u32) -> u32 {
    if !open_wave(hwo) {
        return MMSYSERR_INVALHANDLE;
    }
    // The feeder thread drains and returns pending blocks; a dead thread
    // just means the stream is already gone.
    let _ = state().wave.as_ref().unwrap().sender.send(WaveMsg::Reset);
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn waveOutClose(_ctx: &mut Context, hwo: u32) -> u32 {
    if hwo != 1 {
        return MMSYSERR_INVALHANDLE;
    }
    let mut state = state();
    let Some(wave) = state.wave.take() else {
        return MMSYSERR_INVALHANDLE;
    };
    if wave.pending.load(Ordering::SeqCst) != 0 {
        // Buffers are still out; per the contract the app must reset first.
        state.wave = Some(wave);
        return WAVERR_STILLPLAYING;
    }
    // Dropping the sender ends the feeder thread, and dropping its stream
    // destroys the SDL audio stream.
    MMSYSERR_NOERROR
}

#[repr(C)]
#[derive(
    Debug, zerocopy::FromBytes, zerocopy::Immutable, zerocopy::KnownLayout, zerocopy::IntoBytes,
)]
pub struct WAVEHDR {
    lpData: u32,
    dwBufferLength: u32,
    dwBytesRecorded: u32,
    dwUser: u32,
    dwFlags: WHDR,
    dwLoops: u32,
    lpNext: u32,
    reserved: u32,
}

win32flags! {
    pub struct WHDR {
        const DONE      = 0x00000001;
        const PREPARED  = 0x00000002;
        const BEGINLOOP = 0x00000004;
        const ENDLOOP   = 0x00000008;
        const INQUEUE   = 0x00000010;
    }
}

#[win32_derive::dllexport]
pub fn waveOutPrepareHeader(ctx: &mut Context, hwo: u32, pwh: u32, cbwh: u32) -> u32 {
    if !open_wave(hwo) {
        return MMSYSERR_INVALHANDLE;
    }
    assert_eq!(cbwh, std::mem::size_of::<WAVEHDR>() as u32);
    let header = <WAVEHDR>::mut_from_prefix(&mut ctx.memory[pwh..])
        .unwrap()
        .0;
    header.dwFlags.remove(WHDR::DONE);
    header.dwFlags.insert(WHDR::PREPARED);
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn waveOutUnprepareHeader(ctx: &mut Context, hwo: u32, pwh: u32, cbwh: u32) -> u32 {
    if !open_wave(hwo) {
        return MMSYSERR_INVALHANDLE;
    }
    assert_eq!(cbwh, std::mem::size_of::<WAVEHDR>() as u32);
    let header = <WAVEHDR>::mut_from_prefix(&mut ctx.memory[pwh..])
        .unwrap()
        .0;
    header.dwFlags.remove(WHDR::PREPARED);
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn waveOutWrite(_ctx: &mut Context, hwo: u32, pwh: u32, cbwh: u32) -> u32 {
    if !open_wave(hwo) {
        return MMSYSERR_INVALHANDLE;
    }
    assert_eq!(cbwh, std::mem::size_of::<WAVEHDR>() as u32);
    let mut state = state();
    let wave = state.wave.as_mut().unwrap();
    wave.pending.fetch_add(1, Ordering::SeqCst);
    if wave.sender.send(WaveMsg::Block(pwh)).is_err() {
        wave.pending.fetch_sub(1, Ordering::SeqCst);
        return MMSYSERR_INVALHANDLE;
    }
    MMSYSERR_NOERROR
}
