//! Voice leading: pick the inversion/octave of each chord that moves the fewest semitones
//! from the previous one. This is the single biggest "sounds like a pro" trick.

/// Chooses a close-position voicing for `pitch_classes` near `prev` (or near `center`).
/// Returned notes are ascending MIDI numbers inside [low, high].
pub fn lead(pitch_classes: &[i32], prev: Option<&[i32]>, center: i32, low: i32, high: i32) -> Vec<i32> {
    let pcs: Vec<i32> = dedup_keep_order(pitch_classes);
    let mut best: Option<(f64, Vec<i32>)> = None;

    for inversion in 0..pcs.len() {
        let order: Vec<i32> = pcs[inversion..].iter().chain(pcs[..inversion].iter()).copied().collect();
        let mut start = low + (order[0] - low).rem_euclid(12);
        while start <= high {
            let mut notes = vec![start];
            for pc in &order[1..] {
                let last = *notes.last().unwrap();
                notes.push(last + 1 + (pc - last - 1).rem_euclid(12));
            }
            if *notes.last().unwrap() <= high {
                let c = cost(&notes, prev, center);
                if best.as_ref().map_or(true, |(b, _)| c < *b) {
                    best = Some((c, notes));
                }
            }
            start += 12;
        }
    }

    best.map(|(_, n)| n).unwrap_or_else(|| {
        // Range too narrow for the chord: fall back to stacking from `low`.
        let mut n: Vec<i32> = pcs.iter().map(|pc| low + (pc - low).rem_euclid(12)).collect();
        n.sort_unstable();
        n
    })
}

fn cost(notes: &[i32], prev: Option<&[i32]>, center: i32) -> f64 {
    let mean = notes.iter().sum::<i32>() as f64 / notes.len() as f64;
    let drift = (mean - center as f64).abs();
    match prev {
        Some(prev) if !prev.is_empty() => {
            // Each new note pays the distance to its closest previous note, and vice versa,
            // so chords with different sizes compare fairly.
            let forward: i32 = notes.iter().map(|n| prev.iter().map(|p| (n - p).abs()).min().unwrap()).sum();
            let backward: i32 = prev.iter().map(|p| notes.iter().map(|n| (n - p).abs()).min().unwrap()).sum();
            (forward + backward) as f64 + 0.35 * drift
        }
        _ => drift,
    }
}

fn dedup_keep_order(v: &[i32]) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::with_capacity(v.len());
    for x in v {
        let x = x.rem_euclid(12);
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_c_to_f() {
        let c = lead(&[0, 4, 7], None, 62, 52, 79);
        let f = lead(&[5, 9, 0], Some(&c), 62, 52, 79);
        let moved: i32 = c.iter().zip(&f).map(|(a, b)| (a - b).abs()).sum();
        assert!(moved <= 4, "{c:?} -> {f:?} moved {moved}");
    }

    #[test]
    fn stays_in_range() {
        let mut prev: Option<Vec<i32>> = None;
        for pcs in [[0, 4, 7, 11], [9, 0, 4, 7], [2, 5, 9, 0], [7, 11, 2, 5]].iter().cycle().take(32) {
            let v = lead(pcs, prev.as_deref(), 62, 52, 79);
            assert!(v.iter().all(|n| (52..=79).contains(n)));
            prev = Some(v);
        }
    }
}
