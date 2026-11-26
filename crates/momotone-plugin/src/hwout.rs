//! Direct hardware MIDI out: each part can skip the DAW and go straight to a MIDI port, on
//! its own channel, with its own latency offset. One Momotone then drives every machine.
//!
//! - The settings ([`HwConfig`]) are saved with the project.
//! - The audio thread ([`Router`]) plays each routed part with its own [`Player`], on a beat
//!   window shifted by the part's offset, and stamps every event with the wall-clock time it
//!   is due, derived from the sample clock. It only pushes into a lock-free queue: no MIDI
//!   calls, no locks, no allocation.
//! - A sender thread ([`spawn_sender`]) owns the ports. It sends each event when it is due,
//!   follows setting changes, and sends "all notes off" whenever a part is re-routed or the
//!   plugin goes away, so no machine is left with a hanging note.

use crate::player::{Block, Ev, Player};
use momotone_engine::Pattern;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const PARTS: usize = momotone_engine::Layer::COUNT;

/// Where one part goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartOut {
    /// MIDI output port name; `None` = through the DAW like before.
    pub port: Option<String>,
    /// 1..=16.
    pub channel: u8,
    /// Positive = later, negative = earlier, in milliseconds.
    pub offset_ms: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HwConfig {
    /// Chords, bass, arp, melody, drums. Sets saved before a part existed load with that
    /// part's defaults.
    #[serde(deserialize_with = "parts_of_any_length")]
    pub parts: [PartOut; PARTS],
}

impl Default for HwConfig {
    fn default() -> Self {
        let part = |ch| PartOut { port: None, channel: ch, offset_ms: 0.0 };
        HwConfig { parts: [part(1), part(2), part(3), part(4), part(10)] }
    }
}

fn parts_of_any_length<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[PartOut; PARTS], D::Error> {
    let saved: Vec<PartOut> = Vec::deserialize(d)?;
    let mut parts = HwConfig::default().parts;
    for (slot, p) in parts.iter_mut().zip(saved) {
        *slot = p;
    }
    Ok(parts)
}

pub const OFFSET_RANGE_MS: std::ops::RangeInclusive<f32> = -100.0..=250.0;

/// What the audio thread needs of a routed part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    /// 0-based.
    pub channel: u8,
    pub offset_ms: f32,
}

impl HwConfig {
    pub fn routes(&self) -> [Option<Route>; PARTS] {
        std::array::from_fn(|i| {
            let p = &self.parts[i];
            p.port.as_ref().map(|_| Route {
                channel: p.channel.clamp(1, 16) - 1,
                offset_ms: p.offset_ms.clamp(*OFFSET_RANGE_MS.start(), *OFFSET_RANGE_MS.end()),
            })
        })
    }
}

/// Shared between the plugin, its editor and the sender thread.
#[derive(Default)]
pub struct HwState {
    /// Problems the sender met per part ("port not found"...), for the editor.
    pub status: Mutex<[Option<String>; PARTS]>,
}

/// One MIDI message for one part, due at `due`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwMsg {
    pub due: Instant,
    pub part: u8,
    pub bytes: [u8; 3],
}

pub type Producer = rtrb::Producer<HwMsg>;
pub type Consumer = rtrb::Consumer<HwMsg>;

pub fn queue() -> (Producer, Consumer) {
    rtrb::RingBuffer::new(4096)
}

// ------------------------------------------------------------------ audio side

/// Audio-thread half: turns the pattern into timed hardware messages.
pub struct Router {
    players: [Player; PARTS],
    routed: [Option<Route>; PARTS],
    /// Wall-clock time of a sample position, to stamp events steadily even if the host's
    /// callbacks jitter.
    anchor: Option<(Instant, u64)>,
    samples: u64,
}

impl Default for Router {
    fn default() -> Self {
        Router { players: Default::default(), routed: [None; PARTS], anchor: None, samples: 0 }
    }
}

impl Router {
    /// Wall-clock time of the start of this block. Follows the sample clock, and re-anchors
    /// when it drifts more than 15 ms from the real clock (sample-rate drift, a stall).
    pub fn block_time(&mut self, now: Instant, samples: usize, sample_rate: f64) -> Instant {
        let t = match self.anchor {
            Some((at, pos)) => {
                let expected = at + Duration::from_secs_f64((self.samples - pos) as f64 / sample_rate);
                let drift = if expected > now { expected - now } else { now - expected };
                if drift > Duration::from_millis(15) {
                    self.anchor = Some((now, self.samples));
                    now
                } else {
                    expected
                }
            }
            None => {
                self.anchor = Some((now, self.samples));
                now
            }
        };
        self.samples += samples as u64;
        t
    }

    /// Which parts the DAW output should leave out (they go to hardware).
    pub fn host_mask(routes: &[Option<Route>; PARTS]) -> [bool; PARTS] {
        std::array::from_fn(|i| routes[i].is_none())
    }

    /// Note-offs for everything sounding on hardware (pattern swap, stop).
    pub fn release_all(&mut self, t0: Instant, sample_rate: f64, tx: &mut Producer) {
        for i in 0..PARTS {
            self.players[i].release_all(0);
            Self::flush(&mut self.players[i], i, t0, sample_rate, tx);
        }
    }

    /// Schedules one block of every routed part.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        pattern: &Pattern,
        base: Block,
        routes: [Option<Route>; PARTS],
        tempo: f64,
        sample_rate: f64,
        playing: bool,
        t0: Instant,
        tx: &mut Producer,
    ) {
        for i in 0..PARTS {
            // Re-routed (or un-routed): close what that part had open where it was going.
            if self.routed[i] != routes[i] {
                if self.routed[i].is_some() {
                    self.players[i].stop();
                    Self::flush(&mut self.players[i], i, t0, sample_rate, tx);
                }
                self.routed[i] = routes[i];
            }
            let Some(route) = routes[i] else { continue };
            if playing {
                // Shift the beat window: a positive offset plays the music a bit later.
                let shift = route.offset_ms as f64 / 1000.0 * tempo / 60.0;
                let mut mask = [false; PARTS];
                mask[i] = true;
                let b = Block { start_beat: base.start_beat - shift, mask, channel: Some(route.channel), ..base };
                self.players[i].play(pattern, b);
            } else {
                self.players[i].stop();
            }
            Self::flush(&mut self.players[i], i, t0, sample_rate, tx);
        }
    }

    fn flush(player: &mut Player, part: usize, t0: Instant, sample_rate: f64, tx: &mut Producer) {
        for e in player.events.drain(..) {
            let _ = tx.push(to_msg(&e, part, t0, sample_rate)); // A full queue drops rather than blocks.
        }
    }
}

fn to_msg(e: &Ev, part: usize, t0: Instant, sample_rate: f64) -> HwMsg {
    let due = t0 + Duration::from_secs_f64(e.offset as f64 / sample_rate);
    let ch = e.channel & 0x0F;
    let bytes = if e.on {
        [0x90 | ch, e.note & 0x7F, ((e.velocity * 127.0).round() as u8).clamp(1, 127)]
    } else {
        [0x80 | ch, e.note & 0x7F, 64]
    };
    HwMsg { due, part: part as u8, bytes }
}

// ------------------------------------------------------------------ sender side

/// Where the sender thread delivers messages. The real one is [`MidirSink`]; tests use a fake.
pub trait Sink: Send {
    /// Applies new routing; returns a problem per part, if any.
    fn configure(&mut self, cfg: &HwConfig) -> [Option<String>; PARTS];
    fn send(&mut self, part: usize, bytes: &[u8]);
}

fn all_notes_off(sink: &mut dyn Sink, part: usize, channel: u8) {
    let ch = (channel.clamp(1, 16) - 1) & 0x0F;
    sink.send(part, &[0xB0 | ch, 123, 0]); // All Notes Off
    sink.send(part, &[0xB0 | ch, 64, 0]); // Sustain off, for good measure
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Scheduled {
    due: Instant,
    seq: u64,
    part: u8,
    bytes: [u8; 3],
}

/// Runs the sender loop on its own thread until the plugin drops its [`Producer`].
pub fn spawn_sender(
    config: Arc<RwLock<HwConfig>>,
    state: Arc<HwState>,
    rx: Consumer,
    sink: Box<dyn Sink>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("momotone-midi-out".into())
        .spawn(move || sender_loop(config, state, rx, sink))
        .expect("spawn MIDI sender thread")
}

fn sender_loop(config: Arc<RwLock<HwConfig>>, state: Arc<HwState>, mut rx: Consumer, mut sink: Box<dyn Sink>) {
    let mut heap: BinaryHeap<Reverse<Scheduled>> = BinaryHeap::new();
    let mut seq = 0u64;
    let mut current: Option<HwConfig> = None;
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    loop {
        while let Ok(m) = rx.pop() {
            heap.push(Reverse(Scheduled { due: m.due, seq, part: m.part, bytes: m.bytes }));
            seq += 1;
        }

        let now = Instant::now();
        if now - last_poll >= Duration::from_millis(250) {
            last_poll = now;
            let cfg = config.read().map(|c| c.clone()).unwrap_or_default();
            if current.as_ref() != Some(&cfg) {
                if let Some(old) = &current {
                    for (i, (a, b)) in old.parts.iter().zip(&cfg.parts).enumerate() {
                        if a.port.is_some() && (a.port != b.port || a.channel != b.channel) {
                            all_notes_off(sink.as_mut(), i, a.channel);
                        }
                    }
                }
                let problems = sink.configure(&cfg);
                if let Ok(mut s) = state.status.lock() {
                    *s = problems;
                }
                current = Some(cfg);
            }
        }

        let now = Instant::now();
        while heap.peek().is_some_and(|Reverse(s)| s.due <= now) {
            let Reverse(s) = heap.pop().unwrap();
            sink.send(s.part as usize, &s.bytes);
        }

        if rx.is_abandoned() && heap.is_empty() && rx.is_empty() {
            if let Some(cfg) = &current {
                for (i, p) in cfg.parts.iter().enumerate() {
                    if p.port.is_some() {
                        all_notes_off(sink.as_mut(), i, p.channel);
                    }
                }
            }
            return;
        }

        let routed = current.as_ref().is_some_and(|c| c.parts.iter().any(|p| p.port.is_some()));
        let next = heap.peek().map(|Reverse(s)| s.due.saturating_duration_since(Instant::now()));
        let nap = match next {
            Some(d) => d.min(Duration::from_micros(500)),
            None if routed => Duration::from_millis(1),
            None => Duration::from_millis(20),
        };
        std::thread::sleep(nap);
    }
}

/// Real MIDI ports through `midir` (CoreMIDI, Windows MIDI, ALSA).
#[derive(Default)]
pub struct MidirSink {
    conns: HashMap<String, midir::MidiOutputConnection>,
    part_port: [Option<String>; PARTS],
}

/// Names of the MIDI output ports on this computer.
pub fn port_names() -> Vec<String> {
    let Ok(out) = midir::MidiOutput::new("Momotone") else { return Vec::new() };
    out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect()
}

fn connect(name: &str) -> Result<midir::MidiOutputConnection, String> {
    let out = midir::MidiOutput::new("Momotone").map_err(|e| e.to_string())?;
    let port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).ok().as_deref() == Some(name))
        .ok_or_else(|| format!("port '{name}' not found"))?;
    out.connect(&port, "Momotone").map_err(|e| e.to_string())
}

impl Sink for MidirSink {
    fn configure(&mut self, cfg: &HwConfig) -> [Option<String>; PARTS] {
        let wanted: Vec<&String> = cfg.parts.iter().filter_map(|p| p.port.as_ref()).collect();
        self.conns.retain(|name, _| wanted.contains(&name));
        let mut problems: [Option<String>; PARTS] = Default::default();
        for (i, p) in cfg.parts.iter().enumerate() {
            self.part_port[i] = p.port.clone();
            if let Some(name) = &p.port {
                if !self.conns.contains_key(name) {
                    match connect(name) {
                        Ok(c) => {
                            self.conns.insert(name.clone(), c);
                        }
                        Err(e) => problems[i] = Some(e),
                    }
                }
            }
        }
        problems
    }

    fn send(&mut self, part: usize, bytes: &[u8]) {
        if let Some(conn) = self.part_port[part].as_ref().and_then(|n| self.conns.get_mut(n)) {
            let _ = conn.send(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use momotone_engine::{generate, Layer, Layers, PitchClass, STYLES};

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<(Instant, usize, Vec<u8>)>>>);

    impl Sink for Fake {
        fn configure(&mut self, cfg: &HwConfig) -> [Option<String>; PARTS] {
            std::array::from_fn(|i| cfg.parts[i].port.as_deref().filter(|p| *p == "missing").map(|_| "port 'missing' not found".into()))
        }
        fn send(&mut self, part: usize, bytes: &[u8]) {
            self.0.lock().unwrap().push((Instant::now(), part, bytes.to_vec()));
        }
    }

    fn base(beat: f64, bps: f64, samples: usize) -> Block {
        Block { start_beat: beat, beats_per_sample: bps, samples, octaves: [0; 5], split_channels: true, mask: [true; 5], channel: None }
    }

    fn routed(bass: Option<Route>) -> [Option<Route>; PARTS] {
        [None, bass, None, None, None]
    }

    /// Runs the router over `beats` of music and returns the bass note-ons as (seconds from
    /// start, note).
    fn bass_ons(offset_ms: f32, beats: f64) -> Vec<(f64, u8)> {
        let plan = STYLES[0].plan(PitchClass::parse("A").unwrap(), 2, 0.6, 3);
        let pattern = generate(&plan, Layers::all(), 3);
        let (sr, tempo, n) = (48_000.0, 120.0, 256usize);
        let bps = tempo / 60.0 / sr;
        let (mut tx, mut rx) = queue();
        let mut router = Router::default();
        let start = Instant::now();
        let mut out = Vec::new();
        let mut k = 0;
        while (k as f64 * n as f64 * bps) < beats {
            let beat = k as f64 * n as f64 * bps;
            let t0 = start + Duration::from_secs_f64(k as f64 * n as f64 / sr);
            router.process(&pattern, base(beat, bps, n), routed(Some(Route { channel: 4, offset_ms })), tempo, sr, true, t0, &mut tx);
            while let Ok(m) = rx.pop() {
                assert_eq!(m.part, 1);
                assert_eq!(m.bytes[0] & 0x0F, 4, "channel override");
                if m.bytes[0] & 0xF0 == 0x90 {
                    out.push(((m.due - start).as_secs_f64(), m.bytes[1]));
                }
            }
            k += 1;
        }
        let expected: Vec<f64> = pattern.notes.iter().filter(|n| n.layer == Layer::Bass).map(|n| n.start * 0.5).collect();
        assert!(!expected.is_empty());
        out
    }

    #[test]
    fn offset_shifts_hardware_notes_in_time() {
        let straight = bass_ons(0.0, 16.0);
        let late = bass_ons(100.0, 16.0);
        let early = bass_ons(-40.0, 16.0);
        // Same notes; times shifted by the offset (within one sample).
        let pairs = |a: &[(f64, u8)], b: &[(f64, u8)], d: f64| {
            let mut matched = 0;
            for (t, n) in a {
                if let Some((u, m)) = b.iter().find(|(u, _)| (u - (t + d)).abs() < 1.0 / 48_000.0 + 1e-9) {
                    assert_eq!(n, m);
                    let _ = u;
                    matched += 1;
                }
            }
            matched
        };
        assert!(pairs(&straight, &late, 0.1) >= straight.len() - 2, "late");
        assert!(pairs(&straight, &early, -0.04) >= straight.len() - 2, "early");
    }

    #[test]
    fn host_skips_routed_parts() {
        let mask = Router::host_mask(&routed(Some(Route { channel: 0, offset_ms: 0.0 })));
        assert_eq!(mask, [true, false, true, true, true]);
    }

    #[test]
    fn unrouting_closes_open_notes() {
        let plan = STYLES[1].plan(PitchClass::parse("C").unwrap(), 1, 1.0, 1);
        let pattern = generate(&plan, Layers::all(), 1);
        let bps = 120.0 / 60.0 / 48_000.0;
        let (mut tx, mut rx) = queue();
        let mut router = Router::default();
        let t0 = Instant::now();
        let route = Some(Route { channel: 0, offset_ms: 0.0 });
        let mut open = 0i32;
        for k in 0..40 {
            let r = if k < 30 { routed(route) } else { routed(None) };
            router.process(&pattern, base(k as f64 * 512.0 * bps, bps, 512), r, 120.0, 48_000.0, true, t0, &mut tx);
            while let Ok(m) = rx.pop() {
                open += if m.bytes[0] & 0xF0 == 0x90 { 1 } else { -1 };
            }
        }
        assert_eq!(open, 0, "every hardware note-on got its note-off");
    }

    #[test]
    fn sender_delivers_on_time_and_cleans_up() {
        let fake = Fake::default();
        let log = fake.0.clone();
        let mut cfg = HwConfig::default();
        cfg.parts[1].port = Some("MS-1".into());
        cfg.parts[1].channel = 3;
        cfg.parts[2].port = Some("missing".into());
        let config = Arc::new(RwLock::new(cfg));
        let state = Arc::new(HwState::default());
        let (mut tx, rx) = queue();
        let handle = spawn_sender(config, state.clone(), rx, Box::new(fake));
        let start = Instant::now();
        for (i, ms) in [30u64, 10, 20].into_iter().enumerate() {
            tx.push(HwMsg { due: start + Duration::from_millis(ms), part: 1, bytes: [0x92, 60 + i as u8, 100] }).unwrap();
        }
        std::thread::sleep(Duration::from_millis(80));
        drop(tx);
        handle.join().unwrap();

        let log = log.lock().unwrap();
        let notes: Vec<_> = log.iter().filter(|(_, _, b)| b[0] == 0x92).collect();
        assert_eq!(notes.iter().map(|(_, _, b)| b[1]).collect::<Vec<_>>(), [61, 62, 60], "sent in time order");
        for (at, _, b) in &notes {
            let due = start + Duration::from_millis([30, 10, 20][(b[1] - 60) as usize]);
            assert!(*at >= due, "never early");
            assert!(*at - due < Duration::from_millis(8), "late by {:?}", *at - due);
        }
        // Leaving: all notes off on the routed part's channel (3 → 0xB2).
        assert!(log.iter().any(|(_, p, b)| *p == 1 && b == &vec![0xB2, 123, 0]));
        assert_eq!(state.status.lock().unwrap()[2].as_deref(), Some("port 'missing' not found"));
    }

    #[test]
    fn sets_saved_with_four_parts_still_load() {
        let old = r#"{"parts":[{"port":"MS-1","channel":1,"offset_ms":12.0},{"port":null,"channel":2,"offset_ms":0.0},{"port":null,"channel":3,"offset_ms":0.0},{"port":null,"channel":4,"offset_ms":0.0}]}"#;
        let cfg: HwConfig = serde_json::from_str(old).unwrap();
        assert_eq!(cfg.parts[0].port.as_deref(), Some("MS-1"));
        assert_eq!(cfg.parts[4], PartOut { port: None, channel: 10, offset_ms: 0.0 }, "drums get channel 10");
    }

    #[test]
    fn config_round_trips_and_routes() {
        let mut cfg = HwConfig::default();
        cfg.parts[0] = PartOut { port: Some("Prophet".into()), channel: 2, offset_ms: 900.0 };
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<HwConfig>(&json).unwrap(), cfg);
        let r = cfg.routes();
        assert_eq!(r[0], Some(Route { channel: 1, offset_ms: 250.0 }), "clamped");
        assert_eq!(r[1], None);
    }
}
