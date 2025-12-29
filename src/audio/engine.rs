use gstreamer as gst;
use std::cell::RefCell;
use std::rc::Rc;

use super::mixer::{AudioChannel, AudioMixer, CpuPlayback, PerCoreCpuPlayer};
use super::pitch::GranularPitchElement;
use crate::monitor::SystemMetrics;
use crate::pack::SoundPack;

#[derive(Debug)]
pub enum AudioEngineError {
    GstreamerInit(gst::glib::Error),
    GstreamerError(gst::glib::BoolError),
    NoPackLoaded,
}

impl std::fmt::Display for AudioEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GstreamerInit(e) => write!(f, "GStreamer initialization error: {}", e),
            Self::GstreamerError(e) => write!(f, "GStreamer error: {}", e),
            Self::NoPackLoaded => write!(f, "No sound pack loaded"),
        }
    }
}

impl std::error::Error for AudioEngineError {}

impl From<gst::glib::BoolError> for AudioEngineError {
    fn from(e: gst::glib::BoolError) -> Self {
        Self::GstreamerError(e)
    }
}

/// Main audio engine that coordinates playback based on system metrics
pub struct AudioEngine {
    mixer: Rc<RefCell<AudioMixer>>,
    current_pack: Option<SoundPack>,
    is_playing: bool,
    /// Enable/disable individual monitoring (user toggle)
    cpu_enabled: bool,
    ram_enabled: bool,
    disk_enabled: bool,
    /// Whether using per-core CPU or averaged
    use_averages: bool,
}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioEngineError> {
        gst::init().map_err(AudioEngineError::GstreamerInit)?;

        // Register our custom granular pitch element
        GranularPitchElement::register()?;

        Ok(Self {
            mixer: Rc::new(RefCell::new(AudioMixer::new())),
            current_pack: None,
            is_playing: false,
            cpu_enabled: true,
            ram_enabled: true,
            disk_enabled: true,
            use_averages: true,
        })
    }

    /// Load a sound pack and prepare for playback
    pub fn load_pack(&mut self, pack: SoundPack, num_cpu_cores: usize) -> Result<(), AudioEngineError> {
        // Stop current playback
        self.stop()?;

        let mut mixer = self.mixer.borrow_mut();
        mixer.clear();

        let config = &pack.config;
        let slide_interval = config.slide_interval;
        let freq_fluct = config.frequency_fluctuation;
        self.use_averages = config.use_averages;

        // Create CPU playback
        if pack.cpu_sounds.has_sounds() {
            if let Some(primary_path) = &pack.cpu_sounds.primary {
                if config.use_averages {
                    // Single averaged CPU channel, centered
                    let cpu_channel = AudioChannel::new(
                        config.cpu_mode,
                        Some(primary_path.as_path()),
                        pack.cpu_sounds.secondary.as_deref(),
                        slide_interval,
                        freq_fluct,
                        0.0, // center
                    )?;
                    mixer.cpu_playback = Some(CpuPlayback::Averaged(cpu_channel));
                } else {
                    // Per-core mode: single source split to multiple panned outputs
                    // This ensures perfect sync - no stereo position weirdness on loop
                    // Uses lightweight granular pitch shifting per core
                    let player = PerCoreCpuPlayer::new(
                        primary_path,
                        num_cpu_cores,
                        slide_interval,
                        freq_fluct,
                    )?;
                    mixer.cpu_playback = Some(CpuPlayback::PerCore(player));
                }
            }
        }

        // Create RAM channel (centered)
        if pack.ram_sounds.has_sounds() {
            let ram_channel = AudioChannel::new(
                config.ram_mode,
                pack.ram_sounds.primary.as_deref(),
                pack.ram_sounds.secondary.as_deref(),
                slide_interval,
                freq_fluct,
                0.0, // center
            )?;
            mixer.ram_channel = Some(ram_channel);
        }

        // Create Disk channel (centered)
        if pack.disk_sounds.has_sounds() {
            let disk_channel = AudioChannel::new(
                config.disk_mode,
                pack.disk_sounds.primary.as_deref(),
                pack.disk_sounds.secondary.as_deref(),
                slide_interval,
                freq_fluct,
                0.0, // center
            )?;
            mixer.disk_channel = Some(disk_channel);
        }

        drop(mixer);
        self.current_pack = Some(pack);

        Ok(())
    }

    /// Start audio playback
    pub fn play(&mut self) -> Result<(), AudioEngineError> {
        if self.current_pack.is_none() {
            return Err(AudioEngineError::NoPackLoaded);
        }

        self.mixer.borrow().play_all();
        self.is_playing = true;
        Ok(())
    }

    /// Stop audio playback
    pub fn stop(&mut self) -> Result<(), AudioEngineError> {
        self.mixer.borrow().stop_all();
        self.is_playing = false;

        // Reset channel values
        let mut mixer = self.mixer.borrow_mut();
        match &mut mixer.cpu_playback {
            Some(CpuPlayback::Averaged(ch)) => ch.reset(),
            Some(CpuPlayback::PerCore(player)) => player.reset(),
            None => {}
        }
        if let Some(ref mut ch) = mixer.ram_channel {
            ch.reset();
        }
        if let Some(ref mut ch) = mixer.disk_channel {
            ch.reset();
        }

        Ok(())
    }

    /// Update audio based on current system metrics
    pub fn update(&mut self, metrics: &SystemMetrics) {
        let mut mixer = self.mixer.borrow_mut();

        // Update CPU playback
        match &mut mixer.cpu_playback {
            Some(CpuPlayback::Averaged(ch)) => {
                if self.cpu_enabled {
                    ch.update(metrics.cpu_average.get());
                } else {
                    ch.update(0.0);
                }
            }
            Some(CpuPlayback::PerCore(player)) => {
                for i in 0..player.core_count() {
                    if self.cpu_enabled {
                        let value = metrics.cpu_cores.get(i)
                            .map(|v| v.get())
                            .unwrap_or(0.0);
                        player.update_core(i, value);
                    } else {
                        player.update_core(i, 0.0);
                    }
                }
            }
            None => {}
        }

        // Update RAM channel
        if self.ram_enabled {
            if let Some(ref mut ch) = mixer.ram_channel {
                ch.update(metrics.memory.get());
            }
        } else {
            if let Some(ref mut ch) = mixer.ram_channel {
                ch.update(0.0);
            }
        }

        // Update Disk channel
        if self.disk_enabled {
            if let Some(ref mut ch) = mixer.disk_channel {
                ch.update(metrics.disk.get());
            }
        } else {
            if let Some(ref mut ch) = mixer.disk_channel {
                ch.update(0.0);
            }
        }
    }

    pub fn set_master_volume(&mut self, volume: f64) {
        self.mixer.borrow_mut().set_master_volume(volume);
    }

    pub fn set_cpu_enabled(&mut self, enabled: bool) {
        self.cpu_enabled = enabled;
    }

    pub fn set_ram_enabled(&mut self, enabled: bool) {
        self.ram_enabled = enabled;
    }

    pub fn set_disk_enabled(&mut self, enabled: bool) {
        self.disk_enabled = enabled;
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new().expect("Failed to initialize audio engine")
    }
}
