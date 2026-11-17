#!/usr/bin/env python3
"""Check stereo PCM16 routing and sample alignment against a known signal."""

import argparse
import array
import math
import sys
from dataclasses import dataclass

RATE = 16_000
SAMPLE_BYTES = 2
MAX_CLIPPED_SAMPLE = 32_760
MIN_RMS = 8.0
MAX_REFERENCE_SAMPLES = RATE
MAX_DOT_PRODUCTS = 100_000_000
AMBIGUOUS_PEAK_GAP = 0.01
PEAK_EXCLUSION_SAMPLES = RATE // 100


class InputError(Exception):
    pass


@dataclass(frozen=True)
class Match:
    correlation: float
    lag: int
    second_peak: float


def read_pcm16(path):
    try:
        with open(path, "rb") as source:
            raw = source.read()
    except OSError as error:
        raise InputError(f"cannot read {path}: {error}") from error
    if not raw:
        raise InputError(f"{path} is empty")
    if len(raw) % SAMPLE_BYTES:
        raise InputError(f"{path} has a partial PCM16 sample")
    samples = array.array("h")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    return samples


def read_inputs(capture_path, reference_path):
    capture = read_pcm16(capture_path)
    reference = read_pcm16(reference_path)
    if len(capture) % 2:
        raise InputError("capture must contain complete interleaved stereo frames")
    if len(reference) > MAX_REFERENCE_SAMPLES:
        raise InputError("reference must be at most 1 second at 16 kHz")
    if len(reference) < RATE // 20:
        raise InputError("reference must be at least 50 ms at 16 kHz")

    clipped = [name for name, samples in (("capture", capture), ("reference", reference))
               if any(abs(sample) >= MAX_CLIPPED_SAMPLE for sample in samples)]
    if clipped:
        raise InputError("clipped PCM detected in " + " and ".join(clipped))

    left = array.array("h", capture[0::2])
    right = array.array("h", capture[1::2])
    for name, channel in (("left", left), ("right", right)):
        rms = math.sqrt(sum(sample * sample for sample in channel) / len(channel))
        if rms < MIN_RMS:
            raise InputError(f"{name} capture channel is silent or near-silent (RMS {rms:.1f})")
    if not any(reference):
        raise InputError("reference has no signal")
    return left, right, reference


def prefixes(samples):
    sums = [0]
    squares = [0]
    total = square_total = 0
    for sample in samples:
        total += sample
        square_total += sample * sample
        sums.append(total)
        squares.append(square_total)
    return sums, squares


def correlation_at(capture, capture_prefix, reference, reference_prefix, lag):
    overlap = min(len(reference), len(capture) - lag)
    if overlap <= 0:
        return 0.0
    cap_sum, cap_square = capture_prefix
    ref_sum, ref_square = reference_prefix
    sum_x = cap_sum[lag + overlap] - cap_sum[lag]
    sum_y = ref_sum[overlap]
    square_x = cap_square[lag + overlap] - cap_square[lag]
    square_y = ref_square[overlap]
    dot = 0
    for index in range(overlap):
        dot += capture[lag + index] * reference[index]
    covariance = dot - (sum_x * sum_y / overlap)
    variance_x = square_x - (sum_x * sum_x / overlap)
    variance_y = square_y - (sum_y * sum_y / overlap)
    if variance_x <= 0 or variance_y <= 0:
        return 0.0
    return covariance / math.sqrt(variance_x * variance_y)


def match_channel(capture, reference, max_lag):
    capture_prefix = prefixes(capture)
    reference_prefix = prefixes(reference)
    last_lag = min(max_lag, len(capture) - len(reference))
    if last_lag < 0:
        raise InputError("capture is shorter than the reference")
    scores = []
    for lag in range(last_lag + 1):
        scores.append((abs(correlation_at(capture, capture_prefix, reference,
                                          reference_prefix, lag)), lag))
    scores.sort(reverse=True)
    best_score, best_lag = scores[0]
    exclusion = max(2, PEAK_EXCLUSION_SAMPLES)
    second_score = max((score for score, lag in scores if abs(lag - best_lag) > exclusion),
                       default=0.0)
    signed_score = correlation_at(capture, capture_prefix, reference,
                                  reference_prefix, best_lag)
    return Match(signed_score, best_lag, second_score)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Correlate a known mono PCM16 signal with each stereo capture channel.")
    parser.add_argument("capture", help="16 kHz interleaved stereo little-endian PCM16 file")
    parser.add_argument("reference", help="known signal as 16 kHz mono little-endian PCM16")
    parser.add_argument("--max-lag-ms", type=float, default=150.0,
                        help="largest non-negative capture offset to search (default: 150 ms)")
    parser.add_argument("--min-correlation", type=float, default=0.5,
                        help="minimum absolute Pearson correlation for a source match (default: 0.5)")
    return parser.parse_args()


def main():
    args = parse_args()
    if not math.isfinite(args.max_lag_ms) or args.max_lag_ms < 0:
        raise InputError("--max-lag-ms must be a finite non-negative number")
    if not math.isfinite(args.min_correlation) or not 0 < args.min_correlation <= 1:
        raise InputError("--min-correlation must be greater than 0 and at most 1")
    max_lag = round(args.max_lag_ms * RATE / 1000)

    left, right, reference = read_inputs(args.capture, args.reference)
    estimated_products = 2 * (max_lag + 1) * len(reference)
    if estimated_products > MAX_DOT_PRODUCTS:
        raise InputError(
            f"search requires about {estimated_products:,} sample products; "
            f"limit is {MAX_DOT_PRODUCTS:,}. Reduce --max-lag-ms or shorten the reference.")
    if len(left) < len(reference):
        raise InputError("capture is shorter than the reference")

    matches = (match_channel(left, reference, max_lag),
               match_channel(right, reference, max_lag))
    names = ("left", "right")
    detected = []
    ambiguous = []
    for name, result in zip(names, matches):
        score = abs(result.correlation)
        if score >= args.min_correlation:
            detected.append(name)
            if result.second_peak >= args.min_correlation and \
                    score - result.second_peak <= AMBIGUOUS_PEAK_GAP:
                ambiguous.append(name)
    if not detected:
        raise InputError("no reference signal found in either channel")
    if ambiguous:
        raise InputError("ambiguous correlation peaks in " + " and ".join(ambiguous) +
                         "; use a less repetitive reference signal")

    print("PCM format: signed 16-bit little-endian, 16 kHz")
    for name, result in zip(names, matches):
        polarity = "inverted" if result.correlation < 0 else "normal"
        if name in detected:
            print(f"{name}: reference detected, correlation={result.correlation:.4f}, "
                  f"capture offset={result.lag} samples ({result.lag / RATE * 1000:.3f} ms), "
                  f"polarity={polarity}")
        else:
            print(f"{name}: reference not detected, correlation={result.correlation:.4f}")
    if len(detected) == 2:
        relative = matches[1].lag - matches[0].lag
        print(f"right minus left alignment: {relative:+d} samples ({relative / RATE * 1000:+.3f} ms)")
    else:
        print("relative alignment: unavailable because the reference was detected on one channel only")
    print("Hardware-path latency: not measured.")
    print("Result scope: supplied signal in these files only; physical-device alignment is not established.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except InputError as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(2)
