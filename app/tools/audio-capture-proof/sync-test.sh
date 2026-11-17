#!/usr/bin/env bash
# Deliver one deterministic signal to both filter ports through an isolated
# null sink and verify that the two output channels are sample-aligned.
#
# usage: sync-test.sh OUTDIR
# The probe must be built first; see README.md.
set -euo pipefail

out=${1:?usage: sync-test.sh OUTDIR}
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
check=${CHECK_SYNC:-$script_dir/check_sync.py}
bin=${PROBE_BIN:-./target/release/audio-capture-proof}

[[ -x "$bin" ]] || { echo "probe not built: $bin" >&2; exit 1; }
[[ -f "$check" ]] || { echo "sync checker not found: $check" >&2; exit 1; }
mkdir -p "$out"

python3 - "$out" <<'PY'
import math
import struct
import sys
import wave

out = sys.argv[1]
rate = 16_000
frames = rate // 4  # 250 ms
samples = []
for index in range(frames):
    seconds = index / rate
    frequency = 300 + (6000 - 300) * (index / frames)
    samples.append(int(12_000 * math.sin(2 * math.pi * frequency * seconds) + 0.5))

with open(f"{out}/reference.raw", "wb") as handle:
    handle.write(struct.pack("<" + "h" * frames, *samples))

# Zero-order hold expands the reference to the 48 kHz playback rate.
playback = [sample for sample in samples for _ in range(3)]
with wave.open(f"{out}/playback.wav", "wb") as handle:
    handle.setnchannels(1)
    handle.setsampwidth(2)
    handle.setframerate(48_000)
    handle.writeframes(struct.pack("<" + "h" * len(playback), *playback))
PY

module=$(pactl load-module module-null-sink sink_name=byok_sync rate=48000 channels=2)
cleanup() { pactl unload-module "$module" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for _ in $(seq 1 50); do
    pactl list short sinks | grep -q byok_sync && break
    sleep 0.1
done

"$bin" "$out/capture.raw" 6 >"$out/probe.stdout" 2>"$out/probe.stderr" &
probe=$!
sleep 0.5
pw-link byok_sync:monitor_FL byok-filter-proof:mic
pw-link byok_sync:monitor_FL byok-filter-proof:monitor
sleep 0.05
paplay --device=byok_sync "$out/playback.wav"
wait "$probe"

python3 "$check" "$out/capture.raw" "$out/reference.raw" --max-lag-ms 500
