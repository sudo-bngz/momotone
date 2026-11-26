//! The contract between the AI layer and the engine.
//!
//! An LLM never writes notes. It answers a prompt like "rainy-night garage, a bit soulful"
//! with a small JSON [`PatternSpec`]; the engine validates it and renders the notes with the
//! chosen style's grooves. Output stays musically valid and every suggestion is editable.

use crate::chord::Chord;
use crate::pitch::PitchClass;
use crate::plan::Plan;
use crate::scale::{Mode, Scale};
use crate::style;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PatternSpec {
    /// Tonic, e.g. "A", "F#", "Eb".
    pub key: String,
    /// "major", "minor", "dorian", "phrygian", "lydian", "mixolydian", "harmonic minor".
    pub mode: String,
    /// Roman numerals relative to the tonic's major scale, e.g. ["i","bVI","bIII","bVII"].
    pub progression: Vec<String>,
    #[serde(default = "default_bars")]
    pub bars: u32,
    /// Groove style id: see [`style::STYLES`].
    #[serde(default)]
    pub style: Option<String>,
    /// 0.0 (sparse, laid back) .. 1.0 (busy, peak time).
    #[serde(default)]
    pub energy: Option<f64>,
    #[serde(default)]
    pub tempo: Option<f64>,
    /// One short sentence for the user about why this fits their request.
    #[serde(default)]
    pub explanation: Option<String>,
}

fn default_bars() -> u32 {
    4
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpecError(pub String);

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpecError {}

impl PatternSpec {
    /// Parses JSON, tolerating a ```json fence around it (LLMs love those).
    pub fn from_json(text: &str) -> Result<PatternSpec, SpecError> {
        let t = text.trim();
        let t = t
            .strip_prefix("```json")
            .or_else(|| t.strip_prefix("```"))
            .map(|r| r.trim_end().trim_end_matches("```"))
            .unwrap_or(t);
        serde_json::from_str(t).map_err(|e| SpecError(format!("invalid spec JSON: {e}")))
    }

    pub fn to_plan(&self, seed: u64) -> Result<Plan, SpecError> {
        let root = PitchClass::parse(&self.key).ok_or_else(|| SpecError(format!("unknown key '{}'", self.key)))?;
        let mode = Mode::parse(&self.mode).ok_or_else(|| SpecError(format!("unknown mode '{}'", self.mode)))?;
        if self.progression.is_empty() || self.progression.len() > 16 {
            return Err(SpecError("progression must have 1 to 16 chords".into()));
        }
        if !(1..=16).contains(&self.bars) {
            return Err(SpecError("bars must be between 1 and 16".into()));
        }
        let style_id = self.style.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("soul");
        let style = style::find(style_id).ok_or_else(|| SpecError(format!("unknown style '{style_id}'")))?;
        let progression = self
            .progression
            .iter()
            .map(|s| Chord::parse(s).map_err(|e| SpecError(e.to_string())))
            .collect::<Result<Vec<_>, _>>()?;

        // Start from the style (grooves, arp direction), then apply the AI's harmony.
        let mut plan = style.plan(root, self.bars, self.energy.unwrap_or(0.5), seed);
        plan.key = Scale::new(root, mode);
        plan.progression = progression;
        plan.tempo = self.tempo.unwrap_or(style.tempo).clamp(40.0, 220.0);
        Ok(plan)
    }
}

/// System prompt for the LLM that turns a vibe into a [`PatternSpec`]. Lists every style.
pub fn system_prompt() -> String {
    let styles: Vec<String> = style::STYLES.iter().map(|s| format!("{} ({})", s.id, s.label)).collect();
    format!(
        r#"You are the harmony brain of Momotone, a MIDI loop generator used by electronic producers who don't read music theory.
The user describes a vibe, genre or reference. Reply with ONLY a JSON object, no prose, matching:

{{
  "key": "A",                       // tonic: C, C#, Db, D, ... B
  "mode": "minor",                  // major | minor | dorian | phrygian | lydian | mixolydian | harmonic minor
  "progression": ["i9","iv9","bVImaj7","v7"],
  "bars": 4,                        // 1..16; chords are spread evenly over the bars
  "style": "ukg",                   // one of the style ids below
  "energy": 0.6,                    // 0 sparse/laid back .. 1 busy/peak time
  "tempo": 132,
  "explanation": "One friendly sentence, no jargon, on why this matches the vibe."
}}

Style ids: {}.

Roman numeral rules (strict):
- Numerals are relative to the MAJOR scale of the tonic, with b/# for other roots. In A minor, Am F C G is i bVI bIII bVII.
- Upper case = major chord, lower case = minor chord.
- Allowed suffixes: 7, maj7, 6, 9, 11, 13, add9, sus2, sus4, ° (diminished), ø (half-diminished), + (augmented).
- Prefer 4 chords. Techno, dub and dub techno often use 1 or 2 chords; that's fine.
- Pick the style whose groove fits the request best, and a tempo typical of it."#,
        styles.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_llm_output_with_fence() {
        let text = "```json\n{\"key\":\"D\",\"mode\":\"dorian\",\"progression\":[\"i7\",\"IV7\"],\"bars\":2,\"style\":\"deep-house\",\"energy\":0.8}\n```";
        let plan = PatternSpec::from_json(text).unwrap().to_plan(1).unwrap();
        assert_eq!(plan.chord_names(), ["Dm7", "G7"]);
        assert_eq!(plan.style.id, "deep-house");
        assert!((plan.energy - 0.8).abs() < 1e-9);
    }

    #[test]
    fn reports_bad_input() {
        let bad_chord = PatternSpec::from_json(r#"{"key":"C","mode":"major","progression":["I","Q"]}"#).unwrap();
        assert!(bad_chord.to_plan(1).unwrap_err().0.contains("'Q'"));
        let bad_style = PatternSpec::from_json(r#"{"key":"C","mode":"major","progression":["I"],"style":"polka"}"#).unwrap();
        assert!(bad_style.to_plan(1).unwrap_err().0.contains("polka"));
    }

    #[test]
    fn prompt_lists_every_style() {
        for s in style::STYLES {
            assert!(system_prompt().contains(s.id), "{} missing from prompt", s.id);
        }
    }
}
