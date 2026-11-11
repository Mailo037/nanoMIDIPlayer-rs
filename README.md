# nanoMIDIPlayer-rs

An **unofficial fork** of [NotHammer043/nanoMIDIPlayer](https://github.com/NotHammer043/nanoMIDIPlayer), rebuilt in Rust on [GPUI](https://www.gpui.rs/). It keeps the original's features and themes and runs natively without Python on Windows, Linux and macOS. All credit for the original app, the MIDI Hub and the nanoMIDI community goes to NotHammer043 and the nanoMIDI team. Report problems with this version in this fork's issues, not the original project's.

**Download:** see [Releases](https://github.com/Mailo037/nanoMIDIPlayer-rs/releases).

## Features (same as the original)
- **Play a song** (MIDI Player): MIDI → QWERTY or direct to a MIDI output device; Sustain, No Doubles, Velocity, 88 Keys, Pitch (octave) and Key (transpose), finger limit, speed 1–500 %
- **Play drums** (Drums Player): GM drum notes → QWERTY (keys from `drumsMap`)
- **MIDI Hub**: browse, search, sort and download MIDIs from nanomidi.net
- **Live keyboard** (MIDI To QWERTY): live MIDI input device → keys, with an on-screen piano
- **Settings**: loop, release on pause, custom hold length, random fail (speed/transpose), hotkey step size, keyboard module (`pynput` = virtual-key, `keyboard` = scancode), always on top, console, tooltips, timestamp, update check, debug console, themes (API + custom), hotkeys
- Global hotkeys (default F1–F5), active on the "Play a song" and "Play drums" tabs

## Added in this fork
- Guided layout: every play tab is three numbered steps (choose, play, adjust), with each option explained in plain words
- Own icon set (`assets/icons`, rounded line icons embedded in the binary and tinted by the theme)
- Runs on Linux and macOS as well (see [Platforms](#platforms))
- Right-click menus on songs, the player, hub songs, hotkeys and the log (play from start, speed presets, loop, show in Explorer, copy link, favorites, Alt+1…9 song slots, …)
- Click or drag the timeline to jump inside a playing or paused song
- Settings split into categories (General, Players, Hotkeys, Library, Theme, Updates, Advanced)
- Built-in updater: "Check now", optional automatic install in the background, "Restart now"

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
The binary is written to `target/release/nanoMIDIPlayer` (`.exe` on Windows). Run with `--debug` to open the debug console at startup.

Linux needs the usual GPUI build libraries first (Debian/Ubuntu names):
```
sudo apt install libasound2-dev libfontconfig1-dev libfreetype-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libssl-dev libzstd-dev pkg-config clang cmake
```

## Platforms
| | Key presses | Global hotkeys | Pause on focus loss | Always on top | Self-update |
|---|---|---|---|---|---|
| Windows | SendInput (virtual keys or scan codes) | low-level keyboard hook | yes | yes | yes |
| Linux | XTest | keyboard devices (`/dev/input`), else X11 key state polling | yes | X11 only | yes (x64) |
| macOS | Quartz events | event tap | yes (per app) | yes | opens the release page |

Linux and macOS support is new: the Linux build is compile-checked, the macOS code has not run on a real Mac yet. Please report what does not work.

- **Linux**: key presses and hotkeys go through X11, so they work in X11 sessions and with XWayland windows (most games) on Wayland. Native Wayland windows cannot receive simulated keys this way. Keys are sent by position on a US keyboard layout. Hotkeys work everywhere when your user can read `/dev/input` (`sudo usermod -aG input $USER`, then log in again); without that they only work while the app window or an X11 game window is focused. Song previews use FluidSynth/TiMidity if one is running, otherwise a small built-in synth.
- **macOS**: allow nanoMIDIPlayer under System Settings → Privacy & Security → **Accessibility** (key presses) and **Input Monitoring** (hotkeys), then restart the app. The app shows a note until that is done. F1–F5 may need the Fn key, depending on your keyboard settings. Releases are not notarized, so the first start needs right-click → Open.

## Notes
- Updates come from this fork's GitHub releases. On Windows and Linux the new binary is downloaded next to the running one and swapped in (the old file is kept as `<name>.old` until the next start); on macOS "Update" opens the release page. `NANOMIDI_PRETEND_VERSION=1.0.0` makes the app think it is older, to test the update flow.
- Licensed GPL-3.0, like the original.
