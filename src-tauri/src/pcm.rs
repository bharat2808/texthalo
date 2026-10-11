//! Continuous 24 kHz mono PCM playback for the local speech engines.
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;

pub const SAMPLE_RATE: usize = 24_000;
const BUFFER_FRAMES: usize = 512;
const BUFFER_COUNT: usize = 3;

// Audiobooks use their committed PCM file directly. Mapping avoids copying an
// entire multi-hour recording into the speech player's retained sample vector.
struct DiskSamples {
    address: *mut c_void,
    bytes: usize,
    samples: usize,
}
impl DiskSamples {
    fn open(path: &std::path::Path, samples: u64) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let count = usize::try_from(samples).map_err(|_| "Audiobook exceeds address space")?;
        let bytes = count
            .checked_mul(2)
            .and_then(|n| n.checked_add(44))
            .ok_or("Audiobook exceeds address space")?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() < bytes as u64 {
            return Err("Committed audio is unavailable".into());
        }
        let address = unsafe {
            libc::mmap(
                ptr::null_mut(),
                bytes,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            address,
            bytes,
            samples: count,
        })
    }
    fn copy(&self, start: usize, output: &mut [f32]) {
        // The service never truncates a committed prefix while its player owns it.
        // The enclosing SampleBuffer mutex serializes remapping and callbacks.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (self.address as *const u8).add(44 + start * 2),
                output.len() * 2,
            )
        };
        for (sample, pcm) in output.iter_mut().zip(bytes.chunks_exact(2)) {
            *sample = i16::from_le_bytes([pcm[0], pcm[1]]) as f32 / 32768.0;
        }
    }
}
impl Drop for DiskSamples {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.address, self.bytes);
        }
    }
}

#[derive(Default)]
struct SampleBuffer {
    samples: Vec<f32>,
    disk: Option<DiskSamples>,
    cursor: usize,
    playback_start: usize,
    paused: bool,
    speed: f32,
    finished: bool,
    cancelled: bool,
    underrun_frames: usize,
    draining: bool,
    resetting: bool,
    empty_queue: bool,
    error: i32,
    played_audio_bytes: u64,
}
impl SampleBuffer {
    fn len(&self) -> usize {
        self.disk
            .as_ref()
            .map_or(0, |d| d.samples)
            .saturating_add(self.samples.len())
    }
    fn read(&mut self, output: &mut [f32]) -> usize {
        if self.cancelled {
            self.samples.clear();
            self.disk = None;
            return 0;
        }
        let copied = output.len().min(self.len().saturating_sub(self.cursor));
        let disk_len = self.disk.as_ref().map_or(0, |d| d.samples);
        let disk_count = copied.min(disk_len.saturating_sub(self.cursor));
        if disk_count > 0 {
            self.disk
                .as_ref()
                .unwrap()
                .copy(self.cursor, &mut output[..disk_count]);
        }
        if copied > disk_count {
            let start = (self.cursor + disk_count).saturating_sub(disk_len);
            output[disk_count..copied]
                .copy_from_slice(&self.samples[start..start + copied - disk_count]);
        }
        self.cursor += copied;
        // Hosted acknowledgements use a monotonic high-water mark, including after rewind.
        self.played_audio_bytes = self.played_audio_bytes.max(self.cursor as u64 * 2);
        if self.finished {
            return copied;
        }
        output[copied..].fill(0.0);
        self.underrun_frames += output.len() - copied;
        output.len()
    }
}

// Layouts and declarations follow AudioQueue.h and CoreAudioBaseTypes.h in the macOS SDK.
#[repr(C)]
struct StreamFormat {
    sample_rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}
#[repr(C)]
struct AudioBuffer {
    capacity: u32,
    data: *mut c_void,
    size: u32,
    user_data: *mut c_void,
    packet_capacity: u32,
    packets: *mut c_void,
    packet_count: u32,
}
type Queue = *mut c_void;
#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioQueueNewOutput(
        format: *const StreamFormat,
        callback: unsafe extern "C" fn(*mut c_void, Queue, *mut AudioBuffer),
        user: *mut c_void,
        run_loop: *const c_void,
        mode: *const c_void,
        flags: u32,
        queue: *mut Queue,
    ) -> i32;
    fn AudioQueueAllocateBuffer(queue: Queue, capacity: u32, buffer: *mut *mut AudioBuffer) -> i32;
    fn AudioQueueEnqueueBuffer(
        queue: Queue,
        buffer: *mut AudioBuffer,
        packets: u32,
        descriptions: *const c_void,
    ) -> i32;
    fn AudioQueueStart(queue: Queue, time: *const c_void) -> i32;
    fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
    fn AudioQueuePause(queue: Queue) -> i32;
    fn AudioQueueReset(queue: Queue) -> i32;
    fn AudioQueueSetProperty(queue: Queue, property: u32, data: *const c_void, size: u32) -> i32;
    fn AudioQueueSetParameter(queue: Queue, parameter: u32, value: f32) -> i32;
    fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    fn AudioQueueGetProperty(queue: Queue, property: u32, data: *mut c_void, size: *mut u32)
        -> i32;
}

unsafe extern "C" fn refill(user: *mut c_void, queue: Queue, buffer: *mut AudioBuffer) {
    // The boxed mutex outlives the queue; synchronous Dispose joins its callbacks.
    let shared = &*(user as *const Mutex<SampleBuffer>);
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    if state.cancelled || state.draining || state.resetting {
        return;
    }
    let output = std::slice::from_raw_parts_mut((*buffer).data as *mut f32, BUFFER_FRAMES);
    let frames = state.read(output);
    let result = if frames == 0 {
        state.draining = true;
        // Async stop drains already submitted audio. A refill callback does NOT mean
        // the last samples are audible yet; the owner waits for IsRunning to become 0.
        let result = AudioQueueStop(queue, 0);
        drop(state);
        result
    } else {
        (*buffer).size = (frames * 4) as u32;
        // Keep the state lock until enqueue returns so seek cannot reset mid-enqueue.
        let result = AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null());
        drop(state);
        result
    };
    if result != 0 {
        // Reset may return buffers through this callback without playing them.
        shared.lock().unwrap_or_else(|e| e.into_inner()).error = result;
    }
}

#[derive(serde::Serialize, Clone, Default)]
pub struct PlaybackStatus {
    pub start: f64,
    pub position: f64,
    pub buffered: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub speed: f32,
    pub available: bool,
}

/// One native queue for an entire reading. Producer writes and callback reads only PCM;
/// no inference, file I/O or process launches occur in the callback.
pub struct Player {
    queue: Queue,
    state: Box<Mutex<SampleBuffer>>,
    buffers: [*mut AudioBuffer; BUFFER_COUNT],
    control: Mutex<bool>, // serialized start/stop; value means started
}
// AudioQueue supports calls from multiple threads. Start/stop/property operations are
// serialized with control; callbacks use only state, and never wait for control.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

fn check(status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("Core Audio playback failed ({status})"))
    }
}

impl Player {
    pub fn new() -> Result<Self, String> {
        let state = Box::new(Mutex::new(SampleBuffer {
            speed: 1.0,
            ..Default::default()
        }));
        let format = StreamFormat {
            sample_rate: SAMPLE_RATE as f64,
            format: u32::from_be_bytes(*b"lpcm"),
            flags: 1 | (1 << 3), // native-endian float, packed
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels: 1,
            bits: 32,
            reserved: 0,
        };
        let mut queue = ptr::null_mut();
        unsafe {
            check(AudioQueueNewOutput(
                &format,
                refill,
                &*state as *const Mutex<SampleBuffer> as *mut c_void,
                ptr::null(),
                ptr::null(),
                0,
                &mut queue,
            ))?;
        }
        let mut player = Self {
            queue,
            state,
            buffers: [ptr::null_mut(); BUFFER_COUNT],
            control: Mutex::new(false),
        };
        unsafe {
            let enabled = 1u32;
            check(AudioQueueSetProperty(
                queue,
                u32::from_be_bytes(*b"q_tp"),
                &enabled as *const _ as *const c_void,
                4,
            ))?;
            let algorithm = u32::from_be_bytes(*b"spec");
            check(AudioQueueSetProperty(
                queue,
                u32::from_be_bytes(*b"qtpa"),
                &algorithm as *const _ as *const c_void,
                4,
            ))?;
        }
        for buffer in &mut player.buffers {
            unsafe {
                check(AudioQueueAllocateBuffer(
                    queue,
                    (BUFFER_FRAMES * 4) as u32,
                    buffer,
                ))?;
            }
        }
        Ok(player)
    }

    pub fn push(&self, samples: Vec<f32>, last: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.cancelled {
            state.samples.extend(samples);
            state.finished = last;
        }
    }

    pub(crate) fn update_file(
        &self,
        path: &std::path::Path,
        samples: u64,
        last: bool,
    ) -> Result<(), String> {
        let disk = DiskSamples::open(path, samples)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.cancelled {
            state.disk = Some(disk);
            state.finished = last;
        }
        Ok(())
    }

    pub(crate) fn promote_file(
        &self,
        path: &std::path::Path,
        samples: u64,
        promoted: usize,
    ) -> Result<(), String> {
        let disk = DiskSamples::open(path, samples)?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            return Ok(());
        }
        let previous = state.disk.as_ref().map_or(0, |d| d.samples);
        if disk.samples.checked_sub(previous) != Some(promoted) || state.samples.len() < promoted {
            return Err("Streamed audio did not match the committed recording".into());
        }
        state.samples.drain(..promoted).for_each(drop);
        state.disk = Some(disk);
        Ok(())
    }

    pub fn buffered_seconds(&self) -> f64 {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.len().saturating_sub(state.cursor) as f64 / SAMPLE_RATE as f64
    }

    pub fn played_audio_bytes(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .played_audio_bytes
    }

    pub fn underrun_seconds(&self) -> f64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .underrun_frames as f64
            / SAMPLE_RATE as f64
    }

    pub fn start(&self) -> Result<(), String> {
        let mut started = self.control.lock().unwrap_or_else(|e| e.into_inner());
        if *started {
            return Ok(());
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            return Ok(());
        }
        for buffer in self.buffers {
            unsafe {
                let output =
                    std::slice::from_raw_parts_mut((*buffer).data as *mut f32, BUFFER_FRAMES);
                let frames = state.read(output);
                if frames == 0 {
                    break;
                }
                (*buffer).size = (frames * 4) as u32;
                check(AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()))?;
            }
        }
        drop(state);
        unsafe {
            check(AudioQueueStart(self.queue, ptr::null()))?;
        }
        *started = true;
        Ok(())
    }

    pub fn is_playing(&self) -> Result<bool, String> {
        let started = self.control.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        check(state.error)?;
        if !*started || state.cancelled {
            return Ok(false);
        }
        if state.paused || !state.draining {
            return Ok(true);
        }
        drop(state);
        let mut running = 0u32;
        let mut size = 4u32;
        unsafe {
            check(AudioQueueGetProperty(
                self.queue,
                u32::from_be_bytes(*b"aqrn"),
                &mut running as *mut u32 as *mut c_void,
                &mut size,
            ))?;
        }
        Ok(running != 0)
    }

    pub fn snapshot(&self) -> PlaybackStatus {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        PlaybackStatus {
            start: state.playback_start as f64 / SAMPLE_RATE as f64,
            position: state.cursor as f64 / SAMPLE_RATE as f64,
            buffered: state.len() as f64 / SAMPLE_RATE as f64,
            duration: state
                .finished
                .then_some(state.len() as f64 / SAMPLE_RATE as f64),
            paused: state.paused,
            speed: state.speed,
            available: !state.cancelled && state.len() > 0,
        }
    }

    pub fn pause(&self, paused: bool) -> Result<(), String> {
        let _control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        let empty = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .empty_queue;
        unsafe {
            if paused {
                check(AudioQueuePause(self.queue))?;
            } else if !empty {
                check(AudioQueueStart(self.queue, ptr::null()))?;
            }
        }
        self.state.lock().unwrap_or_else(|e| e.into_inner()).paused = paused;
        Ok(())
    }

    pub fn set_speed(&self, speed: f32) -> Result<(), String> {
        if !speed.is_finite() || !(0.5..=2.0).contains(&speed) {
            return Err("Playback speed must be between 0.5× and 2×.".into());
        }
        let _control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            check(AudioQueueSetParameter(self.queue, 2, speed))?;
        }
        self.state.lock().unwrap_or_else(|e| e.into_inner()).speed = speed;
        Ok(())
    }

    pub(crate) fn set_playback_start(&self, sample: u64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let start = usize::try_from(sample).map_err(|_| "Invalid chapter start")?;
        if start >= state.len() {
            return Err("Chapter audio is unavailable".into());
        }
        state.playback_start = start;
        state.cursor = start;
        Ok(())
    }

    pub fn seek(&self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() {
            return Err("Invalid playback position.".into());
        }
        let mut started = self.control.lock().unwrap_or_else(|e| e.into_inner());
        // Reset returns outstanding buffers through refill; suppress those callbacks.
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resetting = true;
        let reset = unsafe {
            check(AudioQueuePause(self.queue)).and_then(|_| check(AudioQueueReset(self.queue)))
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.resetting = false;
        reset?;
        if state.cancelled {
            return Err("This reading has stopped.".into());
        }
        state.cursor = ((seconds.max(0.0) * SAMPLE_RATE as f64) as usize)
            .clamp(state.playback_start, state.len());
        state.draining = false;
        let mut enqueued = 0;
        for buffer in self.buffers {
            unsafe {
                let output =
                    std::slice::from_raw_parts_mut((*buffer).data as *mut f32, BUFFER_FRAMES);
                let frames = state.read(output);
                if frames == 0 {
                    break;
                }
                (*buffer).size = (frames * 4) as u32;
                check(AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()))?;
                enqueued += 1;
            }
        }
        state.empty_queue = enqueued == 0;
        if enqueued == 0 {
            state.draining = true;
            unsafe {
                check(AudioQueueStop(self.queue, 1))?;
            }
        }
        if !state.paused && !state.draining {
            unsafe {
                check(AudioQueueStart(self.queue, ptr::null()))?;
            }
        }
        *started = true;
        Ok(())
    }

    pub fn stop(&self) {
        let _control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.cancelled = true;
            state.samples.clear();
            state.disk = None;
        }
        unsafe {
            AudioQueueStop(self.queue, 1);
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        // Stop before disposing; state is freed only after no callbacks can reference it.
        self.stop();
        unsafe {
            AudioQueueDispose(self.queue, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a macOS audio output device"]
    fn saved_chapter_seek_stays_between_verified_boundaries() {
        let path = std::env::temp_dir().join(format!("pcm-chapter-{}.wav", uuid::Uuid::new_v4()));
        crate::kokoro::write_wav(&path, &vec![0.; 72000], SAMPLE_RATE as u32).unwrap();
        let player = Player::new().unwrap();
        player.update_file(&path, 48000, true).unwrap(); // No access to the partial third second.
        player.set_playback_start(24000).unwrap(); // No access to the partial first second.
        player.pause(true).unwrap();
        player.seek(0.).unwrap();
        let at = player.snapshot();
        assert_eq!(at.start, 1.);
        assert_eq!(at.buffered, 2.);
        assert!(at.position >= 1. && at.position <= 2.);
        player.seek(3.).unwrap();
        assert_eq!(player.snapshot().position, 2.);
        player.stop();
        drop(player);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn disk_playback_exposes_only_the_committed_prefix_and_can_grow() {
        let path = std::env::temp_dir().join(format!("texthalo-pcm-{}.wav", uuid::Uuid::new_v4()));
        crate::kokoro::write_wav(&path, &[0.25, 0.5, 0.75], SAMPLE_RATE as u32).unwrap();
        let mut buffer = SampleBuffer {
            disk: Some(DiskSamples::open(&path, 2).unwrap()),
            ..Default::default()
        };
        let mut out = [0.0; 3];
        assert_eq!(buffer.read(&mut out), 3); // The uncommitted suffix remains silence.
        assert!((out[0] - 0.25).abs() < 0.001);
        assert!((out[1] - 0.5).abs() < 0.001);
        assert_eq!(out[2], 0.0);
        assert_eq!(buffer.cursor, 2);
        buffer.disk = Some(DiskSamples::open(&path, 3).unwrap());
        buffer.finished = true;
        assert_eq!(buffer.read(&mut out), 1);
        assert!((out[0] - 0.75).abs() < 0.001);
        assert!(buffer.samples.is_empty());
        assert!(DiskSamples::open(&path, 4).is_err());
        drop(buffer);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn samples_cross_generation_boundaries_without_gaps() {
        let mut buffer = SampleBuffer::default();
        buffer.samples.extend([0.1, 0.2]);
        buffer.samples.extend([0.3, 0.4]);
        buffer.finished = true;
        let mut out = [0.; 8];
        assert_eq!(buffer.read(&mut out), 4);
        assert_eq!(&out[..4], &[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(buffer.read(&mut out), 0);
        assert_eq!(buffer.underrun_frames, 0);
    }
    #[test]
    fn starvation_keeps_output_alive_and_counts_only_missing_samples() {
        let mut buffer = SampleBuffer::default();
        buffer.samples.push(0.5);
        let mut out = [9.; 4];
        assert_eq!(buffer.read(&mut out), 4);
        assert_eq!(out, [0.5, 0., 0., 0.]);
        assert_eq!(buffer.underrun_frames, 3);
        buffer.samples.extend([0.6, 0.7]);
        buffer.finished = true;
        assert_eq!(buffer.read(&mut out), 2);
        assert_eq!(&out[..2], &[0.6, 0.7]);
    }
    #[test]
    fn cancellation_drops_buffered_audio() {
        let mut buffer = SampleBuffer::default();
        buffer.samples.extend([1.; 20]);
        buffer.cancelled = true;
        assert_eq!(buffer.read(&mut [0.; 4]), 0);
        assert!(buffer.samples.is_empty());
    }
    #[test]
    fn rewind_preserves_samples_and_acknowledgement_high_water_mark() {
        let mut buffer = SampleBuffer {
            samples: vec![0.1, 0.2, 0.3, 0.4],
            finished: true,
            ..Default::default()
        };
        let mut out = [0.0; 2];
        assert_eq!(buffer.read(&mut out), 2);
        assert_eq!(buffer.played_audio_bytes, 4);
        buffer.cursor = 0;
        assert_eq!(buffer.read(&mut out), 2);
        assert_eq!(out, [0.1, 0.2]);
        assert_eq!(buffer.played_audio_bytes, 4);
        assert_eq!(buffer.read(&mut out), 2);
        assert_eq!(buffer.played_audio_bytes, 8);
    }

    #[test]
    #[ignore = "requires a macOS audio output device"]
    fn native_controls_pause_seek_speed_drain_and_replay() {
        let player = Player::new().unwrap();
        player.push(vec![0.0; SAMPLE_RATE / 2], true);
        player.start().unwrap();
        player.pause(true).unwrap();
        assert!(player.is_playing().unwrap());
        assert!(player.snapshot().paused);
        player.set_speed(1.5).unwrap();
        assert_eq!(player.snapshot().speed, 1.5);
        assert!(player.set_speed(f32::NAN).is_err());
        assert!(player.seek(f64::INFINITY).is_err());
        player.seek(0.1).unwrap();
        assert!(player.snapshot().paused);
        player.pause(false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while player.is_playing().unwrap() {
            assert!(std::time::Instant::now() < deadline, "queue did not drain");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(player.snapshot().available);
        player.seek(0.0).unwrap();
        assert!(player.is_playing().unwrap());
        // Seeking past the end clamps and must not start an empty queue forever.
        player.pause(true).unwrap();
        player.seek(100.0).unwrap();
        player.pause(false).unwrap();
        assert!(!player.is_playing().unwrap());
        player.seek(0.0).unwrap();
        for position in [0.2, 0.1, 0.3, 0.0, 0.25] {
            player.seek(position).unwrap();
        }
        assert!(player.is_playing().unwrap());
        player.stop();
        assert!(!player.snapshot().available);
    }
}
