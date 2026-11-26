//! Styles: the "no theory needed" entry point. Each style bundles a mode, a pool of
//! progressions known to work in that genre, its swing, and one groove grid per layer
//! (see [`crate::grid`]). The seed picks among the progressions, so turning Variation
//! explores the style; Energy thins out or fills in the grids.

use crate::chord::Chord;
use crate::pitch::PitchClass;
use crate::plan::Plan;
use crate::rng::Rng;
use crate::scale::{Mode, Scale};

/// A layer's groove: grid + how long notes ring.
pub struct Part {
    pub grid: &'static str,
    /// Fraction of the gap to the next hit that a note lasts (0.3 = stabby, 1.0 = legato).
    pub gate: f64,
    /// Longest note, in beats.
    pub max_len: f64,
}

/// The bass groove also says which chord tone each hit plays.
pub struct BassPart {
    pub grid: &'static str,
    /// Same length as `grid`, read at the hit positions:
    /// `r` root, `5` fifth, `8` octave, `3` third, `7` scale step under the octave,
    /// `a` chromatic approach into the next chord.
    pub tones: &'static str,
    pub gate: f64,
    pub max_len: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpDir {
    Up,
    Down,
    UpDown,
    Random,
}

pub struct Style {
    pub id: &'static str,
    pub label: &'static str,
    pub blurb: &'static str,
    pub mode: Mode,
    pub progressions: &'static [&'static [&'static str]],
    /// Probability of turning a plain triad into a 7th chord.
    pub seventh_chance: f64,
    pub tempo: f64,
    /// 0.5 = straight 16ths, 0.66 = triplet feel (MPC-style swing amount).
    pub swing: f64,
    pub chords: Part,
    pub bass: BassPart,
    pub arp: Part,
    pub arp_dirs: &'static [ArpDir],
    /// Candidate melody positions; the melody picks a motif among them.
    pub melody: Part,
}

impl std::fmt::Debug for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Style({})", self.id)
    }
}

use ArpDir::*;

pub const STYLES: &[Style] = &[
    Style {
        id: "ukg",
        label: "UK Garage",
        blurb: "UK garage: swung 2-step, minor 7th/9th organ stabs that dodge beat one, a bouncing sub bass.",
        mode: Mode::Minor,
        progressions: &[
            &["i9", "iv9", "bVImaj7", "v7"],
            &["i7", "bVII", "bVImaj7", "bVII"],
            &["i7", "iv7", "i7", "bVII"],
            &["bVImaj7", "v7", "i9", "i9"],
            &["i9", "bIIImaj7", "iv7", "v7"],
        ],
        seventh_chance: 0.8,
        tempo: 132.0,
        swing: 0.6,
        chords: Part { grid: "...0..1...0..2.6", gate: 0.55, max_len: 0.6 },
        bass: BassPart {
            grid: "0..3..1.....2.4.0..3..1...5.2..7",
            tones: "r..8..5.....r.a.r..8..5...3.r..a",
            gate: 0.8,
            max_len: 1.0,
        },
        arp: Part { grid: "0.6.0.6.0.6.0.6.", gate: 0.6, max_len: 0.4 },
        arp_dirs: &[Up, Random],
        melody: Part { grid: "0.42.3.1.52.4.3.", gate: 0.85, max_len: 1.0 },
    },
    Style {
        id: "deep-house",
        label: "Deep House",
        blurb: "Deep house: dorian 9th chords stabbed on the off-beats, a rolling bass that answers the kick.",
        mode: Mode::Dorian,
        progressions: &[
            &["i9", "IV9", "i9", "IV9"],
            &["i7", "bVII", "IV", "i7"],
            &["i9", "bIIImaj7", "IV9", "IV9"],
            &["i9", "i9", "bVII", "bVII"],
        ],
        seventh_chance: 0.7,
        tempo: 122.0,
        swing: 0.54,
        chords: Part { grid: "..0...0...0..60.", gate: 0.6, max_len: 0.45 },
        bass: BassPart { grid: "0.....0...1.0.3.", tones: "r.....8...5.r.a.", gate: 0.7, max_len: 0.75 },
        arp: Part { grid: "0.5.0.5.0.5.0.5.", gate: 0.6, max_len: 0.4 },
        arp_dirs: &[Up, UpDown],
        melody: Part { grid: "0...2.1...3..1..", gate: 0.9, max_len: 1.5 },
    },
    Style {
        id: "dub",
        label: "Dub / Reggae",
        blurb: "Dub: one or two minor chords skanked on beats 2 and 4, the bass carries the tune with space around it.",
        mode: Mode::Minor,
        progressions: &[
            &["i", "i", "iv", "iv"],
            &["i", "bVII", "i", "bVII"],
            &["i", "i", "i", "bVII"],
            &["i", "iv", "i", "v"],
            &["i", "bVI", "bVII", "i"],
        ],
        seventh_chance: 0.1,
        tempo: 72.0,
        swing: 0.55,
        chords: Part { grid: "....0.7.....0.5.", gate: 0.4, max_len: 0.3 },
        bass: BassPart {
            grid: "0.....1.0..3.2..0.....1.0...2.4.",
            tones: "r.....3.5..8.7..r.....5.r...3.a.",
            gate: 0.9,
            max_len: 2.5,
        },
        arp: Part { grid: "......0.......5.", gate: 0.6, max_len: 0.75 },
        arp_dirs: &[Down, Random],
        melody: Part { grid: "0.....3.2...1...", gate: 0.9, max_len: 2.0 },
    },
    Style {
        id: "dub-techno",
        label: "Dub Techno",
        blurb: "Dub techno: one hypnotic minor chord stab, made for long delays and filter rides, over a steady sub.",
        mode: Mode::Minor,
        progressions: &[&["i7"], &["i9"], &["i7", "i7", "iv7", "iv7"], &["i9", "i9", "bVImaj7", "bVImaj7"]],
        seventh_chance: 0.5,
        tempo: 120.0,
        swing: 0.52,
        chords: Part { grid: "..0.......1..7..", gate: 0.35, max_len: 0.4 },
        bass: BassPart { grid: "0.....0...0...4.", tones: "r.....r...8...5.", gate: 0.6, max_len: 0.6 },
        arp: Part { grid: "0.4.0.4.0.4.0.4.", gate: 0.5, max_len: 0.3 },
        arp_dirs: &[Up, Random],
        melody: Part { grid: "0.......4...2...", gate: 0.9, max_len: 2.0 },
    },
    Style {
        id: "soul",
        label: "Soul",
        blurb: "Soul: warm major 7ths and ii-V moves played legato, a bass that walks between the chords.",
        mode: Mode::Major,
        progressions: &[
            &["Imaj7", "vi7", "ii7", "V7"],
            &["IVmaj7", "III7", "vi7", "vi7"],
            &["ii7", "V7", "Imaj7", "IVmaj7"],
            &["Imaj7", "iii7", "IVmaj7", "Vsus4"],
            &["ii9", "V7", "iii7", "vi7"],
        ],
        seventh_chance: 0.9,
        tempo: 92.0,
        swing: 0.55,
        chords: Part { grid: "0.....1.0...3.2.", gate: 0.95, max_len: 2.0 },
        bass: BassPart { grid: "0...2.1.0...1.3.", tones: "r...8.5.r...5.a.", gate: 0.85, max_len: 1.5 },
        arp: Part { grid: "0.2.0.2.0.2.0.2.", gate: 0.7, max_len: 0.5 },
        arp_dirs: &[UpDown, Up],
        melody: Part { grid: "0.3.1.4.0.2.1.5.", gate: 0.9, max_len: 1.5 },
    },
    Style {
        id: "lofi",
        label: "Lo-fi Hip-hop",
        blurb: "Lo-fi: jazzy 7th chords behind a lazy, heavily swung groove.",
        mode: Mode::Major,
        progressions: &[
            &["ii7", "V7", "Imaj7", "vi7"],
            &["IVmaj7", "iii7", "ii7", "Imaj7"],
            &["ii9", "V9", "Imaj7", "Imaj7"],
            &["IVmaj7", "V7", "iii7", "vi7"],
        ],
        seventh_chance: 0.8,
        tempo: 82.0,
        swing: 0.62,
        chords: Part { grid: "0.....0.......4.", gate: 0.95, max_len: 2.5 },
        bass: BassPart { grid: "0.....1...0..2..", tones: "r.....5...r..a..", gate: 0.85, max_len: 1.5 },
        arp: Part { grid: "0.3.0.3.0.3.0.3.", gate: 0.7, max_len: 0.5 },
        arp_dirs: &[Up, Random],
        melody: Part { grid: "0.3.2.4.1.3.2.5.", gate: 0.9, max_len: 1.5 },
    },
    Style {
        id: "disco",
        label: "Disco / Nu-disco",
        blurb: "Disco: dorian 7th chords chopped on the off-beats, the classic octave-jumping bass.",
        mode: Mode::Dorian,
        progressions: &[
            &["i7", "IV7", "i7", "IV7"],
            &["i7", "bVII", "bVI", "bVII"],
            &["i9", "IV9", "bVII", "i9"],
            &["i7", "iv7", "bVII", "bIII"],
        ],
        seventh_chance: 0.6,
        tempo: 118.0,
        swing: 0.52,
        chords: Part { grid: "..0.5.0.6.0.5.0.", gate: 0.5, max_len: 0.4 },
        bass: BassPart { grid: "0.0.0.0.0.0.0.06", tones: "r.8.r.8.r.8.r.8a", gate: 0.6, max_len: 0.3 },
        arp: Part { grid: "0605060506050605", gate: 0.6, max_len: 0.2 },
        arp_dirs: &[Up, UpDown],
        melody: Part { grid: "0.2.1.3.0.2.1.4.", gate: 0.85, max_len: 1.0 },
    },
    Style {
        id: "ambient",
        label: "Ambient / Cinematic",
        blurb: "Ambient: long minor chords that swell and hold, with a slow melody floating on top.",
        mode: Mode::Minor,
        progressions: &[
            &["i", "bVI", "bIII", "bVII"],
            &["bVI", "bVII", "i", "i"],
            &["i", "bVII", "bVI", "bVII"],
            &["i", "v", "bVI", "bIII"],
        ],
        seventh_chance: 0.3,
        tempo: 90.0,
        swing: 0.5,
        chords: Part { grid: "0...............", gate: 1.0, max_len: 16.0 },
        bass: BassPart { grid: "0...............", tones: "r...............", gate: 1.0, max_len: 16.0 },
        arp: Part { grid: "0.4.0.4.0.4.0.4.", gate: 0.8, max_len: 0.5 },
        arp_dirs: &[UpDown, Up, Random],
        melody: Part { grid: "0.......3...2...", gate: 0.95, max_len: 3.0 },
    },
];

pub fn find(id: &str) -> Option<&'static Style> {
    let id = id.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    STYLES.iter().find(|s| s.id == id).or_else(|| {
        STYLES.iter().find(|s| s.label.to_ascii_lowercase().replace([' ', '_'], "-").contains(&id))
    })
}

impl Style {
    /// Turns the style into a concrete plan. The seed chooses progression and arp direction.
    pub fn plan(&'static self, root: PitchClass, bars: u32, energy: f64, seed: u64) -> Plan {
        let mut rng = Rng::new(seed).fork(0x7374796C);
        let key = Scale::new(root, self.mode);
        let symbols = rng.pick(self.progressions);
        let progression = symbols
            .iter()
            .map(|s| {
                let chord = Chord::parse(s).expect("built-in progressions are valid");
                if rng.chance(self.seventh_chance) {
                    chord.with_seventh(&key)
                } else {
                    chord
                }
            })
            .collect();
        Plan {
            key,
            progression,
            bars: bars.clamp(1, 16),
            style: self,
            energy: energy.clamp(0.0, 1.0),
            swing: self.swing,
            arp_dir: *rng.pick(self.arp_dirs),
            tempo: self.tempo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid;

    #[test]
    fn every_style_is_well_formed() {
        for s in STYLES {
            for p in s.progressions {
                for c in *p {
                    Chord::parse(c).unwrap_or_else(|e| panic!("{}: {e}", s.id));
                }
            }
            for (name, g) in [("chords", s.chords.grid), ("bass", s.bass.grid), ("arp", s.arp.grid), ("melody", s.melody.grid)] {
                assert!(grid::is_valid(g), "{} {name} grid '{g}'", s.id);
                assert!(grid::parse(g).iter().any(|c| *c == Some(0)), "{} {name} has no core hit", s.id);
            }
            assert_eq!(s.bass.grid.len(), s.bass.tones.len(), "{} bass tones length", s.id);
            for (g, t) in s.bass.grid.bytes().zip(s.bass.tones.bytes()) {
                if g != b'.' {
                    assert!(b"r5837a".contains(&t), "{}: bad tone '{}'", s.id, t as char);
                }
            }
            assert_eq!(s.melody.grid.len(), 16, "{} melody grid is one bar", s.id);
            assert!(find(s.id).is_some());
        }
    }

    #[test]
    fn finds_by_label() {
        assert_eq!(find("uk garage").unwrap().id, "ukg");
        assert_eq!(find("Dub Techno").unwrap().id, "dub-techno");
        assert_eq!(find("dub").unwrap().id, "dub");
    }

    #[test]
    fn seed_changes_choice_but_is_deterministic() {
        let s = find("soul").unwrap();
        let c = PitchClass::parse("C").unwrap();
        assert_eq!(s.plan(c, 4, 0.5, 1).chord_names(), s.plan(c, 4, 0.5, 1).chord_names());
        let distinct: std::collections::HashSet<_> = (0..40).map(|n| s.plan(c, 4, 0.5, n).chord_names()).collect();
        assert!(distinct.len() > 3);
    }
}
