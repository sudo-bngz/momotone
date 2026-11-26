//! The Momotone screen: pick a style and a key, set the energy, hit "New idea", see the
//! chords and every part, export the loop as MIDI files.
//!
//! Everything here goes through the plugin's parameters, so the DAW records and recalls it,
//! and the Push can still drive the same controls.

use crate::link;
use crate::params::{KeyChoice, MomotoneParams, PartChoice, RoleChoice, StyleChoice};
use crate::{plan_for, settings_from, GenSettings, UiShared};
use momotone_engine::{explain, generate, midi, Layer, Layers, Pattern, PitchClass, Plan, STYLES};
use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, vec2, Align, Align2, Color32, CursorIcon, FontId, Layout, Pos2, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui,
};
use nih_plug_egui::create_egui_editor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const WIDTH: u32 = 940;
pub const HEIGHT: u32 = 790;

const BG: Color32 = Color32::from_rgb(0x15, 0x12, 0x10);
const PANEL: Color32 = Color32::from_rgb(0x23, 0x1e, 0x1a);
const PANEL_HI: Color32 = Color32::from_rgb(0x30, 0x29, 0x23);
const LINE: Color32 = Color32::from_rgb(0x3b, 0x33, 0x2c);
const TEXT: Color32 = Color32::from_rgb(0xef, 0xe6, 0xda);
const DIM: Color32 = Color32::from_rgb(0x9b, 0x8d, 0x7d);
const ACCENT: Color32 = Color32::from_rgb(0xf2, 0xa9, 0x3b);
const LIVE: Color32 = Color32::from_rgb(0x7c, 0xd9, 0x8a);

const LANE_H: f32 = 50.0;
const LEFT_W: f32 = 190.0;

fn part_color(layer: Layer) -> Color32 {
    match layer {
        Layer::Chords => Color32::from_rgb(0xf2, 0xa9, 0x3b),
        Layer::Bass => Color32::from_rgb(0xe8, 0x5d, 0x5a),
        Layer::Arp => Color32::from_rgb(0x4f, 0xc1, 0xb0),
        Layer::Melody => Color32::from_rgb(0xb4, 0x8c, 0xf2),
    }
}

#[derive(Default)]
struct EditorData {
    /// Settings the cached plan/pattern were rendered for. The pattern holds every part,
    /// muted ones included, so they can still be shown (dimmed).
    cache: Option<(GenSettings, Plan, Pattern)>,
    selected_chord: usize,
    status: Option<(String, bool)>,
    last_export: Option<PathBuf>,
}

pub fn create(params: Arc<MomotoneParams>, shared: Arc<UiShared>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        EditorData::default(),
        |ctx, _| setup(ctx),
        move |ctx, setter, data| draw(ctx, setter, data, &params, &shared),
    )
}

fn setup(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    ctx.set_visuals(v);
}

fn set<P: Param>(setter: &ParamSetter, p: &P, v: P::Plain) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}

fn part_params(params: &MomotoneParams, layer: Layer) -> (&BoolParam, &IntParam) {
    match layer {
        Layer::Chords => (&params.chords, &params.oct_chords),
        Layer::Bass => (&params.bass, &params.oct_bass),
        Layer::Arp => (&params.arp, &params.oct_arp),
        Layer::Melody => (&params.melody, &params.oct_melody),
    }
}

fn draw(ctx: &egui::Context, setter: &ParamSetter, data: &mut EditorData, params: &MomotoneParams, shared: &UiShared) {
    let follower = params.role.value() == RoleChoice::Follower;
    let group = (params.group.value() - 1).max(0) as usize;
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
            let pattern = generate(&plan, Layers::all(), settings.seed);
            data.selected_chord = data.selected_chord.min(plan.progression.len() - 1);
            data.cache = Some((settings, plan, pattern));
        }
    }
    if shared.playing() || follower {
        ctx.request_repaint();
    }

    let frame = egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::symmetric(22, 18));
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let tempo_hint = data.cache.as_ref().map_or(120.0, |c| c.1.tempo);
        header(ui, tempo_hint, shared);
        link_row(ui, setter, params, follower, group, part);
        if shared.conflict() {
            ui.label(
                RichText::new(format!(
                    "Another Momotone is also Main in group {}: give this one another group, or make it a Follower.",
                    group + 1
                ))
                .size(13.0)
                .color(Color32::from_rgb(0xe8, 0x5d, 0x5a)),
            );
        }
        ui.add_space(2.0);

        let (settings, plan, pattern) = match (view, data.cache.clone()) {
            (Some(_), Some(c)) => c,
            _ => {
                ui.add_space(30.0);
                ui.label(RichText::new(format!("No main Momotone in group {} yet.", group + 1)).size(18.0));
                ui.label(
                    RichText::new("Set one Momotone to Main in this group: this one will then play its part.")
                        .size(13.0)
                        .color(DIM),
                );
                return;
            }
        };

        if follower {
            follower_body(ui, setter, data, params, &plan, &pattern, &settings, part, shared);
            return;
        }

        section(ui, "STYLE");
        ui.horizontal_wrapped(|ui| {
            for (i, s) in STYLES.iter().enumerate() {
                if pill(ui, s.label, settings.style == i, ACCENT, 0.0).clicked() {
                    set(setter, &params.style, StyleChoice::from_index(i));
                }
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section(ui, &format!("KEY  ·  {}", plan.key.mode.name()));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, pc) in PitchClass::ALL.iter().enumerate() {
                        if pill(ui, pc.name(plan.key.prefers_flats()), settings.key == i, ACCENT, 36.0).clicked() {
                            set(setter, &params.key, KeyChoice::from_index(i));
                        }
                    }
                });
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                section(ui, "BARS");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for b in [1, 2, 4, 8] {
                        if pill(ui, &b.to_string(), settings.bars == b as u32, ACCENT, 36.0).clicked() {
                            set(setter, &params.bars, b);
                        }
                    }
                });
            });
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section(ui, "ENERGY");
                energy_fader(ui, setter, params);
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                section(ui, "IDEA");
                idea_controls(ui, setter, params);
            });
        });

        ui.add_space(4.0);
        section(ui, "CHORDS");
        chord_strip(ui, &plan, data, shared);
        if let Some(chord) = plan.progression.get(data.selected_chord) {
            ui.label(RichText::new(explain::chord_line(&plan, chord)).size(13.0));
        }
        ui.label(RichText::new(plan.style.blurb).size(12.0).color(DIM));

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            section(ui, "PARTS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let split = params.split_channels.value();
                let label = if split { "One MIDI channel per part" } else { "All parts on channel 1" };
                if pill(ui, label, split, LIVE, 0.0).clicked() {
                    set(setter, &params.split_channels, !split);
                }
            });
        });
        for layer in Layer::ALL {
            part_lane(ui, setter, params, &pattern, layer, shared, false);
        }

        ui.add_space(6.0);
        footer(ui, data, &plan, &pattern, &settings, params, shared);
    });
}

fn link_row(ui: &mut Ui, setter: &ParamSetter, params: &MomotoneParams, follower: bool, group: usize, part: PartChoice) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        section(ui, "ROLE ");
        if pill(ui, "Main", !follower, ACCENT, 70.0).on_hover_text("This Momotone is the one you play with").clicked() {
            set(setter, &params.role, RoleChoice::Main);
        }
        if pill(ui, "Follower", follower, LIVE, 90.0)
            .on_hover_text("Plays one part of the main Momotone of its group")
            .clicked()
        {
            set(setter, &params.role, RoleChoice::Follower);
        }
        ui.add_space(14.0);
        section(ui, "GROUP ");
        for g in 1..=link::GROUPS as i32 {
            let on = g as usize == group + 1;
            if pill(ui, &g.to_string(), on, if follower { LIVE } else { ACCENT }, 30.0).clicked() {
                set(setter, &params.group, g);
            }
        }
        if follower {
            ui.add_space(14.0);
            section(ui, "PLAYS ");
            for (i, l) in Layer::ALL.into_iter().enumerate() {
                if pill(ui, l.name(), part.layer() == l, part_color(l), 0.0).clicked() {
                    set(setter, &params.part, PartChoice::from_index(i));
                }
            }
        }
    });
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
) {
    ui.add_space(6.0);
    let key = plan.key.root.name(plan.key.prefers_flats());
    ui.label(
        RichText::new(format!(
            "Following group {}:  {} · {} {} · {} bars · energy {} % · idea #{}",
            params.group.value(),
            plan.style.label,
            key,
            plan.key.mode.name(),
            plan.bars,
            settings.energy,
            settings.seed
        ))
        .size(15.0),
    );
    ui.label(
        RichText::new(format!(
            "Style, key, energy and ideas are set on the main Momotone. This one plays the {}.",
            part.layer().name().to_lowercase()
        ))
        .size(12.0)
        .color(DIM),
    );
    ui.add_space(6.0);
    section(ui, "CHORDS");
    chord_strip(ui, plan, data, shared);
    if let Some(chord) = plan.progression.get(data.selected_chord) {
        ui.label(RichText::new(explain::chord_line(plan, chord)).size(13.0));
    }
    ui.add_space(6.0);
    section(ui, "PART");
    part_lane(ui, setter, params, pattern, part.layer(), shared, true);
    ui.add_space(6.0);
    footer(ui, data, plan, pattern, settings, params, shared);
}

fn section(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(DIM));
}

fn header(ui: &mut Ui, tempo_hint: f64, shared: &UiShared) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("MOMOTONE").size(26.0).strong().color(TEXT));
        ui.label(RichText::new("by Momo's Basement").size(13.0).color(DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (dot, text) = if shared.playing() {
                (LIVE, "playing".to_string())
            } else {
                (DIM, "press play in your DAW to hear it".to_string())
            };
            ui.label(RichText::new(text).size(12.0).color(dot));
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            let tempo = shared.tempo().unwrap_or(tempo_hint);
            ui.label(RichText::new(format!("{:.0} bpm   ", tempo)).size(12.0).color(DIM));
        });
    });
}

/// A rounded button that lights up in `color` when selected.
fn pill(ui: &mut Ui, text: &str, selected: bool, color: Color32, min_w: f32) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), TEXT);
    let w = (galley.size().x + 24.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        color
    } else if resp.hovered() {
        PANEL_HI
    } else {
        PANEL
    };
    painter.rect_filled(rect, 7.0_f32, fill);
    if !selected {
        painter.rect_stroke(rect, 7.0_f32, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, if selected { BG } else { TEXT });
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn energy_fader(ui: &mut Ui, setter: &ParamSetter, params: &MomotoneParams) {
    let (rect, resp) = ui.allocate_exact_size(vec2(360.0, 30.0), Sense::click_and_drag());
    let value_at = |p: Pos2| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    if resp.drag_started() {
        setter.begin_set_parameter(&params.energy);
    }
    if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            setter.set_parameter(&params.energy, value_at(p));
        }
    }
    if resp.drag_stopped() {
        setter.end_set_parameter(&params.energy);
    }
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            set(setter, &params.energy, value_at(p));
        }
    }
    if resp.double_clicked() {
        set(setter, &params.energy, 0.5);
    }

    let v = params.energy.value();
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0_f32, PANEL);
    painter.rect_stroke(rect, 7.0_f32, Stroke::new(1.0_f32, LINE), StrokeKind::Inside);
    let filled = Rect::from_min_max(rect.min, Pos2::new(rect.left() + rect.width() * v, rect.bottom()));
    painter.rect_filled(filled, 7.0_f32, ACCENT.gamma_multiply(0.85));
    let font = FontId::proportional(12.0);
    if v > 0.45 {
        painter.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, "laid back", font.clone(), BG);
    }
    if v < 0.75 {
        painter.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, "peak time", font, DIM);
    }
    // Value sits on the edge of the fill, in the colour that reads on its background.
    let edge = rect.left() + rect.width() * v;
    let text = format!("{:.0} %", v * 100.0);
    if v > 0.5 {
        painter.text(Pos2::new(edge - 8.0, rect.center().y), Align2::RIGHT_CENTER, text, FontId::proportional(14.0), BG);
    } else {
        painter.text(Pos2::new(edge + 8.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), TEXT);
    }
    resp.on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text("Drag to add ghost notes and fills. Double-click: 50 %");
}

fn idea_controls(ui: &mut Ui, setter: &ParamSetter, params: &MomotoneParams) {
    let cur = params.variation.value();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if pill(ui, "◀", false, ACCENT, 36.0).on_hover_text("Previous idea").clicked() {
            set(setter, &params.variation, if cur <= 1 { 500 } else { cur - 1 });
        }
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 30.0), Sense::hover());
        ui.painter().rect_filled(rect, 7.0_f32, PANEL);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, format!("#{cur}"), FontId::proportional(15.0), TEXT);
        if pill(ui, "▶", false, ACCENT, 36.0).on_hover_text("Next idea").clicked() {
            set(setter, &params.variation, if cur >= 500 { 1 } else { cur + 1 });
        }
        ui.add_space(8.0);
        if pill(ui, "New idea", true, ACCENT, 130.0).on_hover_text("Jump to a random idea in this style").clicked() {
            set(setter, &params.variation, random_idea(cur));
        }
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

fn chord_strip(ui: &mut Ui, plan: &Plan, data: &mut EditorData, shared: &UiShared) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    let n = plan.progression.len();
    let length = plan.length_beats();
    let now = shared.playing().then(|| plan.chord_index(shared.beat().rem_euclid(length)));
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let i = ((p.x - rect.left()) / rect.width() * n as f32).floor() as usize;
            data.selected_chord = i.min(n - 1);
        }
    }
    let painter = ui.painter();
    let w = rect.width() / n as f32;
    for (i, chord) in plan.progression.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + i as f32 * w, rect.top()), vec2(w, rect.height())).shrink(3.0);
        let selected = i == data.selected_chord;
        painter.rect_filled(r, 8.0_f32, if selected { PANEL_HI } else { PANEL });
        let stroke = if now == Some(i) {
            Stroke::new(2.0_f32, LIVE)
        } else if selected {
            Stroke::new(1.5_f32, ACCENT)
        } else {
            Stroke::new(1.0_f32, LINE)
        };
        painter.rect_stroke(r, 8.0_f32, stroke, StrokeKind::Inside);
        painter.text(r.center() - vec2(0.0, 7.0), Align2::CENTER_CENTER, chord.name(&plan.key), FontId::proportional(19.0), TEXT);
        painter.text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, &chord.symbol, FontId::proportional(11.0), DIM);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Click a chord to see what it does");
}

fn part_lane(
    ui: &mut Ui,
    setter: &ParamSetter,
    params: &MomotoneParams,
    pattern: &Pattern,
    layer: Layer,
    shared: &UiShared,
    follower: bool,
) {
    let (enabled, octave) = part_params(params, layer);
    // A follower always plays its part; the on/off switches belong to the main.
    let on = follower || enabled.value();
    let color = part_color(layer);
    let channel = if params.split_channels.value() { layer.channel() + 1 } else { 1 };

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(LEFT_W, LANE_H), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if pill(ui, layer.name(), on, color, 74.0).on_hover_text("Turn this part on or off").clicked() && !follower {
                set(setter, enabled, !on);
            }
            ui.label(RichText::new(format!("ch {channel}")).size(11.0).color(DIM));
            let o = octave.value();
            if pill(ui, "−", false, color, 24.0).on_hover_text("Octave down").clicked() && o > -3 {
                set(setter, octave, o - 1);
            }
            ui.label(RichText::new(format!("{o:+}")).size(12.0).color(if o == 0 { DIM } else { TEXT }));
            if pill(ui, "+", false, color, 24.0).on_hover_text("Octave up").clicked() && o < 3 {
                set(setter, octave, o + 1);
            }
        });

        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), LANE_H), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0_f32, PANEL);
        let length = pattern.length_beats.max(1.0);
        let x_at = |beat: f64| rect.left() + (beat / length) as f32 * rect.width();
        for b in 1..length as usize {
            let stroke = if b % 4 == 0 { Stroke::new(1.0_f32, LINE) } else { Stroke::new(1.0_f32, LINE.gamma_multiply(0.45)) };
            let x = x_at(b as f64);
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);
        }

        let notes: Vec<_> = pattern.notes.iter().filter(|n| n.layer == layer).collect();
        if let (Some(lo), Some(hi)) = (notes.iter().map(|n| n.pitch).min(), notes.iter().map(|n| n.pitch).max()) {
            let range = (hi - lo) as f32 + 1.0;
            let h = ((rect.height() - 8.0) / range).clamp(2.0, 8.0);
            let fill = if on { color } else { color.gamma_multiply(0.22) };
            for n in notes {
                let y = rect.bottom() - 4.0 - ((n.pitch - lo) as f32 + 0.5) / range * (rect.height() - 8.0);
                let r = Rect::from_min_max(
                    Pos2::new(x_at(n.start) + 1.0, y - h / 2.0),
                    Pos2::new((x_at(n.start + n.length) - 1.0).max(x_at(n.start) + 3.0), y + h / 2.0),
                );
                painter.rect_filled(r, 2.0_f32, fill.gamma_multiply(0.55 + 0.45 * n.velocity as f32 / 127.0));
            }
        }
        if !on {
            painter.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, "off", FontId::proportional(11.0), DIM);
        }
        if shared.playing() {
            let x = x_at(shared.beat().rem_euclid(length));
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.5_f32, TEXT));
        }
    });
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
    ui.horizontal(|ui| {
        let hint = "Saves each part as its own .mid, ready to drag onto your machine tracks";
        if pill(ui, "Export MIDI loop", true, ACCENT, 160.0).on_hover_text(hint).clicked() {
            let octaves = [params.oct_chords.value(), params.oct_bass.value(), params.oct_arp.value(), params.oct_melody.value()];
            let tempo = shared.tempo().unwrap_or(plan.tempo);
            data.status = Some(match export(plan, pattern, settings, octaves, tempo) {
                Ok(dir) => {
                    let msg = format!("Saved to {}", dir.display());
                    data.last_export = Some(dir);
                    (msg, true)
                }
                Err(e) => (format!("Export failed: {e}"), false),
            });
        }
        if let Some(dir) = data.last_export.clone() {
            if pill(ui, "Open folder", false, ACCENT, 0.0).clicked() {
                open_folder(&dir);
            }
        }
        match &data.status {
            Some((msg, ok)) => {
                ui.label(RichText::new(msg).size(12.0).color(if *ok { LIVE } else { Color32::from_rgb(0xe8, 0x5d, 0x5a) }));
            }
            None => {
                ui.label(
                    RichText::new("Tip: add ~/Music/Momotone to Live's Places, then drag parts onto tracks.")
                        .size(12.0)
                        .color(DIM),
                );
            }
        }
    });
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
fn export(plan: &Plan, pattern: &Pattern, s: &GenSettings, octaves: [i32; 4], tempo: f64) -> std::io::Result<PathBuf> {
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
    let dir = music_dir().join(name);
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
        let file = format!("{} {}.mid", layer.channel() + 1, layer.name());
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
        let tmp = std::env::temp_dir().join(format!("momotone-test-{}", std::process::id()));
        std::env::set_var("HOME", &tmp);
        let s = GenSettings { style: 2, key: 9, bars: 4, seed: 3, energy: 60, layers: Layers { arp: false, ..Layers::all() } };
        let plan = plan_for(&s);
        let pattern = generate(&plan, Layers::all(), s.seed);
        let dir = export(&plan, &pattern, &s, [0, -1, 0, 1], 72.0).unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        files.sort();
        assert_eq!(files, ["1 Chords.mid", "2 Bass.mid", "4 Melody.mid", "All parts.mid"]);
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
}
