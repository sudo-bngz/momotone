use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::Arc;

/// Must stay in the same order as `momotone_engine::STYLES` (a test checks it).
#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum StyleChoice {
    #[name = "UK Garage"]
    Ukg,
    #[name = "Deep House"]
    DeepHouse,
    #[name = "Dub / Reggae"]
    Dub,
    #[name = "Dub Techno"]
    DubTechno,
    #[name = "Soul"]
    Soul,
    #[name = "Lo-fi Hip-hop"]
    Lofi,
    #[name = "Disco / Nu-disco"]
    Disco,
    #[name = "Ambient / Cinematic"]
    Ambient,
}

impl StyleChoice {
    pub fn index(self) -> usize {
        self as usize
    }
}

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

#[derive(Params)]
pub struct MomotoneParams {
    /// Window size, saved with the project. (v2: the window grew with the link row.)
    #[persist = "editor-size-v2"]
    pub editor_state: Arc<EguiState>,

    #[id = "role"]
    pub role: EnumParam<RoleChoice>,
    /// Link group 1..4: a follower follows the main of the same group.
    #[id = "group"]
    pub group: IntParam,
    /// The part a follower plays.
    #[id = "part"]
    pub part: EnumParam<PartChoice>,

    #[id = "style"]
    pub style: EnumParam<StyleChoice>,
    #[id = "key"]
    pub key: EnumParam<KeyChoice>,
    /// Sparse and laid back (0 %) to busy and peak-time (100 %).
    #[id = "energy"]
    pub energy: FloatParam,
    /// Turn it for a new idea in the same style. Saved with the project, so ideas are recallable.
    #[id = "variation"]
    pub variation: IntParam,
    #[id = "bars"]
    pub bars: IntParam,

    #[id = "chords"]
    pub chords: BoolParam,
    #[id = "bass"]
    pub bass: BoolParam,
    #[id = "arp"]
    pub arp: BoolParam,
    #[id = "melody"]
    pub melody: BoolParam,

    /// Per-part octave shift, to fit each machine's sweet spot.
    #[id = "oct_chords"]
    pub oct_chords: IntParam,
    #[id = "oct_bass"]
    pub oct_bass: IntParam,
    #[id = "oct_arp"]
    pub oct_arp: IntParam,
    #[id = "oct_melody"]
    pub oct_melody: IntParam,

    /// On: chords ch1, bass ch2, arp ch3, melody ch4, so each part drives its own machine.
    /// Off: everything on channel 1.
    #[id = "split"]
    pub split_channels: BoolParam,
}

impl Default for MomotoneParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(crate::editor::WIDTH, crate::editor::HEIGHT),
            role: EnumParam::new("Role", RoleChoice::Main),
            group: IntParam::new("Link group", 1, IntRange::Linear { min: 1, max: crate::link::GROUPS as i32 }),
            part: EnumParam::new("Follower part", PartChoice::Bass),
            style: EnumParam::new("Style", StyleChoice::Ukg),
            key: EnumParam::new("Key", KeyChoice::A),
            energy: FloatParam::new("Energy", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            variation: IntParam::new("Variation", 1, IntRange::Linear { min: 1, max: 500 }),
            bars: IntParam::new("Bars", 4, IntRange::Linear { min: 1, max: 8 }),
            chords: BoolParam::new("Chords", true),
            bass: BoolParam::new("Bass", true),
            arp: BoolParam::new("Arp", false),
            melody: BoolParam::new("Melody", true),
            oct_chords: octave("Chords octave"),
            oct_bass: octave("Bass octave"),
            oct_arp: octave("Arp octave"),
            oct_melody: octave("Melody octave"),
            split_channels: BoolParam::new("One channel per part", true),
        }
    }
}
