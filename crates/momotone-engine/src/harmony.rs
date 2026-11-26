//! New chord progressions for an idea, beyond each style's hand-picked ones: the style's
//! progression with a chord swapped, or a fresh progression built from the chords of the
//! mode. Fresh ones start on the home chord and end on a chord that leads back to it, so
//! the loop always comes round naturally.

use crate::rng::Rng;
use crate::scale::Mode;

/// The chords that belong to a mode, as Roman numerals.
pub struct Palette {
    /// The home chord.
    pub tonic: &'static str,
    /// Chords for the middle of a progression.
    pub middle: &'static [&'static str],
    /// Chords that lead back home at the end of the loop.
    pub turn: &'static [&'static str],
}

pub fn palette(mode: Mode) -> Palette {
    match mode {
        Mode::Major => Palette { tonic: "I", middle: &["ii", "iii", "IV", "V", "vi"], turn: &["V", "IV", "ii"] },
        Mode::Minor => Palette { tonic: "i", middle: &["iv", "v", "bVI", "bIII", "bVII"], turn: &["bVII", "iv", "v", "bVI"] },
        Mode::Dorian => Palette { tonic: "i", middle: &["ii", "bIII", "IV", "v", "bVII"], turn: &["IV", "bVII", "v"] },
        Mode::Phrygian => Palette { tonic: "i", middle: &["bII", "bIII", "iv", "bVI", "bvii"], turn: &["bII", "bvii", "iv"] },
        // No V: its 7th would be the natural 4th, outside the mode.
        Mode::Lydian => Palette { tonic: "I", middle: &["II", "iii", "vi", "vii"], turn: &["II", "vii", "vi"] },
        Mode::Mixolydian => Palette { tonic: "I", middle: &["ii", "IV", "v", "vi", "bVII"], turn: &["bVII", "IV", "v"] },
        Mode::HarmonicMinor => Palette { tonic: "i", middle: &["iv", "bVI", "V", "bIII+", "ii°"], turn: &["V", "bVI", "iv"] },
    }
}

/// The progression for an idea: about 4 in 10 ideas keep one of the style's own, 2 in 10
/// swap a chord in it, the rest get a fresh one of a length the style uses.
pub fn progression(rng: &mut Rng, mode: Mode, curated: &'static [&'static [&'static str]]) -> Vec<&'static str> {
    let base: Vec<&'static str> = rng.pick(curated).to_vec();
    let pal = palette(mode);
    match rng.weighted(&[0.4, 0.2, 0.4]) {
        0 => base,
        1 if base.len() >= 2 => substitute(rng, &pal, base),
        _ => {
            let mut len = rng.pick(curated).len().max(1);
            // One-chord styles (dub techno, minimal) sometimes rock between two chords.
            if len == 1 && rng.chance(0.5) {
                len = 2;
            }
            fresh(rng, &pal, len)
        }
    }
}

/// The style's progression with one chord (never the first) swapped for another of the mode.
fn substitute(rng: &mut Rng, pal: &Palette, mut prog: Vec<&'static str>) -> Vec<&'static str> {
    let i = rng.range(1, prog.len() as i32 - 1) as usize;
    let next = prog[(i + 1) % prog.len()];
    let options: Vec<&'static str> = pal.middle.iter().copied().filter(|c| *c != prog[i - 1] && *c != next && *c != prog[i]).collect();
    if !options.is_empty() {
        prog[i] = *rng.pick(&options);
    }
    prog
}

/// Home chord first, chords of the mode in the middle (never the same twice in a row), and a
/// chord that leads home last.
fn fresh(rng: &mut Rng, pal: &Palette, len: usize) -> Vec<&'static str> {
    let mut prog = vec![pal.tonic];
    for k in 1..len {
        let last = k + 1 == len;
        let from: &[&'static str] = if last { pal.turn } else { pal.middle };
        let prev = prog[k - 1];
        let options: Vec<&'static str> = from.iter().copied().filter(|c| *c != prev).collect();
        prog.push(*rng.pick(&options));
    }
    // A 4-chord loop sometimes returns home halfway (i - x - i - y): common in house and soul.
    if len == 4 && rng.chance(0.2) {
        prog[2] = pal.tonic;
        if prog[1] == pal.tonic {
            prog[1] = *rng.pick(pal.middle);
        }
    }
    prog
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Chord;
    use crate::scale::Scale;
    use crate::PitchClass;

    #[test]
    fn palette_chords_stay_in_the_mode() {
        for mode in Mode::ALL {
            let key = Scale::new(PitchClass::parse("D").unwrap(), mode);
            let p = palette(mode);
            for sym in std::iter::once(&p.tonic).chain(p.middle).chain(p.turn) {
                let c = Chord::parse(sym).unwrap();
                for pc in c.pitch_classes(&key) {
                    assert!(key.contains(pc), "{mode:?} {sym}: {pc} is outside the scale");
                }
                // And with a 7th added the way styles do it.
                for pc in c.with_seventh(&key).pitch_classes(&key) {
                    assert!(key.contains(pc), "{mode:?} {sym}7: {pc} is outside the scale");
                }
            }
        }
    }

    #[test]
    fn many_different_progressions_that_come_home() {
        let curated: &'static [&'static [&'static str]] = &[&["i", "bVI", "bIII", "bVII"]];
        let mut seen = std::collections::HashSet::new();
        for seed in 0..200 {
            let mut rng = Rng::new(seed);
            let p = progression(&mut rng, Mode::Minor, curated);
            assert_eq!(p.len(), 4);
            assert!(p.windows(2).all(|w| w[0] != w[1]) || p.iter().all(|c| *c == p[0]), "{p:?}");
            seen.insert(p);
        }
        assert!(seen.len() >= 25, "only {} progressions", seen.len());
    }
}
