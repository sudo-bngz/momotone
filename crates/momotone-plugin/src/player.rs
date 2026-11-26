//! Host-independent playback: given a looping pattern and a block's beat range, produce
//! sample-accurate note on/off events. Kept free of nih-plug types so it can be tested
//! by simulating a host transport.

use momotone_engine::Pattern;

pub const MAX_ACTIVE: usize = 512;
pub const MAX_EVENTS: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ev {
    pub offset: u32,
    pub on: bool,
    pub channel: u8,
    pub note: u8,
    pub velocity: f32,
}

#[derive(Clone, Copy)]
struct ActiveNote {
    note: u8,
    channel: u8,
    end_beat: f64,
}

/// Block parameters coming from the host.
#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub start_beat: f64,
    pub beats_per_sample: f64,
    pub samples: usize,
    /// Octave shift per layer (chords, bass, arp, melody).
    pub octaves: [i32; 4],
    pub split_channels: bool,
    /// Which layers this player plays (chords, bass, arp, melody).
    pub mask: [bool; 4],
    /// Send everything on this channel (0-based) instead of the per-layer channels.
    pub channel: Option<u8>,
}

impl Block {
    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.samples as f64 * self.beats_per_sample
    }

    /// True if this block reaches a bar line (4/4): the moment a new pattern may come in.
    pub fn touches_bar_line(&self) -> bool {
        let s = self.start_beat / 4.0;
        (self.end_beat() / 4.0).floor() > s.floor() || (s - s.round()).abs() < 1e-9
    }
}

pub struct Player {
    active: Vec<ActiveNote>,
    pub events: Vec<Ev>,
    was_playing: bool,
    next_beat: f64,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            active: Vec::with_capacity(MAX_ACTIVE),
            events: Vec::with_capacity(MAX_EVENTS),
            was_playing: false,
            next_beat: 0.0,
        }
    }
}

impl Player {
    pub fn reset(&mut self) {
        self.active.clear();
        self.events.clear();
        self.was_playing = false;
    }

    pub fn next_beat(&self) -> f64 {
        self.next_beat
    }

    fn push(&mut self, ev: Ev) {
        if self.events.len() < self.events.capacity() {
            self.events.push(ev);
        }
    }

    /// Note-offs for everything sounding (transport stopped, pattern swapped, seek).
    pub fn release_all(&mut self, offset: u32) {
        while let Some(a) = self.active.pop() {
            self.push(Ev { offset, on: false, channel: a.channel, note: a.note, velocity: 0.0 });
        }
    }

    pub fn stop(&mut self) {
        self.release_all(0);
        self.was_playing = false;
        self.sort();
    }

    /// Schedules one block. Events land in `self.events`, sorted, offs before ons.
    pub fn play(&mut self, pattern: &Pattern, b: Block) {
        if b.samples == 0 || pattern.notes.is_empty() || pattern.length_beats <= 0.0 {
            self.stop();
            return;
        }
        let start = b.start_beat;
        // Loop jump or seek: silence what was playing before continuing.
        if self.was_playing && (start - self.next_beat).abs() > 0.01 {
            self.release_all(0);
        }
        let end = b.end_beat();
        let last = b.samples as u32 - 1;
        let to_offset =
            |beat: f64| -> u32 { (((beat - start) / b.beats_per_sample).floor().max(0.0) as u32).min(last) };

        let mut i = 0;
        while i < self.active.len() {
            let a = self.active[i];
            if a.end_beat < end {
                self.push(Ev { offset: to_offset(a.end_beat), on: false, channel: a.channel, note: a.note, velocity: 0.0 });
                self.active.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // The pattern loops forever, aligned to the host's beat 1.
        let length = pattern.length_beats;
        for n in &pattern.notes {
            if !b.mask[n.layer.index()] {
                continue;
            }
            let pitch = n.pitch as i32 + b.octaves[n.layer.index()] * 12;
            if !(0..=127).contains(&pitch) {
                continue;
            }
            let note = pitch as u8;
            let channel = b.channel.unwrap_or(if b.split_channels { n.layer.channel() } else { 0 });
            let mut t = n.start + ((start - n.start) / length).ceil() * length;
            while t < end {
                // Nothing before the song's beat 1 (a delayed hardware window starts below 0).
                if t >= start && t >= 0.0 {
                    let offset = to_offset(t);
                    // Retrigger: close the same note if it is still ringing.
                    if let Some(j) = self.active.iter().position(|a| a.note == note && a.channel == channel) {
                        self.active.swap_remove(j);
                        self.push(Ev { offset, on: false, channel, note, velocity: 0.0 });
                    }
                    self.push(Ev { offset, on: true, channel, note, velocity: n.velocity as f32 / 127.0 });
                    if self.active.len() < self.active.capacity() {
                        self.active.push(ActiveNote { note, channel, end_beat: t + n.length });
                    }
                }
                t += length;
            }
        }

        self.next_beat = end;
        self.was_playing = true;
        self.sort();
    }

    fn sort(&mut self) {
        self.events.sort_unstable_by_key(|e| (e.offset, e.on));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use momotone_engine::{generate, Layers, PitchClass, STYLES};
    use std::collections::HashSet;

    fn pattern(style: usize, seed: u64) -> Pattern {
        let plan = STYLES[style].plan(PitchClass::parse("C").unwrap(), 2, 0.8, seed);
        generate(&plan, Layers::all(), seed)
    }

    fn block(beat: f64, bps: f64, samples: usize) -> Block {
        Block { start_beat: beat, beats_per_sample: bps, samples, octaves: [0, -1, 1, 0], split_channels: true, mask: [true; 4], channel: None }
    }

    /// Runs the player like a host would and checks note on/off pairing. Returns note-ons.
    fn simulate(p: &Pattern, block_sizes: &[usize], blocks: usize, tempo: f64, jump_at: Option<usize>) -> usize {
        let bps = tempo / 60.0 / 48_000.0;
        let mut player = Player::default();
        let mut beat = 0.0;
        let mut sounding: HashSet<(u8, u8)> = HashSet::new();
        let mut ons = 0;
        for k in 0..blocks {
            let samples = block_sizes[k % block_sizes.len()];
            if Some(k) == jump_at {
                beat = 1.75; // like a loop brace jumping back
            }
            player.play(p, block(beat, bps, samples));
            let mut last_offset = 0;
            for e in player.events.drain(..) {
                assert!(e.offset < samples as u32);
                assert!(e.offset >= last_offset, "events out of order");
                last_offset = e.offset;
                if e.on {
                    assert!(sounding.insert((e.channel, e.note)), "double note-on {:?}", (e.channel, e.note));
                    ons += 1;
                } else {
                    assert!(sounding.remove(&(e.channel, e.note)), "note-off without note-on");
                }
            }
            beat += samples as f64 * bps;
        }
        player.stop();
        for e in player.events.drain(..) {
            sounding.remove(&(e.channel, e.note));
        }
        assert!(sounding.is_empty(), "hanging notes after stop");
        ons
    }

    #[test]
    fn plays_every_note_once_per_loop() {
        let p = pattern(0, 4);
        // Just under 4 loops of 8 beats at 120 bpm (768 000 samples); no note in the last block.
        let ons = simulate(&p, &[512], 768_000 / 512 - 1, 120.0, None);
        assert_eq!(ons, p.notes.len() * 4);
    }

    #[test]
    fn odd_block_sizes_and_fast_tempo() {
        for style in 0..STYLES.len() {
            let p = pattern(style, style as u64);
            simulate(&p, &[64, 333, 1, 4096, 17], 3000, 174.0, None);
        }
    }

    #[test]
    fn survives_loop_jumps() {
        simulate(&pattern(2, 2), &[256], 2000, 128.0, Some(700));
    }

    #[test]
    fn mask_and_channel_override() {
        let p = pattern(0, 4);
        let bps = 120.0 / 60.0 / 48_000.0;
        let mut player = Player::default();
        let mut ons = 0;
        for k in 0..800 {
            let b = Block { mask: [false, true, false, false], channel: Some(9), ..block(k as f64 * 512.0 * bps, bps, 512) };
            player.play(&p, b);
            for e in player.events.drain(..) {
                assert_eq!(e.channel, 9);
                ons += e.on as usize;
            }
        }
        // 800 blocks of 512 samples at 120 bpm = 17.07 beats: two loops of the 8-beat pattern, plus a bit.
        let bass_per_loop = p.notes.iter().filter(|n| n.layer == momotone_engine::Layer::Bass).count();
        assert!(ons >= 2 * bass_per_loop && ons <= 3 * bass_per_loop, "{ons}");
    }

    #[test]
    fn bar_lines() {
        let bps = 120.0 / 60.0 / 48_000.0;
        assert!(block(0.0, bps, 512).touches_bar_line());
        assert!(!block(1.0, bps, 512).touches_bar_line());
        assert!(block(3.99, bps, 512).touches_bar_line());
    }
}
