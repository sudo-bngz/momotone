//! Momotone plugin: plays generated loops as MIDI, locked to the host transport, one MIDI
//! channel per part so each part can drive its own machine.
//!
//! Threading model:
//! - The audio thread only reads parameters, swaps in a ready-made pattern and emits events.
//!   It never allocates (checked in debug builds by `assert_process_allocs`).
//! - When a parameter that changes the music moves, the audio thread queues a background
//!   task. That task renders the new pattern and parks it in a shared slot. The audio
//!   thread swaps it in on the next bar line, so changes land in time like a Push clip
//!   launch. The previous pattern is left in the slot and freed by the next background run.

use momotone_engine::{generate, Layers, Pattern, PitchClass, Plan, STYLES};
use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

mod editor;
mod params;
mod player;
use params::MomotoneParams;
use player::{Block, Player};

/// Everything that changes which notes are generated (but not how they're played back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenSettings {
    pub style: usize,
    pub key: usize,
    pub bars: u32,
    pub seed: u64,
    /// Energy in percent, so dragging the knob doesn't flood the background thread.
    pub energy: u8,
    pub layers: Layers,
}

pub fn plan_for(s: &GenSettings) -> Plan {
    let style = &STYLES[s.style.min(STYLES.len() - 1)];
    style.plan(PitchClass::ALL[s.key % 12], s.bars, s.energy as f64 / 100.0, s.seed)
}

pub fn render(s: &GenSettings) -> Pattern {
    generate(&plan_for(s), s.layers, s.seed)
}

/// Reads the parameters that shape the music. Used by the audio thread and the editor,
/// so both always see the very same pattern.
pub fn settings_from(p: &MomotoneParams) -> GenSettings {
    GenSettings {
        style: p.style.value().index(),
        key: p.key.value() as usize,
        bars: p.bars.value() as u32,
        seed: p.variation.value() as u64,
        energy: (p.energy.value() * 100.0).round().clamp(0.0, 100.0) as u8,
        layers: Layers { chords: p.chords.value(), bass: p.bass.value(), arp: p.arp.value(), melody: p.melody.value() },
    }
}

/// Transport info published by the audio thread for the editor (playhead, tempo).
#[derive(Default)]
pub struct UiShared {
    beat: AtomicU64,
    tempo: AtomicU64,
    playing: AtomicBool,
}

impl UiShared {
    pub fn beat(&self) -> f64 {
        f64::from_bits(self.beat.load(Ordering::Relaxed))
    }

    /// Host tempo, if the host has told us.
    pub fn tempo(&self) -> Option<f64> {
        let t = f64::from_bits(self.tempo.load(Ordering::Relaxed));
        (t > 0.0).then_some(t)
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Option<Pattern>>,
    fresh: AtomicBool,
}

pub struct Momotone {
    params: Arc<MomotoneParams>,
    shared: Arc<Shared>,
    ui: Arc<UiShared>,
    pattern: Pattern,
    requested: Option<GenSettings>,
    sample_rate: f64,
    player: Player,
}

impl Default for Momotone {
    fn default() -> Self {
        Self {
            params: Arc::new(MomotoneParams::default()),
            shared: Arc::new(Shared::default()),
            ui: Arc::new(UiShared::default()),
            pattern: Pattern::default(),
            requested: None,
            sample_rate: 44_100.0,
            player: Player::default(),
        }
    }
}

impl Momotone {
    fn settings(&self) -> GenSettings {
        settings_from(&self.params)
    }

    /// Takes a freshly rendered pattern if one is ready. Returns true if it changed.
    fn swap_in_pending(&mut self) -> bool {
        if !self.shared.fresh.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut slot) = self.shared.slot.try_lock() else {
            return false;
        };
        match slot.as_mut() {
            Some(p) => {
                std::mem::swap(&mut self.pattern, p);
                self.shared.fresh.store(false, Ordering::Release);
                true
            }
            None => false,
        }
    }

    fn flush(&mut self, context: &mut impl ProcessContext<Self>) {
        for e in self.player.events.drain(..) {
            let event = if e.on {
                NoteEvent::NoteOn { timing: e.offset, voice_id: None, channel: e.channel, note: e.note, velocity: e.velocity }
            } else {
                NoteEvent::NoteOff { timing: e.offset, voice_id: None, channel: e.channel, note: e.note, velocity: 0.0 }
            };
            context.send_event(event);
        }
    }
}

impl Plugin for Momotone {
    const NAME: &'static str = "Momotone";
    const VENDOR: &'static str = "Momo's Basement";
    const URL: &'static str = "https://github.com/sudo-bngz/momotone";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // A silent stereo output: lets hosts (Ableton, Logic, FL...) load this as an instrument.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;

    type SysExMessage = ();
    type BackgroundTask = GenSettings;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.ui.clone())
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        let shared = self.shared.clone();
        Box::new(move |settings| {
            let pattern = render(&settings);
            if let Ok(mut slot) = shared.slot.lock() {
                // Dropping the previous contents here keeps deallocation off the audio thread.
                *slot = Some(pattern);
                shared.fresh.store(true, Ordering::Release);
            }
        })
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        let s = self.settings();
        self.pattern = render(&s);
        self.requested = Some(s);
        true
    }

    fn reset(&mut self) {
        // The host has stopped/restarted processing; any sounding notes are gone already.
        self.player.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel in buffer.as_slice() {
            channel.fill(0.0);
        }
        // Incoming MIDI is ignored for now (a "follow the key I play" mode could go here).
        while context.next_event().is_some() {}

        let settings = self.settings();
        if self.requested != Some(settings) {
            context.execute_background(settings);
            self.requested = Some(settings);
        }

        let transport = context.transport();
        let p = &self.params;
        let block = Block {
            start_beat: transport.pos_beats().unwrap_or(self.player.next_beat()),
            beats_per_sample: transport.tempo.unwrap_or(120.0).max(1.0) / 60.0 / self.sample_rate,
            samples: buffer.samples(),
            octaves: [p.oct_chords.value(), p.oct_bass.value(), p.oct_arp.value(), p.oct_melody.value()],
            split_channels: p.split_channels.value(),
        };

        self.ui.beat.store(block.start_beat.to_bits(), Ordering::Relaxed);
        self.ui.tempo.store(transport.tempo.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
        self.ui.playing.store(transport.playing, Ordering::Relaxed);

        if transport.playing {
            // New ideas come in on the bar line, so they stay in time.
            if block.touches_bar_line() && self.swap_in_pending() {
                self.player.release_all(0);
            }
            self.player.play(&self.pattern, block);
        } else {
            self.swap_in_pending();
            self.player.stop();
        }
        self.flush(context);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Momotone {
    const CLAP_ID: &'static str = "com.momosbasement.momotone";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Genre-aware chord, bass, arp and melody loop generator");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::NoteEffect, ClapFeature::Utility];
}

impl Vst3Plugin for Momotone {
    const VST3_CLASS_ID: [u8; 16] = *b"MomotoneMMBTGen1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Generator];
}

nih_export_clap!(Momotone);
nih_export_vst3!(Momotone);

#[cfg(test)]
mod tests {
    use super::*;
    use params::StyleChoice;

    #[test]
    fn style_choices_match_engine() {
        assert_eq!(StyleChoice::variants().len(), STYLES.len());
        for (i, name) in StyleChoice::variants().iter().enumerate() {
            assert_eq!(*name, STYLES[i].label);
        }
    }

    #[test]
    fn render_is_deterministic() {
        let s = GenSettings { style: 0, key: 5, bars: 4, seed: 9, energy: 70, layers: Layers::all() };
        assert_eq!(render(&s).notes, render(&s).notes);
    }
}
