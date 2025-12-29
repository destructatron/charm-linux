# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Charm Linux is a Rust port of Windows CHARM (Customizable Audio Resource Monitor). It converts system metrics (CPU, RAM, disk activity) into real-time audio feedback using GStreamer.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build
cargo check              # Fast type checking without full compile
cargo run                # Run with GTK dialog
cargo run -- scifi1      # Run headless with specific pack
cargo run -- -h          # Show help
```

## Architecture

### Core Data Flow

```
SystemMonitor (sysinfo) → App::update_tick() → AudioEngine → AudioMixer → GStreamer pipelines
```

The GTK main loop drives periodic updates via `glib::timeout_add_local`. Each tick:
1. `SystemMonitor::refresh()` collects CPU/RAM/disk metrics as normalized 0.0-1.0 values
2. `AudioEngine::update()` receives metrics and updates audio channels
3. Each channel smoothly transitions toward target values based on `SlideInterval`

### Module Structure

- **`app.rs`** - Application lifecycle, GTK event wiring, coordinates all components
- **`audio/`** - GStreamer audio playback
  - `engine.rs` - High-level control (load pack, play/stop, route metrics to channels)
  - `mixer.rs` - GStreamer pipeline construction, `AudioChannel` for single streams, `PerCoreCpuPlayer` for synchronized per-core output
- **`monitor/`** - System metrics via sysinfo crate (cpu.rs, memory.rs, disk.rs)
- **`pack/`** - Sound pack loading, prefs.ini parsing with rust-ini
- **`ui/`** - GTK3 startup dialog and libappindicator system tray

### Sound Modes

Packs use `prefs.ini` with Windows CHARM format:
- **Mode 0 (Disabled)** - Channel silent
- **Mode 1 (Volume)** - Single sound, volume scales with metric
- **Mode 2 (Fade)** - Crossfade between `_A` (idle) and `_B` (active) sounds

### Per-Core CPU Architecture

Per-core mode uses a single GStreamer pipeline with `tee` element splitting to N panned outputs:
```
uridecodebin → tee → [queue → volume → panorama] × N → audiomixer → sink
```
This ensures perfect loop synchronization across cores (prevents perceived stereo drift).

### Key Design Decisions

- Per-core pitch shifting is disabled (too CPU intensive with N pitch processors)
- `Rc<RefCell<>>` pattern used throughout for GTK callback ownership
- Tray icon is reused when changing packs (not recreated) to avoid D-Bus registration conflicts
- `try_borrow_mut()` used in some callbacks to handle reentrancy during GTK signal emission

## Sound Pack Format

Each pack is a directory in `packs/` containing:
- `prefs.ini` - Configuration (UseAverages, CPUSoundMode, RAMSoundMode, DiskSoundMode, SlideInterval, FrequencyFluctuation)
- Audio files: `CPU.ogg`, `RAM.ogg`, `disk.ogg` for volume mode; `CPU_A.ogg`/`CPU_B.ogg` etc. for fade mode

## Dependencies

GTK3, GStreamer (with audiopanorama from gst-plugins-good), libappindicator/ayatana-appindicator, sysinfo crate.
