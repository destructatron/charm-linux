use ini::Ini;
use std::fs;
use std::path::{Path, PathBuf};

/// Sound mode for a channel (matches Windows CHARM)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoundMode {
    /// Channel is disabled (mode 0)
    Disabled = 0,
    /// Volume modulation - single sound file (mode 1)
    #[default]
    Volume = 1,
    /// Fade/crossfade between idle (_A) and active (_B) sounds (mode 2)
    Fade = 2,
}

impl SoundMode {
    pub fn from_int(value: i32) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Volume,
            2 => Self::Fade,
            _ => Self::Volume, // Default to volume mode
        }
    }
}

/// Sound pack configuration (parsed from prefs.ini)
#[derive(Debug, Clone)]
pub struct SoundPackConfig {
    /// Use averaged CPU load instead of individual cores
    pub use_averages: bool,
    /// CPU channel sound mode
    pub cpu_mode: SoundMode,
    /// RAM channel sound mode
    pub ram_mode: SoundMode,
    /// Disk channel sound mode
    pub disk_mode: SoundMode,
    /// Transition/slide interval (higher = smoother but slower)
    pub slide_interval: u32,
    /// Enable pitch/frequency fluctuation
    pub frequency_fluctuation: bool,
}

impl Default for SoundPackConfig {
    fn default() -> Self {
        Self {
            use_averages: false,
            cpu_mode: SoundMode::Volume,
            ram_mode: SoundMode::Volume,
            disk_mode: SoundMode::Volume,
            slide_interval: 20,
            frequency_fluctuation: false,
        }
    }
}

/// Sound files for a channel
#[derive(Debug, Clone)]
pub struct ChannelSounds {
    /// For volume mode: the single sound file
    /// For fade mode: the idle sound (_A)
    pub primary: Option<PathBuf>,
    /// For fade mode: the active sound (_B)
    pub secondary: Option<PathBuf>,
}

impl ChannelSounds {
    pub fn none() -> Self {
        Self {
            primary: None,
            secondary: None,
        }
    }

    pub fn single(path: PathBuf) -> Self {
        Self {
            primary: Some(path),
            secondary: None,
        }
    }

    pub fn pair(idle: PathBuf, active: PathBuf) -> Self {
        Self {
            primary: Some(idle),
            secondary: Some(active),
        }
    }

    pub fn has_sounds(&self) -> bool {
        self.primary.is_some()
    }
}

/// A loaded sound pack with resolved file paths
#[derive(Debug, Clone)]
pub struct SoundPack {
    /// Pack directory path
    pub directory: PathBuf,
    /// Pack name (directory name)
    pub name: String,
    /// Configuration from prefs.ini
    pub config: SoundPackConfig,
    /// CPU sound files
    pub cpu_sounds: ChannelSounds,
    /// RAM sound files
    pub ram_sounds: ChannelSounds,
    /// Disk sound files
    pub disk_sounds: ChannelSounds,
}

impl SoundPack {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> String {
        let mut parts = Vec::new();

        if self.config.use_averages {
            parts.push("Averaged CPU".to_string());
        } else {
            parts.push("Per-core CPU".to_string());
        }

        let modes: Vec<&str> = [
            ("CPU", self.config.cpu_mode),
            ("RAM", self.config.ram_mode),
            ("Disk", self.config.disk_mode),
        ]
        .iter()
        .filter_map(|(name, mode)| match mode {
            SoundMode::Disabled => None,
            SoundMode::Volume => Some(*name),
            SoundMode::Fade => Some(*name),
        })
        .collect();

        if !modes.is_empty() {
            parts.push(format!("Monitors: {}", modes.join(", ")));
        }

        parts.join(" | ")
    }
}

#[derive(Debug)]
pub enum SoundPackError {
    IoError(std::io::Error),
    ParseError(String),
    MissingSoundFile(PathBuf),
}

impl std::fmt::Display for SoundPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::ParseError(e) => write!(f, "Config parse error: {}", e),
            Self::MissingSoundFile(path) => write!(f, "Missing sound file: {}", path.display()),
        }
    }
}

impl std::error::Error for SoundPackError {}

impl From<std::io::Error> for SoundPackError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<ini::Error> for SoundPackError {
    fn from(e: ini::Error) -> Self {
        Self::ParseError(e.to_string())
    }
}

pub struct PackLoader {
    packs_directory: PathBuf,
}

impl PackLoader {
    pub fn new(packs_directory: impl Into<PathBuf>) -> Self {
        Self {
            packs_directory: packs_directory.into(),
        }
    }

    /// Scan the packs directory and return all available packs
    pub fn scan_packs(&self) -> Result<Vec<SoundPack>, SoundPackError> {
        let mut packs = Vec::new();

        if !self.packs_directory.exists() {
            return Ok(packs);
        }

        for entry in fs::read_dir(&self.packs_directory)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                match self.load_pack(&path) {
                    Ok(pack) => packs.push(pack),
                    Err(e) => {
                        eprintln!("Warning: Failed to load pack at {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(packs)
    }

    /// Load a specific pack from a directory
    pub fn load_pack(&self, pack_dir: &Path) -> Result<SoundPack, SoundPackError> {
        let config_path = pack_dir.join("prefs.ini");
        let ini = Ini::load_from_file(&config_path)?;

        let section = ini
            .section(Some("soundpack"))
            .ok_or_else(|| SoundPackError::ParseError("Missing [soundpack] section".to_string()))?;

        // Parse configuration
        let config = SoundPackConfig {
            use_averages: section
                .get("UseAverages")
                .and_then(|v| v.parse().ok())
                .map(|v: i32| v != 0)
                .unwrap_or(false),
            cpu_mode: section
                .get("CPUSoundMode")
                .and_then(|v| v.parse().ok())
                .map(SoundMode::from_int)
                .unwrap_or(SoundMode::Volume),
            ram_mode: section
                .get("RAMSoundMode")
                .and_then(|v| v.parse().ok())
                .map(SoundMode::from_int)
                .unwrap_or(SoundMode::Volume),
            disk_mode: section
                .get("DiskSoundMode")
                .and_then(|v| v.parse().ok())
                .map(SoundMode::from_int)
                .unwrap_or(SoundMode::Volume),
            slide_interval: section
                .get("SlideInterval")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            frequency_fluctuation: section
                .get("FrequencyFluctuation")
                .and_then(|v| v.parse().ok())
                .map(|v: i32| v != 0)
                .unwrap_or(false),
        };

        // Resolve sound files based on modes
        let cpu_sounds = Self::resolve_sounds(pack_dir, "CPU", config.cpu_mode);
        let ram_sounds = Self::resolve_sounds(pack_dir, "RAM", config.ram_mode);
        let disk_sounds = Self::resolve_sounds(pack_dir, "disk", config.disk_mode);

        // Get pack name from directory
        let name = pack_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        Ok(SoundPack {
            directory: pack_dir.to_path_buf(),
            name,
            config,
            cpu_sounds,
            ram_sounds,
            disk_sounds,
        })
    }

    /// Resolve sound files for a channel based on its mode
    fn resolve_sounds(pack_dir: &Path, base_name: &str, mode: SoundMode) -> ChannelSounds {
        if mode == SoundMode::Disabled {
            return ChannelSounds::none();
        }

        // Try to find sound files with various extensions
        let extensions = ["ogg", "wav", "flac", "mp3"];

        if mode == SoundMode::Fade {
            // Look for _A and _B pairs
            for ext in &extensions {
                let idle = pack_dir.join(format!("{}_A.{}", base_name, ext));
                let active = pack_dir.join(format!("{}_B.{}", base_name, ext));

                if idle.exists() && active.exists() {
                    return ChannelSounds::pair(idle, active);
                }

                // Try lowercase variants
                let idle_lower = pack_dir.join(format!("{}_a.{}", base_name.to_lowercase(), ext));
                let active_lower = pack_dir.join(format!("{}_b.{}", base_name.to_lowercase(), ext));

                if idle_lower.exists() && active_lower.exists() {
                    return ChannelSounds::pair(idle_lower, active_lower);
                }
            }
        }

        // Look for single file (volume mode, or fallback for fade mode)
        for ext in &extensions {
            let single = pack_dir.join(format!("{}.{}", base_name, ext));
            if single.exists() {
                return ChannelSounds::single(single);
            }

            // Try lowercase
            let single_lower = pack_dir.join(format!("{}.{}", base_name.to_lowercase(), ext));
            if single_lower.exists() {
                return ChannelSounds::single(single_lower);
            }
        }

        ChannelSounds::none()
    }
}
