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

use momotone_engine::{Layers, Pattern, PitchClass, Plan, STYLES};
use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

mod ai;
mod editor;
mod hwout;
mod link;
mod params;
mod player;
use params::{MomotoneParams, RoleChoice};
use hwout::{HwState, Route, Router};
use player::{Block, Player};
use std::time::Instant;

/// Parts (and the harmony) pinned to the idea they had when locked. `None` = follows the
/// current idea. Saved with the project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Locks {
    pub harmony: Option<u64>,
    pub parts: [Option<u64>; momotone_engine::Layer::COUNT],
}

impl Locks {
    pub fn count(&self) -> usize {
        self.harmony.is_some() as usize + self.parts.iter().filter(|p| p.is_some()).count()
    }
}

/// Everything that changes which notes are generated (but not how they're played back).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenSettings {
    pub style: usize,
    pub key: usize,
    pub bars: u32,
    /// The idea number on the Idea knob.
    pub seed: u64,
    pub locks: Locks,
    /// AI harmony in use (its `rev`), 0 = the style's own progressions.
    pub ai: u16,
    /// 0 = loop, 1..=6 a section (intro, groove, build, drop, break, outro), 7 = the song.
    pub section: u8,
    /// Energy in percent, so dragging the knob doesn't flood the background thread.
    pub energy: u8,
    pub layers: Layers,
}

pub fn plan_for(s: &GenSettings) -> Plan {
    let style = &STYLES[s.style.min(STYLES.len() - 1)];
    let mut plan = style.plan(PitchClass::ALL[s.key % 12], s.bars, s.energy as f64 / 100.0, s.harmony_seed());
    if let Some(h) = ai::lookup(s.ai) {
        h.apply(&mut plan);
    }
    plan
}

pub fn render(s: &GenSettings) -> Pattern {
    let plan = plan_for(s);
    match params::SectionChoice::from_index(s.section) {
        params::SectionChoice::Loop => momotone_engine::generate_seeded(&plan, s.layers, s.part_seeds()),
        params::SectionChoice::Song => momotone_engine::song::song(&plan, s.layers, s.part_seeds()),
        c => momotone_engine::song::section_loop(&plan, s.layers, s.part_seeds(), c.section().unwrap()),
    }
}

impl GenSettings {
    /// The idea the chord progression comes from.
    pub fn harmony_seed(&self) -> u64 {
        self.locks.harmony.unwrap_or(self.seed)
    }

    /// The idea each part's own choices come from.
    pub fn part_seeds(&self) -> [u64; momotone_engine::Layer::COUNT] {
        std::array::from_fn(|i| self.locks.parts[i].unwrap_or(self.seed))
    }
}

fn settings_without_locks(p: &MomotoneParams) -> GenSettings {
    GenSettings { locks: Locks::default(), ai: 0, ..settings_from_parts(p) }
}

/// Reads the parameters that shape the music. Used by the audio thread and the editor,
/// so both always see the very same pattern.
pub fn settings_from(p: &MomotoneParams) -> GenSettings {
    settings_from_parts(p)
}

fn settings_from_parts(p: &MomotoneParams) -> GenSettings {
    GenSettings {
        style: p.style.value().max(0) as usize,
        key: p.key.value() as usize,
        bars: p.bars.value() as u32,
        seed: p.variation.value() as u64,
        locks: p.locks.read().map(|l| *l).unwrap_or_default(),
        ai: p.ai.read().ok().and_then(|a| a.as_ref().map(|h| h.rev)).unwrap_or(0),
        energy: (p.energy.value() * 100.0).round().clamp(0.0, 100.0) as u8,
        section: p.section.value() as u8,
        layers: Layers {
            chords: p.chords.value(),
            bass: p.bass.value(),
            arp: p.arp.value(),
            melody: p.melody.value(),
            drums: p.drums.value(),
        },
    }
}

/// Transport info published by the audio thread for the editor (playhead, tempo).
#[derive(Default)]
pub struct UiShared {
    /// Another main Momotone is publishing in this instance's group.
    conflict: AtomicBool,
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

    pub fn conflict(&self) -> bool {
        self.conflict.load(Ordering::Relaxed)
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

/// Renders requested patterns into `shared.slot`, newest request first; exits when the
/// plugin drops the sender.
fn spawn_renderer(shared: Arc<Shared>, own_ai: Arc<std::sync::RwLock<Option<ai::AiHarmony>>>, mut rx: rtrb::Consumer<GenSettings>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("momotone-render".into())
        .spawn(move || loop {
            let mut latest = None;
            while let Ok(s) = rx.pop() {
                latest = Some(s);
            }
            match latest {
                Some(settings) => {
                    // Our own AI harmony (also after a set loads), for us and our followers.
                    ai::register_own(&own_ai);
                    // A follower may hear of its main's AI harmony a moment before the main
                    // registers it: give that a short while.
                    for _ in 0..50 {
                        if settings.ai == 0 || ai::lookup(settings.ai).is_some() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    let pattern = render(&settings);
                    if let Ok(mut slot) = shared.slot.lock() {
                        // Dropping the previous contents here keeps deallocation off the audio thread.
                        *slot = Some((settings, pattern));
                        shared.fresh.store(true, Ordering::Release);
                    }
                }
                None if rx.is_abandoned() => return,
                None => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        })
        .expect("spawn render thread")
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Option<(GenSettings, Pattern)>>,
    fresh: AtomicBool,
}

pub struct Momotone {
    params: Arc<MomotoneParams>,
    shared: Arc<Shared>,
    ui: Arc<UiShared>,
    id: u64,
    pattern: Pattern,
    /// What `pattern` was rendered from.
    current: Option<GenSettings>,
    requested: Option<GenSettings>,
    /// Beat of the bar line on which the pending pattern may come in.
    switch_at: f64,
    last_beat: f64,
    sample_rate: f64,
    player: Player,
    /// Direct hardware outputs.
    hw: Arc<HwState>,
    hw_tx: Option<hwout::Producer>,
    /// Last locks and AI harmony read on the audio thread.
    locks: Locks,
    ai: u16,
    hw_thread: Option<std::thread::JoinHandle<()>>,
    /// Requests to our own render thread (see `spawn_renderer`).
    render_tx: Option<rtrb::Producer<GenSettings>>,
    render_thread: Option<std::thread::JoinHandle<()>>,
    router: Router,
    routes: [Option<Route>; hwout::PARTS],
}

impl Default for Momotone {
    fn default() -> Self {
        Self {
            params: Arc::new(MomotoneParams::default()),
            shared: Arc::new(Shared::default()),
            ui: Arc::new(UiShared::default()),
            id: link::new_instance_id(),
            pattern: Pattern::default(),
            current: None,
            requested: None,
            switch_at: f64::NEG_INFINITY,
            last_beat: 0.0,
            sample_rate: 44_100.0,
            player: Player::default(),
            hw: Arc::new(HwState::default()),
            hw_tx: None,
            locks: Locks::default(),
            ai: 0,
            hw_thread: None,
            render_tx: None,
            render_thread: None,
            router: Router::default(),
            routes: [None; hwout::PARTS],
        }
    }
}

impl Momotone {
    /// On the audio thread: never waits on the editor holding the locks.
    fn settings(&mut self) -> GenSettings {
        if let Ok(l) = self.params.locks.try_read() {
            self.locks = *l;
        }
        if let Ok(a) = self.params.ai.try_read() {
            self.ai = a.as_ref().map_or(0, |h| h.rev);
        }
        GenSettings { locks: self.locks, ai: self.ai, ..settings_without_locks(&self.params) }
    }

    fn is_follower(&self) -> bool {
        self.params.role.value() == RoleChoice::Follower
    }

    fn group(&self) -> usize {
        (self.params.group.value() - 1).max(0) as usize
    }

    /// What this instance should play now, and from which beat: its own settings for a main,
    /// its part of the group's main for a follower (`None` while there is no main yet).
    fn wanted(&mut self, beat: f64, playing: bool) -> Option<(GenSettings, f64)> {
        if self.is_follower() {
            self.ui.conflict.store(false, Ordering::Relaxed);
            let (mut s, at) = link::read(self.group())?;
            s.layers = link::part_layers(self.params.part.value().layer());
            Some((s, at))
        } else {
            let s = self.settings();
            let at = if self.requested == Some(s) { self.switch_at } else { link::switch_point(beat, playing) };
            let alone = link::publish(self.group(), self.id, &s, at);
            self.ui.conflict.store(!alone, Ordering::Relaxed);
            Some((s, at))
        }
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
            Some((s, p)) => {
                std::mem::swap(&mut self.pattern, p);
                self.current = Some(*s);
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

impl Drop for Momotone {
    /// Stop the MIDI sender before the plugin (and maybe its library) goes away: it sends
    /// "all notes off" to the machines, then exits.
    fn drop(&mut self) {
        self.hw_tx = None;
        self.render_tx = None;
        for thread in [self.hw_thread.take(), self.render_thread.take()].into_iter().flatten() {
            let _ = thread.join();
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
    // Patterns are rendered on our own thread (see `spawn_renderer`), not nih-plug's shared
    // background worker: that worker can abort the host when a plugin instance is removed
    // right after scheduling a task.
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.ui.clone(), self.hw.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        // The host calls this again after loading a set: register its AI harmony first, so
        // this render (and our followers) use it.
        ai::register_own(&self.params.ai);
        let s = self.settings();
        self.pattern = render(&s);
        self.current = Some(s);
        self.requested = Some(s);
        if self.render_tx.is_none() {
            let (tx, rx) = rtrb::RingBuffer::new(64);
            self.render_thread = Some(spawn_renderer(self.shared.clone(), self.params.ai.clone(), rx));
            self.render_tx = Some(tx);
        }
        if self.hw_tx.is_none() {
            // The sender thread owns the MIDI ports; it stops when the plugin drops `hw_tx`.
            let (tx, rx) = hwout::queue();
            let thread = hwout::spawn_sender(self.params.hw.clone(), self.hw.clone(), rx, Box::new(hwout::MidirSink::default()));
            self.hw_tx = Some(tx);
            self.hw_thread = Some(thread);
        }
        true
    }

    fn reset(&mut self) {
        // The host has stopped/restarted processing; any sounding notes are gone already.
        self.player.reset();
        // Hardware keeps playing what it was sent, so close those notes explicitly.
        if let Some(tx) = self.hw_tx.as_mut() {
            self.router.release_all(Instant::now(), self.sample_rate, tx);
        }
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

        let transport = context.transport();
        let p = &self.params;
        let block = Block {
            start_beat: transport.pos_beats().unwrap_or(self.player.next_beat()),
            beats_per_sample: transport.tempo.unwrap_or(120.0).max(1.0) / 60.0 / self.sample_rate,
            samples: buffer.samples(),
            octaves: [p.oct_chords.value(), p.oct_bass.value(), p.oct_arp.value(), p.oct_melody.value(), 0],
            split_channels: p.split_channels.value(),
            mask: [true; momotone_engine::Layer::COUNT],
            channel: None,
        };
        // Parts routed to hardware leave the DAW output.
        if let Ok(cfg) = p.hw.try_read() {
            let routes = cfg.routes();
            if routes != self.routes {
                self.player.release_all(0);
                self.routes = routes;
            }
        }
        let block = Block { mask: Router::host_mask(&self.routes), ..block };
        let t0 = self.router.block_time(Instant::now(), block.samples, self.sample_rate);
        let tempo = transport.tempo.unwrap_or(120.0).max(1.0);

        self.ui.beat.store(block.start_beat.to_bits(), Ordering::Relaxed);
        self.ui.tempo.store(transport.tempo.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
        self.ui.playing.store(transport.playing, Ordering::Relaxed);

        let playing = transport.playing;
        // Loop brace or seek jumped back: don't wait for a bar we may never reach.
        if playing && block.start_beat < self.last_beat - 0.01 {
            self.switch_at = f64::NEG_INFINITY;
        }
        self.last_beat = block.end_beat();

        match self.wanted(block.start_beat, playing) {
            Some((settings, at)) if self.requested != Some(settings) => {
                // If the queue is full, try again on the next block.
                if self.render_tx.as_mut().is_some_and(|tx| tx.push(settings).is_ok()) {
                    self.requested = Some(settings);
                    self.switch_at = at;
                }
            }
            Some(_) => {}
            // Follower without a main: stay silent.
            None => {
                self.requested = None;
                self.player.stop();
                if let Some(tx) = self.hw_tx.as_mut() {
                    self.router.process(&self.pattern, block, self.routes, tempo, self.sample_rate, false, t0, tx);
                }
                self.flush(context);
                return ProcessStatus::Normal;
            }
        }

        // New ideas come in on the bar line (the same one for a main and its followers).
        if link::swap_now(playing, &block, self.switch_at) && self.swap_in_pending() && playing {
            self.player.release_all(0);
            if let Some(tx) = self.hw_tx.as_mut() {
                self.router.release_all(t0, self.sample_rate, tx);
            }
        }
        // A follower stays silent until it holds its own part of the main's loop
        // (just after loading, or after switching role or part).
        let ready = !self.is_follower()
            || self.current.map(|c| c.layers) == Some(link::part_layers(self.params.part.value().layer()));
        if playing && ready {
            self.player.play(&self.pattern, block);
        } else {
            self.player.stop();
        }
        if let Some(tx) = self.hw_tx.as_mut() {
            let all = Block { mask: [true; momotone_engine::Layer::COUNT], ..block };
            self.router.process(&self.pattern, all, self.routes, tempo, self.sample_rate, playing && ready, t0, tx);
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

    #[test]
    fn style_param_shows_names() {
        let p = MomotoneParams::default();
        assert_eq!(STYLES[p.style.value() as usize].id, "ukg");
        assert_eq!(p.style.normalized_value_to_string(0.0, false), STYLES[0].label);
        assert_eq!(p.style.string_to_normalized_value("Dub Techno").map(|v| p.style.preview_plain(v)),
            momotone_engine::style::index_of("dub-techno").map(|i| i as i32));
    }

    #[test]
    fn follower_plays_exactly_the_mains_part() {
        let mut locks = Locks::default();
        locks.harmony = Some(40);
        locks.parts[1] = Some(77);
        let main = GenSettings { style: 0, key: 9, bars: 4, seed: 12, energy: 65, layers: Layers::all(), locks, ai: 0, section: 3 };
        let shared = GenSettings { locks: link::unpack_locks(link::pack_locks(&main.locks)), ..link::unpack(link::pack(&main)).unwrap() };
        for layer in momotone_engine::Layer::ALL {
            let follower = GenSettings { layers: link::part_layers(layer), ..shared };
            assert_eq!(render(&main).only(layer).notes, render(&follower).notes, "{layer:?}");
        }
    }

    #[test]
    fn render_is_deterministic() {
        let s = GenSettings { style: 0, key: 5, bars: 4, seed: 9, energy: 70, layers: Layers::all(), ..Default::default() };
        assert_eq!(render(&s).notes, render(&s).notes);
    }
}

#[cfg(test)]
mod render_fuzz {
    use super::*;

    #[test]
    #[ignore]
    fn any_parameters_render() {
        let mut x: u64 = 0x2545F4914F6CDD1D;
        let mut next = |m: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % m
        };
        for i in 0..400_000u64 {
            let s = GenSettings {
                style: next(STYLES.len() as u64) as usize,
                key: next(12) as usize,
                bars: 1 + next(8) as u32,
                seed: 1 + next(500),
                energy: next(101) as u8,
                layers: Layers { chords: next(2) == 1, bass: next(2) == 1, arp: next(2) == 1, melody: next(2) == 1, drums: next(2) == 1 },
                locks: Locks { harmony: (next(2) == 1).then(|| 1 + next(500)), parts: std::array::from_fn(|_| (next(3) == 1).then(|| 1 + next(500))) },
                ai: 0,
                section: next(8) as u8,
            };
            let r = std::panic::catch_unwind(|| render(&s));
            if r.is_err() {
                panic!("iteration {i}: {s:?}");
            }
        }
    }
}
