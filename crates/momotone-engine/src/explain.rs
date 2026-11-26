//! Plain-language "why this works" notes, so the user picks up theory by ear instead of
//! having to study it first.

use crate::plan::Plan;
use crate::scale::Mode;

pub fn mode_blurb(mode: Mode) -> &'static str {
    match mode {
        Mode::Major => "Major: bright and settled, the default 'happy' colour.",
        Mode::Minor => "Minor: darker and more emotional; most club and film music lives here.",
        Mode::Dorian => "Dorian: minor, but with a hopeful raised 6th. The funk, house and soul minor.",
        Mode::Phrygian => "Phrygian: minor with a half-step above home. Instantly dark and exotic.",
        Mode::Lydian => "Lydian: major with a raised 4th. Floating, dreamy, a bit magical.",
        Mode::Mixolydian => "Mixolydian: major with a flat 7th. Bluesy, rock, laid-back.",
        Mode::HarmonicMinor => "Harmonic minor: minor with a strong pull home. Dramatic, tense.",
    }
}

fn role(root: i32, major_third: bool, minor_key: bool) -> &'static str {
    match (root, major_third) {
        (0, _) => "home: the progression resolves here",
        (1, _) => "a half-step above home: instant darkness and tension",
        (2, true) => "the raised-4th colour that makes things float",
        (2, false) => "a set-up chord that loves to move to V",
        (3, _) => "the relative major: a ray of light inside a minor key",
        (4, true) => "a borrowed dominant that pulls hard towards vi",
        (4, false) => "a soft, wistful in-between chord",
        (5, true) if minor_key => "the bright dorian IV: the soulful lift of house and funk",
        (5, _) if minor_key => "steps away from home, heavier and sadder",
        (5, _) => "steps away from home: a gentle lift",
        (7, true) => "maximum tension: it wants to fall back home",
        (7, false) => "a soft minor v: moves on without forcing a resolution",
        (8, _) if minor_key => "a big, warm lift away from home",
        (8, _) => "borrowed from minor: a big, cinematic lift",
        (9, false) => "the relative minor: the sad twin of home",
        (9, true) => "a bright borrowed chord, surprising and uplifting",
        (10, _) => "a whole step below home: the rock/dub 'answer' chord",
        (11, _) => "unstable and restless: it leans into home",
        _ => "an outside colour chord for surprise",
    }
}

/// Style line, mode line, then one line per distinct chord.
pub fn explain(plan: &Plan) -> Vec<String> {
    let minor_key = plan.key.mode.is_minor();
    let mut lines = vec![plan.style.blurb.to_string(), mode_blurb(plan.key.mode).to_string()];
    let mut seen = Vec::new();
    for chord in &plan.progression {
        let name = chord.name(&plan.key);
        if seen.contains(&name) {
            continue;
        }
        let major_third = chord.intervals.contains(&4);
        lines.push(format!("{name} ({}): {}", chord.symbol, role(chord.root, major_third, minor_key)));
        seen.push(name);
    }
    if plan.progression.iter().any(|c| c.intervals.iter().any(|&i| i >= 10)) {
        lines.push("The 7ths and 9ths add softness and colour: that's the jazzy, soulful sound.".into());
    }
    lines
}
