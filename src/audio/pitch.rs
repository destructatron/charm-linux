//! Granular pitch shifter - a lightweight alternative to SoundTouch/phase vocoder
//!
//! This module provides a simple, CPU-efficient pitch shifting algorithm suitable
//! for real-time audio processing. It uses a two-pointer granular synthesis approach
//! with crossfading to avoid discontinuities.
//!
//! The algorithm is optimized for subtle pitch variations (0.5x - 2.0x) on ambient
//! sounds like those used for system monitoring feedback.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use gstreamer_audio as gst_audio;
use gstreamer_base as gst_base;
use gstreamer_base::subclass::prelude::*;
use once_cell::sync::Lazy;
use std::sync::Mutex;

// Re-export glib from gstreamer to avoid version conflicts with GTK's glib
use gst::glib;

/// A single grain reader with its own position and phase
struct GrainReader {
    /// Current read position in the buffer (fractional)
    read_pos: f64,
    /// Current position within the grain cycle (0.0 to 1.0)
    grain_phase: f64,
}

impl GrainReader {
    fn new(read_pos: f64, grain_phase: f64) -> Self {
        Self { read_pos, grain_phase }
    }
}

/// Core granular pitch shifting algorithm
///
/// Uses two grain readers with overlapping windows to produce smooth
/// pitch-shifted output. Each grain reads from the delay buffer at the
/// pitch rate, and when it completes its cycle, it resets to a new position.
pub struct GranularPitchShifter {
    /// Circular buffer holding input samples
    buffer: Vec<f32>,
    /// Write position in the circular buffer
    write_pos: usize,
    /// First grain reader
    grain_a: GrainReader,
    /// Second grain reader (offset by 0.5 in phase)
    grain_b: GrainReader,
    /// Grain size in samples
    grain_size: usize,
    /// Current pitch ratio (1.0 = no change, 2.0 = octave up, 0.5 = octave down)
    pitch_ratio: f64,
    /// Fixed delay in samples (how far behind write position we read)
    delay_samples: usize,
    /// Number of samples written (for initialization)
    samples_written: usize,
}

impl GranularPitchShifter {
    /// Create a new pitch shifter
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `grain_ms` - Grain size in milliseconds (10-50ms recommended)
    pub fn new(sample_rate: u32, grain_ms: f64) -> Self {
        let grain_size = ((sample_rate as f64 * grain_ms) / 1000.0) as usize;
        // Buffer needs to hold enough for delay + grain overlap
        let delay_samples = grain_size;
        let buffer_size = grain_size * 4;

        // Initial read position: delay_samples behind where write will be
        let initial_read_pos = 0.0;

        Self {
            buffer: vec![0.0; buffer_size],
            write_pos: delay_samples, // Start write position ahead
            grain_a: GrainReader::new(initial_read_pos, 0.0),
            grain_b: GrainReader::new(initial_read_pos, 0.5), // 50% phase offset
            grain_size,
            pitch_ratio: 1.0,
            delay_samples,
            samples_written: 0,
        }
    }

    /// Set the pitch ratio
    ///
    /// # Arguments
    /// * `ratio` - Pitch multiplier (0.5 = octave down, 1.0 = no change, 2.0 = octave up)
    pub fn set_pitch_ratio(&mut self, ratio: f64) {
        self.pitch_ratio = ratio.clamp(0.25, 4.0);
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let buffer_len = self.buffer.len();

        // Write input to circular buffer
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % buffer_len;
        self.samples_written = self.samples_written.saturating_add(1);

        // During initial fill, just output silence
        if self.samples_written < self.delay_samples + self.grain_size {
            return 0.0;
        }

        // Passthrough optimization: at exactly 1.0 pitch, just read with fixed delay
        if (self.pitch_ratio - 1.0).abs() < 0.001 {
            let read_pos = (self.write_pos + buffer_len - self.delay_samples) % buffer_len;
            return self.buffer[read_pos];
        }

        // Read samples from both grains
        let sample_a = self.read_interpolated(self.grain_a.read_pos);
        let sample_b = self.read_interpolated(self.grain_b.read_pos);

        // Calculate crossfade using Hann window based on grain phase
        // Grain A: fades in from 0.0 to 0.5, fades out from 0.5 to 1.0
        // Grain B: offset by 0.5, so when A is fading out, B is fading in
        let fade_a = hann_fade(self.grain_a.grain_phase);
        let fade_b = hann_fade(self.grain_b.grain_phase);

        // Mix the grains
        let output = sample_a * fade_a + sample_b * fade_b;

        // Advance grain phases and read positions
        let phase_increment = 1.0 / self.grain_size as f64;

        self.grain_a.grain_phase += phase_increment;
        self.grain_b.grain_phase += phase_increment;

        self.grain_a.read_pos += self.pitch_ratio;
        self.grain_b.read_pos += self.pitch_ratio;

        // Wrap read positions within buffer
        if self.grain_a.read_pos >= buffer_len as f64 {
            self.grain_a.read_pos -= buffer_len as f64;
        }
        if self.grain_b.read_pos >= buffer_len as f64 {
            self.grain_b.read_pos -= buffer_len as f64;
        }

        // When a grain completes its cycle, reset it
        if self.grain_a.grain_phase >= 1.0 {
            self.grain_a.grain_phase -= 1.0;
            // Reset read position to current delay position
            self.grain_a.read_pos = ((self.write_pos + buffer_len - self.delay_samples) % buffer_len) as f64;
        }
        if self.grain_b.grain_phase >= 1.0 {
            self.grain_b.grain_phase -= 1.0;
            // Reset read position to current delay position
            self.grain_b.read_pos = ((self.write_pos + buffer_len - self.delay_samples) % buffer_len) as f64;
        }

        output
    }

    /// Read from buffer with linear interpolation
    fn read_interpolated(&self, pos: f64) -> f32 {
        let buffer_len = self.buffer.len();
        let pos_wrapped = pos.rem_euclid(buffer_len as f64);
        let index = pos_wrapped as usize;
        let frac = pos_wrapped.fract() as f32;
        let next_index = (index + 1) % buffer_len;

        let s0 = self.buffer[index];
        let s1 = self.buffer[next_index];

        s0 + (s1 - s0) * frac
    }

    /// Reset the shifter state
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = self.delay_samples;
        self.grain_a = GrainReader::new(0.0, 0.0);
        self.grain_b = GrainReader::new(0.0, 0.5);
        self.samples_written = 0;
    }
}

/// Hann window function for smooth crossfading
/// Input: phase from 0.0 to 1.0
/// Output: 0.0 at edges, 1.0 at center (0.5)
fn hann_fade(phase: f64) -> f32 {
    // Hann window: 0.5 * (1 - cos(2 * pi * phase))
    // This gives 0 at phase=0 and phase=1, and 1 at phase=0.5
    (0.5 * (1.0 - (2.0 * std::f64::consts::PI * phase).cos())) as f32
}

// ============================================================================
// GStreamer Element Implementation
// ============================================================================

/// GStreamer element that wraps the granular pitch shifter
#[derive(Default)]
pub struct GranularPitch {
    state: Mutex<Option<GranularPitchShifter>>,
    pitch_ratio: Mutex<f64>,
}

#[glib::object_subclass]
impl ObjectSubclass for GranularPitch {
    const NAME: &'static str = "CharmGranularPitch";
    type Type = super::GranularPitchElement;
    type ParentType = gst_base::BaseTransform;
}

impl ObjectImpl for GranularPitch {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![glib::ParamSpecDouble::builder("pitch")
                .nick("Pitch")
                .blurb("Pitch ratio (1.0 = no change)")
                .minimum(0.25)
                .maximum(4.0)
                .default_value(1.0)
                .mutable_playing()
                .build()]
        });
        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        match pspec.name() {
            "pitch" => {
                let pitch = value.get::<f64>().expect("pitch must be f64");
                *self.pitch_ratio.lock().unwrap() = pitch;
                if let Some(ref mut shifter) = *self.state.lock().unwrap() {
                    shifter.set_pitch_ratio(pitch);
                }
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "pitch" => self.pitch_ratio.lock().unwrap().to_value(),
            _ => unimplemented!(),
        }
    }
}

impl GstObjectImpl for GranularPitch {}

impl ElementImpl for GranularPitch {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
            gst::subclass::ElementMetadata::new(
                "Granular Pitch Shifter",
                "Filter/Effect/Audio",
                "Lightweight granular pitch shifting for real-time audio",
                "Charm Linux",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
            let caps = gst::Caps::builder("audio/x-raw")
                .field("format", gst_audio::AUDIO_FORMAT_F32.to_str())
                .field("rate", gst::IntRange::new(8000i32, 192000i32))
                .field("channels", gst::IntRange::new(1i32, 2i32))
                .field("layout", "interleaved")
                .build();

            vec![
                gst::PadTemplate::new("sink", gst::PadDirection::Sink, gst::PadPresence::Always, &caps).unwrap(),
                gst::PadTemplate::new("src", gst::PadDirection::Src, gst::PadPresence::Always, &caps).unwrap(),
            ]
        });
        PAD_TEMPLATES.as_ref()
    }
}

impl BaseTransformImpl for GranularPitch {
    const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
    const PASSTHROUGH_ON_SAME_CAPS: bool = false;
    const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;

    fn set_caps(&self, incaps: &gst::Caps, _outcaps: &gst::Caps) -> Result<(), gst::LoggableError> {
        let info = gst_audio::AudioInfo::from_caps(incaps)
            .map_err(|_| gst::loggable_error!(gst::CAT_RUST, "Failed to parse caps"))?;

        let sample_rate = info.rate();
        let grain_ms = 25.0; // 25ms grains

        let mut shifter = GranularPitchShifter::new(sample_rate, grain_ms);
        shifter.set_pitch_ratio(*self.pitch_ratio.lock().unwrap());

        *self.state.lock().unwrap() = Some(shifter);

        Ok(())
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        *self.state.lock().unwrap() = None;
        Ok(())
    }

    fn transform_ip(&self, buf: &mut gst::BufferRef) -> Result<gst::FlowSuccess, gst::FlowError> {
        let mut state_guard = self.state.lock().unwrap();
        let shifter = state_guard.as_mut().ok_or_else(|| {
            gst::element_imp_error!(self, gst::CoreError::Negotiation, ["Not negotiated yet"]);
            gst::FlowError::NotNegotiated
        })?;

        // Update pitch ratio in case it changed
        shifter.set_pitch_ratio(*self.pitch_ratio.lock().unwrap());

        let mut map = buf.map_writable().map_err(|_| {
            gst::element_imp_error!(self, gst::LibraryError::Failed, ["Failed to map buffer"]);
            gst::FlowError::Error
        })?;

        // Get the raw bytes and reinterpret as f32 samples
        let data = map.as_mut_slice();
        let samples: &mut [f32] = unsafe {
            std::slice::from_raw_parts_mut(
                data.as_mut_ptr() as *mut f32,
                data.len() / std::mem::size_of::<f32>(),
            )
        };

        // Process each sample
        for sample in samples.iter_mut() {
            *sample = shifter.process_sample(*sample);
        }

        Ok(gst::FlowSuccess::Ok)
    }
}

glib::wrapper! {
    pub struct GranularPitchElement(ObjectSubclass<GranularPitch>) @extends gst_base::BaseTransform, gst::Element, gst::Object;
}

impl GranularPitchElement {
    /// Register the element with GStreamer
    pub fn register() -> Result<(), glib::BoolError> {
        gst::Element::register(
            None,
            "granularpitch",
            gst::Rank::NONE,
            Self::static_type(),
        )
    }
}
