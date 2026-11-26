//! Song mode: the same idea arranged into sections (intro, groove, build, drop, break,
//! outro). Each section is a loop of its own, written for that moment: fewer parts and a
//! lower energy in the intro, the kick cut and a snare roll at the end of the build, a
//! crash on the drop, no kick and no bass in the break. Every section shares the key, the
//! chords and each part's idea, so they all belong to one track.

use crate::generate::{generate_seeded, Layer, Layers, Note, Pattern};
use crate::plan::Plan;
use crate::style::{CLOSED_HAT, KICK, OPEN_HAT};

/// GM crash cymbal.
pub const CRASH: u8 = 49;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Intro,
    Groove,
    Build,
    Drop,
    Break,
    Outro,
}

/// Which drum voices play in a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumVoices {
    pub kick: bool,
    /// Snare, clap, rim and toms.
    pub snare: bool,
    pub hats: bool,
    pub perc: bool,
}

impl DrumVoices {
    pub const ALL: DrumVoices = DrumVoices { kick: true, snare: true, hats: true, perc: true };
}

/// How a section treats the idea.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Treatment {
    /// Section energy = main energy × `scale` + `lift`, clamped to 0..1.
    pub scale: f64,
    pub lift: f64,
    /// Parts that play (the parts switched off in the plugin stay off anyway).
    pub parts: Layers,
    pub drums: DrumVoices,
}

impl Section {
    pub const ALL: [Section; 6] = [Section::Intro, Section::Groove, Section::Build, Section::Drop, Section::Break, Section::Outro];

    pub fn label(self) -> &'static str {
        match self {
            Section::Intro => "Intro",
            Section::Groove => "Groove",
            Section::Build => "Build",
            Section::Drop => "Drop",
            Section::Break => "Break",
            Section::Outro => "Outro",
        }
    }

    /// One line on what the section does, for the plugin's screen.
    pub fn blurb(self) -> &'static str {
        match self {
            Section::Intro => "intro: kick, hats and the chords, low energy. no bass, no melody yet: room for the DJ to mix in.",
            Section::Groove => "groove: the bass comes in, full drums, the arp if it's on. the melody waits for the drop.",
            Section::Build => "build: everything rises, then the last bar cuts the kick and the bass under a snare roll.",
            Section::Drop => "drop: every part at full energy, a crash on the one.",
            Section::Break => "break: no kick, no bass. chords and melody float over the hats: ride your delays and filters.",
            Section::Outro => "outro: kick, hats, percussion and bass. the chords and melody are gone: room to mix out.",
        }
    }

    pub fn treatment(self) -> Treatment {
        let l = |chords, bass, arp, melody, drums| Layers { chords, bass, arp, melody, drums };
        let d = |kick, snare, hats, perc| DrumVoices { kick, snare, hats, perc };
        match self {
            Section::Intro => Treatment { scale: 0.6, lift: 0.0, parts: l(true, false, false, false, true), drums: d(true, false, true, false) },
            Section::Groove => Treatment { scale: 0.85, lift: 0.0, parts: l(true, true, true, false, true), drums: DrumVoices::ALL },
            Section::Build => Treatment { scale: 0.9, lift: 0.1, parts: Layers::all(), drums: DrumVoices::ALL },
            Section::Drop => Treatment { scale: 1.0, lift: 0.1, parts: Layers::all(), drums: DrumVoices::ALL },
            Section::Break => Treatment { scale: 0.5, lift: 0.0, parts: l(true, false, true, true, true), drums: d(false, false, true, false) },
            Section::Outro => Treatment { scale: 0.6, lift: 0.0, parts: l(false, true, false, false, true), drums: d(true, true, true, true) },
        }
    }
}

/// The song's layout: sections and their length in bars (before rounding up to whole loops).
/// About 4½ minutes at 124 bpm with 4-bar loops.
pub const SONG: [(Section, u32); 8] = [
    (Section::Intro, 16),
    (Section::Groove, 16),
    (Section::Build, 8),
    (Section::Drop, 32),
    (Section::Break, 16),
    (Section::Build, 8),
    (Section::Drop, 32),
    (Section::Outro, 16),
];

/// Where each section of the song starts and how long it lasts, in beats, for loops of
/// `loop_bars` bars. A section always lasts whole loops.
pub fn layout(loop_bars: u32) -> Vec<(Section, f64, f64)> {
    let loop_bars = loop_bars.max(1);
    let mut at = 0.0;
    SONG.iter()
        .map(|&(s, bars)| {
            let bars = bars.div_ceil(loop_bars).max(1) * loop_bars;
            let len = bars as f64 * 4.0;
            let out = (s, at, len);
            at += len;
            out
        })
        .collect()
}

/// The section's own loop: same idea, the section's energy, parts and drum voices. A build
/// loop ends with its transition (the kick and bass cut under a snare roll), since a build
/// always leads somewhere; a drop loop starts with a crash.
pub fn section_loop(plan: &Plan, layers: Layers, seeds: [u64; Layer::COUNT], section: Section) -> Pattern {
    let (p, mut pattern) = body(plan, layers, seeds, section);
    let drums = pattern.notes.iter().any(|n| n.layer == Layer::Drums);
    match section {
        Section::Build => cut_into_drop(&p, &mut pattern, drums),
        Section::Drop if drums => crash(&mut pattern.notes, 0.0),
        _ => {}
    }
    pattern.notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    pattern
}

/// The section's loop without its transition, and the plan it was played from.
fn body(plan: &Plan, layers: Layers, seeds: [u64; Layer::COUNT], section: Section) -> (Plan, Pattern) {
    let t = section.treatment();
    let mut p = plan.clone();
    p.energy = (plan.energy * t.scale + t.lift).clamp(0.0, 1.0);
    let on = Layers {
        chords: layers.chords && t.parts.chords,
        bass: layers.bass && t.parts.bass,
        arp: layers.arp && t.parts.arp,
        melody: layers.melody && t.parts.melody,
        drums: layers.drums && t.parts.drums,
    };
    let mut pattern = generate_seeded(&p, on, seeds);
    let perc = plan.style.drums.perc_note;
    pattern.notes.retain(|n| n.layer != Layer::Drums || plays(&t.drums, n.pitch, perc));
    (p, pattern)
}

/// The last bar of a build: kick out under a snare roll, the bass out for the last 2 beats.
fn cut_into_drop(p: &Plan, pattern: &mut Pattern, drums: bool) {
    if drums {
        build_up(p, &mut pattern.notes);
    }
    let cut = pattern.length_beats - 2.0;
    pattern.notes.retain(|n| n.layer != Layer::Bass || n.start < cut);
    for n in pattern.notes.iter_mut().filter(|n| n.layer == Layer::Bass) {
        n.length = n.length.min(cut - n.start).max(0.05);
    }
}

/// The whole song: each section's loop repeated to fill it, the build's cut on its last
/// loop only and a crash on the first beat of each drop. Starts on the host's beat 1.
pub fn song(plan: &Plan, layers: Layers, seeds: [u64; Layer::COUNT]) -> Pattern {
    let mut bodies: Vec<(Section, Plan, Pattern)> = Vec::new();
    let mut notes = Vec::new();
    let mut chords = Vec::new();
    let sections = layout(plan.bars);
    for &(section, start, len) in &sections {
        if !bodies.iter().any(|(s, _, _)| *s == section) {
            let (p, b) = body(plan, layers, seeds, section);
            bodies.push((section, p, b));
        }
        let (_, p, lp) = bodies.iter().find(|(s, _, _)| *s == section).unwrap();
        let n = (len / lp.length_beats).round().max(1.0) as usize;
        let drums = layers.drums && section.treatment().parts.drums;
        for k in 0..n {
            let at = start + k as f64 * lp.length_beats;
            let mut lp = lp.clone();
            if section == Section::Build && k + 1 == n {
                cut_into_drop(p, &mut lp, drums);
            }
            if section == Section::Drop && k == 0 && drums {
                crash(&mut lp.notes, 0.0);
            }
            notes.extend(lp.notes.iter().map(|note| Note { start: note.start + at, ..*note }));
            chords.extend(lp.chords.iter().map(|(t, name)| (t + at, name.clone())));
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let length_beats = sections.last().map_or(plan.length_beats(), |&(_, s, l)| s + l);
    Pattern { length_beats, notes, chords }
}

fn plays(v: &DrumVoices, pitch: u8, perc_note: u8) -> bool {
    match pitch {
        KICK => v.kick,
        CLOSED_HAT | OPEN_HAT => v.hats,
        CRASH => true,
        p if p == perc_note => v.perc,
        _ => v.snare,
    }
}

fn crash(notes: &mut Vec<Note>, at: f64) {
    notes.push(Note { start: at, length: 1.0, pitch: CRASH, velocity: 112, layer: Layer::Drums });
}

/// The build's last bar: no kick, and a snare roll in 8ths then 16ths, getting louder.
fn build_up(plan: &Plan, notes: &mut Vec<Note>) {
    let end = plan.length_beats();
    let from = end - 4.0;
    let snare = plan.style.drums.snare_note;
    notes.retain(|n| !(n.layer == Layer::Drums && n.start >= from && (n.pitch == KICK || n.pitch == snare || matches!(n.pitch, 45 | 47 | 48 | 50))));
    let mut t = from;
    let mut k = 0;
    while t < end - 1e-9 {
        let v = 60 + (k * 4).min(60);
        notes.push(Note { start: t, length: 0.12, pitch: snare, velocity: v as u8, layer: Layer::Drums });
        t += if t < from + 2.0 { 0.5 } else { 0.25 };
        k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PitchClass;

    fn plan(style: &str, energy: f64) -> Plan {
        crate::style::find(style).unwrap().plan(PitchClass::parse("F").unwrap(), 4, energy, 1)
    }

    fn count(p: &Pattern, f: impl Fn(&Note) -> bool) -> usize {
        p.notes.iter().filter(|n| f(n)).count()
    }

    #[test]
    fn sections_shape_the_parts() {
        let plan = plan("dub-techno", 0.8);
        let seeds = [3; Layer::COUNT];
        let all = Layers::all();
        let intro = section_loop(&plan, all, seeds, Section::Intro);
        assert_eq!(count(&intro, |n| n.layer == Layer::Bass || n.layer == Layer::Melody), 0);
        assert!(count(&intro, |n| n.pitch == KICK) > 0);
        assert_eq!(count(&intro, |n| n.layer == Layer::Drums && n.pitch == plan.style.drums.snare_note), 0);

        let brk = section_loop(&plan, all, seeds, Section::Break);
        assert_eq!(count(&brk, |n| n.pitch == KICK && n.layer == Layer::Drums), 0, "the break cuts the kick");
        assert_eq!(count(&brk, |n| n.layer == Layer::Bass), 0);
        assert!(count(&brk, |n| n.layer == Layer::Chords) > 0);

        let drop = section_loop(&plan, all, seeds, Section::Drop);
        assert!(drop.notes.iter().any(|n| n.pitch == CRASH && n.start == 0.0));
        assert!(count(&drop, |n| n.layer == Layer::Bass) > 0 && count(&drop, |n| n.layer == Layer::Melody) > 0);

        let build = section_loop(&plan, all, seeds, Section::Build);
        let last_bar = build.length_beats - 4.0;
        assert_eq!(count(&build, |n| n.pitch == KICK && n.layer == Layer::Drums && n.start >= last_bar), 0, "kick cut");
        assert!(count(&build, |n| n.pitch == plan.style.drums.snare_note && n.start >= last_bar) >= 10, "snare roll");
        assert_eq!(count(&build, |n| n.layer == Layer::Bass && n.start >= build.length_beats - 2.0), 0);
        assert!(build.notes.iter().all(|n| n.layer != Layer::Bass || n.start + n.length <= build.length_beats - 2.0 + 1e-9));
    }

    #[test]
    fn parts_switched_off_stay_off() {
        let plan = plan("techno", 0.7);
        let only_drums = Layers { drums: true, ..Layers::none() };
        for s in Section::ALL {
            let p = section_loop(&plan, only_drums, [1; Layer::COUNT], s);
            assert!(p.notes.iter().all(|n| n.layer == Layer::Drums), "{s:?}");
        }
    }

    #[test]
    fn a_song_has_every_section_in_order() {
        let plan = plan("dub-techno", 0.8);
        let song = song(&plan, Layers::all(), [5; Layer::COUNT]);
        let lay = layout(4);
        assert_eq!(song.length_beats, 144.0 * 4.0);
        assert_eq!(lay.len(), SONG.len());
        assert!(song.notes.windows(2).all(|w| w[0].start <= w[1].start));
        assert!(song.notes.iter().all(|n| n.start < song.length_beats));

        let within = |s: Section, f: &dyn Fn(&Note) -> bool| {
            lay.iter().filter(|l| l.0 == s).map(|&(_, a, len)| song.notes.iter().filter(|n| n.start >= a && n.start < a + len && f(n)).count()).sum::<usize>()
        };
        assert_eq!(within(Section::Intro, &|n| n.layer == Layer::Bass), 0);
        assert!(within(Section::Groove, &|n| n.layer == Layer::Bass) > 0);
        assert_eq!(within(Section::Break, &|n| n.pitch == KICK && n.layer == Layer::Drums), 0);
        // One crash per drop, on its first beat.
        let crashes: Vec<f64> = song.notes.iter().filter(|n| n.pitch == CRASH).map(|n| n.start).collect();
        let drops: Vec<f64> = lay.iter().filter(|l| l.0 == Section::Drop).map(|l| l.1).collect();
        assert_eq!(crashes, drops);
        // Each 8-bar build: the kick plays until its last bar, then the roll.
        for &(_, a, len) in lay.iter().filter(|l| l.0 == Section::Build) {
            let kicks = song.notes.iter().filter(|n| n.pitch == KICK && n.start >= a && n.start < a + len).map(|n| n.start).collect::<Vec<_>>();
            assert!(kicks.iter().any(|&t| t >= a + len - 8.0 && t < a + len - 4.0), "kick in the bar before the last");
            assert!(kicks.iter().all(|&t| t < a + len - 4.0), "no kick in the last bar");
        }
        // The chords line up with the plan in every loop.
        assert_eq!(song.chords.len(), plan.progression.len() * (144 / 4));
    }

    #[test]
    fn long_loops_round_sections_up() {
        let l = layout(16);
        assert!(l.iter().all(|&(_, _, len)| (len / 64.0).fract() == 0.0));
        assert_eq!(l[2].2, 64.0, "an 8-bar build becomes one 16-bar loop");
    }
}
