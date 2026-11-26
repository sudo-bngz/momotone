use std::fmt;

/// One of the 12 pitch classes (a note name without an octave).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PitchClass(u8);

const SHARP_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

impl PitchClass {
    pub const ALL: [PitchClass; 12] = [
        PitchClass(0),
        PitchClass(1),
        PitchClass(2),
        PitchClass(3),
        PitchClass(4),
        PitchClass(5),
        PitchClass(6),
        PitchClass(7),
        PitchClass(8),
        PitchClass(9),
        PitchClass(10),
        PitchClass(11),
    ];

    /// Builds a pitch class from any semitone count (wraps around the octave).
    pub fn from_semitone(n: i32) -> Self {
        PitchClass(n.rem_euclid(12) as u8)
    }

    pub fn semitone(self) -> i32 {
        self.0 as i32
    }

    /// Name using sharps or flats.
    pub fn name(self, flats: bool) -> &'static str {
        if flats {
            FLAT_NAMES[self.0 as usize]
        } else {
            SHARP_NAMES[self.0 as usize]
        }
    }

    /// Parses "C", "c#", "Db", "F♯", "Bb"...
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let mut chars = s.chars();
        let letter = chars.next()?.to_ascii_uppercase();
        let base = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let mut offset = 0;
        for c in chars {
            match c {
                '#' | '♯' | 's' => offset += 1,
                'b' | '♭' => offset -= 1,
                _ => return None,
            }
        }
        Some(Self::from_semitone(base + offset))
    }
}

impl fmt::Display for PitchClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name(false))
    }
}

/// Converts a MIDI note number to a readable name like "A3" (middle C = C3, Ableton convention).
pub fn midi_name(note: u8, flats: bool) -> String {
    let pc = PitchClass::from_semitone(note as i32);
    let octave = note as i32 / 12 - 2;
    format!("{}{}", pc.name(flats), octave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        assert_eq!(PitchClass::parse("C").unwrap().semitone(), 0);
        assert_eq!(PitchClass::parse("db").unwrap().semitone(), 1);
        assert_eq!(PitchClass::parse("F#").unwrap().semitone(), 6);
        assert_eq!(PitchClass::parse("Cb").unwrap().semitone(), 11);
        assert!(PitchClass::parse("H").is_none());
    }

    #[test]
    fn names_midi() {
        assert_eq!(midi_name(60, false), "C3");
        assert_eq!(midi_name(57, false), "A2");
    }
}
