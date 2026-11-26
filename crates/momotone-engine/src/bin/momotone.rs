//! `momotone` — command-line front end to the engine. Audition ideas as .mid files without
//! a DAW, and test AI specs.
//!
//!   momotone styles
//!   momotone gen --style ukg --key F --energy 0.7 --bars 4 --seed 42 --layers all -o idea.mid
//!   momotone spec spec.json -o idea.mid
//!   momotone prompt            # prints the LLM system prompt

use momotone_engine::{explain, generate, midi, spec, style, Layers, PatternSpec, PitchClass, Plan};
use std::process::exit;

fn usage() -> ! {
    eprintln!(
        "usage:\n  momotone styles\n  momotone gen [--style ID] [--key C] [--energy 0..1] [--bars 4] [--seed N] [--layers chords,bass,arp,melody|all] [-o out.mid]\n  momotone spec FILE.json [--seed N] [--layers ...] [-o out.mid]\n  momotone prompt"
    );
    exit(2)
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    exit(1)
}

fn parse_layers(s: &str) -> Layers {
    let mut l = Layers::none();
    for part in s.split(',') {
        match part.trim() {
            "chords" => l.chords = true,
            "bass" => l.bass = true,
            "arp" => l.arp = true,
            "melody" => l.melody = true,
            "all" => l = Layers::all(),
            other => fail(format!("unknown layer '{other}'")),
        }
    }
    l
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };

    let mut style_id = "soul".to_string();
    let mut key = "C".to_string();
    let mut energy = 0.5f64;
    let mut bars = 4u32;
    let mut seed = 1u64;
    let mut layers = Layers::default();
    let mut out: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| usage());
        match a.as_str() {
            "--style" => style_id = val(),
            "--key" => key = val(),
            "--energy" => energy = val().parse().unwrap_or_else(|_| usage()),
            "--bars" => bars = val().parse().unwrap_or_else(|_| usage()),
            "--seed" => seed = val().parse().unwrap_or_else(|_| usage()),
            "--layers" => layers = parse_layers(&val()),
            "-o" | "--out" => out = Some(val()),
            _ if a.starts_with('-') => usage(),
            _ => positional.push(a.clone()),
        }
    }

    let plan: Plan = match cmd.as_str() {
        "styles" => {
            for family in style::FAMILIES {
                println!("{}", family.to_uppercase());
                for s in style::STYLES.iter().filter(|s| s.family == *family) {
                    println!("  {:<12} {:<20} {:>3} bpm  {}", s.id, s.label, s.tempo, s.mode.name());
                }
            }
            return;
        }
        "prompt" => {
            println!("{}", spec::system_prompt());
            return;
        }
        "gen" => {
            let s = style::find(&style_id).unwrap_or_else(|| fail(format!("unknown style '{style_id}', try `momotone styles`")));
            let root = PitchClass::parse(&key).unwrap_or_else(|| fail(format!("unknown key '{key}'")));
            s.plan(root, bars, energy, seed)
        }
        "spec" => {
            let path = positional.first().unwrap_or_else(|| usage());
            let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
            let spec = PatternSpec::from_json(&text).unwrap_or_else(|e| fail(e));
            if let Some(e) = &spec.explanation {
                println!("AI: {e}");
            }
            spec.to_plan(seed).unwrap_or_else(|e| fail(e))
        }
        _ => usage(),
    };

    let pattern = generate(&plan, layers, seed);
    println!(
        "{} · {} {} · {} bars · energy {:.0}% · {} notes",
        plan.style.label,
        plan.key.root.name(plan.key.prefers_flats()),
        plan.key.mode.name(),
        plan.bars,
        plan.energy * 100.0,
        pattern.notes.len()
    );
    println!("Chords: {}", plan.chord_names().join(" → "));
    for line in explain::explain(&plan) {
        println!("  · {line}");
    }

    if let Some(path) = out {
        std::fs::write(&path, midi::to_smf(&pattern, plan.tempo)).unwrap_or_else(|e| fail(format!("{path}: {e}")));
        println!("Wrote {path}");
    }
}
