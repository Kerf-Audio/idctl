//! Rotary control drawn on a canvas: vertical drag, wheel, double-click to
//! reset. A dark disc with a gradient value arc.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use iced::alignment::Vertical;
use iced::mouse::{self, Button, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::widget::{canvas::path::Arc, Action};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Theme};

use super::{mix_colour, Taper};
use crate::theme::{palette as p, BOLD, SEMIBOLD};

const SWEEP_START: f32 = 0.75 * PI;
const SWEEP: f32 = 1.5 * PI;
const DRAG_PIXELS: f32 = 220.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, PartialEq)]
pub enum Style {
    /// The monitor knob: outer track, value arc, readout in the cap.
    Large,
    /// Strip knobs: small cap with a pointer, readout in the cap.
    Small,
    /// Pan knobs in the monitoring section.
    Mini,
}

pub struct Knob<'a, Message> {
    value: f32,
    min: f32,
    max: f32,
    default: f32,
    step: f32,
    taper: Taper,
    bipolar: bool,
    style: Style,
    readout: String,
    labels: (&'a str, &'a str, &'a str),
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

impl<'a, Message> Knob<'a, Message> {
    pub fn new(value: f32, range: (f32, f32), on_change: impl Fn(f32) -> Message + 'a) -> Self {
        Self {
            value: value.clamp(range.0, range.1),
            min: range.0,
            max: range.1,
            default: range.0,
            step: (range.1 - range.0) / 150.0,
            taper: Taper::Linear,
            bipolar: false,
            style: Style::Small,
            readout: format!("{value:.0}"),
            labels: ("", "", ""),
            on_change: Box::new(on_change),
        }
    }
    pub fn default(mut self, v: f32) -> Self {
        self.default = v;
        self
    }
    pub fn step(mut self, s: f32) -> Self {
        self.step = s;
        self
    }
    pub fn taper(mut self, t: Taper) -> Self {
        self.taper = t;
        self
    }
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn readout(mut self, r: String) -> Self {
        self.readout = r;
        self
    }
    /// Captions under the knob: minimum, maximum and unit.
    pub fn labels(mut self, min: &'a str, max: &'a str, unit: &'a str) -> Self {
        self.labels = (min, max, unit);
        self
    }

    fn position(&self) -> f32 {
        self.taper
            .position((self.value - self.min) / (self.max - self.min))
    }

    fn value_at(&self, position: f32) -> f32 {
        let v = self.min + self.taper.linear(position.clamp(0.0, 1.0)) * (self.max - self.min);
        (v / self.step).round() * self.step
    }
}

#[derive(Default)]
pub struct State {
    drag: Option<(f32, f32)>,
    last_click: Option<Instant>,
}

impl<Message> canvas::Program<Message> for Knob<'_, Message> {
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
        match event {
            mouse::Event::ButtonPressed(Button::Left) if cursor.is_over(bounds) => {
                let now = Instant::now();
                if state.last_click.is_some_and(|t| now - t < DOUBLE_CLICK) {
                    state.last_click = None;
                    state.drag = None;
                    return Some(Action::publish((self.on_change)(self.default)).and_capture());
                }
                state.last_click = Some(now);
                let y = cursor.position()?.y;
                state.drag = Some((y, self.position()));
                Some(Action::capture())
            }
            mouse::Event::CursorMoved { position } => {
                let (y0, p0) = state.drag?;
                let v = self.value_at(p0 + (y0 - position.y) / DRAG_PIXELS);
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
                let v = (self.value + lines.signum() * self.step).clamp(self.min, self.max);
                Some(Action::publish((self.on_change)(v)).and_capture())
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
        let large = self.style == Style::Large;
        let caption_h = if large { 34.0 } else { 20.0 };
        let dial = bounds.height - caption_h;
        let centre = Point::new(bounds.width / 2.0, dial / 2.0);
        let outer = (dial.min(bounds.width) / 2.0) - 2.0;
        let cap = if large { outer * 0.80 } else { outer * 0.78 };
        let pos = self.position();
        let angle = |t: f32| SWEEP_START + SWEEP * t;

        // Track ring.
        let track_w = if large { 7.0 } else { 3.0 };
        let track_r = outer - track_w / 2.0;
        frame.stroke(
            &arc(centre, track_r, angle(0.0), angle(1.0)),
            Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))
                .with_width(track_w),
        );

        // Value arc, drawn in short segments to fake a gradient.
        let (from, to) = if self.bipolar {
            (0.5_f32.min(pos), 0.5_f32.max(pos))
        } else {
            (0.0, pos)
        };
        let segments = ((to - from) * 60.0).ceil() as usize;
        for i in 0..segments {
            let a = from + (to - from) * i as f32 / segments as f32;
            let b = from + (to - from) * (i + 1) as f32 / segments as f32;
            let colour = mix_colour(p::ARC_START, p::ARC_END, (a + b) / 2.0);
            frame.stroke(
                &arc(centre, track_r, angle(a), angle(b) + 0.01),
                Stroke::default().with_color(colour).with_width(track_w),
            );
        }

        // Soft shadow, then the cap with a top-lit gradient.
        for (grow, alpha) in [(4.0, 0.10), (2.5, 0.18), (1.2, 0.28)] {
            frame.fill(
                &Path::circle(Point::new(centre.x, centre.y + 2.0), cap + grow),
                Color::from_rgba(0.0, 0.0, 0.0, alpha),
            );
        }
        let lit = canvas::gradient::Linear::new(
            Point::new(centre.x, centre.y - cap),
            Point::new(centre.x, centre.y + cap),
        )
        .add_stop(0.0, Color::from_rgb8(0x3b, 0x3d, 0x40))
        .add_stop(1.0, Color::from_rgb8(0x1b, 0x1c, 0x1e));
        frame.fill(&Path::circle(centre, cap), lit);
        frame.stroke(
            &Path::circle(centre, cap),
            Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.07))
                .with_width(1.0),
        );

        // Pointer: a white block at the arc end (large) or a tick on the cap.
        let a = angle(pos);
        let dir = (a.cos(), a.sin());
        if large {
            let r0 = track_r - track_w * 1.1;
            let r1 = track_r + track_w * 0.9;
            frame.stroke(
                &Path::line(
                    Point::new(centre.x + dir.0 * r0, centre.y + dir.1 * r0),
                    Point::new(centre.x + dir.0 * r1, centre.y + dir.1 * r1),
                ),
                Stroke::default().with_color(Color::WHITE).with_width(8.0),
            );
        } else {
            let r0 = cap * 0.70;
            let r1 = cap * 1.02;
            frame.stroke(
                &Path::line(
                    Point::new(centre.x + dir.0 * r0, centre.y + dir.1 * r0),
                    Point::new(centre.x + dir.0 * r1, centre.y + dir.1 * r1),
                ),
                Stroke::default().with_color(Color::WHITE).with_width(2.5),
            );
        }

        frame.fill_text(Text {
            content: self.readout.clone(),
            position: centre,
            color: p::TEXT,
            size: (if large { cap * 0.42 } else { cap * 0.52 }).into(),
            font: BOLD,
            align_x: Alignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });

        let caption = |frame: &mut Frame, s: &str, x: f32, y: f32, size: f32| {
            frame.fill_text(Text {
                content: s.to_string(),
                position: Point::new(x, y),
                color: p::MUTED,
                size: size.into(),
                font: SEMIBOLD,
                align_x: Alignment::Center,
                align_y: Vertical::Top,
                ..Text::default()
            });
        };
        let (lo, hi, unit) = self.labels;
        let size = if large { 13.0 } else { 9.0 };
        let y = dial - if large { 4.0 } else { 3.0 };
        let spread = outer * 0.72;
        caption(&mut frame, lo, centre.x - spread, y, size);
        caption(&mut frame, hi, centre.x + spread, y, size);
        caption(&mut frame, unit, centre.x, y + size + 1.0, size);

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

fn arc(center: Point, radius: f32, start: f32, end: f32) -> Path {
    Path::new(|b| {
        b.arc(Arc {
            center,
            radius,
            start_angle: Radians(start),
            end_angle: Radians(end),
        })
    })
}

impl<'a, Message: 'a> From<Knob<'a, Message>> for Element<'a, Message> {
    fn from(knob: Knob<'a, Message>) -> Self {
        let (w, h) = match knob.style {
            Style::Large => (150.0, 150.0),
            Style::Small => (72.0, 78.0),
            Style::Mini => (58.0, 60.0),
        };
        canvas::Canvas::new(knob)
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into()
    }
}
