//! Turns a [`Plan`] into timed notes for four layers: chords, bass, arpeggio and melody.
//!
//! Every layer reads its rhythm from the style's groove grid and its pitches from the same
//! chord progression, so they always lock together, whichever ones you keep.

use crate::chord::Chord;
use crate::grid;
use crate::plan::Plan;
use crate::rng::Rng;
use crate::style::ArpDir;
use crate::voicing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Chords = 0,
    Bass = 1,
    Arp = 2,
    Melody = 3,
    Drums = 4,
}

impl Layer {
    pub const COUNT: usize = 5;
    pub const ALL: [Layer; Layer::COUNT] = [Layer::Chords, Layer::Bass, Layer::Arp, Layer::Melody, Layer::Drums];

    /// MIDI channel (0-based) used for this layer, so each part can go to its own machine.
    /// Drums use channel 10, the General MIDI drum channel.
    pub fn channel(self) -> u8 {
        match self {
            Layer::Drums => 9,
            l => l as u8,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Layer::Chords => "Chords",
            Layer::Bass => "Bass",
            Layer::Arp => "Arp",
            Layer::Melody => "Melody",
            Layer::Drums => "Drums",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    pub chords: bool,
    pub bass: bool,
    pub arp: bool,
    pub melody: bool,
    pub drums: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers { chords: true, bass: true, arp: false, melody: true, drums: false }
    }
}

impl Layers {
    pub fn all() -> Self {
        Layers { chords: true, bass: true, arp: true, melody: true, drums: true }
    }

    pub fn none() -> Self {
        Layers { chords: false, bass: false, arp: false, melody: false, drums: false }
    }

    pub fn has(&self, l: Layer) -> bool {
        match l {
            Layer::Chords => self.chords,
            Layer::Bass => self.bass,
            Layer::Arp => self.arp,
            Layer::Melody => self.melody,
            Layer::Drums => self.drums,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Start in beats from the pattern start.
    pub start: f64,
    /// Length in beats.
    pub length: f64,
    pub pitch: u8,
    pub velocity: u8,
    pub layer: Layer,
}

#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub length_beats: f64,
    /// Sorted by start time.
    pub notes: Vec<Note>,
    /// Chord name and start beat, for display.
    pub chords: Vec<(f64, String)>,
}

impl Pattern {
    /// Only one part, e.g. to export the bass on its own.
    pub fn only(&self, layer: Layer) -> Pattern {
        Pattern {
            length_beats: self.length_beats,
            notes: self.notes.iter().filter(|n| n.layer == layer).copied().collect(),
            chords: self.chords.clone(),
        }
    }

    /// Shifts each part by its own number of octaves (chords, bass, arp, melody). Drums keep
    /// their notes: each note is an instrument. Notes pushed outside the MIDI range are dropped.
    pub fn transposed(&self, octaves: [i32; Layer::COUNT]) -> Pattern {
        let notes = self
            .notes
            .iter()
            .filter_map(|n| {
                let shift = if n.layer == Layer::Drums { 0 } else { octaves[n.layer.index()] };
                let p = n.pitch as i32 + 12 * shift;
                (0..=127).contains(&p).then(|| Note { pitch: p as u8, ..*n })
            })
            .collect();
        Pattern { length_beats: self.length_beats, notes, chords: self.chords.clone() }
    }

    pub fn has(&self, layer: Layer) -> bool {
        self.notes.iter().any(|n| n.layer == layer)
    }
}

/// Renders the plan. Each layer has its own random stream, so muting one layer never
/// changes what the others play.
pub fn generate(plan: &Plan, layers: Layers, seed: u64) -> Pattern {
    generate_seeded(plan, layers, [seed; Layer::COUNT])
}

/// Like [`generate`], with one seed per part (chords, bass, arp, melody, drums). A part keeps
/// its own choices (groove, motif, voicing, fills) as long as its seed stays the same, and
/// follows whatever harmony the plan holds. That is what locking a part means.
pub fn generate_seeded(plan: &Plan, layers: Layers, seeds: [u64; Layer::COUNT]) -> Pattern {
    let stream = |layer: Layer| Rng::new(seeds[layer.index()]).fork(layer.index() as u64 + 1);
    let voicings = chord_voicings(plan);
    let mut notes = Vec::new();

    if layers.chords {
        let mut rng = stream(Layer::Chords);
        let varied = chord_voicings_varied(plan, &mut rng);
        chords(plan, &varied, &mut rng, &mut notes);
    }
    if layers.bass {
        bass(plan, &mut stream(Layer::Bass), &mut notes);
    }
    if layers.arp {
        arp(plan, &voicings, &mut stream(Layer::Arp), &mut notes);
    }
    if layers.melody {
        melody(plan, &mut stream(Layer::Melody), &mut notes);
    }
    if layers.drums {
        drums(plan, &mut stream(Layer::Drums), &mut notes);
    }

    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let chords = (0..plan.progression.len())
        .map(|i| (plan.chord_span(i).0, plan.progression[i].name(&plan.key)))
        .collect();
    Pattern { length_beats: plan.length_beats(), notes, chords }
}

/// The chord part's own voicing for this idea: a register and, sometimes, an open "drop 2"
/// spread, so a new idea sounds new even when the progression is locked.
fn chord_voicings_varied(plan: &Plan, rng: &mut Rng) -> Vec<Vec<i32>> {
    let center = 62 + rng.range(-4, 4);
    let drop2 = rng.chance(0.3);
    let mut prev: Option<Vec<i32>> = None;
    plan.progression
        .iter()
        .map(|c| {
            let mut v = voicing::lead(&c.pitch_classes(&plan.key), prev.as_deref(), center, 52, 77);
            prev = Some(v.clone());
            if drop2 && v.len() >= 4 {
                let i = v.len() - 2;
                if v[i] - 12 >= 45 {
                    v[i] -= 12;
                    v.sort_unstable();
                }
            }
            v
        })
        .collect()
}

fn chord_voicings(plan: &Plan) -> Vec<Vec<i32>> {
    let mut prev: Option<Vec<i32>> = None;
    plan.progression
        .iter()
        .map(|c| {
            let v = voicing::lead(&c.pitch_classes(&plan.key), prev.as_deref(), 62, 52, 77);
            prev = Some(v.clone());
            v
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    step: usize,
    start: f64,
    len: f64,
    threshold: u8,
}

/// Lays a grid over the whole pattern at the current energy. Each note lasts `gate` of the
/// gap to the next hit, capped at `max_len` and at the pattern end.
fn hits_of(plan: &Plan, cells: &[Option<u8>], gate: f64, max_len: f64) -> Vec<Hit> {
    let level = grid::level(plan.energy);
    let end = plan.length_beats();
    let on: Vec<(usize, u8)> = (0..plan.total_steps())
        .filter_map(|s| cells[s % cells.len()].filter(|t| *t <= level).map(|t| (s, t)))
        .collect();
    on.iter()
        .enumerate()
        .map(|(i, &(step, threshold))| {
            let start = plan.step_time(step);
            let next = on.get(i + 1).map_or(end, |&(n, _)| plan.step_time(n));
            let len = ((next - start) * gate).min(max_len).min(end - start).max(0.05);
            Hit { step, start, len, threshold }
        })
        .collect()
}

/// Velocity: louder with energy, accents on the beat, ghost notes for the optional hits.
fn vel(rng: &mut Rng, plan: &Plan, step: usize, threshold: u8) -> u8 {
    let mut v = 66.0 + 34.0 * plan.energy;
    if step % 4 == 0 {
        v += 8.0;
    }
    if threshold >= 5 {
        v -= 18.0;
    }
    (v as i32 + rng.range(-4, 4)).clamp(1, 127) as u8
}

fn chords(plan: &Plan, voicings: &[Vec<i32>], rng: &mut Rng, out: &mut Vec<Note>) {
    let part = &plan.style.chords;
    // This idea's chord rhythm: the style's, shaken up, or shifted against the beat; and
    // played shorter or longer.
    let mut feel = rng.fork(0xC0);
    let cells = match feel.weighted(&[0.2, 0.5, 0.3]) {
        0 => grid::parse(part.grid),
        1 => shake(&mut feel, part.grid, Shake { drop: 0.3, extra: (1, 4), lowest: 1, off_beat: true }),
        _ => {
            let mut c = shake(&mut feel, part.grid, Shake { drop: 0.15, extra: (0, 2), lowest: 2, off_beat: true });
            let shift = *feel.pick(&[2usize, 3, 6, 10]);
            let n = c.len();
            c.rotate_right(shift % n);
            c
        }
    };
    let stretch = *feel.pick(&[0.6, 0.8, 1.0, 1.0, 1.3, 1.6]);
    let (gate, max_len) = ((part.gate * stretch).min(0.98), part.max_len * stretch);
    let mut per_chord: Vec<Vec<Hit>> = vec![Vec::new(); plan.progression.len()];
    for mut h in hits_of(plan, &cells, gate, max_len) {
        let i = plan.chord_index(h.start);
        let (s, l) = plan.chord_span(i);
        h.len = h.len.min(s + l - h.start).max(0.05);
        per_chord[i].push(h);
    }
    for (i, hs) in per_chord.iter_mut().enumerate() {
        if hs.is_empty() {
            // Short chord span between two grid hits: still let the chord be heard.
            let (s, l) = plan.chord_span(i);
            hs.push(Hit { step: (s / grid::STEP).round() as usize, start: s, len: (l * gate).min(max_len).max(0.05), threshold: 0 });
        }
        for h in hs.iter() {
            for &p in &voicings[i] {
                out.push(Note {
                    start: h.start,
                    length: h.len,
                    pitch: p as u8,
                    velocity: vel(rng, plan, h.step, h.threshold),
                    layer: Layer::Chords,
                });
            }
        }
    }
}

fn bass_root(chord: &Chord, plan: &Plan) -> i32 {
    // Root placed in MIDI 36..47 (C1..B1 in Ableton naming): deep but still audible.
    let pc = chord.root_pc(&plan.key).semitone();
    36 + (pc - 36).rem_euclid(12)
}

fn bass(plan: &Plan, rng: &mut Rng, out: &mut Vec<Note>) {
    let part = &plan.style.bass;
    let n = plan.progression.len();
    // This idea's bassline: the style's rhythm with some hits moved between energy levels,
    // a few new syncopations, and other chord tones (octave, fifth, third, approach notes)
    // on the off-beat hits. The first hit of each bar stays the root.
    let mut feel = rng.fork(0xBA);
    let written = grid::parse(part.grid);
    let cells = if feel.chance(0.25) {
        written.clone()
    } else {
        shake(&mut feel, part.grid, Shake { drop: 0.25, extra: (1, 4), lowest: 2, off_beat: true })
    };
    let src = part.tones.as_bytes();
    let busy = feel.chance(0.3);
    let tones: Vec<u8> = (0..cells.len())
        .map(|s| {
            let t = src[s % src.len()];
            let new_hit = cells[s].is_some() && written[s % written.len()].is_none();
            if s % grid::STEPS_PER_BAR == 0 {
                b'r'
            } else if new_hit || feel.chance(if busy { 0.5 } else { 0.25 }) {
                *feel.pick(b"rrr8885533a")
            } else {
                t
            }
        })
        .collect();
    let stretch = *feel.pick(&[0.6, 0.8, 1.0, 1.0, 1.2]);
    for h in hits_of(plan, &cells, (part.gate * stretch).min(0.98), part.max_len * stretch) {
        let i = plan.chord_index(h.start);
        let chord = &plan.progression[i];
        let root = bass_root(chord, plan);
        let (s, l) = plan.chord_span(i);
        let pitch = match tones[h.step % tones.len()] {
            b'5' => root + if chord.intervals.contains(&6) { 6 } else { 7 },
            b'8' => root + 12,
            b'3' => root + chord.intervals[1],
            b'7' => plan.key.step(root + 12, -1),
            b'a' => {
                let next = bass_root(&plan.progression[(i + 1) % n], plan);
                if next == root {
                    root + 7
                } else if next > root {
                    next - 1
                } else {
                    next + 1
                }
            }
            _ => root,
        };
        out.push(Note {
            start: h.start,
            length: h.len.min(s + l - h.start).max(0.05),
            pitch: pitch.clamp(0, 127) as u8,
            velocity: vel(rng, plan, h.step, h.threshold),
            layer: Layer::Bass,
        });
    }
}

fn arp(plan: &Plan, voicings: &[Vec<i32>], rng: &mut Rng, out: &mut Vec<Note>) {
    let part = &plan.style.arp;
    let mut feel = rng.fork(0xA4);
    let cells = if feel.chance(0.3) {
        grid::parse(part.grid)
    } else {
        shake(&mut feel, part.grid, Shake { drop: 0.2, extra: (0, 3), lowest: 2, off_beat: false })
    };
    let stretch = *feel.pick(&[0.5, 0.8, 1.0, 1.0, 1.5]);
    let (gate, max_len) = ((part.gate * stretch).min(0.98), part.max_len * stretch);
    let mut current = usize::MAX;
    let mut k = 0usize;
    let mut last = usize::MAX;
    for h in hits_of(plan, &cells, gate, max_len) {
        let i = plan.chord_index(h.start);
        if i != current {
            current = i;
            k = 0;
        }
        let v = &voicings[i];
        let mut up: Vec<i32> = v.iter().map(|p| p + 12).collect();
        up.push(v[0] + 24);
        let seq: Vec<i32> = match plan.arp_dir {
            ArpDir::Down => up.iter().rev().copied().collect(),
            ArpDir::UpDown => {
                let mut s = up.clone();
                s.extend(up[1..up.len() - 1].iter().rev());
                s
            }
            ArpDir::Up | ArpDir::Random => up,
        };
        let idx = if plan.arp_dir == ArpDir::Random {
            let mut j = rng.range(0, seq.len() as i32 - 1) as usize;
            if j == last {
                j = (j + 1) % seq.len();
            }
            last = j;
            j
        } else {
            k % seq.len()
        };
        k += 1;
        let (s, l) = plan.chord_span(i);
        out.push(Note {
            start: h.start,
            length: h.len.min(s + l - h.start).max(0.05),
            pitch: seq[idx].clamp(0, 127) as u8,
            velocity: vel(rng, plan, h.step, h.threshold),
            layer: Layer::Arp,
        });
    }
}

// ---------------------------------------------------------------- drums

/// Kick, snare/clap, hats and percussion from the style's kit, varied by the idea, plus a fill
/// into the loop point at high energy. Every choice is drawn before Energy is looked at, so
/// for one idea turning Energy up only adds hits, never moves or removes any.
fn drums(plan: &Plan, rng: &mut Rng, out: &mut Vec<Note>) {
    use crate::style::{CLOSED_HAT, KICK, OPEN_HAT};
    let kit = plan.style.drums;
    let mut feel = rng.fork(0xD5);
    let kick = vary(&mut feel, kit.kick, Voice::Kick);
    let snare = vary(&mut feel, kit.snare, Voice::Snare);
    let hat = vary(&mut feel, kit.hat, Voice::Hat);
    let open_hat = vary(&mut feel, kit.open_hat, Voice::OpenHat);
    let perc = vary(&mut feel, kit.perc, Voice::Perc);
    let fill = *feel.pick(&[Fill::SnareRoll, Fill::SnareRoll, Fill::KickSnare, Fill::Perc, Fill::OpenHat, Fill::Toms]);
    let fill_beats = if feel.chance(0.3) { 2usize } else { 1 };

    let voices: [Voicing; 5] = [
        (&kick, KICK, 0.9, 0.25, 8),
        (&snare, kit.snare_note, 0.9, 0.25, 4),
        (&hat, CLOSED_HAT, 0.5, 0.12, -10),
        (&open_hat, OPEN_HAT, 0.9, 0.5, -4),
        (&perc, kit.perc_note, 0.5, 0.15, -12),
    ];
    for (cells, note, gate, max_len, accent) in voices {
        for h in hits_of(plan, cells, gate, max_len) {
            let v = vel(rng, plan, h.step, h.threshold) as i32 + accent;
            out.push(Note { start: h.start, length: h.len, pitch: note, velocity: v.clamp(1, 127) as u8, layer: Layer::Drums });
        }
    }

    // Fill over the last beat (or two, at peak energy) of the loop, at 70 % energy and up.
    if plan.bars < 2 || plan.energy < 0.7 {
        return;
    }
    let beats = if plan.energy >= 0.9 { fill_beats } else { 1 };
    let end = plan.bars as usize * crate::grid::STEPS_PER_BAR;
    let from = end - 4 * beats;
    let start_of = |step: usize| plan.step_time(step);
    let has = |out: &Vec<Note>, pitch: u8, start: f64| {
        out.iter().any(|n| n.layer == Layer::Drums && n.pitch == pitch && (n.start - start).abs() < 1e-6)
    };
    let add = |out: &mut Vec<Note>, step: usize, pitch: u8, v: i32, len: f64| {
        let start = start_of(step);
        if !has(out, pitch, start) {
            out.push(Note { start, length: len, pitch, velocity: v.clamp(1, 127) as u8, layer: Layer::Drums });
        }
    };
    for (k, step) in (from..end).enumerate() {
        let rise = 70 + (k * 48 / (end - from)) as i32 + rng.range(-3, 3);
        match fill {
            Fill::SnareRoll => add(out, step, kit.snare_note, rise, 0.12),
            Fill::KickSnare => {
                let pitch = if k % 2 == 0 { KICK } else { kit.snare_note };
                add(out, step, pitch, rise, 0.12)
            }
            Fill::Perc => add(out, step, kit.perc_note, rise - 6, 0.12),
            Fill::OpenHat => {
                if step % 2 == 0 {
                    add(out, step, OPEN_HAT, rise - 4, 0.25)
                }
            }
            Fill::Toms => {
                // High tom down to low tom (GM 50, 48, 47, 45), one per 8th.
                if step % 2 == 0 {
                    let toms = [50u8, 48, 47, 45];
                    let i = (k / 2) * toms.len() / (end - from).div_ceil(2);
                    add(out, step, toms[i.min(toms.len() - 1)], rise, 0.2)
                }
            }
        }
    }
}

/// A drum voice: its grid, GM note, gate, longest note and velocity accent.
type Voicing<'a> = (&'a [Option<u8>], u8, f64, f64, i32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    SnareRoll,
    KickSnare,
    Perc,
    OpenHat,
    Toms,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Voice {
    Kick,
    Snare,
    Hat,
    OpenHat,
    Perc,
}

/// How [`shake`] varies a melodic part's grid.
struct Shake {
    /// Chance that an optional hit sits this idea out.
    drop: f64,
    /// How many new hits come in (min, max).
    extra: (i32, i32),
    /// Lowest energy threshold for a new hit (they're never part of the core groove).
    lowest: u8,
    /// New hits avoid the main beats (syncopation).
    off_beat: bool,
}

/// One idea's take on a part's grid, two bars long. The core hits (threshold 0) stay;
/// optional hits move to another energy level or drop out; a few new ones come in. Drawn
/// without looking at Energy, so Energy still only adds notes.
fn shake(rng: &mut Rng, grid: &str, sh: Shake) -> Vec<Option<u8>> {
    use crate::grid::STEPS_PER_BAR as BAR;
    let src = grid::parse(grid);
    let n = src.len().max(2 * BAR);
    let mut cells: Vec<Option<u8>> = (0..n).map(|s| src[s % src.len()]).collect();
    for c in cells.iter_mut() {
        if let Some(t) = *c {
            if t == 0 {
                continue;
            }
            *c = if rng.chance(sh.drop) { None } else { Some((t as i32 + rng.range(-2, 2)).clamp(1, 9) as u8) };
        }
    }
    for _ in 0..rng.range(sh.extra.0, sh.extra.1) {
        let step = rng.range(0, n as i32 - 1) as usize;
        let beat = step.is_multiple_of(4);
        if cells[step].is_none() && !(sh.off_beat && beat) {
            cells[step] = Some(rng.range(sh.lowest as i32, 6) as u8);
        }
    }
    cells
}

/// One idea's take on a kit grid, two bars long (the second bar answers the first). The
/// style's core hits (threshold 0) on kick and snare always stay, so the genre stays
/// recognisable; the optional hits move between energy levels, some go, ghost notes come
/// in, percussion shifts around the bar and hats change their feel.
fn vary(rng: &mut Rng, grid: &str, voice: Voice) -> Vec<Option<u8>> {
    use crate::grid::STEPS_PER_BAR as BAR;
    let src = grid::parse(grid);
    let n = src.len().max(2 * BAR);
    let mut cells: Vec<Option<u8>> = (0..n).map(|s| src[s % src.len()]).collect();
    let empty = src.iter().all(Option::is_none);
    let core = |c: Option<u8>| c == Some(0) && matches!(voice, Voice::Kick | Voice::Snare);

    // Percussion moves around the bar as a whole.
    if voice == Voice::Perc && !empty {
        let shift = *rng.pick(&[0usize, 0, 2, 3, 6, 10, 11]);
        cells.rotate_right(shift % n);
    }

    // Optional hits: come in at another energy level, or sit this idea out.
    let drop = match voice {
        Voice::Kick | Voice::Snare => 0.15,
        Voice::Hat => 0.1,
        Voice::OpenHat | Voice::Perc => 0.25,
    };
    for c in cells.iter_mut() {
        if let Some(t) = *c {
            if core(Some(t)) || t == 0 {
                continue;
            }
            if rng.chance(drop) {
                *c = None;
            } else {
                *c = Some((t as i32 + rng.range(-2, 2)).clamp(1, 9) as u8);
            }
        }
    }

    // New ghost notes on free 16ths.
    let (count, lo) = match voice {
        Voice::Kick => (rng.range(0, 2), 4),
        Voice::Snare => (rng.range(0, 3), 5),
        Voice::Hat => (rng.range(0, 4), 5),
        Voice::OpenHat => (if empty { 0 } else { rng.range(0, 1) }, 5),
        Voice::Perc => (if empty { rng.range(0, 1) * 3 } else { rng.range(0, 3) }, 4),
    };
    for _ in 0..count {
        let step = rng.range(0, n as i32 - 1) as usize;
        // Kicks and snares keep off the main beats (those belong to the groove).
        let on_beat = step.is_multiple_of(4);
        if cells[step].is_none() && !(on_beat && matches!(voice, Voice::Kick | Voice::Snare)) {
            cells[step] = Some(rng.range(lo, 9) as u8);
        }
    }

    // Hat feel: as written, offbeats only, or a 16th shuffle filling in at high energy.
    if voice == Voice::Hat && !empty {
        match rng.weighted(&[0.5, 0.2, 0.3]) {
            1 => {
                for (s, c) in cells.iter_mut().enumerate() {
                    if s % 4 == 2 && c.is_none() {
                        *c = Some(2);
                    } else if s % 4 == 0 {
                        *c = None;
                    }
                }
            }
            2 => {
                for (s, c) in cells.iter_mut().enumerate() {
                    if c.is_none() && s % 2 == 1 {
                        *c = Some(if s % 4 == 3 { 7 } else { 8 });
                    }
                }
            }
            _ => {}
        }
    }
    cells
}

// ---------------------------------------------------------------- melody

const MEL_LOW: i32 = 65;
const MEL_HIGH: i32 = 86;

#[derive(Clone)]
struct Motif {
    /// (step in bar, length in beats)
    rhythm: Vec<(usize, f64)>,
    /// Scale-step movement for each note after the first.
    steps: Vec<i32>,
}

/// A one-bar motif: picks onsets among the style's melody positions, busier with energy.
fn motif(plan: &Plan, rng: &mut Rng, busy: f64) -> Motif {
    let part = &plan.style.melody;
    let cells = grid::parse(part.grid);
    let level = grid::level(busy);
    let p = 0.45 + 0.4 * busy;
    let mut onsets: Vec<usize> = (0..grid::STEPS_PER_BAR)
        .filter(|&s| matches!(cells[s], Some(t) if t <= level))
        .filter(|&s| s == 0 || rng.chance(p))
        .collect();
    if onsets.len() < 2 {
        onsets = vec![0, 8];
    }
    let rhythm = onsets
        .iter()
        .enumerate()
        .map(|(j, &s)| {
            let next = onsets.get(j + 1).map_or(4.0, |&n| plan.step_time(n));
            let room = (next - plan.step_time(s)).min(part.max_len);
            let len = if rng.chance(0.2) && room > 0.5 { room * 0.5 } else { room * part.gate };
            (s, len)
        })
        .collect();
    let deltas = [-2, -1, 0, 1, 2, 3, -3];
    let weights = [0.12, 0.3, 0.08, 0.3, 0.12, 0.04, 0.04];
    let steps = (1..onsets.len()).map(|_| deltas[rng.weighted(&weights)]).collect();
    Motif { rhythm, steps }
}

fn nearest_chord_tone(plan: &Plan, chord: &Chord, around: i32) -> i32 {
    let pcs = chord.pitch_classes(&plan.key);
    (MEL_LOW..=MEL_HIGH)
        .filter(|p| pcs.contains(&p.rem_euclid(12)))
        .min_by_key(|p| ((p - around).abs(), *p))
        .unwrap_or(around)
}

/// Moves a note by octaves (never by semitones) until it fits the melody range.
fn fold_into_range(mut p: i32) -> i32 {
    while p < MEL_LOW {
        p += 12;
    }
    while p > MEL_HIGH {
        p -= 12;
    }
    p
}

/// Borrowed chords bring notes from outside the key (G# over E in A minor...). A scale note
/// a half-step away from such a chord tone would clash, so it becomes that chord tone.
fn adapt_to_chord(plan: &Plan, chord: &Chord, pitch: i32) -> i32 {
    if chord.contains(&plan.key, pitch) {
        return pitch;
    }
    for pc in chord.pitch_classes(&plan.key) {
        if plan.key.contains(pc) {
            continue;
        }
        for d in [-1, 1] {
            if (pitch + d).rem_euclid(12) == pc {
                return pitch + d;
            }
        }
    }
    pitch
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Statement,
    Answer,
    Contrast,
    Ending,
}

/// Statement, answer, contrast, resolution: a 4-bar phrase that sounds intentional.
fn melody(plan: &Plan, rng: &mut Rng, out: &mut Vec<Note>) {
    let a = motif(plan, rng, plan.energy);
    let b = motif(plan, rng, (plan.energy + 0.15).min(1.0));
    let mut cur = plan.key.snap(74);

    for bar in 0..plan.bars {
        let role = if plan.bars >= 2 && bar == plan.bars - 1 {
            Role::Ending
        } else {
            match bar % 4 {
                0 => Role::Statement,
                1 => Role::Answer,
                2 => Role::Contrast,
                _ => Role::Ending,
            }
        };
        let mut m = if role == Role::Contrast { b.clone() } else { a.clone() };
        if role == Role::Answer {
            // Same rhythm, partly inverted contour: recognisable but not a copy.
            for s in m.steps.iter_mut() {
                if rng.chance(0.35) {
                    *s = -*s;
                }
            }
        }
        if role == Role::Ending {
            m.rhythm.retain(|(s, _)| *s < 8);
            m.steps.truncate(m.rhythm.len().saturating_sub(1));
            m.rhythm.push((8, 1.9));
            m.steps.push(0);
            for j in 0..m.rhythm.len() - 1 {
                let gap = plan.step_time(m.rhythm[j + 1].0) - plan.step_time(m.rhythm[j].0);
                m.rhythm[j].1 = m.rhythm[j].1.min(gap);
            }
        }

        let bar_start = bar as f64 * 4.0;
        let last_index = m.rhythm.len() - 1;
        for (j, &(s, l)) in m.rhythm.iter().enumerate() {
            let t = bar_start + plan.step_time(s);
            let chord = plan.chord_at(t);
            let strong = s % 8 == 0;
            let mut pitch = if j == 0 {
                nearest_chord_tone(plan, chord, cur)
            } else {
                let step = m.steps[j - 1];
                let p = plan.key.step(cur, step);
                if (MEL_LOW..=MEL_HIGH).contains(&p) {
                    p
                } else {
                    plan.key.step(cur, -step)
                }
            };
            if strong && !chord.contains(&plan.key, pitch) {
                pitch = nearest_chord_tone(plan, chord, pitch);
            }
            if role == Role::Ending && j == last_index {
                // Land home: the tonic if the chord allows it, otherwise the chord root.
                let tonic = plan.key.root.semitone();
                let target = if chord.pitch_classes(&plan.key).contains(&tonic) {
                    tonic
                } else {
                    chord.root_pc(&plan.key).semitone()
                };
                pitch = (MEL_LOW..=MEL_HIGH)
                    .filter(|p| p.rem_euclid(12) == target)
                    .min_by_key(|p| (p - pitch).abs())
                    .unwrap_or(pitch);
            }
            pitch = fold_into_range(adapt_to_chord(plan, chord, pitch));
            cur = pitch;
            let threshold = if strong { 0 } else { 5 };
            out.push(Note {
                start: t,
                length: l.min(plan.length_beats() - t).max(0.1),
                pitch: pitch as u8,
                velocity: vel(rng, plan, s, threshold).saturating_add(if strong { 0 } else { 8 }),
                layer: Layer::Melody,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::PitchClass;
    use crate::style::STYLES;

    const EPS: f64 = 1e-6;

    fn count(p: &Pattern, l: Layer) -> usize {
        p.notes.iter().filter(|n| n.layer == l).count()
    }

    #[test]
    fn split_and_transpose() {
        let plan = STYLES[1].plan(PitchClass::parse("E").unwrap(), 2, 0.5, 2);
        let p = generate(&plan, Layers::all(), 2);
        let bass = p.only(Layer::Bass);
        assert!(bass.notes.iter().all(|n| n.layer == Layer::Bass) && !bass.notes.is_empty());
        let up = p.transposed([0, 1, 0, 0, 3]);
        for (a, b) in p.notes.iter().zip(&up.notes) {
            let d = if a.layer == Layer::Bass { 12 } else { 0 };
            assert_eq!(a.pitch as i32 + d, b.pitch as i32);
        }
    }

    #[test]
    fn drums_follow_the_kit() {
        let plan = crate::style::find("techno").unwrap().plan(PitchClass::parse("A").unwrap(), 4, 0.0, 1);
        let p = generate(&plan, Layers { drums: true, ..Layers::none() }, 1);
        let kicks: Vec<f64> = p.notes.iter().filter(|n| n.pitch == crate::style::KICK).map(|n| n.start).collect();
        assert_eq!(kicks.len(), 16, "four on the floor over 4 bars");
        assert!(kicks.iter().all(|t| (t.fract()).abs() < 1e-9));
        assert!(p.notes.iter().all(|n| n.layer == Layer::Drums));
        // A fill appears in the last bar only at high energy.
        let busy = crate::style::find("techno").unwrap().plan(PitchClass::parse("A").unwrap(), 4, 0.9, 1);
        let q = generate(&busy, Layers { drums: true, ..Layers::none() }, 1);
        let late = |p: &Pattern| p.notes.iter().filter(|n| n.start >= 15.0).count();
        assert!(late(&q) >= late(&p) + 2, "{} vs {}", late(&q), late(&p));
        assert_eq!(MIDI_DRUM_CHANNEL, Layer::Drums.channel());
    }

    #[test]
    fn each_idea_has_its_own_drums_on_the_same_core() {
        use crate::style::{KICK, STYLES};
        let drums = Layers { drums: true, ..Layers::none() };
        for style in STYLES.iter() {
            let plan = style.plan(PitchClass::parse("C").unwrap(), 4, 0.6, 1);
            let core = |g: &str| grid::parse(g).iter().enumerate().filter(|(_, c)| **c == Some(0)).map(|(s, _)| s).collect::<Vec<_>>();
            let mut seen = std::collections::HashSet::new();
            for idea in 1..=12u64 {
                let p = generate(&plan, drums, idea);
                let at = |pitch: u8, step: usize| p.notes.iter().any(|n| n.pitch == pitch && (n.start - plan.step_time(step)).abs() < 1e-6);
                for bar in 0..plan.bars as usize {
                    for s in core(style.drums.kick) {
                        let step = bar * grid::STEPS_PER_BAR * (style.drums.kick.len() / grid::STEPS_PER_BAR).max(1) + s;
                        if step < plan.total_steps() {
                            assert!(at(KICK, step), "{} idea {idea}: core kick at step {step}", style.id);
                        }
                    }
                }
                seen.insert(p.notes.iter().map(|n| (n.pitch, (n.start * 4.0).round() as i64)).collect::<Vec<_>>());
            }
            assert!(seen.len() >= 8, "{}: only {} different drum grooves in 12 ideas", style.id, seen.len());
        }
    }

    #[test]
    #[ignore]
    fn print_drum_ideas() {
        let plan = crate::style::find("dub-techno").unwrap().plan(PitchClass::parse("F").unwrap(), 2, 0.83, 1);
        for idea in 1..=4u64 {
            let p = generate(&plan, Layers { drums: true, ..Layers::none() }, idea);
            println!("idea {idea}");
            for (name, pitch) in [("kick ", 36u8), ("snare", plan.style.drums.snare_note), ("hat  ", 42), ("open ", 46), ("perc ", plan.style.drums.perc_note)] {
                let row: String = (0..plan.total_steps()).map(|s| if p.notes.iter().any(|n| n.pitch == pitch && (n.start - plan.step_time(s)).abs() < 1e-6) { 'x' } else { '.' }).collect();
                println!("  {name} {row}");
            }
        }
    }

    #[test]
    fn each_idea_changes_chords_rhythm_and_bass() {
        use std::collections::HashSet;
        for style in crate::style::STYLES.iter() {
            let (mut progs, mut rhythms, mut basses) = (HashSet::new(), HashSet::new(), HashSet::new());
            for idea in 1..=12u64 {
                let plan = style.plan(PitchClass::parse("C").unwrap(), 4, 0.6, idea);
                let p = generate(&plan, Layers::all(), idea);
                let q = (p.notes.iter()).map(|n| (n.layer, n.pitch, (n.start * 4.0).round() as i64));
                progs.insert(plan.chord_names());
                rhythms.insert(q.clone().filter(|n| n.0 == Layer::Chords).map(|n| n.2).collect::<Vec<_>>());
                basses.insert(q.filter(|n| n.0 == Layer::Bass).map(|n| (n.1, n.2)).collect::<Vec<_>>());
            }
            assert!(progs.len() >= 5, "{}: {} progressions in 12 ideas", style.id, progs.len());
            assert!(rhythms.len() >= 6, "{}: {} chord rhythms in 12 ideas", style.id, rhythms.len());
            assert!(basses.len() >= 8, "{}: {} basslines in 12 ideas", style.id, basses.len());
        }
    }

    #[test]
    #[ignore]
    fn print_ideas() {
        let style = crate::style::find(&std::env::var("STYLE").unwrap_or("dub-techno".into())).unwrap();
        for idea in 1..=6u64 {
            let plan = style.plan(PitchClass::parse("F").unwrap(), 2, 0.7, idea);
            let p = generate(&plan, Layers::all(), idea);
            println!("idea {idea}: {}", plan.chord_names().join(" "));
            let row = |l: Layer| -> String {
                (0..plan.total_steps()).map(|s| if p.notes.iter().any(|n| n.layer == l && (n.start - plan.step_time(s)).abs() < 1e-6) { 'x' } else { '.' }).collect()
            };
            println!("  chords {}", row(Layer::Chords));
            println!("  bass   {}", row(Layer::Bass));
            let bass: Vec<String> = p.notes.iter().filter(|n| n.layer == Layer::Bass).map(|n| crate::PitchClass::from_semitone(n.pitch as i32).name(true).to_string()).collect();
            println!("  notes  {}", bass.join(" "));
        }
    }

    const MIDI_DRUM_CHANNEL: u8 = 9;

    #[test]
    fn locked_part_keeps_its_idea() {
        let style = crate::style::find("deep-house").unwrap();
        let root = PitchClass::parse("D").unwrap();
        let only = |l: Layer| Layers { chords: l == Layer::Chords, bass: l == Layer::Bass, arp: false, melody: l == Layer::Melody, drums: l == Layer::Drums };
        let (old, new) = (11u64, 12u64);

        // Harmony and the part both locked: identical notes, whatever the other seeds do.
        let plan = style.plan(root, 4, 0.6, old);
        for l in [Layer::Bass, Layer::Melody, Layer::Drums, Layer::Chords] {
            let mut seeds = [new; Layer::COUNT];
            seeds[l.index()] = old;
            assert_eq!(generate_seeded(&plan, only(l), seeds).notes, generate(&plan, only(l), old).notes, "{l:?}");
        }

        // Only the melody locked, new harmony: same rhythm, pitches follow the new chords.
        let (a, b) = (style.plan(root, 4, 0.6, 3), style.plan(root, 4, 0.6, 4));
        assert_ne!(a.chord_names(), b.chord_names());
        let ma = generate_seeded(&a, only(Layer::Melody), [7; Layer::COUNT]);
        let mb = generate_seeded(&b, only(Layer::Melody), [7; Layer::COUNT]);
        let starts = |p: &Pattern| p.notes.iter().map(|n| n.start).collect::<Vec<_>>();
        assert_eq!(starts(&ma), starts(&mb));

        // Drums don't depend on harmony at all.
        let da = generate_seeded(&a, only(Layer::Drums), [7; Layer::COUNT]);
        let db = generate_seeded(&b, only(Layer::Drums), [7; Layer::COUNT]);
        assert_eq!(da.notes, db.notes);
    }

    #[test]
    fn new_ideas_revoice_the_chords() {
        let plan = STYLES[0].plan(PitchClass::parse("C").unwrap(), 4, 0.5, 1);
        let only_chords = Layers { chords: true, ..Layers::none() };
        let voicings: std::collections::HashSet<Vec<u8>> = (0..20u64)
            .map(|s| {
                let mut seeds = [1; Layer::COUNT];
                seeds[0] = s;
                generate_seeded(&plan, only_chords, seeds).notes.iter().map(|n| n.pitch).collect()
            })
            .collect();
        assert!(voicings.len() >= 4, "{} distinct voicings", voicings.len());
    }

    #[test]
    fn deterministic() {
        let plan = STYLES[0].plan(PitchClass::parse("C").unwrap(), 4, 0.6, 7);
        let a = generate(&plan, Layers::all(), 7);
        let b = generate(&plan, Layers::all(), 7);
        assert_eq!(a.notes, b.notes);
        assert!(!a.notes.is_empty());
    }

    #[test]
    fn muting_a_layer_keeps_the_others() {
        let plan = STYLES[4].plan(PitchClass::parse("D").unwrap(), 4, 0.5, 3);
        let all = generate(&plan, Layers::all(), 3);
        let no_bass = generate(&plan, Layers { bass: false, ..Layers::all() }, 3);
        let mel = |p: &Pattern| p.notes.iter().filter(|n| n.layer == Layer::Melody).copied().collect::<Vec<_>>();
        assert_eq!(mel(&all), mel(&no_bass));
    }

    #[test]
    fn energy_only_adds_groove_notes() {
        for style in STYLES {
            let root = PitchClass::parse("A").unwrap();
            let mut prev: Option<Pattern> = None;
            for e in [0.0, 0.3, 0.6, 1.0] {
                let plan = style.plan(root, 4, e, 5);
                let p = generate(&plan, Layers::all(), 5);
                if let Some(q) = &prev {
                    for l in [Layer::Chords, Layer::Bass, Layer::Arp, Layer::Drums] {
                        assert!(count(&p, l) >= count(q, l), "{} {:?} at energy {e}", style.id, l);
                        // Hits present at lower energy are still there, at the same time.
                        for n in q.notes.iter().filter(|n| n.layer == l) {
                            assert!(p.notes.iter().any(|m| m.layer == l && (m.start - n.start).abs() < EPS));
                        }
                    }
                }
                prev = Some(p);
            }
        }
    }

    #[test]
    fn swing_moves_offbeat_sixteenths_only() {
        let plan = crate::style::find("ukg").unwrap().plan(PitchClass::parse("F").unwrap(), 2, 1.0, 1);
        assert!((plan.step_time(4) - 1.0).abs() < EPS);
        assert!(plan.step_time(3) > 0.75 + 0.04);
    }

    #[test]
    fn every_style_every_key_is_sane() {
        for style in STYLES {
            for root in PitchClass::ALL {
                for bars in [1, 2, 4, 8] {
                    for (seed, energy) in [(0, 0.0), (1, 0.25), (2, 0.5), (3, 0.75), (4, 1.0)] {
                        let plan = style.plan(root, bars, energy, seed);
                        let p = generate(&plan, Layers::all(), seed);
                        let ctx = format!("{} {root} {bars} bars e={energy}", style.id);
                        for n in &p.notes {
                            assert!(n.start >= 0.0 && n.start < p.length_beats, "{ctx}");
                            assert!(n.length > 0.0, "{ctx}");
                            assert!(n.start + n.length <= p.length_beats + EPS, "{ctx} {n:?}");
                        }
                        // Every chord gets played inside its own span.
                        for i in 0..plan.progression.len() {
                            let (s, l) = plan.chord_span(i);
                            assert!(
                                p.notes.iter().any(|n| n.layer == Layer::Chords && n.start >= s - EPS && n.start < s + l - EPS),
                                "{ctx}: chord {i} silent"
                            );
                        }
                        for n in p.notes.iter().filter(|n| n.layer == Layer::Melody) {
                            let c = plan.chord_at(n.start);
                            // Never a half-step rub against a borrowed chord tone.
                            assert_eq!(adapt_to_chord(&plan, c, n.pitch as i32), n.pitch as i32, "{ctx}");
                            // Beats 1 and 3 are chord tones.
                            if (n.start % 2.0).abs() < EPS {
                                assert!(c.contains(&plan.key, n.pitch as i32), "{ctx}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::pitch::PitchClass;
    use crate::rng::Rng;
    use crate::style::STYLES;

    /// Every bar length the plugin allows, random energies and ideas: never panics, notes
    /// stay inside the loop.
    #[test]
    fn any_setting_renders() {
        let mut rng = Rng::new(99);
        for _ in 0..6000 {
            let style = &STYLES[rng.range(0, STYLES.len() as i32 - 1) as usize];
            let root = PitchClass::ALL[rng.range(0, 11) as usize];
            let bars = rng.range(1, 16) as u32;
            let energy = rng.f64();
            let seed = rng.range(0, 70_000) as u64;
            let plan = style.plan(root, bars, energy, seed);
            let p = generate(&plan, Layers::all(), seed);
            for n in &p.notes {
                assert!(n.start >= 0.0 && n.start + n.length <= p.length_beats + 1e-6, "{} bars {bars}", style.id);
            }
        }
    }
}
