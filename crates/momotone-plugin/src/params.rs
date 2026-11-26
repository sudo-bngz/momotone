use nih_plug::prelude::*;
use momotone_engine::style::{index_of, STYLES};
use nih_plug_egui::EguiState;
use std::sync::{Arc, RwLock};

/// Main: the instance you play with. Follower: plays one part of its group's main.
#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum RoleChoice {
    Main,
    Follower,
}

/// Must stay in the same order as `momotone_engine::Layer::ALL`.
#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum PartChoice {
    Chords,
    Bass,
    Arp,
    Melody,
    Drums,
}

impl PartChoice {
    pub fn layer(self) -> momotone_engine::Layer {
        momotone_engine::Layer::ALL[self as usize]
    }
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum KeyChoice {
    C,
    #[name = "C# / Db"]
    CSharp,
    D,
    #[name = "D# / Eb"]
    DSharp,
    E,
    F,
    #[name = "F# / Gb"]
    FSharp,
    G,
    #[name = "G# / Ab"]
    GSharp,
    A,
    #[name = "A# / Bb"]
    ASharp,
    B,
}

fn octave(name: &str) -> IntParam {
    IntParam::new(name, 0, IntRange::Linear { min: -3, max: 3 })
}

/// Declaration order = the order hosts list the parameters. Ableton's Push shows them in
/// banks of 8, so bank 1 holds what you play live, bank 2 the setup, bank 3 the rest.
#[derive(Params)]
pub struct MomotoneParams {
    /// Window size, saved with the project.
    #[persist = "editor-size-v6"]
    pub editor_state: Arc<EguiState>,

    /// The chord progression from the AI prompt, if any, saved with the project.
    #[persist = "ai-harmony"]
    pub ai: Arc<RwLock<Option<crate::ai::AiHarmony>>>,

    /// Locked parts and harmony, saved with the project.
    #[persist = "locks"]
    pub locks: Arc<RwLock<crate::Locks>>,

    /// Direct hardware outputs (port, channel, offset per part), saved with the project.
    #[persist = "hw-out"]
    pub hw: Arc<RwLock<crate::hwout::HwConfig>>,

    // ---- Push bank 1: play
    /// Index into `momotone_engine::STYLES`; shown by name in the DAW.
    #[id = "style"]
    pub style: IntParam,
    #[id = "key"]
    pub key: EnumParam<KeyChoice>,
    /// Sparse and laid back (0 %) to busy and peak-time (100 %).
    #[id = "energy"]
    pub energy: FloatParam,
    /// The idea number: each one is a different loop in the same style. Saved with the project.
    #[id = "variation"]
    pub variation: IntParam,
    #[id = "bars"]
    pub bars: IntParam,
    #[id = "chords"]
    pub chords: BoolParam,
    #[id = "bass"]
    pub bass: BoolParam,
    #[id = "melody"]
    pub melody: BoolParam,

    // ---- Push bank 2: parts and routing
    #[id = "arp"]
    pub arp: BoolParam,
    /// Drums on channel 10 (General MIDI drum map).
    #[id = "drums"]
    pub drums: BoolParam,
    /// Per-part octave shift, to fit each machine's sweet spot.
    #[id = "oct_chords"]
    pub oct_chords: IntParam,
    #[id = "oct_bass"]
    pub oct_bass: IntParam,
    #[id = "oct_arp"]
    pub oct_arp: IntParam,
    #[id = "oct_melody"]
    pub oct_melody: IntParam,
    /// On: chords ch1, bass ch2, arp ch3, melody ch4. Off: everything on channel 1.
    #[id = "split"]
    pub split_channels: BoolParam,
    #[id = "role"]
    pub role: EnumParam<RoleChoice>,

    // ---- Push bank 3
    /// Link group 1..4: a follower follows the main of the same group.
    #[id = "group"]
    pub group: IntParam,
    /// The part a follower plays.
    #[id = "part"]
    pub part: EnumParam<PartChoice>,
}

impl Default for MomotoneParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(crate::editor::WIDTH, crate::editor::HEIGHT),
            hw: Arc::new(RwLock::new(crate::hwout::HwConfig::default())),
            locks: Arc::new(RwLock::new(crate::Locks::default())),
            ai: Arc::new(RwLock::new(None)),
            role: EnumParam::new("Role", RoleChoice::Main),
            group: IntParam::new("Link Group", 1, IntRange::Linear { min: 1, max: crate::link::GROUPS as i32 }),
            part: EnumParam::new("Plays", PartChoice::Bass),
            style: IntParam::new("Style", index_of("ukg").unwrap_or(0) as i32, IntRange::Linear { min: 0, max: STYLES.len() as i32 - 1 })
                .with_value_to_string(Arc::new(|v| STYLES[(v.max(0) as usize).min(STYLES.len() - 1)].label.to_string()))
                .with_string_to_value(Arc::new(|text| {
                    let t = text.trim();
                    STYLES.iter().position(|s| s.id == t || s.label.eq_ignore_ascii_case(t)).map(|i| i as i32)
                })),

            key: EnumParam::new("Key", KeyChoice::A),
            energy: FloatParam::new("Energy", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            variation: IntParam::new("Idea", 1, IntRange::Linear { min: 1, max: 500 })
                .with_value_to_string(Arc::new(|v| format!("#{v}")))
                .with_string_to_value(Arc::new(|t| t.trim().trim_start_matches('#').parse().ok())),
            bars: IntParam::new("Bars", 4, IntRange::Linear { min: 1, max: 8 }),
            chords: BoolParam::new("Chords", true),
            bass: BoolParam::new("Bass", true),
            arp: BoolParam::new("Arp", false),
            drums: BoolParam::new("Drums", false),
            melody: BoolParam::new("Melody", true),
            oct_chords: octave("Chords Oct"),
            oct_bass: octave("Bass Oct"),
            oct_arp: octave("Arp Oct"),
            oct_melody: octave("Melody Oct"),
            split_channels: BoolParam::new("Split Ch", true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the Push shows, bank by bank. Ids never change, so saved sets and automation load.
    #[test]
    fn push_banks() {
        let p = MomotoneParams::default();
        let ids: Vec<String> = p.param_map().into_iter().map(|(id, _, _)| id).collect();
        assert_eq!(ids[..8], ["style", "key", "energy", "variation", "bars", "chords", "bass", "melody"]);
        assert_eq!(ids[8..16], ["arp", "drums", "oct_chords", "oct_bass", "oct_arp", "oct_melody", "split", "role"]);
        assert_eq!(ids[16..], ["group", "part"]);
    }

    #[test]
    fn names_fit_the_push_display() {
        let p = MomotoneParams::default();
        for (id, ptr, _) in p.param_map() {
            let name = unsafe { ptr.name() };
            assert!(name.len() <= 10, "{id}: '{name}' is too long for the Push");
        }
    }

    #[test]
    fn idea_reads_as_a_number() {
        let p = MomotoneParams::default();
        assert_eq!(p.variation.to_string(), "#1");
        assert_eq!(p.variation.string_to_normalized_value("#42").map(|v| p.variation.preview_plain(v)), Some(42));
    }
}
