use crate::pitch::PitchClass;

/// The seven-note modes the engine knows. Each gives the music its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    HarmonicMinor,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Major,
        Mode::Minor,
        Mode::Dorian,
        Mode::Phrygian,
        Mode::Lydian,
        Mode::Mixolydian,
        Mode::HarmonicMinor,
    ];

    pub fn intervals(self) -> [i32; 7] {
        match self {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::HarmonicMinor => [0, 2, 3, 5, 7, 8, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Major => "major",
            Mode::Minor => "minor",
            Mode::Dorian => "dorian",
            Mode::Phrygian => "phrygian",
            Mode::Lydian => "lydian",
            Mode::Mixolydian => "mixolydian",
            Mode::HarmonicMinor => "harmonic minor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase().replace(['-', '_'], " ");
        Some(match s.as_str() {
            "major" | "ionian" | "maj" => Mode::Major,
            "minor" | "aeolian" | "natural minor" | "min" | "m" => Mode::Minor,
            "dorian" => Mode::Dorian,
            "phrygian" => Mode::Phrygian,
            "lydian" => Mode::Lydian,
            "mixolydian" | "mixo" => Mode::Mixolydian,
            "harmonic minor" | "harmonic" => Mode::HarmonicMinor,
            _ => return None,
        })
    }

    /// True when the third degree is minor; used for spelling and explanations.
    pub fn is_minor(self) -> bool {
        self.intervals()[2] == 3
    }
}

/// A key: a tonic plus a mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    pub root: PitchClass,
    pub mode: Mode,
}

impl Scale {
    pub fn new(root: PitchClass, mode: Mode) -> Self {
        Self { root, mode }
    }

    /// Whether to spell note names with flats in this key.
    pub fn prefers_flats(&self) -> bool {
        // Spell from the relative major: F, Bb, Eb, Ab, Db, Gb majors use flats.
        let relative_major = if self.mode.is_minor() {
            self.root.semitone() + 3
        } else {
            self.root.semitone()
        };
        matches!(relative_major.rem_euclid(12), 5 | 10 | 3 | 8 | 1 | 6)
    }

    /// Semitone offset of any scale degree (0 = tonic; may be negative or above 6).
    pub fn degree_semitones(&self, degree: i32) -> i32 {
        let octave = degree.div_euclid(7);
        let idx = degree.rem_euclid(7) as usize;
        self.mode.intervals()[idx] + 12 * octave
    }

    pub fn contains(&self, midi: i32) -> bool {
        let rel = (midi - self.root.semitone()).rem_euclid(12);
        self.mode.intervals().contains(&rel)
    }

    /// Absolute scale-degree index of the scale note at or just below `midi`.
    pub fn degree_at_or_below(&self, midi: i32) -> i32 {
        let rel = midi - self.root.semitone();
        let mut degree = rel.div_euclid(12) * 7 + 6;
        while self.degree_semitones(degree) > rel {
            degree -= 1;
        }
        degree
    }

    /// MIDI note of an absolute scale degree.
    pub fn degree_to_midi(&self, degree: i32) -> i32 {
        self.root.semitone() + self.degree_semitones(degree)
    }

    /// Moves `midi` by `steps` scale steps (snapping to the scale first).
    pub fn step(&self, midi: i32, steps: i32) -> i32 {
        let d = self.degree_at_or_below(midi);
        self.degree_to_midi(d + steps)
    }

    /// Nearest in-scale note to `midi` (ties go down).
    pub fn snap(&self, midi: i32) -> i32 {
        let below = self.degree_to_midi(self.degree_at_or_below(midi));
        let above = self.degree_to_midi(self.degree_at_or_below(midi) + 1);
        if midi - below <= above - midi {
            below
        } else {
            above
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_through_c_major() {
        let s = Scale::new(PitchClass::parse("C").unwrap(), Mode::Major);
        assert_eq!(s.step(60, 1), 62);
        assert_eq!(s.step(64, 1), 65);
        assert_eq!(s.step(60, -1), 59);
        assert_eq!(s.step(60, 7), 72);
        // C# is not in C major: snaps down to C, then steps.
        assert_eq!(s.step(61, 1), 62);
    }

    #[test]
    fn a_minor_degrees() {
        let s = Scale::new(PitchClass::parse("A").unwrap(), Mode::Minor);
        assert_eq!(s.degree_to_midi(0), 9);
        assert_eq!(s.degree_to_midi(2), 12);
        assert!(s.contains(60));
        assert!(!s.contains(61));
        assert_eq!(s.snap(61), 60);
    }

    #[test]
    fn spelling() {
        assert!(Scale::new(PitchClass::parse("F").unwrap(), Mode::Major).prefers_flats());
        assert!(Scale::new(PitchClass::parse("D").unwrap(), Mode::Minor).prefers_flats());
        assert!(!Scale::new(PitchClass::parse("E").unwrap(), Mode::Minor).prefers_flats());
    }
}
