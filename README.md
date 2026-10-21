# nanoMIDIPlayer (Rust + GPUI)

A Rust rebuild of [NotHammer043/nanoMIDIPlayer](https://github.com/NotHammer043/nanoMIDIPlayer) on [GPUI](https://www.gpui.rs/). It keeps the original's features, layout and themes, and runs natively without Python.

## Features (same as the original)
- **MIDI Player**: MIDI → QWERTY or direct to a MIDI output device; Sustain, No Doubles, Velocity, 88 Keys, Pitch (octave) and Key (transpose), finger limit, speed 1–500 %
- **Drums Player**: GM drum notes → QWERTY (keys from `drumsMap`)
- **MIDI Hub**: browse, search, sort and download MIDIs from nanomidi.net
- **MIDI To QWERTY**: live MIDI input device → keys, with an on-screen piano
- **Settings**: loop, release on pause, custom hold length, random fail (speed/transpose), hotkey step size, keyboard module (`pynput` = virtual-key, `keyboard` = scancode), always on top, console, tooltips, timestamp, update check, debug console, themes (API + custom), hotkeys
- Global hotkeys (default F1–F5), active on the MIDI Player and Drums tabs

It uses the same data folder (`Documents/nanoMIDIPlayer`) and the same `config.json`, theme and custom-theme formats, so existing settings and downloaded MIDIs carry over.

## Why it's faster
- Each MIDI file is parsed once into a flat event list with times already in seconds. Playback does no tick or tempo math.
- Playback runs on one time-critical thread with a 1 ms timer resolution. It sleeps until shortly before each note, then spin-waits, and targets are absolute so timing doesn't drift.
- Hold-length releases go through one scheduler thread instead of one timer thread per note.
- Settings are cached per config generation, so there are no dictionary lookups per note.
- The GPU-rendered UI redraws only when state changes.
- Config writes are debounced instead of happening on every slider tick.

## Build
```
cargo build --release
```
The binary is written to `target/release/nanoMIDIPlayer.exe`. Run with `--debug` to open the debug console at startup.

## Notes
- Key simulation and global hotkeys are implemented for Windows (SendInput / low-level keyboard hook). The rest builds on macOS and Linux, but key simulation is a no-op there for now.
- When the update check finds a newer version, it opens the releases page instead of running the Python bootstrapper.
- Licensed GPL-3.0, like the original.
