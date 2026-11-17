//! Channel fader with its level meter, drawn on a canvas: a meter capsule
//! with a dBFS scale, then a fader slot with its gain scale and a flat cap.
//! Relative drag, wheel, double-click to 0 dB.

use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::mouse::{self, Button, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::widget::Action;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use super::{mix_colour, Scale};
use crate::mix::OFF_DB;
use crate::theme::{palette as p, SEMIBOLD};

/// Gain scale: dB marks and their height fraction from the top.
pub const GAIN: Scale = Scale(&[
    (6.0, 0.0),
    (0.0, 0.23),
    (-5.0, 0.40),
    (-10.0, 0.49),
    (-15.0, 0.59),
    (-20.0, 0.68),
    (-30.0, 0.77),
    (-40.0, 0.86),
    (-50.0, 0.94),
    (OFF_DB, 1.0),
]);
const GAIN_LABELS: [&str; 10] = [
    "+6", "0", "-5", "10", "15", "20", "30", "40", "50", "\u{221e}",
];

/// Meter scale, evenly spaced marks as on the hardware's own ladder.
const METER_MARKS: [f32; 11] = [
    0.0, -3.0, -6.0, -9.0, -12.0, -15.0, -18.0, -21.0, -27.0, -36.0, -48.0,
];
const METER_FLOOR: f32 = -60.0;

const DOUBLE_CLICK: Duration = Duration::from_millis(350);
const CAP: Size = Size::new(30.0, 20.0);
const PAD_TOP: f32 = 18.0;
const PAD_BOTTOM: f32 = 22.0;

pub struct Fader<'a, Message> {
    value: f32,
    /// Up to two meter channels, in dBFS (`None` = silence).
    meter: Vec<Option<f32>>,
    accent: Color,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

impl<'a, Message> Fader<'a, Message> {
    pub fn new(
        value: f32,
        meter: Vec<Option<f32>>,
        on_change: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self {
            value,
            meter,
            accent: p::SELECTOR,
            on_change: Box::new(on_change),
        }
    }

    /// The colour of the line across the cap.
    pub fn accent(mut self, colour: Color) -> Self {
        self.accent = colour;
        self
    }
}

#[derive(Default)]
pub struct State {
    drag: Option<(f32, f32)>,
    last_click: Option<Instant>,
}

fn meter_fraction(db: Option<f32>) -> f32 {
    let Some(db) = db else { return 0.0 };
    if db >= 0.0 {
        return 1.0;
    }
    let step = 1.0 / METER_MARKS.len() as f32;
    for (i, w) in METER_MARKS.windows(2).enumerate() {
        if db >= w[1] {
            let t = (db - w[1]) / (w[0] - w[1]);
            return 1.0 - step * (i as f32 + 1.0 - t);
        }
    }
    let last = METER_MARKS[METER_MARKS.len() - 1];
    (step * (db - METER_FLOOR) / (last - METER_FLOOR)).max(0.0)
}

/// Smallest vertical gap between two scale labels.
const LABEL_GAP: f32 = 11.0;

/// Drops labels that would overlap on a short fader. Labels arrive top to
/// bottom; the first and last always stay so the scale keeps its ends.
fn thin<T>(labels: impl Iterator<Item = (f32, T)>) -> Vec<(f32, T)> {
    let mut kept: Vec<(f32, T)> = Vec::new();
    let mut pending = None;
    for (y, s) in labels {
        match kept.last() {
            Some(&(prev, _)) if y - prev < LABEL_GAP => pending = Some((y, s)),
            _ => {
                kept.push((y, s));
                pending = None;
            }
        }
    }
    if let Some((y, s)) = pending {
        while kept.len() > 1 && y - kept[kept.len() - 1].0 < LABEL_GAP {
            kept.pop();
        }
        kept.push((y, s));
    }
    kept
}

impl<Message> canvas::Program<Message> for Fader<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };
        let travel = bounds.height - PAD_TOP - PAD_BOTTOM;
        match event {
            mouse::Event::ButtonPressed(Button::Left) if cursor.is_over(bounds) => {
                let now = Instant::now();
                if state.last_click.is_some_and(|t| now - t < DOUBLE_CLICK) {
                    state.last_click = None;
                    state.drag = None;
                    return Some(Action::publish((self.on_change)(0.0)).and_capture());
                }
                state.last_click = Some(now);
                state.drag = Some((cursor.position()?.y, GAIN.fraction(self.value)));
                Some(Action::capture())
            }
            mouse::Event::CursorMoved { position } => {
                let (y0, f0) = state.drag?;
                let f = (f0 + (position.y - y0) / travel).clamp(0.0, 1.0);
                let v = (GAIN.value(f) * 4.0).round() / 4.0;
                (v != self.value).then(|| Action::publish((self.on_change)(v)).and_capture())
            }
            mouse::Event::ButtonReleased(Button::Left) => {
                state.drag.take().map(|_| Action::capture())
            }
            mouse::Event::WheelScrolled { delta } if cursor.is_over(bounds) => {
                let lines = match delta {
                    ScrollDelta::Lines { y, .. } => *y,
                    ScrollDelta::Pixels { y, .. } => y / 30.0,
                };
                if lines == 0.0 {
                    return None;
                }
                let f = (GAIN.fraction(self.value) - lines.signum() * 0.01).clamp(0.0, 1.0);
                let v = (GAIN.value(f) * 4.0).round() / 4.0;
                // Capture even at the end stop: an uncaptured wheel event
                // scrolls the mixer, which then keeps the wheel for itself.
                Some(if v == self.value {
                    Action::capture()
                } else {
                    Action::publish((self.on_change)(v)).and_capture()
                })
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let top = PAD_TOP;
        let travel = bounds.height - PAD_TOP - PAD_BOTTOM;
        let y_at = |f: f32| top + f * travel;
        let label = |frame: &mut Frame, s: &str, x: f32, y: f32, align: Alignment| {
            frame.fill_text(Text {
                content: s.to_string(),
                position: Point::new(x, y),
                color: p::MUTED,
                size: 9.0.into(),
                font: SEMIBOLD,
                align_x: align,
                align_y: Vertical::Center,
                ..Text::default()
            });
        };

        // Meter capsule.
        let well = Rectangle::new(Point::new(2.0, 2.0), Size::new(38.0, bounds.height - 4.0));
        frame.fill(
            &Path::rounded_rectangle(well.position(), well.size(), 8.0.into()),
            p::CHROME,
        );
        frame.stroke(
            &Path::rounded_rectangle(well.position(), well.size(), 8.0.into()),
            Stroke::default().with_color(p::STRIP_LINE).with_width(1.0),
        );
        // Clip lamp at the top of the capsule.
        let clipped = self.meter.iter().any(|m| m.is_some_and(|db| db >= -0.1));
        frame.fill(
            &Path::rounded_rectangle(Point::new(11.0, 8.0), Size::new(20.0, 6.0), 3.0.into()),
            if clipped {
                p::METER_HIGH
            } else {
                p::STRIP_LINE
            },
        );

        let bar_w = 4.0;
        let bars = self.meter.len().max(1) as f32;
        let bars_x = 9.0;
        for (i, level) in self.meter.iter().enumerate() {
            let x = bars_x + i as f32 * (bar_w + 2.0);
            let slot = Rectangle::new(Point::new(x, y_at(0.0)), Size::new(bar_w, travel));
            frame.fill_rectangle(slot.position(), slot.size(), p::STRIP);
            let f = meter_fraction(*level);
            let h = f * travel;
            let segments = (h / 2.0).ceil() as usize;
            for s in 0..segments {
                let y0 = y_at(1.0) - (s as f32 + 1.0) * 2.0;
                let frac = 1.0 - (y0 - top) / travel;
                let colour = if frac > 0.9 {
                    p::METER_HIGH
                } else if frac > 0.72 {
                    mix_colour(p::METER_MID, p::METER_HIGH, (frac - 0.72) / 0.18)
                } else {
                    mix_colour(p::METER_LOW, p::METER_MID, frac / 0.72)
                };
                frame.fill_rectangle(Point::new(x, y0.max(top)), Size::new(bar_w, 1.6), colour);
            }
        }
        let scale_x = bars_x + bars * (bar_w + 2.0) + 9.0;
        let meter_labels = METER_MARKS
            .iter()
            .map(|&db| {
                (
                    y_at(1.0 - meter_fraction(Some(db))),
                    ((-db) as i32).to_string(),
                )
            })
            .chain([(y_at(1.0), "\u{221e}".to_string())]);
        for (y, s) in thin(meter_labels) {
            label(&mut frame, &s, scale_x, y, Alignment::Center);
        }

        // Fader slot and gain scale.
        let slot_x = 62.0;
        frame.fill_rectangle(
            Point::new(slot_x - 2.0, y_at(0.0)),
            Size::new(4.0, travel),
            p::STRIP_LINE,
        );
        let gain_labels = GAIN
            .0
            .iter()
            .zip(GAIN_LABELS)
            .map(|(&(_, f), s)| (y_at(f), s));
        for (y, text) in thin(gain_labels) {
            frame.fill_rectangle(
                Point::new(slot_x + 10.0, y - 0.5),
                Size::new(4.0, 1.0),
                p::FAINT,
            );
            label(&mut frame, text, slot_x + 30.0, y, Alignment::Right);
        }

        // Cap.
        let y = y_at(GAIN.fraction(self.value));
        let cap = Rectangle::new(
            Point::new(slot_x - CAP.width / 2.0, y - CAP.height / 2.0),
            CAP,
        );
        let shape = Path::rounded_rectangle(cap.position(), cap.size(), 4.0.into());
        frame.fill(&shape, p::BUTTON_TOP);
        frame.stroke(
            &shape,
            Stroke::default()
                .with_color(Color::from_rgb8(0x3a, 0x4a, 0x60))
                .with_width(1.0),
        );
        frame.fill_rectangle(
            Point::new(cap.x + 5.0, y - 1.0),
            Size::new(cap.width - 10.0, 2.0),
            self.accent,
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: 'a> From<Fader<'a, Message>> for Element<'a, Message> {
    fn from(fader: Fader<'a, Message>) -> Self {
        canvas::Canvas::new(fader)
            .width(Length::Fixed(100.0))
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_fraction_is_monotonic_and_bounded() {
        let mut last = -1.0;
        for db in (-80..=3).map(|d| d as f32) {
            let f = meter_fraction(Some(db));
            assert!((0.0..=1.0).contains(&f) && f >= last, "{db}");
            last = f;
        }
        assert_eq!(meter_fraction(None), 0.0);
    }

    #[test]
    fn thin_keeps_ends_and_spacing() {
        let ys = [0.0, 4.0, 12.0, 20.0, 26.0, 30.0];
        let kept: Vec<f32> = thin(ys.iter().map(|&y| (y, ())))
            .into_iter()
            .map(|l| l.0)
            .collect();
        assert_eq!(kept, [0.0, 12.0, 30.0]);
        assert!(kept.windows(2).all(|w| w[1] - w[0] >= LABEL_GAP));
    }
}
