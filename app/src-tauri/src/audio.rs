use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const TARGET_RATE: u32 = 48_000;
const MAX_CAPTURE_SAMPLES: usize = TARGET_RATE as usize * 60 * 10;
const QUEUE_BLOCKS: usize = 128;
const BLOCK_SAMPLES: usize = 1024;

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

enum CaptureSource {
    Microphone,
    System,
}

enum CaptureMessage {
    Samples(CaptureSource, Vec<f32>),
}

#[derive(Default)]
struct CaptureFault(Mutex<Option<String>>);

impl CaptureFault {
    fn fail(&self, app: &AppHandle, message: String) {
        let mut fault = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if fault.is_none() {
            let _ = app.emit("audio-error", message.clone());
            *fault = Some(message);
        }
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

enum RecordingState {
    Idle,
    Recording(Session),
}

pub struct AudioManager {
    session: Mutex<RecordingState>,
}

impl AudioManager {
    pub fn new() -> Self {
        Self { session: Mutex::new(RecordingState::Idle) }
    }
}

struct Session {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<Result<CapturedAudio, String>>>,
}

impl Session {
    fn stop(mut self) -> Result<CapturedAudio, String> {
        let _ = self.stop.send(());
        self.worker
            .take()
            .ok_or_else(|| "recording worker already stopped".to_string())?
            .join()
            .map_err(|_| "recording worker panicked".to_string())?
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct CapturedAudio {
    microphone: Vec<f32>,
    microphone_rate: u32,
    system: Vec<f32>,
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

pub fn start(app: AppHandle, manager: &AudioManager, microphone: String, monitor: String) -> Result<(), String> {
    let mut slot = manager.session.lock().map_err(|_| "Audio state is unavailable".to_string())?;
    if !matches!(&*slot, RecordingState::Idle) {
        return Err("A recording is already active".into());
    }
    let (capture_tx, capture_rx) = mpsc::sync_channel(QUEUE_BLOCKS);
    let (stop_tx, stop_rx) = mpsc::channel();
    let fault = Arc::new(CaptureFault::default());
    let (microphone_rate, mic_stream) = open_microphone_stream(&app, &microphone, capture_tx.clone(), fault.clone())?;
    let sources = pactl(&["list", "short", "sources"])?;
    let source_matches = sources
        .lines()
        .filter(|line| line.split_whitespace().nth(1) == Some(monitor.as_str()))
        .count();
    if source_matches != 1 || !monitor.ends_with(".monitor") {
        return Err(format!("System monitor source is invalid or ambiguous: {monitor}"));
    }
    let mut child = Command::new("parec")
        .args(["--device", &monitor, "--format=float32le", "--rate=48000", "--channels=2", "--latency-msec=20"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start system monitor capture with parec: {e}"))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        let _ = child.kill();
        let _ = child.wait();
        "parec did not provide capture output".to_string()
    })?;
    let monitor_reader = match spawn_monitor_reader(stdout, capture_tx.clone(), app.clone(), fault.clone()) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if let Err(error) = mic_stream.play() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = monitor_reader.join();
        return Err(format!("Could not start microphone capture: {error}"));
    }
    let child = ChildCapture(child);
    let worker_app = app.clone();
    let worker = thread::Builder::new()
        .name("audio-recording-controller".into())
        .spawn(move || {
            capture_loop(worker_app, capture_rx, stop_rx, fault, mic_stream, child, monitor_reader, microphone_rate)
        })
        .map_err(|e| format!("Could not start recording controller: {e}"))?;
    *slot = RecordingState::Recording(Session { stop: stop_tx, worker: Some(worker) });
    Ok(())
}

pub fn stop(manager: &AudioManager, app_data: Result<PathBuf, String>) -> Result<AudioPaths, String> {
    let session = {
        let mut state = manager.session.lock().map_err(|_| "Audio state is unavailable".to_string())?;
        match std::mem::replace(&mut *state, RecordingState::Idle) {
            RecordingState::Idle => return Err("No recording is active".into()),
            RecordingState::Recording(session) => session,
        }
    };
    let captured = session.stop()?;
    let app_data = app_data?;
    if captured.microphone.is_empty() || captured.system.is_empty() {
        return Err("Recording stopped without microphone and system audio samples".into());
    }
    let microphone = resample_linear(&captured.microphone, captured.microphone_rate, TARGET_RATE);
    let dir = private_recording_dir(app_data)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let microphone_path = dir.join(format!("microphone-{stamp}.wav"));
    let denoised_path = dir.join(format!("microphone-denoised-{stamp}.wav"));
    let system_path = dir.join(format!("system-{stamp}.wav"));
    write_wav(&microphone_path, &microphone, TARGET_RATE)?;
    let clean = denoise_48k(&microphone);
    write_wav(&denoised_path, &clean, TARGET_RATE)?;
    write_wav(&system_path, &captured.system, TARGET_RATE)?;
    Ok(AudioPaths {
        microphone_path: microphone_path.to_string_lossy().into_owned(),
        denoised_path: denoised_path.to_string_lossy().into_owned(),
        system_path: system_path.to_string_lossy().into_owned(),
    })
}

#[derive(Serialize)]
pub struct AudioPaths {
    microphone_path: String,
    denoised_path: String,
    system_path: String,
}

fn open_microphone_stream(
    app: &AppHandle,
    requested: &str,
    sender: SyncSender<CaptureMessage>,
    fault: Arc<CaptureFault>,
) -> Result<(u32, cpal::Stream), String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| format!("Could not enumerate microphones: {e}"))?;
    let mut matches = devices.filter_map(|device| match device.name() {
        Ok(name) if name == requested => Some(Ok(device)),
        Ok(_) => None,
        Err(error) => Some(Err(error)),
    });
    let device = matches.next().ok_or_else(|| format!("Microphone not found: {requested}"))?
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
    let stream_error = {
        let app = app.clone();
        let fault = fault.clone();
        move |error| fault.fail(&app, format!("Microphone capture failed: {error}"))
    };
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_mic_stream::<f32, _>(&device, &config, channels, sender, app.clone(), fault, stream_error)?,
        cpal::SampleFormat::I16 => build_mic_stream::<i16, _>(&device, &config, channels, sender, app.clone(), fault, stream_error)?,
        cpal::SampleFormat::U16 => build_mic_stream::<u16, _>(&device, &config, channels, sender, app.clone(), fault, stream_error)?,
        format => return Err(format!("Microphone sample format is unsupported: {format:?}")),
    };
    Ok((rate, stream))
}


fn build_mic_stream<T, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sender: SyncSender<CaptureMessage>,
    app: AppHandle,
    fault: Arc<CaptureFault>,
    on_error: E,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Copy,
    f32: FromSample<T>,
    E: FnMut(cpal::StreamError) + Send + 'static,
{
    let callback_app = app.clone();
    let callback_fault = fault.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut block = Vec::with_capacity(BLOCK_SAMPLES);
            for frame in data.chunks_exact(channels) {
                let mono = frame.iter().map(|sample| f32::from_sample(*sample)).sum::<f32>() / channels as f32;
                block.push(mono);
                if block.len() == BLOCK_SAMPLES {
                    if send_samples(&sender, CaptureSource::Microphone, std::mem::take(&mut block)).is_err() {
                        callback_fault.fail(&callback_app, "Audio capture queue overflowed".into());
                        return;
                    }
                    block = Vec::with_capacity(BLOCK_SAMPLES);
                }
            }
            if !block.is_empty() && send_samples(&sender, CaptureSource::Microphone, block).is_err() {
                callback_fault.fail(&callback_app, "Audio capture queue overflowed".into());
            }
        },
        on_error,
        None,
    ).map_err(|e| format!("Could not build microphone capture stream: {e}"))
}

fn send_samples(sender: &SyncSender<CaptureMessage>, source: CaptureSource, samples: Vec<f32>) -> Result<(), ()> {
    sender.try_send(CaptureMessage::Samples(source, samples)).map_err(|_| ())
}

fn spawn_monitor_reader(
    mut stdout: impl Read + Send + 'static,
    sender: SyncSender<CaptureMessage>,
    app: AppHandle,
    fault: Arc<CaptureFault>,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("audio-monitor-reader".into())
        .spawn(move || {
            let mut bytes = [0u8; 8192];
            let mut pending = Vec::with_capacity(7);
            let mut block = Vec::with_capacity(BLOCK_SAMPLES);
            loop {
                match stdout.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(length) => {
                        pending.extend_from_slice(&bytes[..length]);
                        let usable = pending.len() / 8 * 8;
                        for frame in pending[..usable].chunks_exact(8) {
                            let left = f32::from_le_bytes(frame[0..4].try_into().unwrap());
                            let right = f32::from_le_bytes(frame[4..8].try_into().unwrap());
                            block.push((left + right) * 0.5);
                            if block.len() == BLOCK_SAMPLES {
                                if send_samples(&sender, CaptureSource::System, std::mem::take(&mut block)).is_err() {
                                    fault.fail(&app, "Audio capture queue overflowed".into());
                                    return;
                                }
                                block = Vec::with_capacity(BLOCK_SAMPLES);
                            }
                        }
                        pending.drain(..usable);
                    }
                    Err(error) => {
                        fault.fail(&app, format!("System monitor capture read failed: {error}"));
                        return;
                    }
                }
            }
            if !block.is_empty() && send_samples(&sender, CaptureSource::System, block).is_err() {
                fault.fail(&app, "Audio capture queue overflowed".into());
            }
        })
        .map_err(|e| format!("Could not start system monitor reader: {e}"))
}

struct ChildCapture(Child);

impl Drop for ChildCapture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn capture_loop(
    app: AppHandle,
    receiver: Receiver<CaptureMessage>,
    stop: Receiver<()>,
    fault: Arc<CaptureFault>,
    mic_stream: cpal::Stream,
    mut child: ChildCapture,
    reader: JoinHandle<()>,
    microphone_rate: u32,
) -> Result<CapturedAudio, String> {
    let mut microphone = Vec::new();
    let mut system = Vec::new();
    let result = 'capture: loop {
        if let Some(error) = fault.take() {
            break Err(error);
        }
        match child.0.try_wait() {
            Ok(Some(status)) => {
                let error = format!("System monitor process exited unexpectedly: {status}");
                fault.fail(&app, error.clone());
                break Err(error);
            }
            Err(error) => {
                let error = format!("Could not monitor system audio process: {error}");
                fault.fail(&app, error.clone());
                break Err(error);
            }
            Ok(None) => {}
        }
        match stop.recv_timeout(Duration::from_millis(10)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        loop {
            match receiver.try_recv() {
                Ok(CaptureMessage::Samples(source, samples)) => {
                    let target = match source {
                        CaptureSource::Microphone => &mut microphone,
                        CaptureSource::System => &mut system,
                    };
                    if target.len().saturating_add(samples.len()) > MAX_CAPTURE_SAMPLES {
                        let error = "Recording exceeded the 10-minute capture limit".to_string();
                        fault.fail(&app, error.clone());
                        break 'capture Err(error);
                    }
                    target.extend(samples);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let error = "Audio capture streams stopped unexpectedly".to_string();
                    fault.fail(&app, error.clone());
                    break 'capture Err(error);
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if let Some(error) = fault.take() {
            break Err(error);
        }
    };
    drop(mic_stream);
    let _ = child.0.kill();
    let _ = child.0.wait();
    let _ = reader.join();
    result?;
    while let Ok(CaptureMessage::Samples(source, samples)) = receiver.try_recv() {
        let target = match source {
            CaptureSource::Microphone => &mut microphone,
            CaptureSource::System => &mut system,
        };
        if target.len().saturating_add(samples.len()) > MAX_CAPTURE_SAMPLES {
            return Err("Recording exceeded the 10-minute capture limit".into());
        }
        target.extend(samples);
    }
    if microphone.is_empty() || system.is_empty() {
        return Err("Capture ended without microphone or system audio samples".into());
    }
    Ok(CapturedAudio { microphone, microphone_rate, system })
}

fn private_recording_dir(app_data: PathBuf) -> Result<PathBuf, String> {
    let directory = app_data.join("recordings");
    fs::create_dir_all(&directory).map_err(|e| format!("Could not create private recording directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Could not secure recording directory: {e}"))?;
    }
    Ok(directory)
}

fn write_wav(path: &PathBuf, samples: &[f32], rate: u32) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|e| format!("Could not create WAV output: {e}"))?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::new(file, spec).map_err(|e| format!("Could not create WAV writer: {e}"))?;
    for sample in samples {
        writer.write_sample(*sample).map_err(|e| format!("Could not write WAV output: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("Could not finalize WAV output: {e}"))
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl").args(args).output().map_err(|e| format!("Could not run pactl: {e}"))?;
    if !output.status.success() {
        return Err(format!("pactl {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("pactl output was not UTF-8: {e}"))
}

fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let output_len = (input.len() as f64 / ratio) as usize;
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let sample_index = position as usize;
            let fraction = (position - sample_index as f64) as f32;
            let a = input[sample_index];
            let b = *input.get(sample_index + 1).unwrap_or(&a);
            a + (b - a) * fraction
        })
        .collect()
}

fn denoise_48k(input: &[f32]) -> Vec<f32> {
    use nnnoiseless::{DenoiseState, FRAME_SIZE};
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(input.len());
    let mut input_frame = [0.0; FRAME_SIZE];
    let mut output_frame = [0.0; FRAME_SIZE];
    let mut first = true;
    for chunk in input.chunks(FRAME_SIZE) {
        input_frame.fill(0.0);
        for (destination, sample) in input_frame.iter_mut().zip(chunk) {
            *destination = sample * 32768.0;
        }
        state.process_frame(&mut output_frame, &input_frame);
        if first {
            output.extend(std::iter::repeat_n(0.0, chunk.len()));
            first = false;
        } else {
            output.extend(output_frame[..chunk.len()].iter().map(|sample| sample / 32768.0));
        }
    }
    output
}

