//! Frequency-response charts with a logarithmic frequency axis, drawn directly
//! with the painter so they match the app's style.

use egui::epaint::{CornerRadius, Mesh};
use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

use crate::eq_design::EqDesign;
use crate::freq_point::{self, FreqPoint};
use crate::hearing_test::{Ear, HearingProfile};
use crate::reference;
use crate::staircase::Threshold;
use crate::theme::{self, Palette};

const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;
const LABELLED_HZ: [f32; 10] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
];

/// One line on a chart.
#[derive(Debug, Clone)]
pub struct Curve {
    pub name: String,
    pub color: Color32,
    /// The line, sorted by frequency.
    pub line: Vec<FreqPoint>,
    /// Dots drawn on top of the line, such as measured points.
    pub markers: Vec<FreqPoint>,
    /// Shade the area between the line and 0 dB.
    pub fill: bool,
}

/// Returns the colour for an ear, or for both ears tested together.
#[must_use]
pub fn ear_color(palette: &Palette, ear: Option<Ear>) -> Color32 {
    match ear {
        Some(Ear::Left) => palette.left_ear,
        Some(Ear::Right) => palette.right_ear,
        None => palette.accent,
    }
}

/// Plots the EQ for the ears in `visible`, or one curve if both ears were
/// tested together.
pub fn eq_chart(ui: &mut Ui, design: &EqDesign, ears_together: bool, visible: [bool; 2]) {
    let palette = Palette::of(ui);
    let freqs = freq_point::log_spaced(LOW_HZ, HIGH_HZ, 240);
    let mut curves = Vec::new();
    for (ear, name) in ears_to_draw(ears_together) {
        if !ears_together && !visible[ear as usize] {
            continue;
        }
        let eq = design.ear(ear);
        if eq.target.is_empty() {
            continue;
        }
        let color = ear_color(&palette, (!ears_together).then_some(ear));
        curves.push(Curve {
            name: name.to_owned(),
            color,
            line: freqs
                .iter()
                .zip(eq.response_curve_db(&freqs))
                .map(|(&hz, db)| FreqPoint::new(hz, db))
                .collect(),
            markers: Vec::new(),
            fill: true,
        });
    }
    frequency_chart(ui, &curves, 240.0);
}

/// Plots how sensitive each ear is in each band, compared with the listener's
/// own average, after removing the normal shape of human hearing.
pub fn sensitivity_chart(ui: &mut Ui, profile: &HearingProfile) {
    let palette = Palette::of(ui);
    let deviations: Vec<(Ear, FreqPoint)> = profile
        .measurements
        .iter()
        .filter_map(|m| match m.threshold {
            Threshold::Level(level) => Some((
                m.ear,
                FreqPoint::new(m.hz, level - reference::threshold_db(m.hz)),
            )),
            Threshold::BeyondMax => None,
        })
        .collect();
    let mut sorted: Vec<f32> = deviations.iter().map(|(_, p)| p.db).collect();
    sorted.sort_by(f32::total_cmp);
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
    let curves: Vec<Curve> = ears_to_draw(profile.ears_together)
        .into_iter()
        .map(|(ear, name)| {
            let points: Vec<FreqPoint> = deviations
                .iter()
                .filter(|(e, _)| *e == ear)
                .map(|(_, p)| FreqPoint::new(p.hz, median - p.db))
                .collect();
            Curve {
                name: name.to_owned(),
                color: ear_color(&palette, (!profile.ears_together).then_some(ear)),
                line: points.clone(),
                markers: points,
                fill: false,
            }
        })
        .collect();
    frequency_chart(ui, &curves, 200.0);
}

fn ears_to_draw(ears_together: bool) -> Vec<(Ear, &'static str)> {
    if ears_together {
        vec![(Ear::Left, "Both ears")]
    } else {
        vec![(Ear::Left, "Left"), (Ear::Right, "Right")]
    }
}

/// Draws curves on a chart with a log frequency axis and a dB axis that fits
/// the data (at least ±10 dB).
pub fn frequency_chart(ui: &mut Ui, curves: &[Curve], height: f32) {
    let palette = Palette::of(ui);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(36.0, 8.0),
        rect.max - Vec2::new(8.0, 24.0),
    );
    let extent = curves
        .iter()
        .flat_map(|c| c.line.iter().chain(&c.markers))
        .map(|p| p.db.abs())
        .fold(0.0, f32::max);
    let range_db = ((extent + 1.0) / 5.0).ceil().max(2.0) * 5.0;
    let step_db = if range_db > 15.0 { 10.0 } else { 5.0 };
    let x_of = |hz: f32| {
        let t = (hz / LOW_HZ).log10() / (HIGH_HZ / LOW_HZ).log10();
        plot.min.x + t.clamp(0.0, 1.0) * plot.width()
    };
    let y_of = |db: f32| {
        let t = (db.clamp(-range_db, range_db) + range_db) / (2.0 * range_db);
        plot.max.y - t * plot.height()
    };
    let painter = ui.painter_at(rect);
    let grid = Stroke::new(1.0, theme::fade(palette.border, 0.8));
    let label_font = theme::regular(11.0);

    // Horizontal grid and dB labels.
    let mut db = -range_db;
    while db <= range_db + 0.01 {
        let y = y_of(db);
        let stroke = if db == 0.0 {
            Stroke::new(1.0, palette.text_muted)
        } else {
            grid
        };
        painter.line_segment([Pos2::new(plot.min.x, y), Pos2::new(plot.max.x, y)], stroke);
        let label = if db > 0.0 {
            format!("+{db:.0}")
        } else {
            format!("{db:.0}")
        };
        painter.text(
            Pos2::new(plot.min.x - 8.0, y),
            Align2::RIGHT_CENTER,
            label,
            label_font.clone(),
            palette.text_muted,
        );
        db += step_db;
    }
    // Frequency labels, with faint lines at each decade.
    for hz in LABELLED_HZ {
        let x = x_of(hz);
        if [100.0, 1000.0, 10_000.0].contains(&hz) {
            painter.line_segment([Pos2::new(x, plot.min.y), Pos2::new(x, plot.max.y)], grid);
        }
        let align = if hz == LOW_HZ {
            Align2::LEFT_TOP
        } else if hz == HIGH_HZ {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            Pos2::new(x, plot.max.y + 6.0),
            align,
            format_hz(f64::from(hz)),
            label_font.clone(),
            palette.text_muted,
        );
    }

    let zero_y = y_of(0.0);
    for curve in curves {
        let points: Vec<Pos2> = curve
            .line
            .iter()
            .map(|p| Pos2::new(x_of(p.hz), y_of(p.db)))
            .collect();
        if curve.fill && points.len() > 1 {
            let mut mesh = Mesh::default();
            let fill = theme::fade(curve.color, 0.16);
            for (i, point) in points.iter().enumerate() {
                mesh.colored_vertex(*point, fill);
                mesh.colored_vertex(Pos2::new(point.x, zero_y), fill);
                if i > 0 {
                    let base = (i as u32 - 1) * 2;
                    mesh.add_triangle(base, base + 1, base + 2);
                    mesh.add_triangle(base + 1, base + 3, base + 2);
                }
            }
            painter.add(Shape::mesh(mesh));
        }
        painter.add(Shape::line(points, Stroke::new(2.5, curve.color)));
        for marker in &curve.markers {
            let centre = Pos2::new(x_of(marker.hz), y_of(marker.db));
            painter.circle_filled(centre, 4.5, palette.surface);
            painter.circle_filled(centre, 3.5, curve.color);
        }
    }

    // Hover readout: a vertical line and the value of every curve.
    let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) else {
        return;
    };
    let t = (pointer.x - plot.min.x) / plot.width();
    let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(t);
    painter.line_segment(
        [
            Pos2::new(pointer.x, plot.min.y),
            Pos2::new(pointer.x, plot.max.y),
        ],
        Stroke::new(1.0, palette.text_muted),
    );
    let mut lines = vec![(format_hz(f64::from(hz)), palette.text)];
    for curve in curves.iter().filter(|c| !c.line.is_empty()) {
        let db = freq_point::interpolate(&curve.line, hz);
        let y = y_of(db);
        painter.circle_filled(Pos2::new(pointer.x, y), 4.0, curve.color);
        lines.push((format!("{}  {:+.1} dB", curve.name, db), curve.color));
    }
    let galleys: Vec<_> = lines
        .into_iter()
        .map(|(text, color)| painter.layout_no_wrap(text, theme::regular(12.0), color))
        .collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
    let height: f32 = galleys.iter().map(|g| g.size().y).sum::<f32>() + 12.0;
    let left = if pointer.x + 12.0 + width < plot.max.x {
        pointer.x + 12.0
    } else {
        pointer.x - 12.0 - width
    };
    let tip = Rect::from_min_size(Pos2::new(left, plot.min.y + 4.0), Vec2::new(width, height));
    painter.rect_filled(tip, CornerRadius::same(8), palette.surface);
    painter.rect_stroke(
        tip,
        CornerRadius::same(8),
        Stroke::new(1.0, palette.border),
        StrokeKind::Inside,
    );
    let mut y = tip.min.y + 6.0;
    for galley in galleys {
        let row_height = galley.size().y;
        painter.galley(Pos2::new(tip.min.x + 8.0, y), galley, palette.text);
        y += row_height;
    }
}

/// Formats a frequency compactly, such as "250 Hz" or "2.5 kHz".
#[must_use]
pub fn format_hz(hz: f64) -> String {
    if hz >= 1000.0 {
        let khz = hz / 1000.0;
        if (khz - khz.round()).abs() < 0.05 {
            format!("{khz:.0} kHz")
        } else {
            format!("{khz:.1} kHz")
        }
    } else {
        format!("{hz:.0} Hz")
    }
}
