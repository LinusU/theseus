//! MCI, the Media Control Interface: enough of the `cdaudio` device for games
//! that play their soundtrack off their own CD.
//!
//! The audio lives in the files a CD rip leaves beside the game — `02.wav` for
//! CD track 2, and so on, 44.1 kHz 16-bit stereo, exactly what the drive would
//! have handed the sound card. A track with no file (track 1 on a mixed-mode
//! disc, which holds the data) is reported as a non-audio track, so a game that
//! walks the table of contents sees the disc it expects.
//!
//! Playback streams from the file into its own host audio stream, fed by
//! [`pump`] from the same places the DirectSound mixer is pumped. Playback
//! position — which is what `MCI_STATUS` reports, and how a game decides a
//! track has finished — counts what the device has actually consumed, not what
//! has been queued, so a track ends when it is heard to end.
//!
//! MCI_NOTIFY, which asks for an MM_MCINOTIFY message when a command
//! finishes, is accepted and ignored: games of this era poll MCI_STATUS.

use std::{io::Read, io::Seek, io::SeekFrom, path::PathBuf};

use runtime::Context;

use crate::kernel32;

/// Commands, from mmsystem.h. Only the ones a CD player needs are here.
mod command {
    pub const OPEN: u32 = 0x0803;
    pub const CLOSE: u32 = 0x0804;
    pub const PLAY: u32 = 0x0806;
    pub const SEEK: u32 = 0x0807;
    pub const STOP: u32 = 0x0808;
    pub const PAUSE: u32 = 0x0809;
    pub const GETDEVCAPS: u32 = 0x080b;
    pub const SET: u32 = 0x080d;
    pub const STATUS: u32 = 0x0814;
    pub const RESUME: u32 = 0x0855;
}

/// Position/length arguments are for the track named in `dwTrack`.
const MCI_TRACK: u32 = 0x0010;
const MCI_FROM: u32 = 0x0004;
const MCI_TO: u32 = 0x0008;

const MCI_OPEN_TYPE: u32 = 0x2000;
/// `lpstrDeviceType` holds a device type number, not a string.
const MCI_OPEN_TYPE_ID: u32 = 0x1000;

const MCI_STATUS_ITEM: u32 = 0x0100;
const MCI_SET_TIME_FORMAT: u32 = 0x0400;
const MCI_GETDEVCAPS_ITEM: u32 = 0x0100;

const MCI_SEEK_TO_START: u32 = 0x0100;
const MCI_SEEK_TO_END: u32 = 0x0200;

/// `MCI_STATUS` items.
mod status {
    pub const LENGTH: u32 = 1;
    pub const POSITION: u32 = 2;
    pub const NUMBER_OF_TRACKS: u32 = 3;
    pub const MODE: u32 = 4;
    pub const MEDIA_PRESENT: u32 = 5;
    pub const TIME_FORMAT: u32 = 6;
    pub const READY: u32 = 7;
    pub const CURRENT_TRACK: u32 = 8;
    /// cdaudio's own item: is this track audio or data?
    pub const CDA_TYPE_TRACK: u32 = 0x4001;
}

const MCI_CDA_TRACK_AUDIO: u32 = 1088;
const MCI_CDA_TRACK_OTHER: u32 = 1089;

/// `MCI_GETDEVCAPS` items.
mod devcaps {
    pub const CAN_PLAY: u32 = 3;
    pub const DEVICE_TYPE: u32 = 4;
    pub const HAS_AUDIO: u32 = 5;
    pub const USES_FILES: u32 = 6;
    pub const COMPOUND_DEVICE: u32 = 7;
    pub const CAN_RECORD: u32 = 1;
    pub const CAN_EJECT: u32 = 2;
    pub const CAN_SAVE: u32 = 9;
}

const MCI_DEVTYPE_CD_AUDIO: u32 = 516;

/// Playback modes, as `MCI_STATUS_MODE` reports them.
mod mode {
    pub const STOP: u32 = 525;
    pub const PLAY: u32 = 526;
    pub const PAUSE: u32 = 529;
}

/// Time formats, as `MCI_SET_TIME_FORMAT` selects them.
mod format {
    pub const MILLISECONDS: u32 = 0;
    pub const MSF: u32 = 2;
    pub const TMSF: u32 = 10;
}

const MCIERR_BASE: u32 = 256;
const MCIERR_INVALID_DEVICE_ID: u32 = MCIERR_BASE + 1;
const MCIERR_UNRECOGNIZED_COMMAND: u32 = MCIERR_BASE + 5;
const MCIERR_INVALID_DEVICE_NAME: u32 = MCIERR_BASE + 7;
const MCIERR_UNSUPPORTED_FUNCTION: u32 = MCIERR_BASE + 18;
const MCIERR_OUTOFRANGE: u32 = MCIERR_BASE + 40;

const MMSYSERR_NOERROR: u32 = 0;

/// CD audio: 44.1 kHz, 16-bit, stereo.
const BYTES_PER_SEC: u64 = 44100 * 2 * 2;
/// A CD addresses audio in 1/75-second frames; MSF and TMSF positions count
/// them.
const FRAMES_PER_SEC: u64 = 75;
const BYTES_PER_FRAME: u64 = BYTES_PER_SEC / FRAMES_PER_SEC;

/// How far ahead of playback to keep the host queue, in bytes (~0.5s). Deep
/// enough to ride out a slow frame, short enough that a stop is not heard
/// half a second late.
const TARGET_QUEUE_BYTES: u32 = (BYTES_PER_SEC / 2) as u32;
/// Bytes read from the track file per pump iteration (~0.12s).
const READ_CHUNK: usize = 1 << 16;

/// The highest CD track number to look for a file for. Red Book allows 99.
const MAX_TRACKS: u32 = 99;

/// One audio track, as a file of raw CD-format PCM.
struct Track {
    path: PathBuf,
    /// Offset of the PCM within the file, past the RIFF header.
    data_offset: u64,
    /// Length of the PCM, in bytes.
    len: u64,
    /// Offset of this track's start within the whole disc, in bytes. Tracks we
    /// have no file for take up no room, which keeps disc-absolute positions
    /// consistent across the tracks that do exist.
    disc_offset: u64,
}

/// A track being played.
struct Playing {
    track: u32,
    file: host::fs::File,
    /// Where in the track playback started, in bytes.
    start: u64,
    /// Where in the track to stop, in bytes.
    end: u64,
    /// How far through the track the file has been read to, in bytes.
    read: u64,
    /// Bytes handed to the host stream. Together with what the stream still
    /// has queued, this says how much has actually been played.
    fed: u64,
    paused: bool,
}

pub struct State {
    /// Device id handed out by MCI_OPEN, or 0 while closed. One device is
    /// enough: there is only one CD drive.
    device: u32,
    /// Whether [`State::load_tracks`] has run. The files are the disc, and a
    /// disc cannot be swapped under a running program, so one scan does.
    scanned: bool,
    /// Tracks by number, so index 0 is track 1. `None` for a track with no
    /// file — the data track of a mixed-mode disc.
    tracks: Vec<Option<Track>>,
    time_format: u32,
    playing: Option<Playing>,
    /// Where the disc sits while nothing is playing, as a track number — what
    /// a real drive would have left its head on. A play with no MCI_FROM
    /// starts here.
    position: u32,
    stream: Option<host::AudioStream>,
}

impl Default for State {
    fn default() -> Self {
        State {
            device: 0,
            scanned: false,
            tracks: Vec::new(),
            // MCI's own default, until the app sets one.
            time_format: format::MILLISECONDS,
            playing: None,
            position: 1,
            stream: None,
        }
    }
}

/// Read a track file's RIFF header, returning where its PCM starts and how
/// long it is. Anything that isn't CD-format PCM is rejected: these files are
/// CD rips, so a mismatch means we're looking at the wrong file rather than
/// something worth resampling.
fn probe_track(path: &std::path::Path) -> Option<(u64, u64)> {
    let mut file = host::fs::OpenOptions::new().read(true).open(path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        log::warn!("cd track {}: not a RIFF WAVE file", path.display());
        return None;
    }

    let mut pos = 12u64;
    let mut format_ok = false;
    loop {
        let mut chunk = [0u8; 8];
        if file.seek(SeekFrom::Start(pos)).is_err() || file.read_exact(&mut chunk).is_err() {
            return None;
        }
        let id = &chunk[0..4];
        let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as u64;
        let body = pos + 8;
        if id == b"fmt " {
            let mut fmt = [0u8; 16];
            file.read_exact(&mut fmt).ok()?;
            let tag = u16::from_le_bytes(fmt[0..2].try_into().unwrap());
            let channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
            let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
            let bits = u16::from_le_bytes(fmt[14..16].try_into().unwrap());
            if (tag, channels, rate, bits) != (1, 2, 44100, 16) {
                log::warn!(
                    "cd track {}: {rate} Hz {channels}ch {bits}-bit (format {tag}), not CD audio",
                    path.display()
                );
                return None;
            }
            format_ok = true;
        } else if id == b"data" {
            if !format_ok {
                return None;
            }
            return Some((body, size));
        }
        // Chunks are word-aligned.
        pos = body + size + size % 2;
    }
}

impl State {
    /// Find the disc's tracks. The game's own directory is the disc: `NN.wav`
    /// is CD track NN.
    fn load_tracks(&mut self) {
        if self.scanned {
            return;
        }
        self.scanned = true;
        let mut disc_offset = 0;
        let mut tracks = Vec::new();
        for number in 1..=MAX_TRACKS {
            let path = kernel32::resolve_path(&format!("{number:02}.wav"));
            let track = host::fs::exists(&path)
                .then(|| probe_track(&path))
                .flatten()
                .map(|(data_offset, len)| {
                    let track = Track {
                        path: path.clone(),
                        data_offset,
                        len,
                        disc_offset,
                    };
                    disc_offset += len;
                    track
                });
            tracks.push(track);
        }
        // The disc ends at its last audio track; everything past it is padding
        // from the loop above.
        let last = tracks.iter().rposition(|track| track.is_some());
        match last {
            Some(last) => tracks.truncate(last + 1),
            None => tracks.clear(),
        }
        self.tracks = tracks;
        log::info!(
            "cd audio: {} tracks ({} audio)",
            self.tracks.len(),
            self.tracks.iter().filter(|t| t.is_some()).count()
        );
    }

    fn track(&self, number: u32) -> Option<&Track> {
        self.tracks.get(number.checked_sub(1)? as usize)?.as_ref()
    }

    /// `number`, or the next audio track after it, wrapping at the end of the
    /// disc. `None` for a disc with no audio at all. Used to skip the data
    /// track the way a drive asked to play from one would.
    fn audio_track_from(&self, number: u32) -> Option<u32> {
        let count = self.tracks.len() as u32;
        (0..count)
            .map(|step| (number.saturating_sub(1) + step) % count + 1)
            .find(|number| self.track(*number).is_some())
    }

    fn stream(&mut self) -> &host::AudioStream {
        self.stream.get_or_insert_with(|| {
            let stream = host::host().create_audio_stream(host::AudioSpec {
                sample_rate: 44100,
                channels: 2,
            });
            stream.resume();
            stream
        })
    }

    /// How far playback has actually got into the current track, in bytes.
    fn played(&mut self) -> u64 {
        let Some(playing) = self.playing.as_ref() else {
            return 0;
        };
        let (start, fed) = (playing.start, playing.fed);
        let queued = self.stream().queued_bytes() as u64;
        start + fed.saturating_sub(queued)
    }

    fn mode(&mut self) -> u32 {
        match self.playing.as_ref() {
            None => mode::STOP,
            Some(playing) if playing.paused => mode::PAUSE,
            Some(_) => mode::PLAY,
        }
    }

    fn stop(&mut self) {
        if let Some(playing) = self.playing.take() {
            // The head stays on the track it was stopped in.
            self.position = playing.track;
            let stream = self.stream();
            stream.clear();
            // Playing again after a pause+stop must not stay paused.
            stream.resume();
        }
    }

    fn play(&mut self, track: u32, start: u64, end: u64) -> u32 {
        let Some(found) = self.track(track) else {
            return MCIERR_OUTOFRANGE;
        };
        let (path, data_offset, len) = (found.path.clone(), found.data_offset, found.len);
        let start = start.min(len);
        let end = end.clamp(start, len);

        let mut file = match host::fs::OpenOptions::new().read(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                log::warn!("cd track {}: {err}", path.display());
                return MCIERR_OUTOFRANGE;
            }
        };
        if file.seek(SeekFrom::Start(data_offset + start)).is_err() {
            return MCIERR_OUTOFRANGE;
        }

        self.stop();
        log::info!(
            "cd audio: playing track {track} from {:.1}s to {:.1}s of {:.1}s",
            start as f64 / BYTES_PER_SEC as f64,
            end as f64 / BYTES_PER_SEC as f64,
            len as f64 / BYTES_PER_SEC as f64,
        );
        self.playing = Some(Playing {
            track,
            file,
            start,
            end,
            read: start,
            fed: 0,
            paused: false,
        });
        MMSYSERR_NOERROR
    }

    /// Top the host queue up, and notice when the track has played out.
    fn pump(&mut self) {
        let Some(playing) = self.playing.as_ref() else {
            return;
        };
        if playing.paused {
            return;
        }
        if !self.stream().is_open() {
            // No audio device. Nothing will ever drain the queue, so the
            // position can't advance; report the track as over rather than
            // leaving the game waiting on music it will never hear.
            self.playing = None;
            return;
        }

        while self.stream().queued_bytes() < TARGET_QUEUE_BYTES {
            let playing = self.playing.as_mut().unwrap();
            let want = (playing.end - playing.read).min(READ_CHUNK as u64) as usize;
            if want == 0 {
                break;
            }
            let mut buf = vec![0u8; want];
            let read = match playing.file.read(&mut buf) {
                Ok(read) if read > 0 => read,
                // The file ran out early, or stopped being readable. Either
                // way the track ends here rather than staying forever in
                // MCI_MODE_PLAY waiting for bytes that aren't coming.
                result => {
                    if let Err(err) = result {
                        log::warn!("cd audio: read failed: {err}");
                    }
                    playing.end = playing.read;
                    break;
                }
            };
            buf.truncate(read);
            playing.read += read as u64;
            playing.fed += read as u64;
            let stream = self.stream.as_ref().unwrap();
            stream.put_data(&buf);
        }

        // Finished only once the device has played what it was given: a game
        // that watches for MCI_MODE_STOP to start the next track would
        // otherwise cut this one short.
        let playing = self.playing.as_ref().unwrap();
        let done = playing.read >= playing.end;
        if done && self.stream().queued_bytes() == 0 {
            let playing = self.playing.take().unwrap();
            // Playing to the end of a track leaves the head at the start of
            // the next one.
            self.position = playing.track + 1;
        }
    }
}

/// Split a disc-absolute byte position into a track and an offset within it.
fn split_disc_position(state: &State, position: u64) -> (u32, u64) {
    for (index, track) in state.tracks.iter().enumerate().rev() {
        let Some(track) = track else { continue };
        if position >= track.disc_offset {
            return (index as u32 + 1, position - track.disc_offset);
        }
    }
    (1, 0)
}

/// Turn a position argument into a track and a byte offset within it, in
/// whatever time format is currently set. Only TMSF names a track; the other
/// formats address the disc as a whole, so the track falls out of the layout.
fn decode_position(state: &State, value: u32) -> (u32, u64) {
    match state.time_format {
        format::TMSF => {
            let track = value & 0xff;
            let minutes = (value >> 8) & 0xff;
            let seconds = (value >> 16) & 0xff;
            let frames = (value >> 24) & 0xff;
            let offset = ((minutes as u64 * 60 + seconds as u64) * FRAMES_PER_SEC + frames as u64)
                * BYTES_PER_FRAME;
            (track, offset)
        }
        format::MSF => {
            let minutes = value & 0xff;
            let seconds = (value >> 8) & 0xff;
            let frames = (value >> 16) & 0xff;
            let position = ((minutes as u64 * 60 + seconds as u64) * FRAMES_PER_SEC
                + frames as u64)
                * BYTES_PER_FRAME;
            split_disc_position(state, position)
        }
        _ => {
            // MCI_FORMAT_MILLISECONDS, and anything we don't know.
            let position = value as u64 * BYTES_PER_SEC / 1000;
            split_disc_position(state, position)
        }
    }
}

/// The inverse of [`decode_position`], for the positions MCI_STATUS reports.
/// `track` is 0 for a disc-absolute value.
fn encode_position(state: &State, track: u32, offset: u64) -> u32 {
    let frames = offset / BYTES_PER_FRAME;
    let (minutes, seconds, frames) = (
        frames / FRAMES_PER_SEC / 60,
        frames / FRAMES_PER_SEC % 60,
        frames % FRAMES_PER_SEC,
    );
    let (minutes, seconds, frames) = (minutes as u32, seconds as u32, frames as u32);
    match state.time_format {
        format::TMSF => track | (minutes << 8) | (seconds << 16) | (frames << 24),
        format::MSF => minutes | (seconds << 8) | (frames << 16),
        _ => {
            let absolute = match state.track(track) {
                Some(track) => track.disc_offset + offset,
                None => offset,
            };
            (absolute * 1000 / BYTES_PER_SEC) as u32
        }
    }
}

/// Keep CD playback fed. Cheap with nothing playing, so it can go anywhere the
/// app passes through regularly.
pub fn pump() {
    let mut winmm = super::state();
    let mci = winmm.mci();
    if mci.playing.is_none() {
        return;
    }
    mci.pump();
}

fn open(ctx: &mut Context, dwParam1: u32, dwParam2: u32) -> u32 {
    // We are the cdaudio device and nothing else. The device type arrives
    // either as a number (MCI_OPEN_TYPE_ID) or as a string.
    if dwParam1 & MCI_OPEN_TYPE == 0 {
        return MCIERR_INVALID_DEVICE_NAME;
    }
    let device_type = ctx.memory.read::<u32>(dwParam2 + 8);
    let is_cdaudio = if dwParam1 & MCI_OPEN_TYPE_ID != 0 {
        device_type == MCI_DEVTYPE_CD_AUDIO
    } else {
        ctx.memory
            .read_str(device_type)
            .eq_ignore_ascii_case("cdaudio")
    };
    if !is_cdaudio {
        return MCIERR_INVALID_DEVICE_NAME;
    }
    // MCI_OPEN_ELEMENT, when set, names the drive ("d:"). Every drive is the
    // same directory here, so which one it is doesn't change what we play.

    let mut winmm = super::state();
    let mci = winmm.mci();
    mci.load_tracks();
    if mci.tracks.is_empty() {
        // No rip beside the game: report an empty drive rather than a disc
        // whose tracks are all silent.
        log::warn!("cd audio: no NN.wav track files found; no disc in the drive");
        return MCIERR_INVALID_DEVICE_NAME;
    }
    // A single device, so a fixed id will do; it just has to be non-zero, and
    // distinct from MCI_ALL_DEVICE_ID.
    mci.device = 1;
    // wDeviceID, which the caller passes back as mciId from here on.
    ctx.memory.write(dwParam2 + 4, mci.device);
    MMSYSERR_NOERROR
}

fn status(ctx: &mut Context, dwParam1: u32, dwParam2: u32) -> u32 {
    if dwParam1 & MCI_STATUS_ITEM == 0 {
        return MCIERR_UNSUPPORTED_FUNCTION;
    }
    let item = ctx.memory.read::<u32>(dwParam2 + 8);
    let track = if dwParam1 & MCI_TRACK != 0 {
        ctx.memory.read::<u32>(dwParam2 + 12)
    } else {
        0
    };

    let mut winmm = super::state();
    let mci = winmm.mci();
    let current = mci.playing.as_ref().map(|playing| playing.track);
    let value = match item {
        status::MODE => mci.mode(),
        status::READY | status::MEDIA_PRESENT => true as u32,
        status::TIME_FORMAT => mci.time_format,
        status::NUMBER_OF_TRACKS => mci.tracks.len() as u32,
        status::CURRENT_TRACK => current.unwrap_or(1),
        status::CDA_TYPE_TRACK => match mci.track(track) {
            Some(_) => MCI_CDA_TRACK_AUDIO,
            // The tracks with no file are the data ones.
            None => MCI_CDA_TRACK_OTHER,
        },
        status::LENGTH => {
            let (track, len) = if dwParam1 & MCI_TRACK != 0 {
                (track, mci.track(track).map_or(0, |track| track.len))
            } else {
                // The whole disc.
                (0, mci.tracks.iter().flatten().map(|track| track.len).sum())
            };
            encode_position(mci, track, len)
        }
        // Where the head is: in the track being played, or at the start of the
        // disc when it is stopped.
        status::POSITION => {
            let offset = mci.played();
            encode_position(mci, current.unwrap_or(1), offset)
        }
        _ => {
            log::warn!("mciSendCommandA: unhandled MCI_STATUS item {item:#x}");
            return MCIERR_UNSUPPORTED_FUNCTION;
        }
    };
    drop(winmm);
    // dwReturn.
    ctx.memory.write(dwParam2 + 4, value);
    MMSYSERR_NOERROR
}

fn play(ctx: &mut Context, dwParam1: u32, dwParam2: u32) -> u32 {
    let mut winmm = super::state();
    let mci = winmm.mci();

    // Without MCI_FROM, play carries on from where the disc is now — which for
    // a paused disc means resuming it.
    if dwParam1 & MCI_FROM == 0 {
        if let Some(playing) = mci.playing.as_mut() {
            playing.paused = false;
            mci.stream().resume();
            return MMSYSERR_NOERROR;
        }
    }

    let from = ctx.memory.read::<u32>(dwParam2 + 4);
    let to = ctx.memory.read::<u32>(dwParam2 + 8);
    let (track, start) = match dwParam1 & MCI_FROM != 0 {
        true => decode_position(mci, from),
        // Nothing playing and nowhere named: start where the head was left.
        // This is the path a game takes to resume music it believes it
        // paused, so failing here loses the soundtrack for good.
        false => (mci.audio_track_from(mci.position).unwrap_or(1), 0),
    };
    let end = match dwParam1 & MCI_TO != 0 {
        true => {
            let (to_track, offset) = decode_position(mci, to);
            match to_track == track {
                true => offset,
                // Playing across a track boundary; we only ever play one track
                // at a time, so stop at the end of this one.
                false => u64::MAX,
            }
        }
        false => u64::MAX,
    };
    mci.play(track, start, end)
}

/// MCI_SEEK moves the disc without playing it. A stopped disc has no position
/// here — there is nothing to move a head to — so all this does is stop, which
/// is the part a caller can observe. A following MCI_PLAY should name where to
/// play from; without MCI_FROM it starts at the track it was given last.
fn seek(_ctx: &mut Context, dwParam1: u32, _dwParam2: u32) -> u32 {
    if dwParam1 & (MCI_SEEK_TO_START | MCI_SEEK_TO_END | MCI_TO) == 0 {
        return MCIERR_UNSUPPORTED_FUNCTION;
    }
    super::state().mci().stop();
    MMSYSERR_NOERROR
}

fn getdevcaps(ctx: &mut Context, dwParam1: u32, dwParam2: u32) -> u32 {
    if dwParam1 & MCI_GETDEVCAPS_ITEM == 0 {
        return MCIERR_UNSUPPORTED_FUNCTION;
    }
    let item = ctx.memory.read::<u32>(dwParam2 + 8);
    let value = match item {
        devcaps::DEVICE_TYPE => MCI_DEVTYPE_CD_AUDIO,
        devcaps::CAN_PLAY | devcaps::HAS_AUDIO | devcaps::CAN_EJECT => true as u32,
        devcaps::CAN_RECORD | devcaps::CAN_SAVE => false as u32,
        devcaps::USES_FILES | devcaps::COMPOUND_DEVICE => false as u32,
        _ => {
            log::warn!("mciSendCommandA: unhandled MCI_GETDEVCAPS item {item:#x}");
            return MCIERR_UNSUPPORTED_FUNCTION;
        }
    };
    ctx.memory.write(dwParam2 + 4, value);
    MMSYSERR_NOERROR
}

#[win32_derive::dllexport]
pub fn mciSendCommandA(
    ctx: &mut Context,
    mciId: u32,
    uMsg: u32,
    dwParam1: u32,
    dwParam2: u32,
) -> u32 {
    if uMsg == command::OPEN {
        return open(ctx, dwParam1, dwParam2);
    }

    if mciId == 0 || mciId != super::state().mci().device {
        return MCIERR_INVALID_DEVICE_ID;
    }
    match uMsg {
        command::CLOSE => {
            let mut winmm = super::state();
            let mci = winmm.mci();
            mci.stop();
            mci.device = 0;
            MMSYSERR_NOERROR
        }
        command::PLAY => play(ctx, dwParam1, dwParam2),
        command::STOP => {
            super::state().mci().stop();
            MMSYSERR_NOERROR
        }
        command::PAUSE => {
            let mut winmm = super::state();
            let mci = winmm.mci();
            if let Some(playing) = mci.playing.as_mut() {
                playing.paused = true;
                mci.stream().pause();
            }
            MMSYSERR_NOERROR
        }
        command::RESUME => {
            let mut winmm = super::state();
            let mci = winmm.mci();
            if let Some(playing) = mci.playing.as_mut() {
                playing.paused = false;
                mci.stream().resume();
            }
            MMSYSERR_NOERROR
        }
        command::SEEK => seek(ctx, dwParam1, dwParam2),
        command::STATUS => status(ctx, dwParam1, dwParam2),
        command::GETDEVCAPS => getdevcaps(ctx, dwParam1, dwParam2),
        command::SET => {
            if dwParam1 & MCI_SET_TIME_FORMAT != 0 {
                let format = ctx.memory.read::<u32>(dwParam2 + 4);
                super::state().mci().time_format = format;
            }
            // The other MCI_SET flags are the door and the audio channels,
            // neither of which we have.
            MMSYSERR_NOERROR
        }
        _ => {
            log::warn!("mciSendCommandA: unhandled command {uMsg:#x}");
            MCIERR_UNRECOGNIZED_COMMAND
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disc shaped like Moto Racer's: a data track 1, then audio.
    fn disc(audio: &[u32]) -> State {
        let mut state = State::default();
        let last = *audio.iter().max().unwrap();
        state.tracks = (1..=last)
            .map(|number| {
                audio.contains(&number).then(|| Track {
                    path: Default::default(),
                    data_offset: 44,
                    len: BYTES_PER_SEC * 60,
                    disc_offset: 0,
                })
            })
            .collect();
        state
    }

    #[test]
    fn audio_track_from_skips_the_data_track() {
        let state = disc(&[2, 3, 4]);
        // The head sitting on an audio track stays there.
        assert_eq!(state.audio_track_from(3), Some(3));
        // Track 1 holds the data, so playing "from the start" means track 2 —
        // the case a game hits when it resumes music it believes it paused.
        assert_eq!(state.audio_track_from(1), Some(2));
        // Past the last track, the head wraps around rather than falling off.
        assert_eq!(state.audio_track_from(5), Some(2));
        // A disc with no audio at all has nowhere to go.
        assert_eq!(State::default().audio_track_from(1), None);
    }
}
