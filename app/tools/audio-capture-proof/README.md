# Native audio capture proof

Client-owned PipeWire filter that carries the selected microphone and the
selected monitor in one graph callback and writes interleaved 16 kHz stereo
PCM16: microphone left, monitor right. This is a proof tool, not the shipped
capture path.

## What it demonstrates

- Both sources are opened by one filter node, so every graph cycle delivers a
  pair of buffers that the writer interleaves in lockstep.
- A 63-tap low-pass FIR decimates 48 kHz graph audio to 16 kHz. Each channel
  retains its own filter history, with a shared output phase across callbacks.
  The filter adds approximately 0.65 ms of latency to both channels.
- A missing or disconnected source leaves its channel silent and emits a
  `capture_missing` line; the other channel keeps flowing.
- Sources that never appear are reported after two seconds, not only when a
  running source disappears.
- The process callback allocates nothing and never blocks. Records are published
  through a fixed-size single-producer/single-consumer ring (`256` blocks of
  `4088` bytes, about 16 seconds of output) and drained by a writer thread.
- The callback reports timeline gaps, overlaps, `XRUN_RECOVER` cycles, and ring
  drops:
  `gap_events`, `gap_frames`, `overlap_events`, `overlap_frames`,
  `xrun_cycles`, `dropped_blocks`, `dropped_frames`, `overrun_blocks`.
  During capture, `capture_clock_discontinuity` reports unexpected graph-clock
  positions and `capture_xrun` reports PipeWire XRUN recovery flags. Events
  can be dropped if the bounded ring is full; the final counters expose drops.
  Position discontinuities are not verified counts of lost audio: a large
  jump was observed after relinking, but its cause is unknown. `xrun_cycles`
  counts only cycles marked `XRUN_RECOVER` by PipeWire.

## Build

The `pipewire` crate needs the PipeWire development headers (`pipewire-devel`
on Fedora). With the headers installed, build with:

```sh
cargo build --release --locked
```

## Run

```sh
./target/release/audio-capture-proof /path/to/capture.raw 10 \
  <microphone-node>:capture_FL <monitor-node>:monitor_FL
```

The probe creates `byok-filter-proof` with input ports `mic` and `monitor`.
Find exact source names with `pw-link -o`. Explicit source names are resolved
and linked by the probe; missing or ambiguous names fail without falling back
to another source. Omit both source arguments to link manually with `pw-link`.
Output contains microphone audio and must be treated as sensitive.

## Alignment check

`sync-test.sh` plays one deterministic chirp through an isolated null sink and
links it to both filter ports. It then runs the bundled `check_sync.py`
(override with `CHECK_SYNC`) and reports each channel's capture offset.

```sh
./sync-test.sh /tmp/byok-1/audio-proof/gate/sync
```

A pass is the same capture offset on both channels. The check does not measure
physical device or acoustic path latency.

## Limits

- The caller must select source ports; automatic device discovery and
  reconnection are left to the future application.
- The proof does not determine whether a hot input was clipped by the device,
  the graph, or the probe.
- Physical microphone-versus-monitor latency could not be measured reliably on
  the test host; the headset path did not capture the reference chirp.
