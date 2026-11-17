use crate::settings::AudioSource;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TARGET_RATE: u32 = 48_000;
const CHUNK_FRAMES_16K: usize = 640; // 40 ms at 16 kHz
const LEVEL_INTERVAL: Duration = Duration::from_millis(120);
const MAX_LAG_FRAMES_16K: usize = 3_200; // 200 ms guard against suspended monitor
const QUEUE_BLOCKS: usize = 128;
const BLOCK_SAMPLES: usize = 1024;

/// 63-tap Blackman-windowed sinc low-pass, normalized for unity DC gain.
/// The 7 kHz cutoff retains speech while rejecting frequencies above 8 kHz Nyquist.
const FIR_TAPS: usize = 63;
const FIR_COEFFICIENTS: [f32; FIR_TAPS] = [
    0.0000000000, 0.0000069608, 0.0000408285, 0.0000488369, -0.0000705569,
    -0.0002973525, -0.0003778328, 0.0000000000, 0.0007846933, 0.0013122989,
    0.0006977618, -0.0012003576, -0.0030837016, -0.0028101916, 0.0006549086,
    0.0054288729, 0.0071706868, 0.0024680210, -0.0070862006, -0.0141376403,
    -0.0104154964, 0.0053490740, 0.0230700279, 0.0262041970, 0.0048167561,
    -0.0321940211, -0.0567760541, -0.0371881136, 0.0390924893, 0.1511686694,
    0.2514823967, 0.2916800788, 0.2514823967, 0.1511686694, 0.0390924893,
    -0.0371881136, -0.0567760541, -0.0321940211, 0.0048167561, 0.0262041970,
    0.0230700279, 0.0053490740, -0.0104154964, -0.0141376403, -0.0070862006,
    0.0024680210, 0.0071706868, 0.0054288729, 0.0006549086, -0.0028101916,
    -0.0030837016, -0.0012003576, 0.0006977618, 0.0013122989, 0.0007846933,
    0.0000000000, -0.0003778328, -0.0002973525, -0.0000705569, 0.0000488369,
    0.0000408285, 0.0000069608, 0.0000000000,
];

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioLevels {
    pub microphone: Option<f32>,
    pub system: Option<f32>,
}

#[derive(Serialize)]
pub struct AudioDevice {
    id: String,
    label: String,
}

#[derive(Serialize)]
pub struct AudioDevices {
    microphones: Vec<AudioDevice>,
    monitors: Vec<AudioDevice>,
}

enum CaptureMessage {
    MicBlock([f32; BLOCK_SAMPLES], usize),
    SystemBlock([f32; BLOCK_SAMPLES], usize),
    Fault(String),
}

pub struct LiveCapture {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl LiveCapture {
    pub fn start(
        source: AudioSource,
        microphone: &str,
        monitor: &str,
        denoise_mic: bool,
        paused: Arc<AtomicBool>,
        mut on_pcm16: impl FnMut(Vec<u8>) + Send + 'static,
        mut on_levels: impl FnMut(AudioLevels) + Send + 'static,
        on_error: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let want_mic = matches!(source, AudioSource::Both | AudioSource::Microphone);
        let want_sys = matches!(source, AudioSource::Both | AudioSource::System);
        if want_mic && microphone.trim().is_empty() {
            return Err("Select a microphone in Settings before listening.".into());
        }
        if want_sys && monitor.trim().is_empty() {
            return Err("Select a system monitor in Settings before listening.".into());
        }

        let (capture_tx, capture_rx) = mpsc::sync_channel(QUEUE_BLOCKS);
        let (stop_tx, stop_rx) = mpsc::channel();
        let on_error: Arc<dyn Fn(String) + Send + Sync> = Arc::new(on_error);
        let faulted = Arc::new(AtomicBool::new(false));
        let report_error = {
            let on_error = on_error.clone();
            let faulted = faulted.clone();
            move |msg: String| {
                if !faulted.swap(true, Ordering::SeqCst) {
                    on_error(msg);
                }
            }
        };

        let (mic_rate, mic_stream) = if want_mic {
            let (rate, stream) = open_microphone_stream_cb(microphone, capture_tx.clone())?;
            (rate, Some(stream))
        } else {
            (TARGET_RATE, None)
        };

        let (child, monitor_reader) = if want_sys {
            let sources = pactl(&["list", "short", "sources"])?;
            let source_matches = sources
                .lines()
                .filter(|line| line.split_whitespace().nth(1) == Some(monitor))
                .count();
            if source_matches != 1 || !monitor.ends_with(".monitor") {
                return Err(format!("System monitor source is missing or invalid: {monitor}"));
            }
            let mut child = Command::new("parec")
                .args(["--device", monitor, "--format=float32le", "--rate=48000", "--channels=2", "--latency-msec=20"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Could not start system monitor capture with parec: {e}"))?;
            let stdout = child.stdout.take().ok_or_else(|| {
                let _ = child.kill();
                let _ = child.wait();
                "parec did not provide capture output".to_string()
            })?;
            let reporter = report_error.clone();
            let reader = match spawn_monitor_reader_cb(stdout, capture_tx, reporter) {
                Ok(reader) => reader,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            (Some(ChildCapture(child)), Some(reader))
        } else {
            (None, None)
        };

        if let Some(stream) = &mic_stream
            && let Err(error) = stream.play()
        {
            drop(child);
            if let Some(reader) = monitor_reader {
                let _ = reader.join();
            }
            return Err(format!("Could not start microphone capture: {error}"));
        }

        let worker = thread::Builder::new()
            .name("audio-live-pipeline".into())
            .spawn(move || {
                live_capture_loop(
                    capture_rx,
                    stop_rx,
                    paused,
                    mic_stream,
                    child,
                    monitor_reader,
                    mic_rate,
                    want_mic,
                    want_sys,
                    denoise_mic,
                    &mut on_pcm16,
                    &mut on_levels,
                    &report_error,
                );
            })
            .map_err(|e| format!("Could not start live audio controller: {e}"))?;

        Ok(Self { stop: stop_tx, worker: Some(worker) })
    }
}

impl Drop for LiveCapture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn live_capture_loop(
    receiver: Receiver<CaptureMessage>,
    stop: Receiver<()>,
    paused: Arc<AtomicBool>,
    mic_stream: Option<cpal::Stream>,
    mut child: Option<ChildCapture>,
    reader: Option<JoinHandle<()>>,
    mic_rate: u32,
    want_mic: bool,
    want_sys: bool,
    denoise_mic: bool,
    on_pcm16: &mut impl FnMut(Vec<u8>),
    on_levels: &mut impl FnMut(AudioLevels),
    report_error: &impl Fn(String),
) {
    let mut mic_resampler = LinearResampler::new(mic_rate, TARGET_RATE);
    let mut mic_denoiser = denoise_mic.then(MicDenoiser::new);
    let mut mic_decimator = FirDecimator3::new();
    let mut sys_decimator = FirDecimator3::new();
    let mut mic_16k = VecDeque::<f32>::with_capacity(CHUNK_FRAMES_16K * 4);
    let mut sys_16k = VecDeque::<f32>::with_capacity(CHUNK_FRAMES_16K * 4);
    let mut mic_rms_acc = RmsAccumulator::default();
    let mut sys_rms_acc = RmsAccumulator::default();
    let mut last_levels = Instant::now();

    'outer: loop {
        if let Some(ref mut proc) = child {
            match proc.0.try_wait() {
                Ok(Some(status)) => {
                    report_error(format!("System monitor process exited unexpectedly: {status}"));
                    break;
                }
                Err(error) => {
                    report_error(format!("Could not monitor system audio process: {error}"));
                    break;
                }
                Ok(None) => {}
            }
        }

        match stop.recv_timeout(Duration::from_millis(10)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let is_paused = paused.load(Ordering::Relaxed);
        loop {
            match receiver.try_recv() {
                Ok(CaptureMessage::MicBlock(buf, len)) => {
                    if is_paused {
                        continue;
                    }
                    let at_48k = mic_resampler.push(&buf[..len]);
                    let clean = match mic_denoiser.as_mut() {
                        Some(denoiser) => denoiser.process(&at_48k),
                        None => at_48k,
                    };
                    let at_16k = mic_decimator.push(&clean);
                    mic_rms_acc.push(&at_16k);
                    mic_16k.extend(at_16k);
                }
                Ok(CaptureMessage::SystemBlock(buf, len)) => {
                    if is_paused {
                        continue;
                    }
                    let at_16k = sys_decimator.push(&buf[..len]);
                    sys_rms_acc.push(&at_16k);
                    sys_16k.extend(at_16k);
                }
                Ok(CaptureMessage::Fault(err)) => {
                    report_error(err);
                    break 'outer;
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        if is_paused {
            mic_16k.clear();
            sys_16k.clear();
        } else {
            emit_ready_chunks(want_mic, want_sys, &mut mic_16k, &mut sys_16k, on_pcm16);
        }

        if last_levels.elapsed() >= LEVEL_INTERVAL {
            last_levels = Instant::now();
            on_levels(AudioLevels {
                microphone: want_mic.then(|| if is_paused { 0.0 } else { mic_rms_acc.take() }),
                system: want_sys.then(|| if is_paused { 0.0 } else { sys_rms_acc.take() }),
            });
        }
    }

    drop(mic_stream);
    if let Some(mut proc) = child.take() {
        let _ = proc.0.kill();
        let _ = proc.0.wait();
    }
    if let Some(reader) = reader {
        let _ = reader.join();
    }
}

fn emit_ready_chunks(
    want_mic: bool,
    want_sys: bool,
    mic_16k: &mut VecDeque<f32>,
    sys_16k: &mut VecDeque<f32>,
    on_pcm16: &mut impl FnMut(Vec<u8>),
) {
    loop {
        let mic_ready = !want_mic || mic_16k.len() >= CHUNK_FRAMES_16K;
        let sys_ready = !want_sys || sys_16k.len() >= CHUNK_FRAMES_16K;
        let mic_leading = want_mic && want_sys && mic_16k.len() >= CHUNK_FRAMES_16K + MAX_LAG_FRAMES_16K;
        let sys_leading = want_mic && want_sys && sys_16k.len() >= CHUNK_FRAMES_16K + MAX_LAG_FRAMES_16K;
        if !(mic_ready && sys_ready) && !mic_leading && !sys_leading {
            break;
        }
        let mut mic_frame = [0.0f32; CHUNK_FRAMES_16K];
        let mut sys_frame = [0.0f32; CHUNK_FRAMES_16K];
        if want_mic {
            for slot in &mut mic_frame {
                *slot = mic_16k.pop_front().unwrap_or(0.0);
            }
        }
        if want_sys {
            for slot in &mut sys_frame {
                *slot = sys_16k.pop_front().unwrap_or(0.0);
            }
        }
        on_pcm16(interleave_stereo_pcm16(&mic_frame, &sys_frame));
    }
}

pub fn interleave_stereo_pcm16(left: &[f32], right: &[f32]) -> Vec<u8> {
    let frames = left.len().max(right.len());
    let mut out = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let l = f32_to_i16(*left.get(i).unwrap_or(&0.0));
        let r = f32_to_i16(*right.get(i).unwrap_or(&0.0));
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    out
}

fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    if clamped < 0.0 {
        (clamped * 32768.0).round() as i16
    } else {
        (clamped * 32767.0).round() as i16
    }
}

#[derive(Default)]
struct RmsAccumulator {
    sum_sq: f64,
    count: usize,
}

impl RmsAccumulator {
    fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            let v = s as f64;
            self.sum_sq += v * v;
            self.count += 1;
        }
    }

    fn take(&mut self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let rms = (self.sum_sq / self.count as f64).sqrt() as f32;
        self.sum_sq = 0.0;
        self.count = 0;
        (rms * 4.0).min(1.0)
    }
}

pub struct LinearResampler {
    from: u32,
    to: u32,
    pos: f64,
    carry: Option<f32>,
}

impl LinearResampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { from, to, pos: 0.0, carry: None }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        if self.from == self.to {
            return input.to_vec();
        }
        let step = self.from as f64 / self.to as f64;
        let mut combined = Vec::with_capacity(input.len() + 1);
        let offset = if let Some(prev) = self.carry {
            combined.push(prev);
            1.0
        } else {
            0.0
        };
        combined.extend_from_slice(input);
        let limit = (combined.len() - 1) as f64;
        let mut pos = self.pos + offset;
        let mut out = Vec::with_capacity(((input.len() as f64 / step).ceil() as usize) + 1);
        while pos < limit {
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = combined[idx];
            let b = combined[idx + 1];
            out.push(a + (b - a) * frac);
            pos += step;
        }
        self.pos = pos - limit;
        self.carry = input.last().copied();
        out
    }
}

pub struct FirDecimator3 {
    history: [f32; FIR_TAPS - 1],
    phase: usize,
}

impl Default for FirDecimator3 {
    fn default() -> Self {
        Self::new()
    }
}

impl FirDecimator3 {
    pub fn new() -> Self {
        Self { history: [0.0; FIR_TAPS - 1], phase: 0 }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut work = Vec::with_capacity(self.history.len() + input.len());
        work.extend_from_slice(&self.history);
        work.extend_from_slice(input);

        let mut out = Vec::with_capacity(input.len() / 3 + 1);
        let start = self.phase;
        let mut idx = start;
        while idx < input.len() {
            let window = &work[idx..idx + FIR_TAPS];
            let acc: f32 = window.iter().zip(FIR_COEFFICIENTS.iter()).map(|(s, c)| s * c).sum();
            out.push(acc);
            idx += 3;
        }
        self.phase = idx - input.len();
        let tail_start = work.len() - (FIR_TAPS - 1);
        self.history.copy_from_slice(&work[tail_start..]);
        out
    }
}

struct MicDenoiser {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    pending: Vec<f32>,
    warmed: bool,
}

impl MicDenoiser {
    fn new() -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            pending: Vec::with_capacity(nnnoiseless::FRAME_SIZE * 2),
            warmed: false,
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        const N: usize = nnnoiseless::FRAME_SIZE;
        self.pending.extend_from_slice(input);
        let mut out = Vec::with_capacity(self.pending.len());
        let mut in_frame = [0.0f32; N];
        let mut out_frame = [0.0f32; N];
        while self.pending.len() >= N {
            for (dst, src) in in_frame.iter_mut().zip(self.pending.drain(..N)) {
                *dst = src * 32768.0;
            }
            self.state.process_frame(&mut out_frame, &in_frame);
            if !self.warmed {
                self.warmed = true;
                out.extend(std::iter::repeat_n(0.0, N));
            } else {
                out.extend(out_frame.iter().map(|s| s / 32768.0));
            }
        }
        out
    }
}

fn open_microphone_stream_cb(
    requested: &str,
    sender: SyncSender<CaptureMessage>,
) -> Result<(u32, cpal::Stream), String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| format!("Could not enumerate microphones: {e}"))?;
    let mut matches = devices.filter_map(|device| match device.name() {
        Ok(name) if name == requested => Some(Ok(device)),
        Ok(_) => None,
        Err(error) => Some(Err(error)),
    });
    let device = matches
        .next()
        .ok_or_else(|| format!("Microphone not found: {requested}"))?
        .map_err(|error| format!("Could not read microphone name: {error}"))?;
    if matches.next().is_some() {
        return Err(format!("Microphone name is ambiguous: {requested}"));
    }
    let supported = device.default_input_config().map_err(|e| format!("Microphone has no input configuration: {e}"))?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    if channels == 0 {
        return Err("Microphone reports zero channels".into());
    }
    let config: cpal::StreamConfig = supported.clone().into();
    let err_tx = sender.clone();
    let stream_error = move |error| {
        let _ = err_tx.try_send(CaptureMessage::Fault(format!("Microphone capture failed: {error}")));
    };
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_mic_stream_cb::<f32, _>(&device, &config, channels, sender, stream_error)?,
        cpal::SampleFormat::I16 => build_mic_stream_cb::<i16, _>(&device, &config, channels, sender, stream_error)?,
        cpal::SampleFormat::U16 => build_mic_stream_cb::<u16, _>(&device, &config, channels, sender, stream_error)?,
        format => return Err(format!("Microphone sample format is unsupported: {format:?}")),
    };
    Ok((rate, stream))
}

fn build_mic_stream_cb<T, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sender: SyncSender<CaptureMessage>,
    on_error: E,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Copy,
    f32: FromSample<T>,
    E: FnMut(cpal::StreamError) + Send + 'static,
{
    let mut block = [0.0f32; BLOCK_SAMPLES];
    let mut len = 0usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for frame in data.chunks_exact(channels) {
                    let mono = frame.iter().map(|sample| f32::from_sample(*sample)).sum::<f32>() / channels as f32;
                    block[len] = mono;
                    len += 1;
                    if len == BLOCK_SAMPLES {
                        len = 0;
                        if sender.try_send(CaptureMessage::MicBlock(block, BLOCK_SAMPLES)).is_err() {
                            return;
                        }
                    }
                }
                if len > 0 {
                    let tail = len;
                    len = 0;
                    let _ = sender.try_send(CaptureMessage::MicBlock(block, tail));
                }
            },
            on_error,
            None,
        )
        .map_err(|e| format!("Could not build microphone capture stream: {e}"))
}

fn spawn_monitor_reader_cb(
    mut stdout: impl Read + Send + 'static,
    sender: SyncSender<CaptureMessage>,
    report_error: impl Fn(String) + Send + Sync + 'static,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("audio-monitor-reader".into())
        .spawn(move || {
            let mut bytes = [0u8; 8192];
            let mut pending = Vec::with_capacity(7);
            let mut block = [0.0f32; BLOCK_SAMPLES];
            let mut len = 0usize;
            loop {
                match stdout.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(length) => {
                        pending.extend_from_slice(&bytes[..length]);
                        let usable = pending.len() / 8 * 8;
                        for frame in pending[..usable].chunks_exact(8) {
                            let left = f32::from_le_bytes(frame[0..4].try_into().unwrap());
                            let right = f32::from_le_bytes(frame[4..8].try_into().unwrap());
                            block[len] = (left + right) * 0.5;
                            len += 1;
                            if len == BLOCK_SAMPLES {
                                if sender.try_send(CaptureMessage::SystemBlock(block, BLOCK_SAMPLES)).is_err() {
                                    report_error("Audio capture queue overflowed".into());
                                    return;
                                }
                                len = 0;
                            }
                        }
                        pending.drain(..usable);
                    }
                    Err(error) => {
                        report_error(format!("System monitor capture read failed: {error}"));
                        return;
                    }
                }
            }
            if len > 0 && sender.try_send(CaptureMessage::SystemBlock(block, len)).is_err() {
                report_error("Audio capture queue overflowed".into());
            }
        })
        .map_err(|e| format!("Could not start system monitor reader: {e}"))
}

pub fn devices() -> Result<AudioDevices, String> {
    let host = cpal::default_host();
    let inputs = host.input_devices().map_err(|e| format!("Could not enumerate microphones: {e}"))?;
    let mut microphones = Vec::new();
    for device in inputs {
        let id = device.name().map_err(|e| format!("Could not read microphone name: {e}"))?;
        microphones.push(AudioDevice { label: id.clone(), id });
    }
    let sources = pactl(&["list", "short", "sources"])?;
    let monitors = sources
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|source| source.ends_with(".monitor"))
        .map(|source| AudioDevice { id: source.to_string(), label: source.to_string() })
        .collect();
    Ok(AudioDevices { microphones, monitors })
}

struct ChildCapture(Child);

impl Drop for ChildCapture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl").args(args).output().map_err(|e| format!("Could not run pactl: {e}"))?;
    if !output.status.success() {
        return Err(format!("pactl {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("pactl output was not UTF-8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn fir_decimator_preserves_speech_band_and_rejects_12khz() {
        let n = 4800; // 100 ms at 48 kHz
        let low: Vec<f32> = (0..n).map(|i| (TAU * 1000.0 * i as f32 / 48_000.0).sin() * 0.5).collect();
        let high: Vec<f32> = (0..n).map(|i| (TAU * 12_000.0 * i as f32 / 48_000.0).sin() * 0.5).collect();
        let mut d_low = FirDecimator3::new();
        let mut d_high = FirDecimator3::new();
        let out_low = d_low.push(&low);
        let out_high = d_high.push(&high);
        assert_eq!(out_low.len(), 1600);
        let rms = |xs: &[f32]| (xs.iter().skip(100).map(|&x| (x * x) as f64).sum::<f64>() / (xs.len() - 100) as f64).sqrt();
        assert!(rms(&out_low) > 0.30, "1 kHz tone should pass: {}", rms(&out_low));
        assert!(rms(&out_high) < 0.002, "12 kHz tone should be attenuated: {}", rms(&out_high));
    }

    #[test]
    fn interleave_keeps_mic_left_and_system_right() {
        let bytes = interleave_stereo_pcm16(&[0.5, -0.5], &[-0.25, 0.25]);
        assert_eq!(bytes.len(), 8);
        let l0 = i16::from_le_bytes(bytes[0..2].try_into().unwrap());
        let r0 = i16::from_le_bytes(bytes[2..4].try_into().unwrap());
        let l1 = i16::from_le_bytes(bytes[4..6].try_into().unwrap());
        let r1 = i16::from_le_bytes(bytes[6..8].try_into().unwrap());
        assert!(l0 > 15_000 && r0 < -7_000);
        assert!(l1 < -15_000 && r1 > 7_000);
    }

    #[test]
    fn missing_selected_source_fails_explicitly() {
        let err = LiveCapture::start(
            AudioSource::Both,
            "",
            "sink.monitor",
            true,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
            |_| {},
        )
        .err()
        .unwrap();
        assert!(err.contains("microphone"), "{err}");

        let err_sys = LiveCapture::start(
            AudioSource::System,
            "",
            "",
            true,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
            |_| {},
        )
        .err()
        .unwrap();
        assert!(err_sys.contains("system monitor"), "{err_sys}");
    }

    #[test]
    fn linear_resampler_produces_expected_length_across_chunks() {
        let mut r = LinearResampler::new(44_100, 48_000);
        let chunk: Vec<f32> = (0..4410).map(|i| (TAU * 440.0 * i as f32 / 44_100.0).sin()).collect();
        let a = r.push(&chunk);
        let b = r.push(&chunk);
        let total = a.len() + b.len();
        assert!((9595..=9605).contains(&total), "unexpected resampled len: {total}");
    }
}

