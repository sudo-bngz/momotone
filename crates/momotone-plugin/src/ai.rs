//! The AI prompt: "dark rolling dub techno, a bit hypnotic" → style, key, energy, bars and a
//! chord progression, set on the plugin.
//!
//! Claude never writes notes. It answers with the small JSON [`PatternSpec`] described by
//! `momotone_engine::spec::system_prompt()`; Momotone checks it and turns it into its own
//! parameters plus an [`AiHarmony`] (the progression), so the result stays playable,
//! editable and recallable.
//!
//! The API key lives in a small config file on this computer (never in the Live set), or in
//! the `ANTHROPIC_API_KEY` environment variable.

use momotone_engine::spec::{system_prompt, PatternSpec};
use momotone_engine::{style, Chord, Mode, PitchClass, Plan, Scale};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
pub const API_VERSION: &str = "2026-10-21";
pub const DEFAULT_URL: &str = "https://api.anthropic.com";

/// A progression chosen by the AI. Saved with the set; identified by `rev`, a hash of its
/// content, so the audio thread and followers can refer to it with a plain number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiHarmony {
    pub prompt: String,
    pub mode: String,
    pub progression: Vec<String>,
    pub explanation: Option<String>,
    pub tempo: Option<f64>,
    pub rev: u16,
}

impl AiHarmony {
    pub fn new(prompt: &str, spec: &PatternSpec) -> AiHarmony {
        let mut h = AiHarmony {
            prompt: prompt.to_string(),
            mode: spec.mode.clone(),
            progression: spec.progression.clone(),
            explanation: spec.explanation.clone(),
            tempo: spec.tempo,
            rev: 0,
        };
        h.rev = content_rev(&h.mode, &h.progression);
        h
    }

    /// Puts this harmony on a plan built from the style (which keeps its grooves).
    pub fn apply(&self, plan: &mut Plan) {
        if let Some(mode) = Mode::parse(&self.mode) {
            plan.key = Scale::new(plan.key.root, mode);
        }
        let chords: Vec<Chord> = self
            .progression
            .iter()
            .filter_map(|c| Chord::parse(c).ok())
            .collect();
        if !chords.is_empty() {
            plan.progression = chords;
        }
    }
}

/// FNV-1a of the harmony, folded to 16 bits; never 0 (0 means "no AI harmony").
fn content_rev(mode: &str, progression: &[String]) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for b in mode.bytes().chain(
        progression
            .iter()
            .flat_map(|c| c.bytes().chain(std::iter::once(b'|'))),
    ) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    ((h ^ (h >> 16)) as u16).max(1)
}

/// Every AI harmony this process knows, by `rev`: mains register theirs, followers and the
/// render threads look them up.
fn registry() -> &'static Mutex<HashMap<u16, AiHarmony>> {
    static R: OnceLock<Mutex<HashMap<u16, AiHarmony>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub fn register(h: &AiHarmony) {
    if let Ok(mut r) = registry().lock() {
        r.entry(h.rev).or_insert_with(|| h.clone());
    }
}

/// Registers an instance's own saved harmony (call before rendering with it).
pub fn register_own(own: &std::sync::RwLock<Option<AiHarmony>>) {
    if let Ok(a) = own.read() {
        if let Some(h) = a.as_ref() {
            register(h);
        }
    }
}

pub fn lookup(rev: u16) -> Option<AiHarmony> {
    if rev == 0 {
        return None;
    }
    registry().lock().ok()?.get(&rev).cloned()
}

// ------------------------------------------------------------------ config

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub api_key: Option<String>,
    /// Defaults to [`DEFAULT_MODEL`].
    pub model: Option<String>,
    /// Only for API keys that aren't scoped to a workspace.
    pub workspace_id: Option<String>,
}

pub fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Momotone/config.json")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("Momotone")
            .join("config.json")
    } else {
        home.join(".config/momotone/config.json")
    }
}

/// The saved config, with `ANTHROPIC_API_KEY` taking over the key when set.
pub fn load_config() -> AiConfig {
    let mut cfg: AiConfig = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if let Some(k) = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
    {
        cfg.api_key = Some(k);
    }
    cfg
}

/// Saves the key in the config file, readable by this user only.
pub fn save_api_key(key: &str) -> std::io::Result<PathBuf> {
    save_api_key_to(config_path(), key)
}

/// Writes the key into the config file at `path`, keeping its other settings.
fn save_api_key_to(path: PathBuf, key: &str) -> std::io::Result<PathBuf> {
    let mut cfg: AiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    cfg.api_key = Some(key.trim().to_string()).filter(|k| !k.is_empty());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&cfg).map_err(std::io::Error::other)?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

// ------------------------------------------------------------------ the call

/// Asks Claude for a pattern spec. Blocking: call it off the GUI and audio threads.
pub fn ask(cfg: &AiConfig, prompt: &str) -> Result<PatternSpec, String> {
    let base = std::env::var("MOMOTONE_API_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    ask_at(&base, cfg, prompt)
}

/// [`ask`] against the API at `base` (tests point this at a local mock server).
fn ask_at(base: &str, cfg: &AiConfig, prompt: &str) -> Result<PatternSpec, String> {
    let key = cfg
        .api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .ok_or("no API key")?;
    let body = serde_json::json!({
        "model": cfg.model.as_deref().unwrap_or(DEFAULT_MODEL),
        "max_tokens": 4096,
        "system": system_prompt(),
        "messages": [{ "role": "user", "content": prompt }],
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let mut req = agent
        .post(&format!("{}/v1/messages", base.trim_end_matches('/')))
        .set("authorization", &format!("Bearer {key}"))
        .set("anthropic-version", API_VERSION)
        .set("content-type", "application/json");
    if let Some(ws) = cfg.workspace_id.as_deref().filter(|w| !w.is_empty()) {
        req = req.set("anthropic-workspace-id", ws);
    }
    let reply: serde_json::Value = match req.send_json(body) {
        Ok(r) => r
            .into_json()
            .map_err(|e| format!("unreadable answer: {e}"))?,
        Err(ureq::Error::Status(code, r)) => {
            let detail = r
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                .unwrap_or_default();
            return Err(match code {
                401 => format!("API key refused (401) {detail}"),
                429 => format!("rate limited, try again in a moment (429) {detail}"),
                _ => format!("API error {code}: {detail}"),
            });
        }
        Err(e) => return Err(format!("can't reach the API: {e}")),
    };
    spec_from_reply(&reply)
}

/// The text blocks of a Messages reply (thinking blocks skipped), read as a spec.
pub fn spec_from_reply(reply: &serde_json::Value) -> Result<PatternSpec, String> {
    let text: String = reply["content"]
        .as_array()
        .ok_or("answer has no content")?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    // Tolerate prose or fences around the JSON object.
    let json = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => return Err("the answer held no JSON".into()),
    };
    let spec = PatternSpec::from_json(json).map_err(|e| e.to_string())?;
    spec.to_plan(1).map_err(|e| e.to_string())?; // Validates key, mode, chords, style.
    Ok(spec)
}

/// What a spec sets on the plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub style: usize,
    pub key: usize,
    pub energy: Option<f32>,
    pub bars: Option<i32>,
    pub harmony: AiHarmony,
}

pub fn to_applied(prompt: &str, spec: &PatternSpec) -> Result<Applied, String> {
    let style_id = spec.style.as_deref().unwrap_or("soul");
    let st = style::find(style_id).ok_or_else(|| format!("unknown style '{style_id}'"))?;
    let style = style::index_of(st.id).unwrap_or(0);
    let key = PitchClass::parse(&spec.key)
        .ok_or_else(|| format!("unknown key '{}'", spec.key))?
        .semitone() as usize;
    Ok(Applied {
        style,
        key,
        energy: spec.energy.map(|e| e.clamp(0.0, 1.0) as f32),
        // The plugin loops 1..8 bars.
        bars: Some(spec.bars.clamp(1, 8) as i32),
        harmony: AiHarmony::new(prompt, spec),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// One-shot HTTP server that records the request and answers with `status` / `body`.
    fn mock(status: u16, body: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.to_ascii_lowercase().starts_with("content-length:") {
                    len = line[15..].trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut req_body = vec![0u8; len];
            reader.read_exact(&mut req_body).unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            head + &String::from_utf8(req_body).unwrap()
        });
        (url, handle)
    }

    fn reply_with(text: &str) -> String {
        serde_json::json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": DEFAULT_MODEL,
            "content": [
                { "type": "thinking", "thinking": "dub techno wants one chord...", "signature": "x" },
                { "type": "text", "text": text }
            ],
            "stop_reason": "end_turn"
        })
        .to_string()
    }

    #[test]
    fn asks_claude_and_reads_the_spec() {
        let answer = "Here you go:\n```json\n{\"key\":\"F\",\"mode\":\"minor\",\"progression\":[\"i9\",\"i9\",\"bVImaj7\",\"bVImaj7\"],\"bars\":4,\"style\":\"dub-techno\",\"energy\":0.35,\"tempo\":118,\"explanation\":\"One deep chord, lots of space.\"}\n```";
        let (url, server) = mock(200, reply_with(answer));
        let cfg = AiConfig {
            api_key: Some("sk-test".into()),
            ..Default::default()
        };
        let spec = ask_at(&url, &cfg, "dark rolling dub techno, a bit hypnotic").unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();

        assert!(request.starts_with("post /v1/messages "), "{request}");
        assert!(request.contains("authorization: bearer sk-test"));
        assert!(request.contains(&format!("anthropic-version: {API_VERSION}")));
        assert!(request.contains(&format!("\"model\":\"{DEFAULT_MODEL}\"")));
        assert!(request.contains("dark rolling dub techno"));
        assert!(
            request.contains("dub-techno"),
            "the system prompt lists the styles"
        );

        let applied = to_applied("dark rolling dub techno", &spec).unwrap();
        assert_eq!(STYLE("dub-techno"), applied.style);
        assert_eq!(applied.key, 5);
        assert_eq!(applied.energy, Some(0.35));
        assert_eq!(
            applied.harmony.progression,
            ["i9", "i9", "bVImaj7", "bVImaj7"]
        );
        assert_ne!(applied.harmony.rev, 0);
    }

    #[allow(non_snake_case)]
    fn STYLE(id: &str) -> usize {
        style::index_of(id).unwrap()
    }

    #[test]
    fn reports_api_errors() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#.to_string();
        let (url, server) = mock(401, body);
        let err = ask_at(
            &url,
            &AiConfig {
                api_key: Some("bad".into()),
                ..Default::default()
            },
            "house",
        )
        .unwrap_err();
        server.join().unwrap();
        assert!(
            err.contains("401") && err.contains("invalid x-api-key"),
            "{err}"
        );
        assert_eq!(
            ask(&AiConfig::default(), "house").unwrap_err(),
            "no API key"
        );
    }

    #[test]
    fn rejects_answers_that_are_not_music() {
        assert!(spec_from_reply(
            &serde_json::json!({"content":[{"type":"text","text":"Sorry, I can't."}]})
        )
        .is_err());
        let bad_chord = r#"{"key":"C","mode":"major","progression":["I","Zz"],"style":"house"}"#;
        assert!(spec_from_reply(
            &serde_json::json!({"content":[{"type":"text","text":bad_chord}]})
        )
        .unwrap_err()
        .contains("Zz"));
    }

    #[test]
    fn harmony_goes_on_the_plan_and_is_shared_by_rev() {
        let spec = PatternSpec::from_json(
            r#"{"key":"D","mode":"dorian","progression":["i7","IV9"],"style":"deep-house"}"#,
        )
        .unwrap();
        let h = AiHarmony::new("deep", &spec);
        let mut plan =
            style::find("techno")
                .unwrap()
                .plan(PitchClass::parse("D").unwrap(), 4, 0.5, 1);
        h.apply(&mut plan);
        assert_eq!(plan.chord_names(), ["Dm7", "G9"]);
        assert_eq!(plan.key.mode, Mode::Dorian);
        register(&h);
        assert_eq!(lookup(h.rev), Some(h.clone()));
        assert_eq!(lookup(0), None);
        let again = AiHarmony::new("other words, same chords", &spec);
        assert_eq!(again.rev, h.rev, "rev depends on the harmony only");
    }

    #[test]
    fn key_file_is_private() {
        // No HOME juggling: tests run in parallel and share the environment.
        let tmp = std::env::temp_dir().join(format!("momotone-ai-{}", std::process::id()));
        let path = save_api_key_to(tmp.join("Momotone").join("config.json"), "  sk-ant-123  ").unwrap();
        let cfg: AiConfig = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(cfg.api_key.as_deref(), Some("sk-ant-123"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(tmp).ok();
    }
}
