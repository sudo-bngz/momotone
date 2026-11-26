//! The Momotone screen, in the Momo's Basement look: black, white mono caps, thin lines,
//! iridescent accents.
//!
//! Dropdowns pick the style, key, bars and link role; knobs set energy, the idea and each
//! part's octave. Everything goes through the plugin's parameters, so the DAW records and
//! recalls it and the Push can drive the same controls.

use crate::ai;
use crate::hwout::{self, HwState};
use crate::link;
use crate::params::{KeyChoice, MomotoneParams, PartChoice, RoleChoice};
use crate::{plan_for, settings_from, GenSettings, UiShared};
use momotone_engine::style::{FAMILIES, STYLES};
use momotone_engine::{explain, midi, Layer, Layers, Pattern, PitchClass, Plan};
use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, epaint::Shadow, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontData, FontDefinitions, FontFamily,
    FontId, Layout, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui,
};
use nih_plug_egui::create_egui_editor;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const WIDTH: u32 = 1020;
pub const HEIGHT: u32 = 610;

const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const PANEL: Color32 = Color32::from_rgb(0x07, 0x07, 0x07);
const HOVER: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
const LINE: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const LINE_HI: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const DIM: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
const FAINT: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
const MINT: Color32 = Color32::from_rgb(0x8e, 0xf5, 0xb0);
const CYAN: Color32 = Color32::from_rgb(0x8f, 0xf0, 0xff);
const LILAC: Color32 = Color32::from_rgb(0xc9, 0xa6, 0xff);
const PINK: Color32 = Color32::from_rgb(0xf2, 0x8c, 0xf0);
const WARN: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);

const LANE_H: f32 = 40.0;
const LEFT_W: f32 = 396.0;

fn part_color(layer: Layer) -> Color32 {
    match layer {
        Layer::Chords => LILAC,
        Layer::Bass => PINK,
        Layer::Arp => CYAN,
        Layer::Melody => MINT,
        Layer::Drums => TEXT,
    }
}

/// Mint → cyan → lilac → pink, like the label's artwork.
fn iridescent(t: f32) -> Color32 {
    let stops = [MINT, CYAN, LILAC, PINK];
    let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    let f = x - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    let mix = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * f).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn caps(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text.to_uppercase()).font(mono(size)).color(color)
}

#[derive(Default)]
struct EditorData {
    /// Settings the cached plan/pattern were rendered for. The pattern holds every part,
    /// muted ones included, so they can still be shown (dimmed).
    cache: Option<(GenSettings, Plan, Pattern)>,
    selected_chord: usize,
    status: Option<(String, bool)>,
    last_export: Option<PathBuf>,
    ports: PortCache,
    ask: AskUi,
}

/// Where the background request drops Claude's answer.
type AiReply = Arc<std::sync::Mutex<Option<Result<momotone_engine::PatternSpec, String>>>>;

/// The AI prompt box.
#[derive(Default)]
struct AskUi {
    /// The AI panel is open (it's optional: closed, nothing of it shows on the main screen).
    open: bool,
    /// Where the header's AI button is, to hang the panel under it.
    anchor: Option<Rect>,
    prompt: String,
    key_input: String,
    editing_key: bool,
    /// Whether a key is configured (file or env), read once and after saving.
    has_key: Option<bool>,
    /// The prompt being answered, and where the answer lands.
    pending: Option<(String, AiReply)>,
    status: Option<(String, bool)>,
}

/// MIDI output ports, listed again at most every 2 s while a menu is open.
#[derive(Default)]
struct PortCache {
    names: Vec<String>,
    at: Option<Instant>,
}

impl PortCache {
    fn refresh(&mut self) {
        if self.at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.names = hwout::port_names();
            self.at = Some(Instant::now());
        }
    }
}

pub fn create(params: Arc<MomotoneParams>, shared: Arc<UiShared>, hw: Arc<HwState>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        EditorData::default(),
        |ctx, _| setup(ctx),
        move |ctx, setter, data| draw(ctx, setter, data, &params, &shared, &hw),
    )
}

fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "mono".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/ShareTechMono-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "display".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Audiowide-Regular.ttf"))),
    );
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "mono".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    let mut display_family = vec!["display".to_string()];
    display_family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("display".into()), display_family);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, LINE_HI);
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = CornerRadius::same(2);
    v.menu_corner_radius = CornerRadius::same(2);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(2);
        w.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.weak_bg_fill = BG;
    v.widgets.inactive.bg_fill = BG;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE_HI);
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.active.weak_bg_fill = HOVER;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, MINT);
    v.widgets.open.weak_bg_fill = PANEL;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, MINT);
    v.selection.bg_fill = MINT;
    v.selection.stroke = Stroke::new(1.0_f32, BG);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.text_styles.insert(egui::TextStyle::Body, mono(13.0));
        s.text_styles.insert(egui::TextStyle::Button, mono(13.0));
    });
}

fn set<P: Param>(setter: &ParamSetter, p: &P, v: P::Plain) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}

/// A part's on/off switch and octave (drums have none: each note is an instrument).
fn part_params(params: &MomotoneParams, layer: Layer) -> (&BoolParam, Option<&IntParam>) {
    match layer {
        Layer::Chords => (&params.chords, Some(&params.oct_chords)),
        Layer::Bass => (&params.bass, Some(&params.oct_bass)),
        Layer::Arp => (&params.arp, Some(&params.oct_arp)),
        Layer::Melody => (&params.melody, Some(&params.oct_melody)),
        Layer::Drums => (&params.drums, None),
    }
}

fn octaves(params: &MomotoneParams) -> [i32; Layer::COUNT] {
    [params.oct_chords.value(), params.oct_bass.value(), params.oct_arp.value(), params.oct_melody.value(), 0]
}

fn draw(ctx: &egui::Context, setter: &ParamSetter, data: &mut EditorData, params: &MomotoneParams, shared: &UiShared, hw: &HwState) {
    let follower = params.role.value() == RoleChoice::Follower;
    let group = (params.group.value() - 1).max(0) as usize;
    ai::register_own(&params.ai);
    let part = params.part.value();
    // A follower shows its group's main; a main shows itself.
    let view = if follower {
        link::read(group).map(|(s, _)| GenSettings { layers: link::part_layers(part.layer()), ..s })
    } else {
        Some(settings_from(params))
    };
    if let Some(settings) = view {
        if data.cache.as_ref().map(|c| c.0) != Some(settings) {
            let plan = plan_for(&settings);
            let pattern = momotone_engine::generate_seeded(&plan, Layers::all(), settings.part_seeds());
            data.selected_chord = data.selected_chord.min(plan.progression.len() - 1);
            data.cache = Some((settings, plan, pattern));
        }
    }
    if shared.playing() || follower {
        ctx.request_repaint();
    }

    let frame = egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::symmetric(22, 16));
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let tempo_hint = data.cache.as_ref().map_or(120.0, |c| c.1.tempo);
        header(ui, tempo_hint, shared, (!follower).then_some(&mut data.ask));
        rule(ui);

        let cached = view.and(data.cache.clone());
        ui.horizontal(|ui| {
            if !follower {
                if let Some((s, plan, _)) = &cached {
                    music_selectors(ui, setter, params, s, plan);
                }
                ui.add_space(10.0);
                let (r, _) = ui.allocate_exact_size(vec2(1.0, 44.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0_f32, LINE);
                ui.add_space(10.0);
            }
            link_selectors(ui, setter, params, follower, group, part);
        });
        if shared.conflict() {
            ui.label(caps(
                &format!("! another momotone is also main in group {}: change group or make this one a follower", group + 1),
                12.0,
                WARN,
            ));
        }

        let Some((settings, plan, pattern)) = cached else {
            ui.add_space(40.0);
            ui.label(caps(&format!("signal acquisition... no main momotone in group {}", group + 1), 16.0, DIM));
            ui.label(caps("set one momotone to main in this group: this one will then play its part.", 12.0, FAINT));
            return;
        };

        ui.add_space(4.0);
        if follower {
            follower_body(ui, setter, data, params, &plan, &pattern, &settings, part, shared, hw);
        } else {
            main_body(ui, setter, data, params, &plan, &pattern, &settings, shared, hw);
        }
    });

    if !follower {
        poll_ask(ctx, setter, params, &mut data.ask);
        if data.ask.open {
            ask_panel(ctx, &mut data.ask);
        }
    }
}

fn rule(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0_f32, LINE);
}

fn header(ui: &mut Ui, tempo_hint: f64, shared: &UiShared, ask: Option<&mut AskUi>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("MOMOTONE").font(display(28.0)).color(TEXT));
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.add_space(9.0);
            ui.label(caps("midi loop engine // momo's basement", 11.0, DIM));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let tempo = shared.tempo().unwrap_or(tempo_hint);
            if shared.playing() {
                ui.label(caps(&format!("live // {tempo:.0} bpm"), 12.0, MINT));
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, MINT);
            } else {
                ui.label(caps(&format!("signal acquisition... // {tempo:.0} bpm"), 12.0, DIM));
            }
            if let Some(a) = ask {
                ui.add_space(14.0);
                let label = if a.pending.is_some() { "ai ..." } else { "ai" };
                let r = btn(ui, label, a.open, MINT, 44.0).on_hover_text("Ask AI (optional): describe a vibe, Claude sets the style, key, energy and chords");
                if r.clicked() {
                    a.open = !a.open;
                }
                a.anchor = Some(r.rect);
            }
        });
    });
}

/// Small labelled column: CAPS label on top, widget below.
fn field(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.label(caps(label, 11.0, DIM));
        add(ui);
    });
}

/// One entry of a dropdown, white text, black on the mint selection.
fn choice(ui: &mut Ui, on: bool, text: &str, color: Color32) -> bool {
    ui.selectable_label(on, caps(text, 13.0, if on { BG } else { color })).clicked()
}

fn music_selectors(ui: &mut Ui, setter: &ParamSetter, params: &MomotoneParams, s: &GenSettings, plan: &Plan) {
    field(ui, "style", |ui| {
        let cur = s.style.min(STYLES.len() - 1);
        egui::ComboBox::from_id_salt("style")
            .width(230.0)
            .height(460.0)
            .selected_text(caps(STYLES[cur].label, 13.0, TEXT))
            .show_ui(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for family in FAMILIES {
                    ui.add_space(4.0);
                    ui.label(caps(&format!("// {family}"), 10.0, DIM));
                    for (i, st) in STYLES.iter().enumerate().filter(|(_, st)| st.family == *family) {
                        let text = format!("{:<19}{:>4}", st.label, st.tempo as u32);
                        if choice(ui, i == cur, &text, TEXT) {
                            set(setter, &params.style, i as i32);
                        }
                    }
                }
            });
    });
    field(ui, "key", |ui| {
        let flats = plan.key.prefers_flats();
        egui::ComboBox::from_id_salt("key")
            .width(56.0)
            .selected_text(caps(PitchClass::ALL[s.key % 12].name(flats), 13.0, TEXT))
            .show_ui(ui, |ui| {
                for (i, pc) in PitchClass::ALL.iter().enumerate() {
                    if choice(ui, s.key == i, pc.name(flats), TEXT) {
                        set(setter, &params.key, KeyChoice::from_index(i));
                    }
                }
            });
    });
    field(ui, "scale", |ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(118.0, 26.0), Sense::hover());
        ui.painter().rect_stroke(r, 2.0_f32, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
        ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, plan.key.mode.name().to_uppercase(), mono(13.0), DIM);
        resp.on_hover_text("Set by the style");
    });
    field(ui, "bars", |ui| {
        egui::ComboBox::from_id_salt("bars")
            .width(46.0)
            .selected_text(caps(&s.bars.to_string(), 13.0, TEXT))
            .show_ui(ui, |ui| {
                for b in [1, 2, 4, 8] {
                    if choice(ui, s.bars == b as u32, &b.to_string(), TEXT) {
                        set(setter, &params.bars, b);
                    }
                }
            });
    });
}

fn link_selectors(ui: &mut Ui, setter: &ParamSetter, params: &MomotoneParams, follower: bool, group: usize, part: PartChoice) {
    field(ui, "role", |ui| {
        egui::ComboBox::from_id_salt("role")
            .width(92.0)
            .selected_text(caps(if follower { "follower" } else { "main" }, 13.0, if follower { CYAN } else { TEXT }))
            .show_ui(ui, |ui| {
                if choice(ui, !follower, "main", TEXT) {
                    set(setter, &params.role, RoleChoice::Main);
                }
                if choice(ui, follower, "follower", CYAN) {
                    set(setter, &params.role, RoleChoice::Follower);
                }
            });
    });
    field(ui, "group", |ui| {
        egui::ComboBox::from_id_salt("group")
            .width(40.0)
            .selected_text(caps(&(group + 1).to_string(), 13.0, TEXT))
            .show_ui(ui, |ui| {
                for g in 1..=link::GROUPS as i32 {
                    if choice(ui, g as usize == group + 1, &g.to_string(), TEXT) {
                        set(setter, &params.group, g);
                    }
                }
            });
    });
    if follower {
        field(ui, "plays", |ui| {
            egui::ComboBox::from_id_salt("part")
                .width(90.0)
                .selected_text(caps(part.layer().name(), 13.0, part_color(part.layer())))
                .show_ui(ui, |ui| {
                    for (i, l) in Layer::ALL.into_iter().enumerate() {
                        if choice(ui, part.layer() == l, l.name(), part_color(l)) {
                            set(setter, &params.part, PartChoice::from_index(i));
                        }
                    }
                });
        });
    }
}

// ------------------------------------------------------------------ AI prompt

/// Picks up Claude's answer (also with the panel closed): sets the style, key, energy, bars
/// and the progression.
fn poll_ask(ctx: &egui::Context, setter: &ParamSetter, params: &MomotoneParams, a: &mut AskUi) {
    // An answer came back: set the style, key, energy, bars and the progression.
    if let Some((prompt, slot)) = &a.pending {
        let done = slot.lock().ok().and_then(|mut s| s.take());
        if let Some(result) = done {
            let prompt = prompt.clone();
            a.pending = None;
            a.status = Some(
                match result.and_then(|spec| ai::to_applied(&prompt, &spec)) {
                    Ok(applied) => {
                        set(setter, &params.style, applied.style as i32);
                        set(setter, &params.key, KeyChoice::from_index(applied.key));
                        if let Some(e) = applied.energy {
                            set(setter, &params.energy, e);
                        }
                        if let Some(b) = applied.bars {
                            set(setter, &params.bars, b);
                        }
                        ai::register(&applied.harmony);
                        let n = applied.harmony.progression.len();
                        if let Ok(mut h) = params.ai.write() {
                            *h = Some(applied.harmony);
                        }
                        // Done: the chord strip shows the result, the panel gets out of the way.
                        a.open = false;
                        (format!("done // {} · {n} chords", STYLES[applied.style].label), true)
                    }
                    Err(e) => (e, false),
                },
            );
        } else {
            ctx.request_repaint();
        }
    }
}

/// The optional AI panel, floating under the header's AI button.
fn ask_panel(ctx: &egui::Context, a: &mut AskUi) {
    let has_key = *a.has_key.get_or_insert_with(|| ai::load_config().api_key.is_some());
    let w = 560.0;
    let pos = a.anchor.map_or(Pos2::new(WIDTH as f32 - w - 22.0, 60.0), |r| Pos2::new(r.right() - w, r.bottom() + 8.0));
    let resp = egui::Area::new(egui::Id::new("ask-ai"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::NONE.fill(BG).stroke(Stroke::new(1.0_f32, MINT)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.set_width(w - 28.0);
                ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                ui.horizontal(|ui| {
                    ui.label(caps("ask ai // optional", 12.0, MINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if btn(ui, "close", false, MINT, 0.0).clicked() {
                            a.open = false;
                        }
                    });
                });
                ui.label(caps("describe a vibe: claude sets style, key, energy and chords. you can always go back to the style's chords.", 10.0, DIM));

                if !has_key || a.editing_key {
                    ui.horizontal(|ui| {
                        let edit = egui::TextEdit::singleline(&mut a.key_input)
                            .password(true)
                            .hint_text(RichText::new("paste your Claude API key (sk-ant-...)").color(FAINT))
                            .font(mono(13.0))
                            .desired_width(ui.available_width() - 170.0);
                        let resp = ui.add(edit);
                        let save = btn(ui, "save key", true, MINT, 0.0).clicked() || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if save && !a.key_input.trim().is_empty() {
                            a.status = Some(match ai::save_api_key(&a.key_input) {
                                Ok(_) => ("key saved on this computer (not in your set)".into(), true),
                                Err(e) => (format!("couldn't save the key: {e}"), false),
                            });
                            a.key_input.clear();
                            a.editing_key = false;
                            a.has_key = None;
                        }
                        if has_key && btn(ui, "cancel", false, MINT, 0.0).clicked() {
                            a.editing_key = false;
                        }
                    });
                    if !has_key {
                        ui.label(caps("needs your own claude api key: get one at platform.claude.com", 10.0, FAINT));
                    }
                } else {
                    ui.horizontal(|ui| {
                        let busy = a.pending.is_some();
                        let field = egui::TextEdit::singleline(&mut a.prompt)
                            .hint_text(RichText::new("dark rolling dub techno, a bit hypnotic").color(FAINT))
                            .font(mono(13.0))
                            .desired_width(ui.available_width() - 150.0);
                        let resp = ui.add_enabled(!busy, field);
                        if a.prompt.is_empty() && !busy {
                            resp.request_focus();
                        }
                        let go = btn(ui, if busy { "thinking..." } else { "ask" }, !busy, MINT, 84.0).clicked()
                            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if go && !busy && !a.prompt.trim().is_empty() {
                            let prompt = a.prompt.trim().to_string();
                            let slot = Arc::new(std::sync::Mutex::new(None));
                            let out = slot.clone();
                            let ctx = ui.ctx().clone();
                            let asked = prompt.clone();
                            std::thread::spawn(move || {
                                let result = ai::ask(&ai::load_config(), &asked);
                                if let Ok(mut s) = out.lock() {
                                    *s = Some(result);
                                }
                                ctx.request_repaint();
                            });
                            a.pending = Some((prompt, slot));
                            a.status = Some(("asking claude...".into(), true));
                        }
                        if btn(ui, "key", false, MINT, 0.0).on_hover_text("Change the Claude API key").clicked() {
                            a.editing_key = true;
                        }
                    });
                }
                if let Some((msg, ok)) = &a.status {
                    ui.label(RichText::new(msg).font(mono(11.0)).color(if *ok { MINT } else { WARN }));
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        a.open = false;
    }
    // A click outside the panel (but not on the AI button) closes it.
    let clicked_out = ctx.input(|i| i.pointer.any_click())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !resp.response.rect.contains(p) && !a.anchor.is_some_and(|r| r.contains(p)));
    if clicked_out {
        a.open = false;
    }
}

// ------------------------------------------------------------------ knobs

/// A rotary knob bound to a parameter, drawn in `rect`: drag up/down (Shift = fine),
/// double-click = default. Bipolar knobs fill from 12 o'clock.
fn knob_core<P: Param>(ui: &mut Ui, setter: &ParamSetter, param: &P, rect: Rect, resp: &Response, bipolar: bool) {
    let id = resp.id.with("knob-drag");
    if resp.drag_started() {
        setter.begin_set_parameter(param);
        ui.memory_mut(|m| m.data.insert_temp(id, param.unmodulated_normalized_value()));
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let per_px = if fine { 1.0 / 800.0 } else { 1.0 / 180.0 };
        // Accumulate in our own value: stepped parameters would otherwise swallow small moves.
        let v: f32 = ui.memory(|m| m.data.get_temp(id)).unwrap_or(param.unmodulated_normalized_value());
        let v = (v - resp.drag_delta().y * per_px).clamp(0.0, 1.0);
        ui.memory_mut(|m| m.data.insert_temp(id, v));
        setter.set_parameter_normalized(param, v);
    }
    if resp.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    }

    let v = param.unmodulated_normalized_value();
    let painter = ui.painter();
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0 - 2.0;
    let (a0, a1) = (0.75 * PI, 2.25 * PI);
    let at = |t: f32| a0 + (a1 - a0) * t;
    let point = |a: f32, rad: f32| c + vec2(a.cos(), a.sin()) * rad;
    let steps = 48;
    let track: Vec<Pos2> = (0..=steps).map(|i| point(at(i as f32 / steps as f32), r)).collect();
    painter.add(Shape::line(track, Stroke::new(2.5_f32, FAINT)));
    let (from, to) = if bipolar { (0.5f32.min(v), 0.5f32.max(v)) } else { (0.0, v) };
    let n = ((to - from) * steps as f32).ceil().max(1.0) as usize;
    if to > from {
        for i in 0..n {
            let t0 = from + (to - from) * i as f32 / n as f32;
            let t1 = from + (to - from) * (i + 1) as f32 / n as f32;
            painter.line_segment([point(at(t0), r), point(at(t1), r)], Stroke::new(2.5_f32, iridescent(t1)));
        }
    }
    let hovered = resp.hovered() || resp.dragged();
    painter.circle_filled(c, r - 5.0, PANEL);
    painter.circle_stroke(c, r - 5.0, Stroke::new(1.0_f32, if hovered { TEXT } else { LINE_HI }));
    painter.line_segment([point(at(v), r * 0.2), point(at(v), r - 7.0)], Stroke::new(2.0_f32, TEXT));
}

/// Big labelled knob: LABEL above, value under.
fn knob<P: Param>(ui: &mut Ui, setter: &ParamSetter, param: &P, label: &str, value: &str, size: f32, hint: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let w = size + 16.0;
        ui.set_width(w);
        ui.vertical_centered(|ui| ui.label(caps(label, 11.0, DIM)));
        let (rect, resp) = ui.allocate_exact_size(vec2(w, size), Sense::click_and_drag());
        let knob_rect = Rect::from_center_size(rect.center(), vec2(size, size));
        knob_core(ui, setter, param, knob_rect, &resp, false);
        resp.on_hover_cursor(CursorIcon::ResizeVertical).on_hover_text(hint);
        ui.vertical_centered(|ui| ui.label(RichText::new(value).font(display(13.0)).color(TEXT)));
    });
}

/// Outlined square-ish button; `on` fills it with `color`.
fn btn(ui: &mut Ui, text: &str, on: bool, color: Color32, min_w: f32) -> Response {
    let font = mono(12.0);
    let label = text.to_uppercase();
    let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), TEXT);
    let w = (galley.size().x + 18.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    if on {
        p.rect_filled(rect, 2.0_f32, color);
    } else if hovered {
        p.rect_filled(rect, 2.0_f32, HOVER);
    }
    p.rect_stroke(rect, 2.0_f32, Stroke::new(1.0_f32, if on { color } else if hovered { TEXT } else { LINE_HI }), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, if on { BG } else { TEXT });
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ locks

/// A small padlock: closed when `locked`.
fn padlock(painter: &egui::Painter, c: Pos2, locked: bool, color: Color32) {
    let body = Rect::from_center_size(c + vec2(0.0, 3.0), vec2(10.0, 7.0));
    painter.rect_filled(body, 1.5_f32, color);
    let r = 3.0;
    let lift = if locked { 0.0 } else { 2.5 };
    let top = Pos2::new(c.x, body.top() - 2.0 - lift);
    let arc: Vec<Pos2> = (0..=12).map(|k| {
        let a = PI + PI * k as f32 / 12.0;
        top + vec2(a.cos() * r, a.sin() * r)
    }).collect();
    painter.add(Shape::line(arc, Stroke::new(1.5_f32, color)));
    painter.line_segment([top + vec2(-r, 0.0), Pos2::new(top.x - r, body.top())], Stroke::new(1.5_f32, color));
    if locked {
        painter.line_segment([top + vec2(r, 0.0), Pos2::new(top.x + r, body.top())], Stroke::new(1.5_f32, color));
    }
}

/// A padlock button. Returns true when clicked.
fn lock_button(ui: &mut Ui, locked: bool, hint: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 24.0), Sense::click());
    let color = if locked { MINT } else if resp.hovered() { TEXT } else { FAINT };
    padlock(ui.painter(), rect.center(), locked, color);
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(hint).clicked()
}

fn edit_locks(params: &MomotoneParams, f: impl FnOnce(&mut crate::Locks)) {
    if let Ok(mut l) = params.locks.write() {
        f(&mut l);
    }
}

// ------------------------------------------------------------------ bodies

#[allow(clippy::too_many_arguments)]
fn main_body(
    ui: &mut Ui,
    setter: &ParamSetter,
    data: &mut EditorData,
    params: &MomotoneParams,
    plan: &Plan,
    pattern: &Pattern,
    settings: &GenSettings,
    shared: &UiShared,
    hw: &HwState,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        knob(
            ui,
            setter,
            &params.energy,
            "energy",
            &format!("{} %", settings.energy),
            62.0,
            "Laid back → peak time: adds ghost notes and fills. Drag (Shift = fine), double-click = 50 %",
        );
        knob(
            ui,
            setter,
            &params.variation,
            "idea",
            &format!("#{}", settings.seed),
            62.0,
            "Each number is a different idea in this style. Drag to browse",
        );
        ui.vertical(|ui| {
            // Fixed width, so the lock counter showing up doesn't shift the chord strip.
            ui.set_width(124.0);
            ui.add_space(18.0);
            let cur = params.variation.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if btn(ui, "◀", false, MINT, 30.0).on_hover_text("Previous idea").clicked() {
                    set(setter, &params.variation, if cur <= 1 { 500 } else { cur - 1 });
                }
                if btn(ui, "▶", false, MINT, 30.0).on_hover_text("Next idea").clicked() {
                    set(setter, &params.variation, if cur >= 500 { 1 } else { cur + 1 });
                }
            });
            if btn(ui, "new idea", true, MINT, 64.0).on_hover_text("Jump to a random idea in this style").clicked() {
                set(setter, &params.variation, random_idea(cur));
            }
            let n = settings.locks.count();
            if n > 0 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(caps(&format!("{n} locked"), 11.0, MINT));
                    if btn(ui, "unlock", false, MINT, 0.0).on_hover_text("Unlock everything: ideas change all parts again").clicked() {
                        edit_locks(params, |l| *l = crate::Locks::default());
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(caps("chords", 11.0, DIM));
                if settings.ai != 0 {
                    ui.label(caps("ai harmony", 10.0, MINT));
                    if btn(ui, "back to style chords", false, MINT, 0.0)
                        .on_hover_text("Forget the AI progression and go back to the style's own chords")
                        .clicked()
                    {
                        if let Ok(mut a) = params.ai.write() {
                            *a = None;
                        }
                    }
                    return;
                }
                let locked = settings.locks.harmony.is_some();
                let hint = if locked {
                    format!("Harmony locked to idea #{}: new ideas keep these chords. Click to unlock", settings.harmony_seed())
                } else {
                    "Lock the harmony: new ideas keep this chord progression and change the parts".to_string()
                };
                if lock_button(ui, locked, &hint) {
                    let seed = settings.harmony_seed();
                    edit_locks(params, |l| l.harmony = if locked { None } else { Some(seed) });
                }
                if locked {
                    ui.label(caps("harmony locked", 10.0, MINT));
                }
            });
            chord_strip(ui, plan, data, shared);
            match ai::lookup(settings.ai) {
                Some(h) => {
                    let tempo = h.tempo.map(|t| format!(" · suggested tempo {t:.0} bpm")).unwrap_or_default();
                    let why = h.explanation.clone().unwrap_or_default();
                    ui.label(RichText::new(format!("ai // \"{}\" → {why}{tempo}", h.prompt)).font(mono(11.0)).color(MINT));
                }
                None => {
                    ui.label(RichText::new(plan.style.blurb).font(mono(11.0)).color(DIM));
                }
            }
        });
    });

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(caps("parts", 11.0, DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let split = params.split_channels.value();
            let text = if split { "out: one channel per part (drums ch 10)" } else { "out: all parts on ch 1" };
            if btn(ui, text, false, MINT, 0.0)
                .on_hover_text("One MIDI channel per part (chords 1, bass 2, arp 3, melody 4, drums 10), or everything on channel 1")
                .clicked()
            {
                set(setter, &params.split_channels, !split);
            }
        });
    });
    for layer in Layer::ALL {
        part_lane(ui, setter, params, pattern, layer, shared, false, &mut data.ports, hw, settings);
    }
    ui.add_space(4.0);
    footer(ui, data, plan, pattern, settings, params, shared);
}

/// A follower's screen: what the main plays, the chords, and its one part.
#[allow(clippy::too_many_arguments)]
fn follower_body(
    ui: &mut Ui,
    setter: &ParamSetter,
    data: &mut EditorData,
    params: &MomotoneParams,
    plan: &Plan,
    pattern: &Pattern,
    settings: &GenSettings,
    part: PartChoice,
    shared: &UiShared,
    hw: &HwState,
) {
    let key = plan.key.root.name(plan.key.prefers_flats());
    ui.label(caps(
        &format!(
            "following group {} // {} · {} {} · {} bars · energy {} % · idea #{}",
            params.group.value(),
            plan.style.label,
            key,
            plan.key.mode.name(),
            plan.bars,
            settings.energy,
            settings.seed
        ),
        14.0,
        CYAN,
    ));
    ui.label(caps(
        &format!("style, key, energy and ideas are set on the main momotone. this one plays the {}.", part.layer().name()),
        11.0,
        DIM,
    ));
    ui.add_space(6.0);
    ui.label(caps("chords", 11.0, DIM));
    chord_strip(ui, plan, data, shared);
    ui.add_space(6.0);
    ui.label(caps("part", 11.0, DIM));
    part_lane(ui, setter, params, pattern, part.layer(), shared, true, &mut data.ports, hw, settings);
    ui.add_space(6.0);
    footer(ui, data, plan, pattern, settings, params, shared);
}

fn chord_strip(ui: &mut Ui, plan: &Plan, data: &mut EditorData, shared: &UiShared) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    let n = plan.progression.len();
    let length = plan.length_beats();
    let now = shared.playing().then(|| plan.chord_index(shared.beat().rem_euclid(length)));
    let index_at = |p: Pos2| (((p.x - rect.left()) / rect.width() * n as f32).floor().max(0.0) as usize).min(n - 1);
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            data.selected_chord = index_at(p);
        }
    }
    let hovered = resp.hover_pos().map(index_at);
    let painter = ui.painter();
    let w = rect.width() / n as f32;
    for (i, chord) in plan.progression.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + i as f32 * w, rect.top()), vec2(w, rect.height())).shrink2(vec2(2.0, 0.0));
        let selected = i == data.selected_chord;
        painter.rect_filled(r, 2.0_f32, if hovered == Some(i) { HOVER } else { PANEL });
        let stroke = if now == Some(i) {
            Stroke::new(1.5_f32, MINT)
        } else if selected {
            Stroke::new(1.0_f32, TEXT)
        } else {
            Stroke::new(1.0_f32, LINE)
        };
        painter.rect_stroke(r, 2.0_f32, stroke, StrokeKind::Inside);
        painter.text(r.center() - vec2(0.0, 6.0), Align2::CENTER_CENTER, chord.name(&plan.key), display(16.0), TEXT);
        painter.text(r.center() + vec2(0.0, 13.0), Align2::CENTER_CENTER, &chord.symbol, mono(11.0), DIM);
    }
    // What the hovered (or last clicked) chord does, in plain words.
    let shown = hovered.unwrap_or(data.selected_chord).min(n - 1);
    let line = explain::chord_line(plan, &plan.progression[shown]);
    resp.on_hover_cursor(CursorIcon::PointingHand);
    ui.label(RichText::new(format!("> {line}")).font(mono(12.0)).color(TEXT));
}

#[allow(clippy::too_many_arguments)]
fn part_lane(
    ui: &mut Ui,
    setter: &ParamSetter,
    params: &MomotoneParams,
    pattern: &Pattern,
    layer: Layer,
    shared: &UiShared,
    follower: bool,
    ports: &mut PortCache,
    hw: &HwState,
    settings: &GenSettings,
) {
    let (enabled, octave) = part_params(params, layer);
    // A follower always plays its part; the on/off switches belong to the main.
    let on = follower || enabled.value();
    let color = part_color(layer);
    let i = layer.index();
    let out = params.hw.read().map(|c| c.parts[i].clone()).unwrap_or_else(|_| hwout::HwConfig::default().parts[i].clone());
    let problem = hw.status.lock().ok().and_then(|s| s[i].clone());

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(LEFT_W, LANE_H), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Lock: keep this part's idea while new ideas change the rest (the main decides).
            if follower {
                ui.allocate_exact_size(vec2(20.0, 24.0), Sense::hover());
            } else {
                let locked = settings.locks.parts[i].is_some();
                let seed = settings.part_seeds()[i];
                let hint = if locked {
                    format!("{} locked to idea #{seed}: new ideas change the other parts. Click to unlock", layer.name())
                } else {
                    format!("Lock the {}: keep it while new ideas change the other parts", layer.name().to_lowercase())
                };
                if lock_button(ui, locked, &hint) {
                    edit_locks(params, |l| l.parts[i] = if locked { None } else { Some(seed) });
                }
            }
            // On/off: a square LED and the part name.
            let (r, resp) = ui.allocate_exact_size(vec2(86.0, LANE_H), Sense::click());
            let led = Rect::from_center_size(Pos2::new(r.left() + 7.0, r.center().y), vec2(10.0, 10.0));
            let p = ui.painter();
            if on {
                p.rect_filled(led, 1.0_f32, color);
            } else {
                p.rect_stroke(led, 1.0_f32, Stroke::new(1.0_f32, DIM), StrokeKind::Inside);
            }
            p.text(
                Pos2::new(r.left() + 20.0, r.center().y),
                Align2::LEFT_CENTER,
                layer.name().to_uppercase(),
                mono(13.0),
                if on { TEXT } else { DIM },
            );
            let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(if follower {
                "Followers always play their part"
            } else {
                "Turn this part on or off"
            });
            if resp.clicked() && !follower {
                set(setter, enabled, !on);
            }
            hardware_out(ui, params, i, &out, ports, problem.as_deref());
            // Octave: a small bipolar knob.
            if let Some(octave) = octave {
                let (kr, kresp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click_and_drag());
                knob_core(ui, setter, octave, kr, &kresp, true);
                kresp.on_hover_cursor(CursorIcon::ResizeVertical).on_hover_text("Octave (drag, double-click = 0)");
                let o = octave.value();
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 24.0), Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, format!("{o:+}"), mono(11.0), if o == 0 { DIM } else { TEXT });
            } else {
                // Same width as the octave knob and its value, so every lane lines up.
                let (r, resp) = ui.allocate_exact_size(vec2(28.0 + 6.0 + 20.0, 24.0), Sense::hover());
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "GM", mono(11.0), FAINT);
                resp.on_hover_text("General MIDI drum notes: 36 kick, 38 snare, 39 clap, 37 rim, 42/46 hats");
            }
        });

        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), LANE_H), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0_f32, PANEL);
        painter.rect_stroke(rect, 0.0_f32, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
        let length = pattern.length_beats.max(1.0);
        let x_at = |beat: f64| rect.left() + (beat / length) as f32 * rect.width();
        for b in 1..length as usize {
            let stroke = if b % 4 == 0 { Stroke::new(1.0_f32, LINE_HI) } else { Stroke::new(1.0_f32, LINE) };
            let x = x_at(b as f64);
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);
        }

        let notes: Vec<_> = pattern.notes.iter().filter(|n| n.layer == layer).collect();
        // Drums: one row per instrument, in pitch order; other parts: by pitch.
        let mut rows: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        rows.sort_unstable();
        rows.dedup();
        let row_of = |p: u8| -> f32 {
            if layer == Layer::Drums {
                rows.iter().position(|r| *r == p).unwrap_or(0) as f32
            } else {
                (p - rows[0]) as f32
            }
        };
        if let (Some(&lo), Some(&hi)) = (rows.first(), rows.last()) {
            let range = if layer == Layer::Drums { rows.len() as f32 } else { (hi - lo) as f32 + 1.0 };
            let h = ((rect.height() - 8.0) / range).clamp(2.0, 6.0);
            let fill = if on { color } else { color.gamma_multiply(0.18) };
            for n in notes {
                let y = rect.bottom() - 4.0 - (row_of(n.pitch) + 0.5) / range * (rect.height() - 8.0);
                let r = Rect::from_min_max(
                    Pos2::new(x_at(n.start) + 1.0, y - h / 2.0),
                    Pos2::new((x_at(n.start + n.length) - 1.0).max(x_at(n.start) + 3.0), y + h / 2.0),
                );
                painter.rect_filled(r, 0.0_f32, fill.gamma_multiply(0.5 + 0.5 * n.velocity as f32 / 127.0));
            }
        }
        if !on {
            painter.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, "OFF", mono(11.0), DIM);
        }
        if shared.playing() {
            let x = x_at(shared.beat().rem_euclid(length));
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, TEXT));
        }
    });
}

/// OUT (DAW or a hardware port), channel and latency offset of one part.
fn hardware_out(ui: &mut Ui, params: &MomotoneParams, i: usize, out: &hwout::PartOut, ports: &mut PortCache, problem: Option<&str>) {
    let edit = |f: &dyn Fn(&mut hwout::PartOut)| {
        if let Ok(mut c) = params.hw.write() {
            f(&mut c.parts[i]);
        }
    };
    let shown = out.port.as_deref().map_or("daw".to_string(), |p| p.chars().take(13).collect());
    let color = if problem.is_some() { WARN } else if out.port.is_some() { CYAN } else { DIM };
    egui::ComboBox::from_id_salt(("out", i))
        .width(118.0)
        .selected_text(caps(&shown, 12.0, color))
        .show_ui(ui, |ui| {
            ports.refresh();
            if choice(ui, out.port.is_none(), "daw (track routing)", TEXT) {
                edit(&|p| p.port = None);
            }
            ui.add_space(4.0);
            ui.label(caps("// hardware midi out", 10.0, DIM));
            if ports.names.is_empty() {
                ui.label(caps("no midi outputs found", 11.0, FAINT));
            }
            for name in ports.names.clone() {
                if choice(ui, out.port.as_deref() == Some(name.as_str()), &name, TEXT) {
                    edit(&|p| p.port = Some(name.clone()));
                }
            }
        })
        .response
        .on_hover_text(match (problem, &out.port) {
            (Some(p), _) => format!("Problem: {p}"),
            (None, Some(port)) => format!("Straight to {port}, skipping the DAW"),
            (None, None) => "Through the DAW: route it with a track's MIDI From".into(),
        });

    if out.port.is_some() {
        egui::ComboBox::from_id_salt(("ch", i))
            .width(44.0)
            .selected_text(caps(&format!("ch{}", out.channel), 12.0, TEXT))
            .show_ui(ui, |ui| {
                for ch in 1..=16u8 {
                    if choice(ui, out.channel == ch, &format!("ch {ch}"), TEXT) {
                        edit(&|p| p.channel = ch);
                    }
                }
            });
        let mut ms = out.offset_ms;
        let resp = ui
            .add_sized(
                vec2(62.0, 24.0),
                egui::DragValue::new(&mut ms).range(hwout::OFFSET_RANGE_MS).speed(0.5).fixed_decimals(0).suffix(" ms"),
            )
            .on_hover_text("Latency offset: positive = later, negative = earlier. Drag, or double-click to type");
        if resp.changed() {
            edit(&|p| p.offset_ms = ms);
        }
    } else {
        let channel = if params.split_channels.value() { Layer::ALL[i].channel() + 1 } else { 1 };
        let (r, _) = ui.allocate_exact_size(vec2(114.0, 24.0), Sense::hover());
        ui.painter().text(r.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, format!("CH{channel}"), mono(11.0), FAINT);
    }
}

fn footer(
    ui: &mut Ui,
    data: &mut EditorData,
    plan: &Plan,
    pattern: &Pattern,
    settings: &GenSettings,
    params: &MomotoneParams,
    shared: &UiShared,
) {
    rule(ui);
    ui.horizontal(|ui| {
        let hint = "Saves each part as its own .mid, ready to drag onto your machine tracks";
        if btn(ui, "export midi loop", false, MINT, 150.0).on_hover_text(hint).clicked() {
            let octaves = octaves(params);
            let tempo = shared.tempo().unwrap_or(plan.tempo);
            data.status = Some(match export(&music_dir(), plan, pattern, settings, octaves, tempo) {
                Ok(dir) => {
                    let msg = format!("saved // {}", dir.display());
                    data.last_export = Some(dir);
                    (msg, true)
                }
                Err(e) => (format!("export failed // {e}"), false),
            });
        }
        if let Some(dir) = data.last_export.clone() {
            if btn(ui, "open folder", false, MINT, 0.0).clicked() {
                open_folder(&dir);
            }
        }
        let (text, color) = match &data.status {
            Some((msg, ok)) => (msg.clone(), if *ok { MINT } else { WARN }),
            None => ("tip // add ~/Music/Momotone to Live's Places, then drag parts onto tracks".into(), FAINT),
        };
        ui.label(RichText::new(text).font(mono(11.0)).color(color));
    });
}

fn random_idea(current: i32) -> i32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs().wrapping_mul(0x9E37_79B9))
        .unwrap_or(7);
    let mut n = (nanos.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as i32 % 500 + 1;
    if n == current {
        n = n % 500 + 1;
    }
    n
}

fn music_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join("Music").join("Momotone")
}

fn clean(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect::<String>()
        .replace("- -", "-")
}

/// Writes one folder per idea: one .mid per enabled part plus "All parts.mid".
/// Writes the loop into a new folder under `root` (`~/Music/Momotone` in the plugin).
fn export(root: &Path, plan: &Plan, pattern: &Pattern, s: &GenSettings, octaves: [i32; Layer::COUNT], tempo: f64) -> std::io::Result<PathBuf> {
    let key = plan.key.root.name(plan.key.prefers_flats());
    let name = clean(&format!(
        "{} - {} {} - {:.0} bpm - energy {} - idea {}",
        plan.style.label,
        key,
        plan.key.mode.name(),
        tempo,
        s.energy,
        s.seed
    ));
    let dir = root.join(name);
    std::fs::create_dir_all(&dir)?;

    let shifted = pattern.transposed(octaves);
    let parts: Vec<Layer> = Layer::ALL.into_iter().filter(|l| s.layers.has(*l) && shifted.has(*l)).collect();
    if parts.is_empty() {
        return Err(std::io::Error::other("all parts are off"));
    }
    let selected = Pattern {
        length_beats: shifted.length_beats,
        notes: shifted.notes.iter().filter(|n| parts.contains(&n.layer)).copied().collect(),
        chords: shifted.chords.clone(),
    };
    std::fs::write(dir.join("All parts.mid"), midi::to_smf(&selected, tempo))?;
    for layer in parts {
        let file = format!("{} {}.mid", layer.index() + 1, layer.name());
        std::fs::write(dir.join(file), midi::to_smf(&shifted.only(layer), tempo))?;
    }
    Ok(dir)
}

fn open_folder(dir: &Path) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_writes_one_file_per_part() {
        // Its own folder, no HOME change: tests run in parallel and share the environment.
        let tmp = std::env::temp_dir().join(format!("momotone-export-{}", std::process::id()));
        let dub = momotone_engine::style::index_of("dub").unwrap();
        let s = GenSettings { style: dub, key: 9, bars: 4, seed: 3, energy: 60, layers: Layers { arp: false, ..Layers::all() }, ..Default::default() };
        let plan = plan_for(&s);
        let pattern = momotone_engine::generate_seeded(&plan, Layers::all(), s.part_seeds());
        let dir = export(&tmp, &plan, &pattern, &s, [0, -1, 0, 1, 0], 72.0).unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        files.sort();
        assert_eq!(files, ["1 Chords.mid", "2 Bass.mid", "4 Melody.mid", "5 Drums.mid", "All parts.mid"]);
        assert!(dir.to_string_lossy().contains("Dub - Reggae - A minor - 72 bpm - energy 60 - idea 3"));
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn random_ideas_stay_in_range() {
        for cur in [1, 250, 500] {
            let n = random_idea(cur);
            assert!((1..=500).contains(&n) && n != cur);
        }
    }

    #[test]
    fn iridescent_endpoints() {
        assert_eq!(iridescent(0.0), MINT);
        assert_eq!(iridescent(1.0), PINK);
    }
}
