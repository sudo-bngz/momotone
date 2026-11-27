//! Chords written as Roman numerals relative to the key's tonic.
//!
//! Convention (the one used by most pop/film theory resources, and the one the AI prompt
//! documents): numerals are measured against the *major* scale of the tonic, with `b`/`#`
//! for anything else. So in A minor the usual progression is `i bVI bIII bVII`
//! (Am F C G). Upper case = major third, lower case = minor third.
//!
//! Suffixes: `7`, `maj7`, `6`, `9`, `11`, `13`, `add9`, `sus2`, `sus4`, `°`/`dim`/`o`,
//! `+`/`aug`, `ø`.

use crate::pitch::PitchClass;
use crate::scale::Scale;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    /// The Roman numeral as written.
    pub symbol: String,
    /// Root in semitones above the key tonic (0..12).
    pub root: i32,
    /// Chord tones in semitones above the chord root, ascending, starting with 0.
    pub intervals: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordParseError(pub String);

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot read chord '{}'", self.0)
    }
}

impl std::error::Error for ChordParseError {}

const NUMERALS: [(&str, i32); 7] = [
    ("VII", 11),
    ("VI", 9),
    ("IV", 5),
    ("V", 7),
    ("III", 4),
    ("II", 2),
    ("I", 0),
];

/// Suffix tokens, longest first so "maj7" wins over "7" and "sus4" over "sus".
const SUFFIXES: [&str; 19] = [
    "maj7", "add9", "sus2", "sus4", "sus", "dim", "aug", "M7", "13", "11", "Δ", "°", "ø", "o", "+", "9", "7",
    "6", "m",
];

impl Chord {
    /// Parses a Roman numeral such as `vi7`, `bVII`, `IVmaj7`, `ii°`, `Vsus4`.
    pub fn parse(symbol: &str) -> Result<Chord, ChordParseError> {
        let err = || ChordParseError(symbol.to_string());
        let s = symbol.trim();
        let mut rest = s;

        let mut accidental = 0;
        loop {
            if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
                accidental -= 1;
                rest = r;
            } else if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
                accidental += 1;
                rest = r;
            } else {
                break;
            }
        }

        // Longest numeral first so "VII" wins over "V" and "IV" over "I".
        let (numeral_len, base, upper) = NUMERALS
            .iter()
            .find_map(|(n, semis)| {
                let head = rest.get(..n.len())?;
                if head == *n {
                    Some((n.len(), *semis, true))
                } else if head == n.to_ascii_lowercase() {
                    Some((n.len(), *semis, false))
                } else {
                    None
                }
            })
            .ok_or_else(err)?;
        let mut suffix = &rest[numeral_len..];

        let mut third = if upper { 4 } else { 3 };
        let mut fifth = 7;
        let mut seventh: Option<i32> = None;
        let mut extras: Vec<i32> = Vec::new();
        let mut diminished = false;
        let mut sus: Option<i32> = None;

        while !suffix.is_empty() {
            let token = SUFFIXES.iter().find(|t| suffix.starts_with(**t)).ok_or_else(err)?;
            suffix = &suffix[token.len()..];
            match *token {
                "maj7" | "M7" | "Δ" => seventh = Some(11),
                "add9" => extras.push(14),
                "sus2" => sus = Some(2),
                "sus4" | "sus" => sus = Some(5),
                "dim" | "°" | "o" => {
                    third = 3;
                    fifth = 6;
                    diminished = true;
                }
                "ø" => {
                    third = 3;
                    fifth = 6;
                    seventh = Some(10);
                }
                "aug" | "+" => {
                    third = 4;
                    fifth = 8;
                }
                "13" => {
                    seventh.get_or_insert(10);
                    extras.extend([14, 21]);
                }
                "11" => {
                    seventh.get_or_insert(10);
                    extras.extend([14, 17]);
                }
                "9" => {
                    seventh.get_or_insert(10);
                    extras.push(14);
                }
                "7" => seventh = Some(if diminished { 9 } else { 10 }),
                "6" => extras.push(9),
                // Tolerate "iim7" / "Im" style input from humans and LLMs.
                "m" => third = 3,
                _ => unreachable!(),
            }
        }

        if let Some(s) = sus {
            third = s;
        }
        let mut intervals = vec![0, third, fifth];
        intervals.extend(seventh);
        intervals.extend(extras);
        intervals.sort_unstable();
        intervals.dedup();

        Ok(Chord { symbol: s.to_string(), root: (base + accidental).rem_euclid(12), intervals })
    }

    /// The chord's pitch classes (absolute semitones 0..12, i.e. including the key).
    pub fn pitch_classes(&self, key: &Scale) -> Vec<i32> {
        self.intervals.iter().map(|i| (key.root.semitone() + self.root + i).rem_euclid(12)).collect()
    }

    pub fn root_pc(&self, key: &Scale) -> PitchClass {
        PitchClass::from_semitone(key.root.semitone() + self.root)
    }

    /// Is this MIDI note one of the chord's tones?
    pub fn contains(&self, key: &Scale, midi: i32) -> bool {
        self.pitch_classes(key).contains(&midi.rem_euclid(12))
    }

    /// Adds a 7th that fits the chord quality (used to make progressions jazzier).
    pub fn with_seventh(&self, key: &Scale) -> Chord {
        if self.intervals.iter().any(|&i| i == 10 || i == 11 || (i == 9 && self.intervals.contains(&6))) {
            return self.clone();
        }
        let has = |i: i32| self.intervals.contains(&i);
        let (seventh, suffix) = if has(3) && has(6) {
            (10, "ø")
        } else if has(4) && has(7) {
            // Major chord: maj7 if the maj7 is in the key, else dominant 7.
            let maj7_in_key = key.contains(key.root.semitone() + self.root + 11);
            if maj7_in_key && self.root != 7 {
                (11, "maj7")
            } else {
                (10, "7")
            }
        } else if has(3) && key.contains(key.root.semitone() + self.root + 10) {
            (10, "7")
        } else {
            // Minor chord whose b7 isn't in the key (i in harmonic minor): stays a triad.
            return self.clone();
        };
        let mut intervals = self.intervals.clone();
        intervals.push(seventh);
        intervals.sort_unstable();
        let mut symbol = self.symbol.clone();
        if suffix == "ø" {
            symbol = symbol.replace(['°', 'o'], "").replace("dim", "");
        }
        symbol.push_str(suffix);
        Chord { symbol, root: self.root, intervals }
    }

    /// Lead-sheet name in a given key, e.g. `Am7`, `Fmaj7`, `G7`, `Bdim`.
    pub fn name(&self, key: &Scale) -> String {
        let root = self.root_pc(key).name(key.prefers_flats());
        let has = |i: i32| self.intervals.contains(&i);
        let quality = if has(3) && has(6) {
            if has(10) {
                "m7b5"
            } else if has(9) {
                "dim7"
            } else {
                "dim"
            }
        } else if has(4) && has(8) {
            "aug"
        } else if has(2) && !has(3) && !has(4) {
            "sus2"
        } else if has(5) && !has(3) && !has(4) {
            "sus4"
        } else if has(3) {
            if has(11) {
                "m(maj7)"
            } else if has(10) && has(14) {
                "m9"
            } else if has(10) {
                "m7"
            } else if has(9) {
                "m6"
            } else {
                "m"
            }
        } else if has(11) {
            if has(14) {
                "maj9"
            } else {
                "maj7"
            }
        } else if has(10) {
            if has(14) {
                "9"
            } else {
                "7"
            }
        } else if has(9) {
            "6"
        } else {
            ""
        };
        let add9 = if has(14) && !quality.contains('9') { "add9" } else { "" };
        let sus7 = if (quality == "sus2" || quality == "sus4") && has(10) { "7" } else { "" };
        format!("{root}{quality}{sus7}{add9}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::Mode;

    fn key(root: &str, mode: Mode) -> Scale {
        Scale::new(PitchClass::parse(root).unwrap(), mode)
    }

    fn names(k: &Scale, symbols: &[&str]) -> Vec<String> {
        symbols.iter().map(|s| Chord::parse(s).unwrap().name(k)).collect()
    }

    #[test]
    fn pop_progression_in_c() {
        assert_eq!(names(&key("C", Mode::Major), &["I", "V", "vi", "IV"]), ["C", "G", "Am", "F"]);
    }

    #[test]
    fn minor_progression_with_flats() {
        assert_eq!(names(&key("A", Mode::Minor), &["i", "bVI", "bIII", "bVII"]), ["Am", "F", "C", "G"]);
    }

    #[test]
    fn extensions() {
        let k = key("C", Mode::Major);
        assert_eq!(
            names(&k, &["ii7", "V7", "IVmaj7", "vii°", "viiø", "Vsus4", "ii9", "Iadd9", "III7", "iim7"]),
            ["Dm7", "G7", "Fmaj7", "Bdim", "Bm7b5", "Gsus4", "Dm9", "Cadd9", "E7", "Dm7"]
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(Chord::parse("X").is_err());
        assert!(Chord::parse("Iwhatever").is_err());
    }

    #[test]
    fn adds_fitting_sevenths() {
        let k = key("C", Mode::Major);
        assert_eq!(Chord::parse("I").unwrap().with_seventh(&k).name(&k), "Cmaj7");
        assert_eq!(Chord::parse("V").unwrap().with_seventh(&k).name(&k), "G7");
        assert_eq!(Chord::parse("vi").unwrap().with_seventh(&k).name(&k), "Am7");
    }
}
