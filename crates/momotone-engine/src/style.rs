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

/// A drum kit pattern: one grid per voice (same grid language as the other parts).
/// Notes follow the General MIDI drum map, which most drum machines understand.
pub struct Drums {
    pub kick: &'static str,
    pub snare: &'static str,
    /// 38 snare, 39 clap or 37 rim shot.
    pub snare_note: u8,
    pub hat: &'static str,
    pub open_hat: &'static str,
    pub perc: &'static str,
    /// e.g. 70 shaker, 75 claves, 64 low conga, 51 ride, 56 cowbell, 54 tambourine.
    pub perc_note: u8,
}

pub const KICK: u8 = 36;
pub const CLOSED_HAT: u8 = 42;
pub const OPEN_HAT: u8 = 46;
pub const SNARE: u8 = 38;
pub const CLAP: u8 = 39;
pub const RIM: u8 = 37;

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
    /// Family it's listed under (see [`FAMILIES`]).
    pub family: &'static str,
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
    pub drums: &'static Drums,
}

impl std::fmt::Debug for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Style({})", self.id)
    }
}

/// Families in display order. Every style belongs to one of them.
pub const FAMILIES: &[&str] =
    &["House", "Techno", "Trance", "UK & Bass", "Breaks & Electro", "Disco & Funk", "Dub & Roots", "Downtempo", "Hard"];

use ArpDir::*;

const fn part(grid: &'static str, gate: f64, max_len: f64) -> Part {
    Part { grid, gate, max_len }
}

const fn bass(grid: &'static str, tones: &'static str, gate: f64, max_len: f64) -> BassPart {
    BassPart { grid, tones, gate, max_len }
}

// Shared grooves (one character per 16th; digits = energy threshold, see `grid`).
const HELD: &str = "0...............";
const OFFBEAT: &str = "..0...0...0...0.";
const GATED: &str = "0.1.0.1.0.1.0.1.";
const A8: &str = "0.6.0.6.0.6.0.6.";
const A16: &str = "0605060506050605";
const M_BUSY: &str = "0.3.1.4.0.2.1.5.";
const M_SYNC: &str = "0...2.1...3..1..";
const M_SPARSE: &str = "0.......3...2...";
const M_16: &str = "0213021302130213";


// Drum kits. Kick, snare/clap, closed hat, open hat, percussion; "." rest, digit = energy threshold.
const K4: &str = "0...0...0...0...";
const BACKBEAT: &str = "....0.......0...";
const OFF8: &str = "..0...0...0...0.";
const EIGHTS: &str = "0.0.0.0.0.0.0.0.";
const SIXTEENS: &str = "0606060606060606";
const NONE: &str = "................";

const fn kit(kick: &'static str, snare: &'static str, snare_note: u8, hat: &'static str, open_hat: &'static str, perc: &'static str, perc_note: u8) -> Drums {
    Drums { kick, snare, snare_note, hat, open_hat, perc, perc_note }
}

const KIT_HOUSE: Drums = kit(K4, BACKBEAT, CLAP, OFF8, "......4.......4.", "5.5.5.5.5.5.5.5.", 70);
const KIT_DEEP: Drums = kit(K4, "....3.......3...", CLAP, OFF8, "..............4.", "...5......5.....", 75);
const KIT_TECH_HOUSE: Drums = kit(K4, BACKBEAT, CLAP, "..0.6.0...0.6.0.", "......5.......5.", "...4.....4...4..", 64);
const KIT_AFRO: Drums = kit(K4, "......5.......5.", RIM, OFF8, NONE, "...0..0...0..0..", 63);
const KIT_AMAPIANO: Drums = kit("0...4...0...4...", BACKBEAT, CLAP, NONE, NONE, SIXTEENS, 70);
const KIT_TECHNO: Drums = kit(K4, "....4.......4...", CLAP, "..0.6.0.6.0.6.0.", "..............5.", "6.6.6.6.6.6.6.6.", 51);
const KIT_MINIMAL: Drums = kit(K4, "......4.....5...", RIM, OFF8, NONE, "...5....4.5.....", 75);
const KIT_DUB_TECHNO: Drums = kit(K4, "........5.......", CLAP, OFF8, NONE, "5.5.5.5.5.5.5.5.", 70);
const KIT_DETROIT: Drums = kit(K4, BACKBEAT, CLAP, "..0.5.0.5.0.5.0.", NONE, "0.6.0.6.0.6.0.6.", 51);
const KIT_ACID: Drums = kit(K4, BACKBEAT, CLAP, "..0.5.0.5.0.5.0.", "......4.......4.", "...6......6.....", 56);
const KIT_HARD: Drums = kit("0...0...0...0..6", "....3.......3...", CLAP, "..0.6.0.6.0.6.0.", NONE, "0.5.0.5.0.5.0.5.", 51);
const KIT_TRANCE: Drums = kit(K4, BACKBEAT, CLAP, "...6...6...6...6", OFF8, "4.4.4.4.4.4.4.4.", 51);
const KIT_PSY: Drums = kit(K4, "....4.......4...", CLAP, OFF8, NONE, "...5...5...5...5", 75);
const KIT_GARAGE: Drums = kit("0.....5...0.....", BACKBEAT, SNARE, SIXTEENS, "......5.......5.", "...4..4....4....", RIM);
const KIT_BASSLINE: Drums = kit(K4, BACKBEAT, SNARE, "..0.6.0.6.0.6.0.", NONE, "...4..4....4....", RIM);
const KIT_DNB: Drums = kit("0.........0.....", BACKBEAT, SNARE, "0.0.0.060.0.0.06", NONE, "..5...5...5...5.", 51);
const KIT_JUNGLE: Drums = kit("0.0.......0.....", "....0..6.0..0...", SNARE, EIGHTS, NONE, "......5.......5.", 51);
const KIT_DUBSTEP: Drums = kit("0.........5.....", "........0.......", SNARE, "0.6.0.6.0.6.0.6.", NONE, "...........5....", 75);
const KIT_BREAKS: Drums = kit("0.....0...0..5..", BACKBEAT, SNARE, EIGHTS, "......5.......5.", NONE, 75);
const KIT_ELECTRO: Drums = kit("0.....0...0.0...", BACKBEAT, CLAP, SIXTEENS, NONE, "...5.....5......", 56);
const KIT_FOOTWORK: Drums = kit("0..0..0...0..0..", "........0.......", CLAP, EIGHTS, NONE, "...5..5....5..5.", 75);
const KIT_DISCO: Drums = kit(K4, BACKBEAT, SNARE, "0...0...0...0...", OFF8, "....5.......5...", 54);
const KIT_ITALO: Drums = kit(K4, BACKBEAT, SNARE, OFF8, NONE, "0606060606060606", 70);
const KIT_FUNK: Drums = kit("0.....0...0..5..", "....0..5....0.6.", SNARE, SIXTEENS, "..............4.", NONE, 56);
const KIT_SOUL: Drums = kit("0......5..0.....", BACKBEAT, SNARE, EIGHTS, NONE, "....5.......5...", 54);
const KIT_DUB: Drums = kit("........0.......", "........0.......", RIM, "..0.5.0.5.0.5.0.", "..............4.", "....5.......5...", 75);
const KIT_DANCEHALL: Drums = kit("0.......0.......", "...0..0....0..0.", SNARE, EIGHTS, NONE, "......5.......5.", 75);
const KIT_AMBIENT: Drums = kit("0.......4.......", NONE, SNARE, "........5.......", NONE, "..........4.....", 75);
const KIT_TRIP_HOP: Drums = kit("0......5..0.....", BACKBEAT, SNARE, EIGHTS, "......4.........", NONE, 75);
const KIT_LOFI: Drums = kit("0......5..0.....", BACKBEAT, SNARE, EIGHTS, NONE, "..5.......5.....", 70);
const KIT_DOWNTEMPO: Drums = kit("0.....0...0.....", BACKBEAT, SNARE, EIGHTS, NONE, "..5.......5.....", 70);
const KIT_IDM: Drums = kit("0..5..0...0..4..", "....0..6.....0..", SNARE, SIXTEENS, NONE, "..5.......5..5..", 75);
const KIT_SYNTHWAVE: Drums = kit("0.......0.......", BACKBEAT, SNARE, EIGHTS, NONE, NONE, 75);
const KIT_HARDCORE: Drums = kit(K4, "....4.......4...", CLAP, OFF8, NONE, NONE, 75);

const MINOR_POP: &[&[&str]] = &[
    &["i", "bVI", "bIII", "bVII"],
    &["bVI", "bVII", "i", "i"],
    &["i", "bVII", "bVI", "bVII"],
    &["i", "v", "bVI", "bIII"],
];

pub const STYLES: &[Style] = &[
    // ------------------------------------------------------------------ House
    Style {
        id: "house",
        label: "House",
        family: "House",
        blurb: "House: syncopated dorian piano chords, a bass that bounces between the kicks.",
        mode: Mode::Dorian,
        progressions: &[&["i7", "IV9", "i7", "IV9"], &["i9", "bVII", "bVImaj7", "bVII"], &["i7", "iv7", "bVII", "bIIImaj7"], &["bVImaj7", "bVII", "i7", "i7"]],
        seventh_chance: 0.7,
        tempo: 124.0,
        swing: 0.54,
        chords: part("0..0..0.4.0..05.", 0.5, 0.5),
        bass: bass("0.....0...1.0.3.", "r.....8...5.r.a.", 0.7, 0.75),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up, UpDown],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_HOUSE,
    },
    Style {
        id: "deep-house",
        label: "Deep House",
        family: "House",
        blurb: "Deep house: dorian 9th chords stabbed on the off-beats, a rolling bass that answers the kick.",
        mode: Mode::Dorian,
        progressions: &[&["i9", "IV9", "i9", "IV9"], &["i7", "bVII", "IV", "i7"], &["i9", "bIIImaj7", "IV9", "IV9"], &["i9", "i9", "bVII", "bVII"]],
        seventh_chance: 0.7,
        tempo: 122.0,
        swing: 0.54,
        chords: part("..0...0...0..60.", 0.6, 0.45),
        bass: bass("0.....0...1.0.3.", "r.....8...5.r.a.", 0.7, 0.75),
        arp: part("0.5.0.5.0.5.0.5.", 0.6, 0.4),
        arp_dirs: &[Up, UpDown],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_DEEP,
    },
    Style {
        id: "tech-house",
        label: "Tech House",
        family: "House",
        blurb: "Tech house: one or two minor chords as short stabs, all the movement in a chopped, groovy bassline.",
        mode: Mode::Minor,
        progressions: &[&["i7"], &["i7", "i7", "iv7", "iv7"], &["i9", "bVII"]],
        seventh_chance: 0.6,
        tempo: 126.0,
        swing: 0.54,
        chords: part("..0.......0..5..", 0.3, 0.3),
        bass: bass("0.0..1.0..0.0.5.", "r.r..8.r..5.r.a.", 0.6, 0.4),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Random, Up],
        melody: part(M_SPARSE, 0.85, 1.0),
        drums: &KIT_TECH_HOUSE,
    },
    Style {
        id: "afro-house",
        label: "Afro House",
        family: "House",
        blurb: "Afro house: hypnotic minor loops on a 3-3-2 push, with a bass that rolls under the percussion.",
        mode: Mode::Minor,
        progressions: &[&["i7", "bVII", "bVI", "bVII"], &["i9", "iv7", "i9", "bVII"], &["i7", "bIIImaj7", "bVII", "iv7"]],
        seventh_chance: 0.6,
        tempo: 122.0,
        swing: 0.56,
        chords: part("0..0..0...0.0...", 0.6, 0.75),
        bass: bass("0..0..0...0..2..", "r..8..5...r..a..", 0.7, 0.75),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[UpDown, Random],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_AFRO,
    },
    Style {
        id: "amapiano",
        label: "Amapiano",
        family: "House",
        blurb: "Amapiano: jazzy dorian 9ths on piano, the log-drum bass answering off the beat.",
        mode: Mode::Dorian,
        progressions: &[&["i9", "IV9", "bIIImaj7", "IV9"], &["i7", "i7", "IV9", "IV9"], &["bVImaj7", "bVII", "i9", "i9"]],
        seventh_chance: 0.9,
        tempo: 112.0,
        swing: 0.58,
        chords: part("0..1..0...2.0...", 0.8, 1.0),
        bass: bass("...0..0...0.0...", "...r..8...5.r...", 0.7, 0.6),
        arp: part(A8, 0.7, 0.4),
        arp_dirs: &[UpDown, Up],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_AMAPIANO,
    },
    // ----------------------------------------------------------------- Techno
    Style {
        id: "techno",
        label: "Techno",
        family: "Techno",
        blurb: "Techno: one dark chord as a short stab, a driving off-beat bass, very little harmony.",
        mode: Mode::Minor,
        progressions: &[&["i"], &["i7"], &["i", "i", "bVI", "bVII"], &["i", "bII"]],
        seventh_chance: 0.3,
        tempo: 130.0,
        swing: 0.5,
        chords: part("..0...1...0..4..", 0.3, 0.3),
        bass: bass("..0.5.0.6.0.5.0.", "..r.8.r.8.r.8.r.", 0.5, 0.25),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Up, Random],
        melody: part(M_SPARSE, 0.85, 1.0),
        drums: &KIT_TECHNO,
    },
    Style {
        id: "minimal",
        label: "Minimal",
        family: "Techno",
        blurb: "Minimal: tiny chord blips with lots of space, a bass that plays only what it must.",
        mode: Mode::Minor,
        progressions: &[&["i7"], &["i9"], &["i7", "bVII"]],
        seventh_chance: 0.5,
        tempo: 126.0,
        swing: 0.55,
        chords: part("......0.....3...", 0.25, 0.3),
        bass: bass("0..2..0.....1...", "r..8..r.....5...", 0.6, 0.5),
        arp: part("0.4.0.4.0.4.0.4.", 0.5, 0.3),
        arp_dirs: &[Random, Up],
        melody: part(M_SPARSE, 0.8, 1.0),
        drums: &KIT_MINIMAL,
    },
    Style {
        id: "dub-techno",
        label: "Dub Techno",
        family: "Techno",
        blurb: "Dub techno: one hypnotic minor chord stab, made for long delays and filter rides, over a steady sub.",
        mode: Mode::Minor,
        progressions: &[&["i7"], &["i9"], &["i7", "i7", "iv7", "iv7"], &["i9", "i9", "bVImaj7", "bVImaj7"]],
        seventh_chance: 0.5,
        tempo: 120.0,
        swing: 0.52,
        chords: part("..0.......1..7..", 0.35, 0.4),
        bass: bass("0.....0...0...4.", "r.....r...8...5.", 0.6, 0.6),
        arp: part("0.4.0.4.0.4.0.4.", 0.5, 0.3),
        arp_dirs: &[Up, Random],
        melody: part(M_SPARSE, 0.9, 2.0),
        drums: &KIT_DUB_TECHNO,
    },
    Style {
        id: "detroit",
        label: "Detroit Techno",
        family: "Techno",
        blurb: "Detroit techno: soulful minor 9th strings over a machine groove, hopeful and melancholic.",
        mode: Mode::Minor,
        progressions: &[&["i9", "bVII", "bVImaj7", "bVII"], &["i7", "iv9", "i7", "iv9"], &["bVImaj7", "bVII", "i9", "i9"]],
        seventh_chance: 0.8,
        tempo: 128.0,
        swing: 0.52,
        chords: part("0.......2.......", 0.95, 4.0),
        bass: bass("..0...0...0..10.", "..r...8...r..5r.", 0.6, 0.4),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up, UpDown],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_DETROIT,
    },
    Style {
        id: "acid",
        label: "Acid",
        family: "Techno",
        blurb: "Acid: a squelchy 303-style 16th bassline is the star, chords stay out of its way.",
        mode: Mode::Minor,
        progressions: &[&["i"], &["i", "i", "bII", "i"], &["i", "bVII"]],
        seventh_chance: 0.1,
        tempo: 128.0,
        swing: 0.55,
        chords: part("......0.......4.", 0.3, 0.3),
        bass: bass("0.03.0.20.0.0310", "r.8r.3.5r.8.7rar", 0.5, 0.25),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Random],
        melody: part(M_SPARSE, 0.85, 1.0),
        drums: &KIT_ACID,
    },
    Style {
        id: "hard-techno",
        label: "Hard Techno",
        family: "Techno",
        blurb: "Hard techno: fast and relentless, one stab, a rumbling off-beat bass.",
        mode: Mode::Minor,
        progressions: &[&["i"], &["i", "bII"], &["i", "bVI"]],
        seventh_chance: 0.0,
        tempo: 145.0,
        swing: 0.5,
        chords: part("..0.......0.....", 0.25, 0.25),
        bass: bass("..0.5.0.6.0.5.0.", "..r.8.r.8.r.8.r.", 0.5, 0.2),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Up, Down],
        melody: part(M_SPARSE, 0.8, 1.0),
        drums: &KIT_HARD,
    },
    // ----------------------------------------------------------------- Trance
    Style {
        id: "trance",
        label: "Trance",
        family: "Trance",
        blurb: "Trance: big minor progressions, gated supersaw chords and the classic off-beat bass.",
        mode: Mode::Minor,
        progressions: MINOR_POP,
        seventh_chance: 0.0,
        tempo: 138.0,
        swing: 0.5,
        chords: part(GATED, 0.45, 0.25),
        bass: bass(OFFBEAT, "..r...r...r...r.", 0.5, 0.25),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up, UpDown],
        melody: part(M_BUSY, 0.85, 1.0),
        drums: &KIT_TRANCE,
    },
    Style {
        id: "progressive",
        label: "Progressive",
        family: "Trance",
        blurb: "Progressive: slow-moving, emotional minor chords and a rolling bass that builds and builds.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVI", "bVII", "i"], &["i9", "bVImaj7", "bIIImaj7", "bVII"], &["i", "iv", "bVI", "bVII"]],
        seventh_chance: 0.3,
        tempo: 124.0,
        swing: 0.5,
        chords: part("0.....0.....3...", 0.9, 1.5),
        bass: bass("..0..60...0..60.", "..r..8r...r..8r.", 0.5, 0.25),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[UpDown, Up],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_TRANCE,
    },
    Style {
        id: "psytrance",
        label: "Psytrance",
        family: "Trance",
        blurb: "Psytrance: phrygian darkness, the rolling 16th bass between every kick, twisting leads.",
        mode: Mode::Phrygian,
        progressions: &[&["i"], &["i", "bII"], &["i", "i", "bVII", "bII"]],
        seventh_chance: 0.0,
        tempo: 142.0,
        swing: 0.5,
        chords: part(HELD, 0.3, 0.5),
        bass: bass(".000.000.000.000", ".rrr.r8r.rrr.r5r", 0.5, 0.2),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Random, Up],
        melody: part(M_16, 0.8, 0.5),
        drums: &KIT_PSY,
    },
    // -------------------------------------------------------------- UK & Bass
    Style {
        id: "ukg",
        label: "UK Garage",
        family: "UK & Bass",
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
        chords: part("...0..1...0..2.6", 0.55, 0.6),
        bass: bass("0..3..1.....2.4.0..3..1...5.2..7", "r..8..5.....r.a.r..8..5...3.r..a", 0.8, 1.0),
        arp: part(A8, 0.6, 0.4),
        arp_dirs: &[Up, Random],
        melody: part("0.42.3.1.52.4.3.", 0.85, 1.0),
        drums: &KIT_GARAGE,
    },
    Style {
        id: "bassline",
        label: "Bassline",
        family: "UK & Bass",
        blurb: "Bassline: swung organ stabs and a bouncy, octave-jumping bass that does all the talking.",
        mode: Mode::Minor,
        progressions: &[&["i7", "bVII", "bVImaj7", "bVII"], &["i", "iv", "i", "bVII"]],
        seventh_chance: 0.5,
        tempo: 135.0,
        swing: 0.58,
        chords: part(OFFBEAT, 0.4, 0.3),
        bass: bass("0.0.0..10.0.0..1", "r.8.r..5r.8.r..a", 0.6, 0.3),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Up, Random],
        melody: part("0.42.3.1.52.4.3.", 0.85, 1.0),
        drums: &KIT_BASSLINE,
    },
    Style {
        id: "dnb",
        label: "Drum & Bass",
        family: "UK & Bass",
        blurb: "Drum & bass: dark minor chords held long, a deep reese bass that moves every bar.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVI", "bIII", "bVII"], &["i9", "i9", "bVImaj7", "bVII"], &["i", "iv", "bVI", "v"]],
        seventh_chance: 0.3,
        tempo: 174.0,
        swing: 0.5,
        chords: part("0.......1.......", 0.9, 2.0),
        bass: bass("0.........0.2...0.......1.0.....", "r.........5.8...r.......7.a.....", 0.9, 2.0),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Up, UpDown],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_DNB,
    },
    Style {
        id: "liquid-dnb",
        label: "Liquid DnB",
        family: "UK & Bass",
        blurb: "Liquid drum & bass: lush major 7ths and soulful melodies at full speed.",
        mode: Mode::Major,
        progressions: &[&["IVmaj7", "iii7", "vi7", "vi7"], &["Imaj7", "iii7", "IVmaj7", "V7"], &["vi9", "IVmaj7", "Imaj7", "V7"]],
        seventh_chance: 0.9,
        tempo: 174.0,
        swing: 0.52,
        chords: part("0.......1...2...", 0.95, 4.0),
        bass: bass("0.........0.2...0.......1.0.....", "r.........5.8...r.......7.a.....", 0.9, 2.0),
        arp: part("0.4.0.4.0.4.0.4.", 0.6, 0.25),
        arp_dirs: &[UpDown, Up],
        melody: part(M_BUSY, 0.9, 1.0),
        drums: &KIT_DNB,
    },
    Style {
        id: "jungle",
        label: "Jungle",
        family: "UK & Bass",
        blurb: "Jungle: warm minor pads and a big, slow sub bass under fast breaks.",
        mode: Mode::Minor,
        progressions: &[&["i", "i", "bVII", "bVII"], &["i7", "iv7", "i7", "bVI"], &["i"]],
        seventh_chance: 0.4,
        tempo: 165.0,
        swing: 0.52,
        chords: part(HELD, 1.0, 4.0),
        bass: bass("0.......0.1.....", "r.......5.8.....", 0.9, 2.0),
        arp: part(A8, 0.6, 0.3),
        arp_dirs: &[Up, Random],
        melody: part(M_SPARSE, 0.9, 2.0),
        drums: &KIT_JUNGLE,
    },
    Style {
        id: "dubstep",
        label: "Dubstep",
        family: "UK & Bass",
        blurb: "Dubstep: half-time weight, dark minor chords, a heavy sub that leaves space for the snare.",
        mode: Mode::Minor,
        progressions: &[&["i", "i", "bVI", "bVII"], &["i", "bII"], &["i", "iv", "bVI", "v"]],
        seventh_chance: 0.1,
        tempo: 140.0,
        swing: 0.5,
        chords: part(HELD, 0.6, 3.0),
        bass: bass("0.......1.....2.", "r.......8.....7.", 0.9, 2.0),
        arp: part("0...0...0...0...", 0.6, 0.4),
        arp_dirs: &[Up, Down],
        melody: part(M_SPARSE, 0.9, 2.0),
        drums: &KIT_DUBSTEP,
    },
    // ------------------------------------------------------- Breaks & Electro
    Style {
        id: "breakbeat",
        label: "Breakbeat",
        family: "Breaks & Electro",
        blurb: "Breakbeat: punchy minor stabs and a bass that locks to the broken kick pattern.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVII", "bVI", "bVII"], &["i7", "iv7"], &["i", "bIII", "iv", "bVI"]],
        seventh_chance: 0.3,
        tempo: 130.0,
        swing: 0.54,
        chords: part("0.....0.....3...", 0.5, 0.75),
        bass: bass("0.....0.0.....1.", "r.....8.5.....a.", 0.7, 0.75),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Up, Random],
        melody: part(M_SYNC, 0.85, 1.0),
        drums: &KIT_BREAKS,
    },
    Style {
        id: "electro",
        label: "Electro",
        family: "Breaks & Electro",
        blurb: "Electro: cold minor stabs, a robotic syncopated bass, Detroit and Kraftwerk DNA.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVII", "i", "bVI"], &["i", "bII", "i", "bVII"], &["i7"]],
        seventh_chance: 0.2,
        tempo: 128.0,
        swing: 0.5,
        chords: part("0.......0.....2.", 0.3, 0.3),
        bass: bass("0..0..0...0.0...", "r..r..8...5.r...", 0.6, 0.4),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Down, Up],
        melody: part(M_BUSY, 0.8, 0.75),
        drums: &KIT_ELECTRO,
    },
    Style {
        id: "footwork",
        label: "Footwork / Juke",
        family: "Breaks & Electro",
        blurb: "Footwork: chopped jazzy chords in triplet-ish stutters over a fast, sparse sub.",
        mode: Mode::Minor,
        progressions: &[&["i9", "bVImaj7"], &["i7", "iv7", "bVII", "bIIImaj7"]],
        seventh_chance: 0.8,
        tempo: 160.0,
        swing: 0.5,
        chords: part("0..0..0...0..0..", 0.5, 0.5),
        bass: bass("0..0..0.....0...", "r..8..r.....5...", 0.7, 0.75),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Random],
        melody: part("0..2..0...3..1..", 0.8, 1.0),
        drums: &KIT_FOOTWORK,
    },
    // ----------------------------------------------------------- Disco & Funk
    Style {
        id: "disco",
        label: "Disco / Nu-disco",
        family: "Disco & Funk",
        blurb: "Disco: dorian 7th chords chopped on the off-beats, the classic octave-jumping bass.",
        mode: Mode::Dorian,
        progressions: &[&["i7", "IV7", "i7", "IV7"], &["i7", "bVII", "bVI", "bVII"], &["i9", "IV9", "bVII", "i9"], &["i7", "iv7", "bVII", "bIII"]],
        seventh_chance: 0.6,
        tempo: 118.0,
        swing: 0.52,
        chords: part("..0.5.0.6.0.5.0.", 0.5, 0.4),
        bass: bass("0.0.0.0.0.0.0.06", "r.8.r.8.r.8.r.8a", 0.6, 0.3),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up, UpDown],
        melody: part("0.2.1.3.0.2.1.4.", 0.85, 1.0),
        drums: &KIT_DISCO,
    },
    Style {
        id: "italo",
        label: "Italo Disco",
        family: "Disco & Funk",
        blurb: "Italo: dramatic minor chords, galloping 16th octave bass, shiny synth melodies.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVI", "bVII", "i"], &["i", "bIII", "bVII", "bVI"], &["i", "iv", "bVII", "bIII"]],
        seventh_chance: 0.1,
        tempo: 120.0,
        swing: 0.5,
        chords: part(OFFBEAT, 0.5, 0.4),
        bass: bass("0606060606060606", "r8r8r8r8r8r8r8r8", 0.6, 0.15),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up],
        melody: part(M_BUSY, 0.85, 1.0),
        drums: &KIT_ITALO,
    },
    Style {
        id: "funk",
        label: "Funk / Boogie",
        family: "Disco & Funk",
        blurb: "Funk: scratchy 16th chord chops on a dorian vamp, a slap-happy syncopated bass.",
        mode: Mode::Dorian,
        progressions: &[&["i7", "IV7"], &["i9", "IV9", "i9", "bVII"], &["i7", "i7", "IV7", "IV7"]],
        seventh_chance: 0.7,
        tempo: 108.0,
        swing: 0.56,
        chords: part("0.20.10.0.20.10.", 0.3, 0.15),
        bass: bass("0..0.20..0..0.5.", "r..8.5r..r..7.a.", 0.6, 0.4),
        arp: part(A16, 0.5, 0.2),
        arp_dirs: &[Random, Up],
        melody: part(M_BUSY, 0.85, 1.0),
        drums: &KIT_FUNK,
    },
    Style {
        id: "soul",
        label: "Soul",
        family: "Disco & Funk",
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
        chords: part("0.....1.0...3.2.", 0.95, 2.0),
        bass: bass("0...2.1.0...1.3.", "r...8.5.r...5.a.", 0.85, 1.5),
        arp: part("0.2.0.2.0.2.0.2.", 0.7, 0.5),
        arp_dirs: &[UpDown, Up],
        melody: part(M_BUSY, 0.9, 1.5),
        drums: &KIT_SOUL,
    },
    // ------------------------------------------------------------ Dub & Roots
    Style {
        id: "dub",
        label: "Dub / Reggae",
        family: "Dub & Roots",
        blurb: "Dub: one or two minor chords skanked on beats 2 and 4, the bass carries the tune with space around it.",
        mode: Mode::Minor,
        progressions: &[&["i", "i", "iv", "iv"], &["i", "bVII", "i", "bVII"], &["i", "i", "i", "bVII"], &["i", "iv", "i", "v"], &["i", "bVI", "bVII", "i"]],
        seventh_chance: 0.1,
        tempo: 72.0,
        swing: 0.55,
        chords: part("....0.7.....0.5.", 0.4, 0.3),
        bass: bass("0.....1.0..3.2..0.....1.0...2.4.", "r.....3.5..8.7..r.....5.r...3.a.", 0.9, 2.5),
        arp: part("......0.......5.", 0.6, 0.75),
        arp_dirs: &[Down, Random],
        melody: part("0.....3.2...1...", 0.9, 2.0),
        drums: &KIT_DUB,
    },
    Style {
        id: "dancehall",
        label: "Dancehall",
        family: "Dub & Roots",
        blurb: "Dancehall: minor riddim, chords pushed off the beat, a bass that bounces with the dembow.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVII", "bVI", "bVII"], &["i", "iv", "bVII", "i"], &["i", "i", "bVI", "bVII"]],
        seventh_chance: 0.2,
        tempo: 100.0,
        swing: 0.52,
        chords: part("...0..0....0..0.", 0.4, 0.3),
        bass: bass("0..0..0.....0...", "r..8..r.....5...", 0.7, 0.75),
        arp: part(A8, 0.6, 0.3),
        arp_dirs: &[Up, Random],
        melody: part(M_SYNC, 0.85, 1.0),
        drums: &KIT_DANCEHALL,
    },
    // -------------------------------------------------------------- Downtempo
    Style {
        id: "ambient",
        label: "Ambient",
        family: "Downtempo",
        blurb: "Ambient: long minor chords that swell and hold, with a slow melody floating on top.",
        mode: Mode::Minor,
        progressions: MINOR_POP,
        seventh_chance: 0.3,
        tempo: 90.0,
        swing: 0.5,
        chords: part(HELD, 1.0, 16.0),
        bass: bass(HELD, "r...............", 1.0, 16.0),
        arp: part("0.4.0.4.0.4.0.4.", 0.8, 0.5),
        arp_dirs: &[UpDown, Up, Random],
        melody: part(M_SPARSE, 0.95, 3.0),
        drums: &KIT_AMBIENT,
    },
    Style {
        id: "trip-hop",
        label: "Trip-hop",
        family: "Downtempo",
        blurb: "Trip-hop: dark, cinematic minor chords dragging behind a slow, heavy groove.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVI", "iv", "v"], &["i7", "bVII", "bVImaj7", "v7"], &["i", "iv", "i", "bII"]],
        seventh_chance: 0.4,
        tempo: 90.0,
        swing: 0.58,
        chords: part(HELD, 1.0, 4.0),
        bass: bass("0.....0...2.0...", "r.....5...8.r...", 0.85, 1.5),
        arp: part(A8, 0.7, 0.4),
        arp_dirs: &[Down, Random],
        melody: part(M_SPARSE, 0.9, 2.0),
        drums: &KIT_TRIP_HOP,
    },
    Style {
        id: "lofi",
        label: "Lo-fi Hip-hop",
        family: "Downtempo",
        blurb: "Lo-fi: jazzy 7th chords behind a lazy, heavily swung groove.",
        mode: Mode::Major,
        progressions: &[&["ii7", "V7", "Imaj7", "vi7"], &["IVmaj7", "iii7", "ii7", "Imaj7"], &["ii9", "V9", "Imaj7", "Imaj7"], &["IVmaj7", "V7", "iii7", "vi7"]],
        seventh_chance: 0.8,
        tempo: 82.0,
        swing: 0.62,
        chords: part("0.....0.......4.", 0.95, 2.5),
        bass: bass("0.....1...0..2..", "r.....5...r..a..", 0.85, 1.5),
        arp: part("0.3.0.3.0.3.0.3.", 0.7, 0.5),
        arp_dirs: &[Up, Random],
        melody: part("0.3.2.4.1.3.2.5.", 0.9, 1.5),
        drums: &KIT_LOFI,
    },
    Style {
        id: "downtempo",
        label: "Downtempo / Chill",
        family: "Downtempo",
        blurb: "Downtempo: relaxed dorian 9ths, a laid-back bass, room to breathe.",
        mode: Mode::Dorian,
        progressions: &[&["i9", "IV9", "i9", "IV9"], &["i7", "bVII", "bVImaj7", "bVII"], &["i9", "bIIImaj7", "IV9", "i9"]],
        seventh_chance: 0.7,
        tempo: 100.0,
        swing: 0.56,
        chords: part("0.....1.....0...", 0.9, 2.0),
        bass: bass("0...1..0..0..2..", "r...8..5..r..a..", 0.8, 1.0),
        arp: part(A8, 0.7, 0.4),
        arp_dirs: &[UpDown, Up],
        melody: part(M_SYNC, 0.9, 1.5),
        drums: &KIT_DOWNTEMPO,
    },
    Style {
        id: "idm",
        label: "IDM",
        family: "Downtempo",
        blurb: "IDM: floating lydian harmony, glitchy off-grid figures, Warp-records curiosity.",
        mode: Mode::Lydian,
        progressions: &[&["Imaj7", "II", "vii7", "Imaj7"], &["Iadd9", "II", "vi7", "II"], &["Imaj7", "II", "iii7", "II"]],
        seventh_chance: 0.4,
        tempo: 110.0,
        swing: 0.5,
        chords: part("0.....1..3..0...", 0.6, 0.75),
        bass: bass("0..1..0.....3.0.", "r..5..8.....7.r.", 0.6, 0.5),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Random],
        melody: part(M_16, 0.7, 0.5),
        drums: &KIT_IDM,
    },
    Style {
        id: "synthwave",
        label: "Synthwave",
        family: "Downtempo",
        blurb: "Synthwave: big 80s minor progressions, pulsing 8th-note bass, nostalgic leads.",
        mode: Mode::Minor,
        progressions: MINOR_POP,
        seventh_chance: 0.1,
        tempo: 108.0,
        swing: 0.5,
        chords: part(HELD, 1.0, 4.0),
        bass: bass("0.0.0.0.0.0.0.0.", "r.r.r.8.r.r.r.8.", 0.7, 0.4),
        arp: part(A16, 0.6, 0.2),
        arp_dirs: &[Up, UpDown],
        melody: part(M_BUSY, 0.9, 1.5),
        drums: &KIT_SYNTHWAVE,
    },
    // ------------------------------------------------------------------- Hard
    Style {
        id: "hardcore",
        label: "Hardcore / Gabber",
        family: "Hard",
        blurb: "Hardcore: very fast, gated minor hoover stabs over a pounding off-beat bass.",
        mode: Mode::Minor,
        progressions: &[&["i", "bVI", "bIII", "bVII"], &["i", "bVII", "bVI", "v"], &["i"]],
        seventh_chance: 0.0,
        tempo: 175.0,
        swing: 0.5,
        chords: part(GATED, 0.4, 0.2),
        bass: bass(OFFBEAT, "..r...r...r...r.", 0.5, 0.2),
        arp: part(A16, 0.5, 0.15),
        arp_dirs: &[Up, Down],
        melody: part(M_BUSY, 0.8, 0.75),
        drums: &KIT_HARDCORE,
    },
];

/// Index of a style id in [`STYLES`].
pub fn index_of(id: &str) -> Option<usize> {
    STYLES.iter().position(|s| s.id == id)
}

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
        let symbols = crate::harmony::progression(&mut rng, self.mode, self.progressions);
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
            assert!(FAMILIES.contains(&s.family), "{} has unknown family {}", s.id, s.family);
            let d = s.drums;
            for (name, g) in [("kick", d.kick), ("snare", d.snare), ("hat", d.hat), ("open hat", d.open_hat), ("perc", d.perc)] {
                assert!(g.len() == 16 && g.bytes().all(|c| c == b'.' || c.is_ascii_digit()), "{} drums {name} '{g}'", s.id);
            }
            assert!(grid::parse(d.kick).iter().any(|c| *c == Some(0)), "{} kick has no core hit", s.id);
            assert!(find(s.id).is_some());
        }
    }

    #[test]
    fn ids_are_unique_and_grouped_by_family() {
        let mut ids: Vec<_> = STYLES.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), STYLES.len());
        assert!(STYLES.len() >= 30);
        // Listed family by family, in FAMILIES order, so menus can show headers.
        let order: Vec<usize> = STYLES.iter().map(|s| FAMILIES.iter().position(|f| *f == s.family).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]));
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
