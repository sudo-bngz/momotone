//! Groove grids: the rhythmic DNA of a style, written as text so styles are easy to author.
//!
//! One character per 16th note, 16 per bar (a grid may span several bars and loops):
//! - `.` rest
//! - `0`..`9` a hit, with its *energy threshold*: it plays when `energy * 10 >= digit`.
//!
//! So `0` hits always play (the core groove) and higher digits are ghost notes and fills
//! that appear as the Energy knob goes up. Turning Energy only adds or removes notes, it
//! never moves the ones that are already there.

/// Steps per bar (16th notes in 4/4).
pub const STEPS_PER_BAR: usize = 16;

/// One step in beats.
pub const STEP: f64 = 0.25;

/// Energy (0..1) as the highest digit that plays.
pub fn level(energy: f64) -> u8 {
    (energy.clamp(0.0, 1.0) * 10.0 + 1e-9).floor().min(9.0) as u8
}

/// Each step's threshold, `None` for rests.
pub fn parse(grid: &str) -> Vec<Option<u8>> {
    grid.bytes().map(|c| c.is_ascii_digit().then(|| c - b'0')).collect()
}

pub fn is_valid(grid: &str) -> bool {
    !grid.is_empty() && grid.len() % STEPS_PER_BAR == 0 && grid.bytes().all(|c| c == b'.' || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level(0.0), 0);
        assert_eq!(level(0.5), 5);
        assert_eq!(level(0.29), 2);
        assert_eq!(level(1.0), 9);
    }

    #[test]
    fn parsing() {
        assert_eq!(parse("0.5."), [Some(0), None, Some(5), None]);
        assert!(is_valid("0..............."));
        assert!(!is_valid("0.x............."));
        assert!(!is_valid("0..."));
    }
}
