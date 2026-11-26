use crate::chord::Chord;
use crate::grid::STEP;
use crate::scale::Scale;
use crate::style::{ArpDir, Style};

/// Everything needed to render a pattern. Built from a style preset or an AI spec.
#[derive(Clone, Debug)]
pub struct Plan {
    pub key: Scale,
    pub progression: Vec<Chord>,
    pub bars: u32,
    /// Grooves and gates come from here.
    pub style: &'static Style,
    /// 0..1: how many of the style's optional hits play, how busy the melody is, how hard it hits.
    pub energy: f64,
    pub swing: f64,
    pub arp_dir: ArpDir,
    /// Tempo suggestion for the CLI / MIDI file (the plugin follows the host).
    pub tempo: f64,
}

impl Plan {
    pub fn length_beats(&self) -> f64 {
        self.bars as f64 * 4.0
    }

    pub fn total_steps(&self) -> usize {
        self.bars as usize * crate::grid::STEPS_PER_BAR
    }

    /// Start and length (beats) of chord `i`, spreading the progression evenly.
    pub fn chord_span(&self, i: usize) -> (f64, f64) {
        let len = self.length_beats() / self.progression.len() as f64;
        (i as f64 * len, len)
    }

    /// Which chord is sounding at beat `t`.
    pub fn chord_index(&self, t: f64) -> usize {
        let len = self.length_beats() / self.progression.len() as f64;
        ((t / len + 1e-6).floor().max(0.0) as usize).min(self.progression.len() - 1)
    }

    pub fn chord_at(&self, t: f64) -> &Chord {
        &self.progression[self.chord_index(t)]
    }

    /// Start of a 16th step in beats, with swing applied to the off-beat 16ths.
    pub fn step_time(&self, step: usize) -> f64 {
        let swing = if step % 2 == 1 { (self.swing - 0.5).max(0.0) * 0.5 } else { 0.0 };
        step as f64 * STEP + swing
    }

    pub fn chord_names(&self) -> Vec<String> {
        self.progression.iter().map(|c| c.name(&self.key)).collect()
    }
}
