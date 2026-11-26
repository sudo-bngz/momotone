# Momotone

**A MIDI loop generator by [Momo's Basement](https://momosbasement.com).** Pick a style and a key, set the energy, press play.
Momotone writes chords, bass, an arpeggio and a melody that all fit together, locked to your DAW's tempo, each on its own MIDI channel, so every part can drive a different machine.

No music theory needed. Every chord comes with a short note in plain words on what it does.

VST3 + CLAP, written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug). Works in Ableton Live, Bitwig, Reaper, FL Studio, Cubase, Studio One and other VST3/CLAP hosts, on macOS, Windows and Linux.

## Styles

| Style | Feel | Tempo |
|---|---|---|
| UK Garage | swung 2-step, minor 7th/9th organ stabs that dodge beat one, bouncing sub | 132 |
| Deep House | dorian 9ths on the off-beats, rolling bass | 122 |
| Dub / Reggae | one or two minor chords skanked on 2 and 4, the bass carries the tune | 72 |
| Dub Techno | one hypnotic minor chord stab over a steady sub, made for delays | 120 |
| Soul | major 7ths and ii-V moves, legato, walking bass | 92 |
| Lo-fi Hip-hop | jazzy 7ths, heavy swing | 82 |
| Disco / Nu-disco | off-beat chord chops, octave bass | 118 |
| Ambient / Cinematic | long minor chords, slow melody | 90 |

The plugin follows your project tempo; the tempo column is what the `.mid` export uses.

## Parameters

| Parameter | What it does |
|---|---|
| Style | One of the styles above: harmony, grooves and swing |
| Key | Root note. The style picks the scale |
| Energy | 0 % sparse and laid back → 100 % busy and peak-time. It only adds or removes ghost notes and fills; the core groove never moves |
| Variation | 1..500. Each number is a different idea. Saved with your project, so you can always get it back |
| Bars | Loop length, 1..8 |
| Chords / Bass / Arp / Melody | Turn parts on or off. Muting one never changes the others |
| *Part* octave | Shift each part ±3 octaves to fit each machine's range |
| One channel per part | On (default): chords ch 1, bass ch 2, arp ch 3, melody ch 4. Off: everything on ch 1 |

New settings come in on the next bar line, like launching a clip, so you can tweak while it plays.

## Studio workflow (Ableton Live + Push + hardware)

Live loads VST plugins as instruments, not MIDI effects, so Momotone sits on its own track and the other tracks listen to it:

1. **Momotone track**: a MIDI track with Momotone on it.
2. **One track per machine** (for example Bass → MS-1, Chords → Prophet, Melody → another synth):
   - *MIDI From*: the Momotone track, second dropdown: the part's channel (Ch. 2 for bass…).
   - *Monitor*: **In**.
   - Device: Live's **External Instrument**, set to that machine's MIDI output, channel and audio return.
3. Press play, find a loop you like with Style / Energy / Variation (map them to Push encoders: they're automatable parameters).
4. **Record the audio** of each machine into clips, then play those clips from the Push.

To keep the MIDI instead, arm the machine tracks and record MIDI clips; you can drag a `.mid` from the CLI below too.

In Bitwig or Reaper you can also put Momotone directly in front of a synth on the same track.

## Build

```sh
cargo test --workspace                                  # engine + playback tests
cargo xtask bundle momotone-plugin --release            # Windows / Linux
cargo xtask bundle-universal momotone-plugin --release  # macOS (Intel + Apple Silicon)
```

Bundles land in `target/bundled/` (`Momotone.vst3`, `Momotone.clap`). Copy them to:

| OS | VST3 | CLAP |
|---|---|---|
| macOS | `~/Library/Audio/Plug-Ins/VST3` | `~/Library/Audio/Plug-Ins/CLAP` |
| Windows | `C:\Program Files\Common Files\VST3` | `C:\Program Files\Common Files\CLAP` |
| Linux | `~/.vst3` | `~/.clap` |

On macOS, unsigned plugins are blocked the first time; clear the flag with
`xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/Momotone.vst3`.

Every push runs `.github/workflows/build.yml`: tests, then macOS universal, Windows and Linux bundles as downloadable artifacts.

## Command line (audition without a DAW)

```sh
cargo run -p momotone-engine --bin momotone -- styles
cargo run -p momotone-engine --bin momotone -- gen --style ukg --key F --energy 0.7 --seed 4 --layers all -o idea.mid
cargo run -p momotone-engine --bin momotone -- spec demos/ai-spec-example.json --layers all -o idea.mid
cargo run -p momotone-engine --bin momotone -- prompt   # the LLM system prompt
```

`.mid` files have one track per part, so dropping one into Live creates separate tracks. `demos/` has examples.

## How it works

```
crates/momotone-engine   pure Rust, no audio deps (CLI, tests, WASM later)
  pitch, scale           notes, 7 modes, scale stepping
  chord                  Roman numerals: "i9", "bVImaj7", "IV7", "ii°" ...
  voicing                voice leading: each chord moves the fewest semitones from the last
  grid                   groove grids with energy thresholds (see below)
  style                  the genres: mode, proven progressions, swing, one grid per part
  generate               chords / bass / arp / melody, deterministic per seed
  explain                "why this works" lines
  spec                   JSON contract for the AI layer + its system prompt
  midi                   Standard MIDI File writer
crates/momotone-plugin   nih-plug wrapper
  params                 host-automatable parameters
  player                 loops the pattern against the host transport, sample-accurate
  lib                    background rendering, bar-quantized swaps, no allocation on the audio thread
```

**Groove grids.** A style's rhythm is written as text, one character per 16th note:

```
UK garage chords   "...0..1...0..2.6"
UK garage bass     "0..3..1.....2.4.0..3..1...5.2..7"   (2 bars)
            tones  "r..8..5.....r.a.r..8..5...3.r..a"   root, octave, fifth, third, approach...
```

`.` is a rest; a digit is a hit that plays when Energy × 10 ≥ that digit. `0` is the core groove; higher digits are ghost notes and fills. Swing pushes the off-beat 16ths late. Adding a genre means writing a few of these lines.

**Melody.** A one-bar motif is picked from the style's melody positions, then stated, answered with a varied contour, contrasted, and resolved home on the last bar. Notes on beats 1 and 3 are always chord tones, and over borrowed chords the clashing scale notes are swapped for the chord's own.

**AI layer.** The LLM never writes notes. It turns a vibe ("rainy-night garage, a bit soulful") into a small JSON spec (key, mode, Roman-numeral progression, style, energy, one-sentence explanation). The engine validates it and renders it with the style's grooves, so the output is always playable and editable.

## Roadmap

- [ ] Custom UI: big style buttons, energy fader, chord strip, "New idea", piano-roll preview
- [ ] Drag the loop from the plugin straight into the DAW as a clip
- [ ] AI prompt box (LLM call from a background thread)
- [ ] Lock a part or a bar you like and regenerate the rest
- [ ] Per-part MIDI channel and velocity curves, humanize
- [ ] Drum part (kick/snare/hats grids per style) for drum machines
- [ ] Follow mode: play a note on the Push to set the key live
- [ ] More styles: breakbeat, afro house, steppers, electro

## License

GPL-3.0-or-later. nih-plug's VST3 bindings are GPLv3, so distributed VST3 builds must be GPLv3.
To ship closed-source, release CLAP only or move the VST3 wrapper to the official SDK.
