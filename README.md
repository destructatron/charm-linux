# Charm Linux

A Linux port of [CHARM](https://iamtalon.me/charm) (Customizable Harmonic Audio Resource Monitor) by Talon. Charm converts your system's CPU, RAM, and disk activity into real-time audio feedback, allowing you to "hear" what your computer is doing.

Originally designed with accessibility in mind for visually impaired users, Charm provides an intuitive way to monitor system resources through sound - whether that's hearing your CPU ramp up during compilation, or noticing disk activity through audio cues.

## Features

- Real-time audio representation of CPU, RAM, and disk activity
- Multiple sound packs with different themes (sci-fi, nature, ambient, etc.)
- Per-core CPU monitoring with stereo panning (cores spread left-to-right)
- Averaged CPU mode for simpler audio feedback
- Two sound modes per channel:
  - **Volume mode**: Sound volume scales with resource usage
  - **Fade mode**: Crossfades between idle and active sounds
- System tray integration with quick controls
- Fully accessible GTK3 interface compatible with Orca screen reader
- Headless operation via command-line arguments
- 100% compatible with Windows CHARM sound packs

## Installation

### Dependencies

Install the required system libraries for your distribution:

**Debian/Ubuntu:**
```bash
sudo apt install build-essential libgtk-3-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libgstreamer-plugins-good1.0-dev \
    libayatana-appindicator3-dev gstreamer1.0-plugins-good rustc cargo
```

**Fedora:**
```bash
sudo dnf install gcc gtk3-devel gstreamer1-devel gstreamer1-plugins-base-devel \
    gstreamer1-plugins-good libappindicator-gtk3-devel rust cargo
```

**Arch Linux:**
```bash
sudo pacman -S base-devel gtk3 gstreamer gst-plugins-base gst-plugins-good \
    libappindicator-gtk3 rust
```

**Gentoo:**
```bash
sudo emerge dev-lang/rust x11-libs/gtk+:3 media-libs/gstreamer \
    media-libs/gst-plugins-base media-libs/gst-plugins-good \
    dev-libs/libayatana-appindicator
```

**openSUSE:**
```bash
sudo zypper install gcc gtk3-devel gstreamer-devel gstreamer-plugins-base-devel \
    gstreamer-plugins-good libappindicator3-devel rust cargo
```

### Building

```bash
git clone https://github.com/destructatron/charm-linux.git
cd charm-linux
cargo build --release
```

The binary will be at `target/release/charm-linux`.

### Installing

```bash
# Install binary
sudo install -Dm755 target/release/charm-linux /usr/local/bin/charm-linux

# Install sound packs
sudo mkdir -p /usr/local/share/charm-linux
sudo cp -r packs /usr/local/share/charm-linux/
```

Or for user-local installation:
```bash
mkdir -p ~/.local/bin ~/.local/share/charm-linux
cp target/release/charm-linux ~/.local/bin/
cp -r packs ~/.local/share/charm-linux/
```

## Usage

### GUI Mode

Simply run `charm-linux` to open the pack selection dialog. Select a sound pack and click "Start Monitoring" (or press Enter).

### Headless Mode

For use without a display or in scripts:

```bash
charm-linux default      # Start with 'default' pack
charm-linux scifi1       # Start with 'scifi1' pack
charm-linux -h           # Show help
```

### System Tray Controls

Once running, right-click the tray icon to:
- Adjust refresh rate (100ms - 1s)
- Change volume
- Toggle CPU/RAM/Disk monitoring individually
- Switch sound packs
- Quit

## Sound Packs

Charm Linux is compatible with Windows CHARM sound packs. Each pack is a folder containing:

- `prefs.ini` - Configuration file
- Audio files (`.ogg`, `.wav`, `.flac`, or `.mp3`)

### Creating Sound Packs

Create a folder in `packs/` with a `prefs.ini`:

```ini
[soundpack]
UseAverages=1          ; 0 = per-core CPU, 1 = averaged CPU
CPUSoundMode=1         ; 0 = disabled, 1 = volume, 2 = fade
RAMSoundMode=1
DiskSoundMode=1
SlideInterval=20       ; Transition smoothness (higher = smoother)
FrequencyFluctuation=0 ; 1 = enable pitch variation
```

For **volume mode** (mode 1), provide single files: `CPU.ogg`, `RAM.ogg`, `disk.ogg`

For **fade mode** (mode 2), provide pairs: `CPU_A.ogg` (idle) + `CPU_B.ogg` (active)

## Packs Directory Search Order

Charm Linux looks for sound packs in:
1. `./packs/` (current directory)
2. `~/.local/share/charm-linux/packs/`
3. `/usr/share/charm-linux/packs/`

## Acknowledgments

This project is a Linux port of [CHARM](https://iamtalon.me/charm) by Talon. The original Windows application inspired this implementation, and all bundled sound packs are from the original project.

## License

MIT License
