"""Create an original, quiet synth score and UI cues for the TextHalo demo."""

import math
import struct
import wave
from pathlib import Path


RATE = 48_000
DURATION = 15.0
SAMPLES = int(RATE * DURATION)
LEFT = [0.0] * SAMPLES
RIGHT = [0.0] * SAMPLES


def add_note(start, duration, frequency, amplitude, pan=0.0, harmonics=((1, 1.0), (2, 0.2), (3, 0.05)), attack=0.25, release=0.6):
    start_i = max(0, int(start * RATE))
    end_i = min(SAMPLES, int((start + duration) * RATE))
    left_gain = math.sqrt((1.0 - pan) / 2.0)
    right_gain = math.sqrt((1.0 + pan) / 2.0)
    for i in range(start_i, end_i):
        t = i / RATE - start
        remaining = duration - t
        envelope = min(1.0, t / attack if attack else 1.0)
        if release:
            envelope *= min(1.0, remaining / release)
        tone = sum(weight * math.sin(2 * math.pi * frequency * partial * t) for partial, weight in harmonics)
        value = amplitude * envelope * tone
        LEFT[i] += value * left_gain
        RIGHT[i] += value * right_gain


def add_sweep(start, duration, start_hz, end_hz, amplitude, pan=0.0):
    start_i = max(0, int(start * RATE))
    end_i = min(SAMPLES, int((start + duration) * RATE))
    left_gain = math.sqrt((1.0 - pan) / 2.0)
    right_gain = math.sqrt((1.0 + pan) / 2.0)
    for i in range(start_i, end_i):
        t = i / RATE - start
        progress = t / duration
        hz = start_hz + (end_hz - start_hz) * progress
        envelope = math.sin(math.pi * progress) ** 1.5
        value = amplitude * envelope * math.sin(2 * math.pi * hz * t)
        LEFT[i] += value * left_gain
        RIGHT[i] += value * right_gain


def add_tap(start, amplitude=0.1):
    add_note(
        start,
        0.075,
        1_150,
        amplitude,
        pan=-0.12,
        harmonics=((1, 1.0), (2, 0.18)),
        attack=0.002,
        release=0.065,
    )
    add_note(
        start,
        0.045,
        165,
        amplitude * 0.42,
        pan=-0.12,
        harmonics=((1, 1.0), (2, 0.2)),
        attack=0.002,
        release=0.042,
    )


def main():
    # A slow C / Am / F / G progression, voiced as soft sustained chords.
    chord_notes = [
        (130.81, 196.00, 246.94, 329.63),
        (110.00, 164.81, 220.00, 261.63),
        (87.31, 130.81, 174.61, 220.00),
        (98.00, 146.83, 196.00, 293.66),
    ]
    for chord_index, notes in enumerate(chord_notes):
        start = chord_index * 3.75
        for note_index, frequency in enumerate(notes):
            add_note(
                start,
                4.0,
                frequency,
                0.032 if note_index == 0 else 0.024,
                pan=(-0.24, -0.08, 0.1, 0.24)[note_index],
                harmonics=((1, 1.0), (2, 0.14)),
                attack=0.7,
                release=0.9,
            )

    # Light bell-like notes mark each beat of the product story.
    plucks = [
        (0.35, 523.25), (1.9, 659.25), (3.65, 587.33),
        (5.25, 783.99), (7.35, 659.25), (9.2, 783.99),
        (11.1, 698.46), (12.3, 783.99), (13.55, 659.25),
    ]
    for start, frequency in plucks:
        add_note(
            start,
            1.15,
            frequency,
            0.034,
            pan=0.18 if int(start * 10) % 2 else -0.18,
            harmonics=((1, 1.0), (2, 0.34), (3, 0.12)),
            attack=0.012,
            release=0.9,
        )

    # Quiet, tactile UI sounds synchronized to selection, shortcut, and playback.
    add_sweep(0.85, 0.24, 920, 510, 0.035, pan=0.12)
    for start, volume in ((5.0, 0.095), (5.16, 0.085), (5.32, 0.1)):
        add_tap(start, volume)
    add_sweep(7.85, 0.42, 480, 880, 0.042, pan=-0.08)
    add_note(8.05, 0.72, 1_046.5, 0.045, pan=0.12, attack=0.015, release=0.62)
    add_note(11.55, 1.2, 1_046.5, 0.052, pan=-0.08, attack=0.04, release=1.0)
    add_note(12.05, 1.45, 1_318.5, 0.035, pan=0.1, attack=0.04, release=1.2)

    # Fade the full mix gently at its head and tail.
    for i in range(SAMPLES):
        t = i / RATE
        fade_in = min(1.0, t / 0.8)
        fade_out = min(1.0, (DURATION - t) / 1.6)
        gain = min(fade_in, fade_out)
        LEFT[i] = max(-0.95, min(0.95, LEFT[i] * gain))
        RIGHT[i] = max(-0.95, min(0.95, RIGHT[i] * gain))

    destination = Path(__file__).parent.parent / "output" / "marketing" / "texthalo-original-soundtrack.wav"
    destination.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(destination), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(RATE)
        for left, right in zip(LEFT, RIGHT):
            output.writeframesraw(struct.pack("<hh", int(left * 32767), int(right * 32767)))
    print(destination.resolve())


if __name__ == "__main__":
    main()
