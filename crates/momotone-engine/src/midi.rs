//! Minimal Standard MIDI File writer (format 1, one track per layer), so a pattern can be
//! dragged into any DAW and land as separate Chords / Bass / Arp / Melody tracks.

use crate::generate::{Layer, Pattern};

pub const PPQ: u16 = 480;

fn vlq(mut v: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 4];
    let mut i = 3;
    buf[i] = (v & 0x7F) as u8;
    v >>= 7;
    while v > 0 {
        i -= 1;
        buf[i] = ((v & 0x7F) as u8) | 0x80;
        v >>= 7;
    }
    out.extend_from_slice(&buf[i..]);
}

fn ticks(beats: f64) -> u32 {
    (beats * PPQ as f64).round().max(0.0) as u32
}

fn track(events: &mut [(u32, u8, Vec<u8>)], name: &str) -> Vec<u8> {
    // Sort by time; note-offs (priority 0) before note-ons (1) at the same tick.
    events.sort_by_key(|(t, prio, _)| (*t, *prio));
    let mut data = vec![0x00, 0xFF, 0x03];
    vlq(name.len() as u32, &mut data);
    data.extend_from_slice(name.as_bytes());
    let mut last = 0;
    for (t, _, bytes) in events.iter() {
        vlq(t - last, &mut data);
        data.extend_from_slice(bytes);
        last = *t;
    }
    data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    let mut chunk = b"MTrk".to_vec();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend(data);
    chunk
}

/// Encodes the pattern as a .mid file. Empty layers are skipped.
pub fn to_smf(pattern: &Pattern, tempo_bpm: f64) -> Vec<u8> {
    let mut tracks = Vec::new();

    let us_per_beat = (60_000_000.0 / tempo_bpm.max(1.0)).round() as u32;
    let t = us_per_beat.to_be_bytes();
    let mut meta = vec![
        (0, 0, vec![0xFF, 0x51, 0x03, t[1], t[2], t[3]]),
        (0, 0, vec![0xFF, 0x58, 0x04, 4, 2, 24, 8]),
    ];
    tracks.push(track(&mut meta, "Momotone"));

    for layer in Layer::ALL {
        let ch = layer.channel();
        let mut ev: Vec<(u32, u8, Vec<u8>)> = Vec::new();
        for n in pattern.notes.iter().filter(|n| n.layer == layer) {
            let on = ticks(n.start);
            let off = ticks(n.start + n.length).max(on + 1);
            ev.push((on, 1, vec![0x90 | ch, n.pitch, n.velocity.max(1)]));
            ev.push((off, 0, vec![0x80 | ch, n.pitch, 64]));
        }
        if !ev.is_empty() {
            tracks.push(track(&mut ev, layer.name()));
        }
    }

    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&PPQ.to_be_bytes());
    for t in tracks {
        out.extend(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_examples() {
        let enc = |v| {
            let mut o = Vec::new();
            vlq(v, &mut o);
            o
        };
        assert_eq!(enc(0), [0x00]);
        assert_eq!(enc(0x7F), [0x7F]);
        assert_eq!(enc(0x80), [0x81, 0x00]);
        assert_eq!(enc(0x3FFF), [0xFF, 0x7F]);
        assert_eq!(enc(0x0FFF_FFFF), [0xFF, 0xFF, 0xFF, 0x7F]);
    }
}
