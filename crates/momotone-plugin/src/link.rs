//! Main/follower link between Momotone instances.
//!
//! A DAW like Live hosts every plugin in one process, so instances can talk through plain
//! statics. The main Momotone of a group publishes what it plays (style, key, bars, energy,
//! idea) and the bar at which that takes effect; followers read it, render only their own
//! part, and switch on that same bar. Everything is lock-free atomics, safe on the audio
//! thread.

use crate::player::Block;
use crate::{GenSettings, Locks};
use momotone_engine::{Layer, Layers};
use std::sync::atomic::{AtomicU64, Ordering};

pub const GROUPS: usize = 4;

static SETTINGS: [AtomicU64; GROUPS] = [const { AtomicU64::new(0) }; GROUPS];
static SWITCH_AT: [AtomicU64; GROUPS] = [const { AtomicU64::new(0) }; GROUPS];
static LOCKS: [AtomicU64; GROUPS] = [const { AtomicU64::new(0) }; GROUPS];
/// Seqlock: odd while a main is writing, so a follower never mixes two updates.
static VERSION: [AtomicU64; GROUPS] = [const { AtomicU64::new(0) }; GROUPS];
/// Which instance last published in each group, to spot two mains in one group.
static OWNER: [AtomicU64; GROUPS] = [const { AtomicU64::new(0) }; GROUPS];
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A unique id for each plugin instance.
pub fn new_instance_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

const VALID: u64 = 1 << 63;

/// Packs everything the main shares into one word, so readers never see half an update.
pub fn pack(s: &GenSettings) -> u64 {
    VALID
        | (s.style as u64 & 0x3F)
        | (s.key as u64 & 0xF) << 6
        | (s.bars as u64 & 0x1F) << 10
        | (s.energy as u64 & 0x7F) << 15
        | (s.seed & 0xFFFF) << 22
        | (s.ai as u64) << 38
        | (s.section as u64 & 0x7) << 54
}

/// The shared settings; `layers` is left to the reader.
pub fn unpack(v: u64) -> Option<GenSettings> {
    (v & VALID != 0).then(|| GenSettings {
        style: (v & 0x3F) as usize,
        key: (v >> 6 & 0xF) as usize,
        bars: (v >> 10 & 0x1F) as u32,
        energy: (v >> 15 & 0x7F) as u8,
        seed: v >> 22 & 0xFFFF,
        ai: (v >> 38 & 0xFFFF) as u16,
        section: (v >> 54 & 0x7) as u8,
        layers: Layers::all(),
        locks: Locks::default(),
    })
}

/// Locks in one word: 10 bits each (a flag + an idea number up to 511) for the harmony and
/// every part.
pub fn pack_locks(l: &Locks) -> u64 {
    let one = |v: Option<u64>| v.map_or(0, |n| 0x200 | (n & 0x1FF));
    let mut w = one(l.harmony);
    for (i, p) in l.parts.iter().enumerate() {
        w |= one(*p) << (10 * (i + 1));
    }
    w
}

pub fn unpack_locks(w: u64) -> Locks {
    let one = |v: u64| (v & 0x200 != 0).then_some(v & 0x1FF);
    Locks { harmony: one(w & 0x3FF), parts: std::array::from_fn(|i| one(w >> (10 * (i + 1)) & 0x3FF)) }
}

/// Called by the main instance on every block. Returns false if another main published in
/// this group since our last block (two mains in one group).
pub fn publish(group: usize, instance: u64, s: &GenSettings, switch_at: f64) -> bool {
    let g = group.min(GROUPS - 1);
    VERSION[g].fetch_add(1, Ordering::AcqRel);
    SWITCH_AT[g].store(switch_at.to_bits(), Ordering::Release);
    SETTINGS[g].store(pack(s), Ordering::Release);
    LOCKS[g].store(pack_locks(&s.locks), Ordering::Release);
    VERSION[g].fetch_add(1, Ordering::AcqRel);
    let previous = OWNER[g].swap(instance, Ordering::AcqRel);
    previous == 0 || previous == instance
}

/// What the group's main is playing, and the beat it switched (or will switch) on.
pub fn read(group: usize) -> Option<(GenSettings, f64)> {
    let g = group.min(GROUPS - 1);
    // Retry while a main is mid-update (a handful of stores; a few tries at most).
    for _ in 0..16 {
        let v = VERSION[g].load(Ordering::Acquire);
        let (word, locks, at) = (SETTINGS[g].load(Ordering::Acquire), LOCKS[g].load(Ordering::Acquire), SWITCH_AT[g].load(Ordering::Acquire));
        if v % 2 == 0 && VERSION[g].load(Ordering::Acquire) == v {
            let s = unpack(word)?;
            return Some((GenSettings { locks: unpack_locks(locks), ..s }, f64::from_bits(at)));
        }
        std::hint::spin_loop();
    }
    let s = unpack(SETTINGS[g].load(Ordering::Acquire))?;
    Some((GenSettings { locks: unpack_locks(LOCKS[g].load(Ordering::Acquire)), ..s }, f64::from_bits(SWITCH_AT[g].load(Ordering::Acquire))))
}

/// Only this part.
pub fn part_layers(part: Layer) -> Layers {
    let mut l = Layers::none();
    match part {
        Layer::Chords => l.chords = true,
        Layer::Bass => l.bass = true,
        Layer::Arp => l.arp = true,
        Layer::Melody => l.melody = true,
        Layer::Drums => l.drums = true,
    }
    l
}

/// When a change made at `beat` should be heard: the next bar line, leaving a quarter beat
/// so every instance has time to render. Stopped transport: right away.
pub fn switch_point(beat: f64, playing: bool) -> f64 {
    if playing {
        ((beat + 0.25) / 4.0).ceil() * 4.0
    } else {
        f64::NEG_INFINITY
    }
}

/// Whether a pattern that is ready may be swapped in during this block. Swaps only happen on
/// a bar line at or after `switch_at`, so a main and its followers change together.
pub fn swap_now(playing: bool, block: &Block, switch_at: f64) -> bool {
    !playing || (block.touches_bar_line() && block.end_beat() > switch_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(beat: f64, samples: usize) -> Block {
        Block { start_beat: beat, beats_per_sample: 124.0 / 60.0 / 48_000.0, samples, octaves: [0; 5], split_channels: true, mask: [true; 5], channel: None }
    }

    #[test]
    fn packing_round_trips() {
        let s = GenSettings { style: 35, key: 11, bars: 16, seed: 500, energy: 100, layers: Layers::all(), ai: 0xBEEF, section: 7, ..Default::default() };
        assert_eq!(unpack(pack(&s)), Some(s));
        assert_eq!(unpack(0), None);
    }

    #[test]
    fn publish_and_read_per_group() {
        let mut locks = Locks::default();
        locks.harmony = Some(500);
        locks.parts[4] = Some(1);
        let s = GenSettings { style: 2, key: 3, bars: 4, seed: 42, energy: 55, layers: Layers::all(), locks, ai: 0, section: 0 };
        let id = new_instance_id();
        assert!(publish(3, id, &s, 8.0));
        assert!(publish(3, id, &s, 8.0));
        let other = new_instance_id();
        assert!(!publish(3, other, &s, 8.0), "second main in the same group is reported");
        let (r, at) = read(3).unwrap();
        assert_eq!((r.style, r.key, r.bars, r.seed, r.energy), (2, 3, 4, 42, 55));
        assert_eq!(r.locks, locks, "followers get the main's locks");
        assert_eq!(at, 8.0);
    }

    #[test]
    fn locks_pack() {
        for l in [
            Locks::default(),
            Locks { harmony: Some(1), parts: [None, Some(500), None, Some(250), Some(499)] },
            Locks { harmony: None, parts: [Some(1); 5] },
        ] {
            assert_eq!(unpack_locks(pack_locks(&l)), l);
        }
    }

    #[test]
    fn switch_points() {
        assert_eq!(switch_point(1.0, true), 4.0);
        assert_eq!(switch_point(3.9, true), 8.0);
        assert_eq!(switch_point(4.0, true), 8.0);
        assert!(switch_point(2.0, false).is_infinite());
    }

    /// A main and a follower see the change one block apart (and render at different times)
    /// but still swap in the same block, on the same bar line.
    #[test]
    fn main_and_follower_swap_on_the_same_bar() {
        let samples = 512;
        let b = block(0.0, samples);
        let step = b.end_beat();
        let change_beat = 2.3;
        let switch_at = switch_point(change_beat, true);
        let mut main_swap = None;
        let mut follower_swap = None;
        let mut beat = 0.0;
        while beat < 16.0 {
            let blk = block(beat, samples);
            let main_ready = beat >= change_beat;
            let follower_ready = beat >= change_beat + 3.0 * step; // a few blocks later
            if main_swap.is_none() && main_ready && swap_now(true, &blk, switch_at) {
                main_swap = Some(beat);
            }
            if follower_swap.is_none() && follower_ready && swap_now(true, &blk, switch_at) {
                follower_swap = Some(beat);
            }
            beat += step;
        }
        assert_eq!(main_swap, follower_swap);
        let at = main_swap.unwrap();
        assert!(at < 4.0 && at + step > 4.0 - 1e-9, "swapped at {at}");
    }
}
