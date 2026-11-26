//! Momotone engine.
//!
//! Everything musical lives here, with no plugin or audio dependencies, so it can be
//! unit-tested, driven from a CLI, and later compiled to WASM for a web UI.
//!
//! Flow: a [`Style`] (UK garage, dub, soul...) plus a key, an energy and a seed gives a
//! [`Plan`]; [`generate`] renders the plan into a looping [`Pattern`] of timed notes, one
//! layer per part (chords, bass, arp, melody). All layers share the same key, chords and
//! groove grid, so any combination of them fits together.

pub mod chord;
pub mod explain;
pub mod generate;
pub mod grid;
pub mod harmony;
pub mod midi;
pub mod pitch;
pub mod plan;
pub mod rng;
pub mod scale;
pub mod song;
pub mod spec;
pub mod style;
pub mod voicing;

pub use chord::Chord;
pub use generate::{generate, generate_seeded, Layer, Layers, Note, Pattern};
pub use pitch::PitchClass;
pub use plan::Plan;
pub use scale::{Mode, Scale};
pub use song::Section;
pub use spec::PatternSpec;
pub use style::{Style, STYLES};
