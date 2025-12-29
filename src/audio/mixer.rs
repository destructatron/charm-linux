use gstreamer as gst;
use gstreamer::prelude::*;
use std::path::Path;

use crate::pack::SoundMode;

/// Represents a single audio playback element with stereo panning
struct PlaybackElement {
    pipeline: gst::Pipeline,
    volume_element: gst::Element,
    panorama_element: Option<gst::Element>,
    _bus_watch: gst::bus::BusWatchGuard,
}

impl PlaybackElement {
    fn new(file_path: &Path, pan: f64) -> Result<Self, gst::glib::BoolError> {
        // Ensure we have an absolute path
        let abs_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(file_path)
        };

        let uri = format!("file://{}", abs_path.display());

        // Create pipeline elements
        let pipeline = gst::Pipeline::new();

        let source = gst::ElementFactory::make("uridecodebin")
            .property("uri", &uri)
            .build()?;

        // Queue for buffering and thread decoupling
        let queue = gst::ElementFactory::make("queue").build()?;
        let convert = gst::ElementFactory::make("audioconvert").build()?;
        let resample = gst::ElementFactory::make("audioresample").build()?;

        let volume_element = gst::ElementFactory::make("volume")
            .property("volume", 0.0f64)
            .build()?;

        // Try to create panorama element for stereo panning
        let panorama_element = gst::ElementFactory::make("audiopanorama")
            .property("panorama", pan as f32)
            .build()
            .ok();

        let sink = gst::ElementFactory::make("autoaudiosink").build()?;

        // Add elements to pipeline and link them
        if let Some(ref pan_elem) = panorama_element {
            pipeline.add_many([&source, &queue, &convert, &resample, &volume_element, pan_elem, &sink])?;
            gst::Element::link_many([&queue, &convert, &resample, &volume_element, pan_elem, &sink])?;
        } else {
            pipeline.add_many([&source, &queue, &convert, &resample, &volume_element, &sink])?;
            gst::Element::link_many([&queue, &convert, &resample, &volume_element, &sink])?;
        }

        // Connect uridecodebin's pad-added signal to link to queue
        let queue_weak = queue.downgrade();
        source.connect_pad_added(move |_, src_pad| {
            if let Some(queue) = queue_weak.upgrade() {
                if let Some(sink_pad) = queue.static_pad("sink") {
                    if !sink_pad.is_linked() {
                        let _ = src_pad.link(&sink_pad);
                    }
                }
            }
        });

        // Set up bus watch for looping and error handling
        let pipeline_weak = pipeline.downgrade();
        let bus_watch = pipeline.bus().unwrap().add_watch_local(move |_, msg| {
            match msg.view() {
                gst::MessageView::Eos(_) => {
                    if let Some(pipeline) = pipeline_weak.upgrade() {
                        // Simple seek back to start for looping
                        let _ = pipeline.seek_simple(
                            gst::SeekFlags::FLUSH,
                            gst::ClockTime::ZERO,
                        );
                    }
                }
                gst::MessageView::Error(err) => {
                    eprintln!(
                        "GStreamer error: {} ({:?})",
                        err.error(),
                        err.debug()
                    );
                }
                _ => {}
            }
            gst::glib::ControlFlow::Continue
        })?;

        Ok(Self {
            pipeline,
            volume_element,
            panorama_element,
            _bus_watch: bus_watch,
        })
    }

    fn play(&self) {
        if self.pipeline.set_state(gst::State::Playing).is_err() {
            eprintln!("Failed to start audio pipeline");
            return;
        }
        // Wait for state change to complete (up to 1 second)
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(1));
    }

    fn stop(&self) {
        if self.pipeline.set_state(gst::State::Null).is_err() {
            eprintln!("Failed to stop audio pipeline");
            return;
        }
        // Wait for state change to complete (up to 500ms)
        let _ = self.pipeline.state(gst::ClockTime::from_mseconds(500));
    }

    fn set_volume(&self, volume: f64) {
        self.volume_element.set_property("volume", volume.clamp(0.0, 1.0));
    }

    fn set_pan(&self, pan: f64) {
        if let Some(ref pan_elem) = self.panorama_element {
            pan_elem.set_property("panorama", pan.clamp(-1.0, 1.0) as f32);
        }
    }

    fn set_rate(&self, _rate: f64) {
        // Pitch shifting disabled for PlaybackElement to avoid audio issues
        // Per-core CPU mode uses PerCoreCpuPlayer which has pitch support
    }
}

impl Drop for PlaybackElement {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// A single pipeline that plays one audio file through multiple panned outputs.
/// Used for per-core CPU mode where all cores must stay perfectly in sync.
/// Uses tee to split one source to N panned branches, mixed back together.
/// Per-core pitch shifting uses lightweight granular synthesis (not SoundTouch).
pub struct PerCoreCpuPlayer {
    pipeline: gst::Pipeline,
    /// Volume elements for each core (index = core number)
    volume_elements: Vec<gst::Element>,
    /// Pitch elements for each core (granular pitch shifter)
    pitch_elements: Vec<gst::Element>,
    /// Current smoothed values per core
    current_values: Vec<f64>,
    /// Transition speed
    transition_speed: f64,
    /// Master volume
    master_volume: f64,
    /// Whether pitch fluctuation is enabled
    frequency_fluctuation: bool,
    _bus_watch: gst::bus::BusWatchGuard,
}

impl PerCoreCpuPlayer {
    pub fn new(
        file_path: &Path,
        num_cores: usize,
        slide_interval: u32,
        frequency_fluctuation: bool,
    ) -> Result<Self, gst::glib::BoolError> {
        let abs_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(file_path)
        };

        let uri = format!("file://{}", abs_path.display());
        let pipeline = gst::Pipeline::new();

        // Source and initial processing
        let source = gst::ElementFactory::make("uridecodebin")
            .property("uri", &uri)
            .build()?;
        let convert = gst::ElementFactory::make("audioconvert").build()?;
        let resample = gst::ElementFactory::make("audioresample").build()?;
        let tee = gst::ElementFactory::make("tee").build()?;

        // Final mixer and sink
        let mixer = gst::ElementFactory::make("audiomixer").build()?;
        let sink = gst::ElementFactory::make("autoaudiosink").build()?;

        pipeline.add_many([&source, &convert, &resample, &tee, &mixer, &sink])?;
        gst::Element::link_many([&convert, &resample, &tee])?;
        gst::Element::link_many([&mixer, &sink])?;

        // Connect source to convert
        let convert_weak = convert.downgrade();
        source.connect_pad_added(move |_, src_pad| {
            if let Some(convert) = convert_weak.upgrade() {
                if let Some(sink_pad) = convert.static_pad("sink") {
                    if !sink_pad.is_linked() {
                        let _ = src_pad.link(&sink_pad);
                    }
                }
            }
        });

        // Create a branch for each core with panning and pitch
        let mut volume_elements = Vec::with_capacity(num_cores);
        let mut pitch_elements = Vec::with_capacity(num_cores);

        for i in 0..num_cores {
            let queue = gst::ElementFactory::make("queue").build()?;
            let branch_convert = gst::ElementFactory::make("audioconvert").build()?;

            // Capsfilter to ensure F32 format for our pitch element
            let capsfilter = gst::ElementFactory::make("capsfilter")
                .property(
                    "caps",
                    gst::Caps::builder("audio/x-raw")
                        .field("format", "F32LE")
                        .field("layout", "interleaved")
                        .build(),
                )
                .build()?;

            // Granular pitch shifter (our lightweight custom element)
            let pitch = gst::ElementFactory::make("granularpitch")
                .property("pitch", 1.0f64)
                .build()?;

            let volume = gst::ElementFactory::make("volume")
                .property("volume", 0.0f64)
                .build()?;

            // Calculate pan position: left (-1.0) to right (1.0)
            let pan = if num_cores == 1 {
                0.0
            } else {
                -1.0 + (2.0 * i as f64 / (num_cores - 1) as f64)
            };

            pipeline.add_many([&queue, &branch_convert, &capsfilter, &pitch, &volume])?;

            // Try to add panorama element
            if let Ok(panorama) = gst::ElementFactory::make("audiopanorama")
                .property("panorama", pan as f32)
                .build()
            {
                pipeline.add(&panorama)?;
                gst::Element::link_many([&queue, &branch_convert, &capsfilter, &pitch, &volume, &panorama])?;

                // Link tee to queue
                let tee_pad = tee.request_pad_simple("src_%u").unwrap();
                let queue_pad = queue.static_pad("sink").unwrap();
                let _ = tee_pad.link(&queue_pad);

                // Link panorama to mixer
                let panorama_pad = panorama.static_pad("src").unwrap();
                let mixer_pad = mixer.request_pad_simple("sink_%u").unwrap();
                let _ = panorama_pad.link(&mixer_pad);
            } else {
                // No panorama support, link directly
                gst::Element::link_many([&queue, &branch_convert, &capsfilter, &pitch, &volume])?;

                let tee_pad = tee.request_pad_simple("src_%u").unwrap();
                let queue_pad = queue.static_pad("sink").unwrap();
                let _ = tee_pad.link(&queue_pad);

                let volume_pad = volume.static_pad("src").unwrap();
                let mixer_pad = mixer.request_pad_simple("sink_%u").unwrap();
                let _ = volume_pad.link(&mixer_pad);
            }

            volume_elements.push(volume);
            pitch_elements.push(pitch);
        }

        // Set up looping
        let pipeline_weak = pipeline.downgrade();
        let bus_watch = pipeline.bus().unwrap().add_watch_local(move |_, msg| {
            match msg.view() {
                gst::MessageView::Eos(_) => {
                    if let Some(pipeline) = pipeline_weak.upgrade() {
                        // Simple seek back to start for looping
                        let _ = pipeline.seek_simple(
                            gst::SeekFlags::FLUSH,
                            gst::ClockTime::ZERO,
                        );
                    }
                }
                gst::MessageView::Error(err) => {
                    eprintln!(
                        "GStreamer error: {} ({:?})",
                        err.error(),
                        err.debug()
                    );
                }
                _ => {}
            }
            gst::glib::ControlFlow::Continue
        })?;

        let transition_speed = 1.0 / (slide_interval as f64).max(1.0);

        Ok(Self {
            pipeline,
            volume_elements,
            pitch_elements,
            current_values: vec![0.0; num_cores],
            transition_speed,
            master_volume: 1.0,
            frequency_fluctuation,
            _bus_watch: bus_watch,
        })
    }

    pub fn play(&self) {
        if self.pipeline.set_state(gst::State::Playing).is_err() {
            eprintln!("Failed to start per-core CPU audio pipeline");
            return;
        }
        // Wait for state change to complete (up to 1 second)
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(1));
    }

    pub fn stop(&self) {
        if self.pipeline.set_state(gst::State::Null).is_err() {
            eprintln!("Failed to stop per-core CPU audio pipeline");
            return;
        }
        // Wait for state change to complete (up to 500ms)
        let _ = self.pipeline.state(gst::ClockTime::from_mseconds(500));
    }

    /// Update a specific core's volume and pitch based on its CPU usage
    pub fn update_core(&mut self, core_index: usize, target_value: f64) {
        if core_index >= self.volume_elements.len() {
            return;
        }

        let target = target_value.clamp(0.0, 1.0);
        self.current_values[core_index] +=
            (target - self.current_values[core_index]) * self.transition_speed;

        let smoothed = self.current_values[core_index];

        // Update volume - normalize by sqrt of cores for balanced mixing
        // Using sqrt means: 4 cores divides by 2, 8 cores by ~2.8, 16 cores by 4
        // This keeps individual cores audible while preventing excessive summing
        let num_cores = self.volume_elements.len() as f64;
        let volume = (smoothed * self.master_volume) / num_cores.sqrt();
        self.volume_elements[core_index].set_property("volume", volume.clamp(0.0, 1.0));

        // Update pitch if frequency fluctuation is enabled
        if self.frequency_fluctuation {
            // Map 0.0-1.0 to pitch range 0.8-1.2
            let pitch = 0.8 + smoothed * 0.4;
            self.pitch_elements[core_index].set_property("pitch", pitch);
        }
    }

    pub fn set_master_volume(&mut self, volume: f64) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        for v in &mut self.current_values {
            *v = 0.0;
        }
    }

    pub fn core_count(&self) -> usize {
        self.volume_elements.len()
    }
}

impl Drop for PerCoreCpuPlayer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// A single audio channel that can operate in different modes
pub struct AudioChannel {
    mode: SoundMode,
    /// Primary sound (volume mode: the sound, fade mode: idle sound)
    primary: Option<PlaybackElement>,
    /// Secondary sound (fade mode only: active sound)
    secondary: Option<PlaybackElement>,
    /// Current smoothed value for transitions
    current_value: f64,
    /// Transition speed (derived from SlideInterval)
    transition_speed: f64,
    /// Enable frequency/pitch fluctuation
    frequency_fluctuation: bool,
    /// Master volume multiplier
    master_volume: f64,
}

impl AudioChannel {
    pub fn new(
        mode: SoundMode,
        primary_path: Option<&Path>,
        secondary_path: Option<&Path>,
        slide_interval: u32,
        frequency_fluctuation: bool,
        pan: f64,
    ) -> Result<Self, gst::glib::BoolError> {
        let primary = primary_path
            .map(|p| PlaybackElement::new(p, pan))
            .transpose()?;
        let secondary = secondary_path
            .map(|p| PlaybackElement::new(p, pan))
            .transpose()?;

        // Convert SlideInterval to transition speed
        // Higher SlideInterval = slower transitions
        let transition_speed = 1.0 / (slide_interval as f64).max(1.0);

        Ok(Self {
            mode,
            primary,
            secondary,
            current_value: 0.0,
            transition_speed,
            frequency_fluctuation,
            master_volume: 1.0,
        })
    }

    pub fn play(&self) {
        // Play primary first, then secondary
        // Each call waits for state change to complete
        if let Some(ref p) = self.primary {
            p.play();
        }
        if let Some(ref s) = self.secondary {
            s.play();
        }
    }

    pub fn stop(&self) {
        // Stop both elements, each waits for state change
        if let Some(ref p) = self.primary {
            p.stop();
        }
        if let Some(ref s) = self.secondary {
            s.stop();
        }
    }

    /// Update the channel with a new metric value (0.0 to 1.0)
    pub fn update(&mut self, target_value: f64) {
        let target = target_value.clamp(0.0, 1.0);

        // Smooth transition
        self.current_value += (target - self.current_value) * self.transition_speed;

        match self.mode {
            SoundMode::Disabled => {
                // Do nothing
            }
            SoundMode::Volume => {
                // Volume mode: modulate volume based on metric
                if let Some(ref p) = self.primary {
                    p.set_volume(self.current_value * self.master_volume);

                    // Apply frequency fluctuation if enabled
                    if self.frequency_fluctuation {
                        // Map 0-1 to pitch range 0.8-1.2
                        let rate = 0.8 + self.current_value * 0.4;
                        p.set_rate(rate);
                    }
                }
            }
            SoundMode::Fade => {
                // Fade mode: crossfade between idle and active sounds
                let idle_vol = (1.0 - self.current_value) * self.master_volume;
                let active_vol = self.current_value * self.master_volume;

                if let Some(ref p) = self.primary {
                    p.set_volume(idle_vol);
                }
                if let Some(ref s) = self.secondary {
                    s.set_volume(active_vol);
                }

                // Apply frequency fluctuation to active sound if enabled
                if self.frequency_fluctuation {
                    if let Some(ref s) = self.secondary {
                        let rate = 0.8 + self.current_value * 0.4;
                        s.set_rate(rate);
                    }
                }
            }
        }
    }

    pub fn set_master_volume(&mut self, volume: f64) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        self.current_value = 0.0;
    }

    pub fn is_enabled(&self) -> bool {
        self.mode != SoundMode::Disabled && self.primary.is_some()
    }
}

/// CPU playback mode - either single averaged channel or per-core with perfect sync
pub enum CpuPlayback {
    /// Single channel for averaged CPU mode
    Averaged(AudioChannel),
    /// Per-core mode with single source split to multiple panned outputs
    PerCore(PerCoreCpuPlayer),
}

/// Manages multiple audio channels
pub struct AudioMixer {
    /// CPU playback - either averaged or per-core
    pub cpu_playback: Option<CpuPlayback>,
    pub ram_channel: Option<AudioChannel>,
    pub disk_channel: Option<AudioChannel>,
    master_volume: f64,
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            cpu_playback: None,
            ram_channel: None,
            disk_channel: None,
            master_volume: 1.0,
        }
    }

    pub fn play_all(&self) {
        match &self.cpu_playback {
            Some(CpuPlayback::Averaged(ch)) => ch.play(),
            Some(CpuPlayback::PerCore(player)) => player.play(),
            None => {}
        }
        if let Some(ref ch) = self.ram_channel {
            ch.play();
        }
        if let Some(ref ch) = self.disk_channel {
            ch.play();
        }
    }

    pub fn stop_all(&self) {
        match &self.cpu_playback {
            Some(CpuPlayback::Averaged(ch)) => ch.stop(),
            Some(CpuPlayback::PerCore(player)) => player.stop(),
            None => {}
        }
        if let Some(ref ch) = self.ram_channel {
            ch.stop();
        }
        if let Some(ref ch) = self.disk_channel {
            ch.stop();
        }
    }

    pub fn set_master_volume(&mut self, volume: f64) {
        self.master_volume = volume.clamp(0.0, 1.0);
        match &mut self.cpu_playback {
            Some(CpuPlayback::Averaged(ch)) => ch.set_master_volume(self.master_volume),
            Some(CpuPlayback::PerCore(player)) => player.set_master_volume(self.master_volume),
            None => {}
        }
        if let Some(ref mut ch) = self.ram_channel {
            ch.set_master_volume(self.master_volume);
        }
        if let Some(ref mut ch) = self.disk_channel {
            ch.set_master_volume(self.master_volume);
        }
    }

    pub fn clear(&mut self) {
        self.stop_all();
        self.cpu_playback = None;
        self.ram_channel = None;
        self.disk_channel = None;
    }
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}
