//! Client-owned PipeWire capture proof for the BYOK audio gate.
//!
//! One filter node carries a microphone input port and a monitor input port,
//! so both sources are delivered by the same graph cycle. Each source is
//! decimated from the 48 kHz graph rate to 16 kHz and written as interleaved
//! stereo PCM16: microphone left, monitor right.
//!
//! The process callback performs no allocation and never blocks. Audio and
//! channel-state records are published through a fixed-size sequence ring that
//! is drained by a writer thread. A source that disappears leaves its channel
//! silent while the other channel keeps flowing, and each transition is
//! reported on stderr.
//!
//! Usage: `audio-capture-proof OUTPUT.raw [SECONDS [MIC_NODE:PORT MONITOR_NODE:PORT]]`

mod links;

use pipewire as pw;
use pw::{spa, sys};
use std::{
    cell::{Cell, UnsafeCell},
    cmp::Ordering as CmpOrdering,
    ffi::{c_void, CStr},
    fs::File,
    io::{BufWriter, Write},
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const GRAPH_RATE: u32 = 48_000;
const OUT_RATE: u32 = 16_000;
const DECIMATE: usize = (GRAPH_RATE / OUT_RATE) as usize;
/// 63-tap Blackman-windowed sinc low-pass, normalized for unity DC gain.
/// The 7 kHz cutoff retains speech-band content while rejecting frequencies
/// above the 8 kHz Nyquist limit of the 16 kHz output.
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

const MAX_QUANTUM: usize = 8192;
const MAX_OUT_FRAMES: usize = (MAX_QUANTUM + DECIMATE - 1) / DECIMATE + 1;

/// Payload capacity of one ring block. A multiple of four so that a split
/// record never separates the two channels of a stereo PCM16 frame.
const BLOCK_DATA: usize = 4088;
/// Power-of-two block count; about 16 seconds of 16 kHz stereo output.
const RING_BLOCKS: usize = 256;
const RING_MASK: usize = RING_BLOCKS - 1;

const KIND_AUDIO: u8 = 0;
const KIND_STATE: u8 = 1;
const KIND_CLOCK: u8 = 2;
const KIND_XRUN: u8 = 3;

const MISSING_MIC: u8 = 1;
const MISSING_MONITOR: u8 = 2;
const BOTH_PRESENT: u8 = 0;

struct Block {
    seq: AtomicU64,
    len: AtomicU32,
    kind: AtomicU8,
    flags: AtomicU8,
    data: UnsafeCell<[u8; BLOCK_DATA]>,
}

impl Block {
    fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            len: AtomicU32::new(0),
            kind: AtomicU8::new(0),
            flags: AtomicU8::new(0),
            data: UnsafeCell::new([0; BLOCK_DATA]),
        }
    }
}

/// Bounded single-producer/single-consumer record ring.
///
/// The audio callback is the only producer and the writer thread is the only
/// consumer. The producer never overwrites a block before the consumer has
/// released it; when the ring is full it drops the record and counts it.
struct Ring {
    blocks: Box<[Block]>,
    write_seq: AtomicU64,
    read_seq: AtomicU64,
    dropped_blocks: AtomicU64,
    max_fill: AtomicU64,
    overrun_blocks: AtomicU64,
}

// Safety: the producer and consumer touch disjoint blocks, and ownership is
// handed over through the release/acquire sequence store in `push` and
// `pop_into`. No block is accessed by both threads at the same time.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    fn new() -> Self {
        Self {
            blocks: (0..RING_BLOCKS).map(|_| Block::new()).collect(),
            write_seq: AtomicU64::new(0),
            read_seq: AtomicU64::new(0),
            dropped_blocks: AtomicU64::new(0),
            max_fill: AtomicU64::new(0),
            overrun_blocks: AtomicU64::new(0),
        }
    }

    /// Producer side. Returns false when the record was dropped.
    fn push(&self, kind: u8, flags: u8, payload: &[u8]) -> bool {
        debug_assert!(payload.len() <= BLOCK_DATA);
        let seq = self.write_seq.load(Ordering::Relaxed);
        let used = seq.wrapping_sub(self.read_seq.load(Ordering::Acquire));
        if used >= RING_BLOCKS as u64 {
            self.dropped_blocks.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let block = &self.blocks[(seq as usize) & RING_MASK];
        // Safety: this block is owned by the producer until `seq` is published.
        unsafe {
            (&mut (*block.data.get()))[..payload.len()].copy_from_slice(payload);
        }
        block.len.store(payload.len() as u32, Ordering::Relaxed);
        block.kind.store(kind, Ordering::Relaxed);
        block.flags.store(flags, Ordering::Relaxed);
        block.seq.store(seq.wrapping_add(1), Ordering::Release);
        let next = seq.wrapping_add(1);
        self.write_seq.store(next, Ordering::Release);
        let fill = next.wrapping_sub(self.read_seq.load(Ordering::Relaxed));
        self.max_fill.fetch_max(fill, Ordering::Relaxed);
        true
    }

    /// Consumer side. Copies the payload into `out`, which must have capacity.
    fn pop_into(&self, out: &mut Vec<u8>) -> Option<(u8, u8)> {
        let seq = self.read_seq.load(Ordering::Relaxed);
        if self.write_seq.load(Ordering::Acquire) == seq {
            return None;
        }
        let block = &self.blocks[(seq as usize) & RING_MASK];
        if block.seq.load(Ordering::Acquire) != seq.wrapping_add(1) {
            // The producer cannot overwrite an unreleased block, so a
            // mismatch means the sequence accounting is broken. Resync and
            // count it rather than reading stale audio.
            self.overrun_blocks.fetch_add(1, Ordering::Relaxed);
            let write = self.write_seq.load(Ordering::Acquire);
            self.read_seq.store(write, Ordering::Release);
            return None;
        }
        let len = block.len.load(Ordering::Relaxed) as usize;
        out.clear();
        // Safety: the producer released this block and will not touch it again
        // until `read_seq` advances.
        out.extend_from_slice(unsafe { &(&*block.data.get())[..len] });
        let kind = block.kind.load(Ordering::Relaxed);
        let flags = block.flags.load(Ordering::Relaxed);
        self.read_seq.store(seq.wrapping_add(1), Ordering::Release);
        Some((kind, flags))
    }
}

struct Data {
    ports: [*mut c_void; 2],
    ring: Arc<Ring>,
    seen: Rc<Cell<u8>>,
    reported: Rc<Cell<u8>>,
    phase: usize,
    filter_history: [[f32; FIR_TAPS]; 2],
    filter_pos: usize,
    frames: u64,
    dropped_frames: u64,
    bad_rate: u64,
    gap_events: u64,
    gap_frames: u64,
    gap_max_frames: u64,
    overlap_events: u64,
    overlap_frames: u64,
    overlap_max_frames: u64,
    /// (previous position, previous duration, observed position) of the most
    /// recent discontinuity, so the numbers can be attributed after the run.
    last_discontinuity: [u64; 3],
    xrun_cycles: u64,
    cycle_restarts: u64,
    have_position: bool,
    last_position: u64,
    last_duration: u64,
    mic_peak: f32,
    mic_over_one: u64,
    mic_nonfinite: u64,
    monitor_peak: f32,
    monitor_over_one: u64,
    monitor_nonfinite: u64,
}

impl Data {
    fn new(ring: Arc<Ring>, seen: Rc<Cell<u8>>, reported: Rc<Cell<u8>>) -> Self {
        Self {
            ports: [ptr::null_mut(); 2],
            ring,
            seen,
            reported,
            phase: 0,
            filter_history: [[0.0; FIR_TAPS]; 2],
            filter_pos: 0,
            frames: 0,
            dropped_frames: 0,
            bad_rate: 0,
            gap_events: 0,
            gap_frames: 0,
            gap_max_frames: 0,
            overlap_events: 0,
            overlap_frames: 0,
            overlap_max_frames: 0,
            last_discontinuity: [0; 3],
            xrun_cycles: 0,
            cycle_restarts: 0,
            have_position: false,
            last_position: 0,
            last_duration: 0,
            mic_peak: 0.0,
            mic_over_one: 0,
            mic_nonfinite: 0,
            monitor_peak: 0.0,
            monitor_over_one: 0,
            monitor_nonfinite: 0,
        }
    }

    fn track_input(peak: &mut f32, over_one: &mut u64, nonfinite: &mut u64, value: f32) {
        if value.is_finite() {
            *peak = peak.max(value.abs());
            *over_one += u64::from(value.abs() >= 1.0);
        } else {
            *nonfinite += 1;
        }
    }
}

unsafe extern "C" fn process(data: *mut c_void, position: *mut spa::sys::spa_io_position) {
    let data = &mut *(data as *mut Data);
    let clock = &(*position).clock;
    if clock.rate.num != 1 || clock.rate.denom != GRAPH_RATE {
        data.bad_rate += 1;
        return;
    }

    if data.have_position {
        let expected = data.last_position.wrapping_add(data.last_duration);
        if clock.position != expected {
            let mut event = [0u8; 16];
            event[..8].copy_from_slice(&expected.to_le_bytes());
            event[8..].copy_from_slice(&clock.position.to_le_bytes());
            data.ring.push(KIND_CLOCK, 0, &event);
        }
        if clock.position < data.last_position {
            // The timeline restarted; do not report the reset as an overlap.
            data.cycle_restarts += 1;
        } else {
            match clock.position.cmp(&expected) {
                CmpOrdering::Greater => {
                    let missing = clock.position - expected;
                    data.gap_events += 1;
                    data.gap_frames += missing;
                    data.gap_max_frames = data.gap_max_frames.max(missing);
                    data.last_discontinuity = [data.last_position, data.last_duration, clock.position];
                }
                CmpOrdering::Less => {
                    let repeated = expected - clock.position;
                    data.overlap_events += 1;
                    data.overlap_frames += repeated;
                    data.overlap_max_frames = data.overlap_max_frames.max(repeated);
                    data.last_discontinuity = [data.last_position, data.last_duration, clock.position];
                }
                CmpOrdering::Equal => {}
            }
        }
    }
    data.last_position = clock.position;
    data.last_duration = clock.duration;
    data.have_position = true;
    if clock.flags & spa::sys::SPA_IO_CLOCK_FLAG_XRUN_RECOVER != 0 {
        data.xrun_cycles += 1;
        data.ring.push(KIND_XRUN, 0, &[]);
    }

    let n = clock.duration as usize;
    if n == 0 {
        return;
    }
    if n > MAX_QUANTUM {
        // The proof declares the quantum it supports instead of overflowing
        // its fixed output buffer.
        data.bad_rate += 1;
        return;
    }

    let left = sys::pw_filter_get_dsp_buffer(data.ports[0], n as u32) as *const f32;
    let right = sys::pw_filter_get_dsp_buffer(data.ports[1], n as u32) as *const f32;

    let mut mask = BOTH_PRESENT;
    if left.is_null() {
        mask |= MISSING_MIC;
    } else {
        data.seen.set(data.seen.get() | MISSING_MIC);
    }
    if right.is_null() {
        mask |= MISSING_MONITOR;
    } else {
        data.seen.set(data.seen.get() | MISSING_MONITOR);
    }
    if mask != data.reported.get() && data.ring.push(KIND_STATE, mask, &[]) {
        data.reported.set(mask);
    }

    let mut out = [0u8; MAX_OUT_FRAMES * 4];
    let mut out_len = 0usize;

    for i in 0..n {
        let mic = if left.is_null() { 0.0 } else { *left.add(i) };
        let monitor = if right.is_null() { 0.0 } else { *right.add(i) };

        if !left.is_null() {
            Data::track_input(
                &mut data.mic_peak,
                &mut data.mic_over_one,
                &mut data.mic_nonfinite,
                mic,
            );
        }
        if !right.is_null() {
            Data::track_input(
                &mut data.monitor_peak,
                &mut data.monitor_over_one,
                &mut data.monitor_nonfinite,
                monitor,
            );
        }

        data.filter_history[0][data.filter_pos] = mic;
        data.filter_history[1][data.filter_pos] = monitor;
        data.filter_pos += 1;
        if data.filter_pos == FIR_TAPS {
            data.filter_pos = 0;
        }


        data.phase += 1;
        if data.phase == DECIMATE {
            // filter_pos is the oldest sample after advancing the history ring.
            let mut history_index = data.filter_pos;
            let mut filtered = [0.0; 2];
            for coefficient in FIR_COEFFICIENTS {
                filtered[0] += coefficient * data.filter_history[0][history_index];
                filtered[1] += coefficient * data.filter_history[1][history_index];
                history_index += 1;
                if history_index == FIR_TAPS {
                    history_index = 0;
                }
            }
            for sample in filtered {
                let value = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                out[out_len..out_len + 2].copy_from_slice(&value.to_le_bytes());
                out_len += 2;
            }
            data.frames += 1;
            data.phase = 0;
        }
    }

    let mut offset = 0;
    while offset < out_len {
        let len = (out_len - offset).min(BLOCK_DATA);
        if !data.ring.push(KIND_AUDIO, 0, &out[offset..offset + len]) {
            data.dropped_frames += (len / 4) as u64;
        }
        offset += len;
    }
}

unsafe extern "C" fn state_changed(
    _: *mut c_void,
    _: sys::pw_filter_state,
    state: sys::pw_filter_state,
    error: *const i8,
) {
    if state == sys::pw_filter_state_PW_FILTER_STATE_ERROR {
        eprintln!(
            "filter_error={}",
            if error.is_null() {
                "unknown".into()
            } else {
                CStr::from_ptr(error).to_string_lossy()
            }
        );
    }
}

fn writer(ring: Arc<Ring>, path: String, done: Arc<AtomicBool>) -> std::io::Result<u64> {
    let mut file = BufWriter::new(File::create(path)?);
    let mut scratch = Vec::with_capacity(BLOCK_DATA);
    let mut bytes = 0u64;
    loop {
        match ring.pop_into(&mut scratch) {
            Some((KIND_AUDIO, _)) => {
                file.write_all(&scratch)?;
                bytes += scratch.len() as u64;
            }
            Some((KIND_STATE, flags)) => {
                eprintln!(
                    "capture_missing mic={} monitor={}",
                    flags & MISSING_MIC != 0,
                    flags & MISSING_MONITOR != 0
                );
            }
            Some((KIND_CLOCK, _)) => {
                let expected = u64::from_le_bytes(scratch[..8].try_into().unwrap());
                let observed = u64::from_le_bytes(scratch[8..16].try_into().unwrap());
                eprintln!("capture_clock_discontinuity expected={expected} observed={observed}");
            }
            Some((KIND_XRUN, _)) => eprintln!("capture_xrun"),
            Some((other, _)) => eprintln!("unknown_record={other}"),
            None => {
                if done.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    file.flush()?;
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: audio-capture-proof OUTPUT.raw [SECONDS [MIC_NODE:PORT MONITOR_NODE:PORT]]")?;
    let seconds: u64 = match args.next() {
        Some(value) => value.parse()?,
        None => 10,
    };
    if !(1..=3600).contains(&seconds) {
        return Err("SECONDS must be between 1 and 3600".into());
    }
    let sources = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(mic), Some(monitor), None) => {
            let parse = |value: String| -> Result<links::PortSelector, &'static str> {
                let (node, port) = value.rsplit_once(':').ok_or("source must be NODE:PORT")?;
                if node.is_empty() || port.is_empty() {
                    return Err("source must be NODE:PORT");
                }
                Ok(links::PortSelector::new(node, port))
            };
            Some([parse(mic)?, parse(monitor)?])
        }
        _ => return Err("usage: audio-capture-proof OUTPUT.raw [SECONDS [MIC_NODE:PORT MONITOR_NODE:PORT]]".into()),
    };
    let link_failed = Rc::new(Cell::new(false));

    let ring = Arc::new(Ring::new());
    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let ring = ring.clone();
        let done = done.clone();
        let path = path.clone();
        thread::spawn(move || writer(ring, path, done))
    };

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let seen = Rc::new(Cell::new(0u8));
    let reported = Rc::new(Cell::new(0u8));

    let mut data = Box::new(Data::new(ring.clone(), seen.clone(), reported.clone()));
    let events = sys::pw_filter_events {
        version: 1,
        destroy: None,
        state_changed: Some(state_changed),
        io_changed: None,
        param_changed: None,
        add_buffer: None,
        remove_buffer: None,
        process: Some(process),
        drained: None,
        command: None,
    };

    unsafe {
        let props = sys::pw_properties_new(
            c"media.type".as_ptr(),
            c"Audio".as_ptr(),
            c"media.category".as_ptr(),
            c"Capture".as_ptr(),
            c"node.name".as_ptr(),
            c"byok-filter-proof".as_ptr(),
            ptr::null::<i8>(),
        );
        let filter = sys::pw_filter_new_simple(
            mainloop.loop_().as_raw_ptr(),
            c"byok-filter-proof".as_ptr(),
            props,
            &events,
            (&mut *data as *mut Data).cast(),
        );
        if filter.is_null() {
            done.store(true, Ordering::Release);
            writer.join().map_err(|_| "writer panic")??;
            return Err("cannot create filter".into());
        }
        for (i, name) in [c"mic", c"monitor"].iter().enumerate() {
            let props = sys::pw_properties_new(
                c"format.dsp".as_ptr(),
                c"32 bit float mono audio".as_ptr(),
                c"port.name".as_ptr(),
                name.as_ptr(),
                ptr::null::<i8>(),
            );
            data.ports[i] = sys::pw_filter_add_port(
                filter,
                spa::sys::SPA_DIRECTION_INPUT,
                sys::pw_filter_port_flags_PW_FILTER_PORT_FLAG_MAP_BUFFERS,
                1,
                props,
                ptr::null_mut(),
                0,
            );
            if data.ports[i].is_null() {
                sys::pw_filter_destroy(filter);
                done.store(true, Ordering::Release);
                writer.join().map_err(|_| "writer panic")??;
                return Err("cannot create input port".into());
            }
        }
        if sys::pw_filter_connect(
            filter,
            sys::pw_filter_flags_PW_FILTER_FLAG_NONE,
            ptr::null_mut(),
            0,
        ) < 0
        {
            sys::pw_filter_destroy(filter);
            done.store(true, Ordering::Release);
            writer.join().map_err(|_| "writer panic")??;
            return Err("cannot connect filter".into());
        }
        let source_links = if let Some([mic, monitor]) = sources {
            let targets = [
                links::PortLink::new(mic, links::PortSelector::new("byok-filter-proof", "mic")),
                links::PortLink::new(monitor, links::PortSelector::new("byok-filter-proof", "monitor")),
            ];
            match links::SourceLinks::new(&mainloop, targets) {
                Ok(value) => Some(Rc::new(value)),
                Err(error) => {
                    sys::pw_filter_destroy(filter);
                    done.store(true, Ordering::Release);
                    writer.join().map_err(|_| "writer panic")??;
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        let link_timer = source_links.as_ref().map(|source_links| {
            let source_links = source_links.clone();
            let failed = link_failed.clone();
            let stop = mainloop.clone();
            mainloop.loop_().add_timer(move |_| {
                let status = source_links.status();
                eprintln!("capture_links={status:?}");
                if status != links::LinkStatus::Linked {
                    failed.set(true);
                    stop.quit();
                }
            })
        });
        if let Some(timer) = &link_timer {
            timer.update_timer(Some(Duration::from_secs(2)), None).into_result()?;
        }

        // Report a source that never appears, not only one that disappears
        // after capture has started.
        let startup_ring = ring.clone();
        let startup_seen = seen.clone();
        let startup_reported = reported.clone();
        let startup_timer = mainloop.loop_().add_timer(move |_| {
            let seen = startup_seen.get();
            let pending = !seen & (MISSING_MIC | MISSING_MONITOR) & !startup_reported.get();
            if pending != 0 && startup_ring.push(KIND_STATE, pending, &[]) {
                startup_reported.set(startup_reported.get() | pending);
            }
        });
        startup_timer
            .update_timer(Some(Duration::from_secs(2)), Some(Duration::from_secs(1)))
            .into_result()?;

        let stop = mainloop.clone();
        let timer = mainloop.loop_().add_timer(move |_| stop.quit());
        timer
            .update_timer(Some(Duration::from_secs(seconds)), None)
            .into_result()?;

        mainloop.run();
        if let Some(source_links) = &source_links {
            let status = source_links.status();
            if status != links::LinkStatus::Linked {
                eprintln!("capture_links={status:?}");
                link_failed.set(true);
            }
        }
        sys::pw_filter_destroy(filter);
    }

    eprintln!(
        "frames={} missing_state={} dropped_frames={} dropped_blocks={} overrun_blocks={} max_fill_blocks={} bad_rate={}",
        data.frames,
        data.reported.get(),
        data.dropped_frames,
        ring.dropped_blocks.load(Ordering::Relaxed),
        ring.overrun_blocks.load(Ordering::Relaxed),
        ring.max_fill.load(Ordering::Relaxed),
        data.bad_rate
    );
    eprintln!(
        "gap_events={} gap_frames={} gap_max_frames={} overlap_events={} overlap_frames={} overlap_max_frames={} xrun_cycles={} cycle_restarts={}",
        data.gap_events,
        data.gap_frames,
        data.gap_max_frames,
        data.overlap_events,
        data.overlap_frames,
        data.overlap_max_frames,
        data.xrun_cycles,
        data.cycle_restarts
    );
    eprintln!(
        "last_discontinuity_prev_position={} prev_duration={} observed_position={}",
        data.last_discontinuity[0], data.last_discontinuity[1], data.last_discontinuity[2]
    );
    eprintln!(
        "mic_input_peak={} mic_input_at_or_over_one={} mic_input_nonfinite={}",
        data.mic_peak, data.mic_over_one, data.mic_nonfinite
    );
    eprintln!(
        "monitor_input_peak={} monitor_input_at_or_over_one={} monitor_input_nonfinite={}",
        data.monitor_peak, data.monitor_over_one, data.monitor_nonfinite
    );

    drop(data);
    done.store(true, Ordering::Release);
    let bytes = writer.join().map_err(|_| "writer panic")??;
    eprintln!("written_bytes={bytes}");
    if link_failed.get() {
        return Err("selected capture sources could not be linked".into());
    }
    Ok(())
}
