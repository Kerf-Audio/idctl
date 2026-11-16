//! Four-band EQ response on a canvas: summed curve over a log frequency axis,
//! with draggable band handles in the full editor.

use iced::alignment::Vertical;
use iced::mouse::{self, Button, Cursor};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::widget::Action;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use idctl_protocol::dsp::{self, Band};

use super::mix_colour;
use crate::theme::{palette as p, SEMIBOLD};

const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;
/// Vertical axis half-range in dB.
const AXIS_DB: f32 = 12.0;
/// Handles can be dragged this far either side of 0 dB.
const GAIN_LIMIT_DB: f32 = 6.0;
const HANDLE_RADIUS: f32 = 9.0;
const HIT_RADIUS: f32 = 14.0;
const FULL_HEIGHT: f32 = 220.0;
const MINI_HEIGHT: f32 = 60.0;
const DIM: f32 = 0.35;

const GRID: [(f32, &str); 20] = [
    (20.0, "20"),
    (30.0, "30"),
    (40.0, "40"),
    (50.0, "50"),
    (60.0, "60"),
    (100.0, "100"),
    (200.0, "200"),
    (300.0, "300"),
    (400.0, "400"),
    (500.0, "500"),
    (800.0, "800"),
    (1_000.0, "1k"),
    (2_000.0, "2k"),
    (3_000.0, "3k"),
    (4_000.0, "4k"),
    (5_000.0, "5k"),
    (6_000.0, "6k"),
    (8_000.0, "8k"),
    (10_000.0, "10k"),
    (20_000.0, "20k"),
];
const DB_LABELS: [f32; 5] = [12.0, 6.0, 0.0, -6.0, -12.0];
const HANDLE_LABELS: [&str; 4] = ["LS", "", "", "HS"];

/// Called with `(band index, Hz, gain dB)`.
type OnChange<'a, Message> = Box<dyn Fn(usize, f32, f32) -> Message + 'a>;

pub struct EqGraph<'a, Message> {
    bands: [Band; 4],
    fs: f32,
    enabled: bool,
    mini: bool,
    on_change: Option<OnChange<'a, Message>>,
}

impl<'a, Message> EqGraph<'a, Message> {
    pub fn new(bands: [Band; 4], fs: f32) -> Self {
        Self {
            bands,
            fs,
            enabled: true,
            mini: false,
            on_change: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Strip thumbnail: curve only, no labels or handles.
    pub fn mini(mut self) -> Self {
        self.mini = true;
        self
    }

    /// Called with `(band index, Hz, gain dB)` while a handle is dragged.
    pub fn on_change(mut self, f: impl Fn(usize, f32, f32) -> Message + 'a) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    fn plot(&self, size: Size) -> Rectangle {
        if self.mini {
            Rectangle::new(
                Point::new(5.0, 5.0),
                Size::new(size.width - 10.0, size.height - 10.0),
            )
        } else {
            Rectangle::new(
                Point::new(30.0, 12.0),
                Size::new(size.width - 42.0, size.height - 32.0),
            )
        }
    }

    fn handle_at(&self, plot: Rectangle, position: Point) -> Option<usize> {
        self.bands
            .iter()
            .enumerate()
            .map(|(i, b)| (i, handle_point(plot, b).distance(position)))
            .filter(|&(_, d)| d <= HIT_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

#[derive(Default)]
pub struct State {
    dragging: Option<usize>,
}

fn x_of(plot: Rectangle, hz: f32) -> f32 {
    plot.x + plot.width * (hz / F_MIN).ln() / (F_MAX / F_MIN).ln()
}

fn y_of(plot: Rectangle, db: f32) -> f32 {
    plot.y + plot.height * (AXIS_DB - db) / (2.0 * AXIS_DB)
}

fn hz_at(plot: Rectangle, x: f32) -> f32 {
    let t = ((x - plot.x) / plot.width).clamp(0.0, 1.0);
    (F_MIN * (F_MAX / F_MIN).powf(t)).clamp(F_MIN, F_MAX)
}

fn db_at(plot: Rectangle, y: f32) -> f32 {
    (AXIS_DB - (y - plot.y) / plot.height * 2.0 * AXIS_DB).clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB)
}

fn handle_point(plot: Rectangle, band: &Band) -> Point {
    let db = band.gain_db.clamp(-AXIS_DB, AXIS_DB);
    Point::new(x_of(plot, band.hz.clamp(F_MIN, F_MAX)), y_of(plot, db))
}

fn label(frame: &mut Frame, content: &str, at: Point, size: f32, x: Alignment, y: Vertical) {
    frame.fill_text(Text {
        content: content.to_string(),
        position: at,
        color: p::MUTED,
        size: size.into(),
        font: SEMIBOLD,
        align_x: x,
        align_y: y,
        ..Text::default()
    });
}

impl<Message> canvas::Program<Message> for EqGraph<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let on_change = self.on_change.as_ref()?;
        let Event::Mouse(event) = event else {
            return None;
        };
        let plot = self.plot(bounds.size());
        match event {
            mouse::Event::ButtonPressed(Button::Left) if !self.mini => {
                let index = self.handle_at(plot, cursor.position_in(bounds)?)?;
                state.dragging = Some(index);
                Some(Action::capture())
            }
            mouse::Event::CursorMoved { .. } => {
                let index = state.dragging?;
                let at = cursor.position_in(bounds).or_else(|| {
                    cursor
                        .position()
                        .map(|c| Point::new(c.x - bounds.x, c.y - bounds.y))
                })?;
                let message = on_change(index, hz_at(plot, at.x), db_at(plot, at.y));
                Some(Action::publish(message).and_capture())
            }
            mouse::Event::ButtonReleased(Button::Left) => {
                state.dragging.take().map(|_| Action::capture())
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
        let plot = self.plot(bounds.size());
        let alpha = if self.enabled { 1.0 } else { DIM };

        if self.mini {
            frame.fill(
                &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()),
                p::CHROME,
            );
        }

        let grid = Color::from_rgba(1.0, 1.0, 1.0, 0.06);
        let stroke = |colour: Color| Stroke::default().with_color(colour).with_width(1.0);
        if !self.mini {
            for (hz, name) in GRID {
                let x = x_of(plot, hz);
                frame.stroke(
                    &Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                    stroke(grid),
                );
                label(
                    &mut frame,
                    name,
                    Point::new(x, plot.y + plot.height + 4.0),
                    9.0,
                    Alignment::Center,
                    Vertical::Top,
                );
            }
            for db in DB_LABELS {
                let y = y_of(plot, db);
                if db != 0.0 {
                    frame.stroke(
                        &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                        stroke(grid),
                    );
                }
                let text = if db > 0.0 {
                    format!("+{db}")
                } else {
                    format!("{db}")
                };
                label(
                    &mut frame,
                    &text,
                    Point::new(plot.x - 6.0, y),
                    9.0,
                    Alignment::Right,
                    Vertical::Center,
                );
            }
        }
        let zero = y_of(plot, 0.0);
        frame.stroke(
            &Path::line(
                Point::new(plot.x, zero),
                Point::new(plot.x + plot.width, zero),
            ),
            stroke(Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
        );

        let steps = if self.mini { 80 } else { 240 };
        let points: Vec<Point> = (0..=steps)
            .map(|i| {
                let hz = hz_at(plot, plot.x + plot.width * i as f32 / steps as f32);
                let db: f32 = self
                    .bands
                    .iter()
                    .map(|b| dsp::magnitude_db(b, hz, self.fs))
                    .sum();
                Point::new(x_of(plot, hz), y_of(plot, db.clamp(-AXIS_DB, AXIS_DB)))
            })
            .collect();

        let fill = Path::new(|b| {
            b.move_to(Point::new(plot.x, zero));
            for &pt in &points {
                b.line_to(pt);
            }
            b.line_to(Point::new(plot.x + plot.width, zero));
            b.close();
        });
        let left = with_alpha(p::ARC_END, alpha);
        let right = with_alpha(p::ARC_START, alpha);
        frame.fill(
            &fill,
            with_alpha(mix_colour(p::ARC_END, p::ARC_START, 0.5), 0.14 * alpha),
        );

        let line = Path::new(|b| {
            b.move_to(points[0]);
            for &pt in &points[1..] {
                b.line_to(pt);
            }
        });
        let gradient = canvas::gradient::Linear::new(
            Point::new(plot.x, 0.0),
            Point::new(plot.x + plot.width, 0.0),
        )
        .add_stop(0.0, left)
        .add_stop(1.0, right);
        frame.stroke(
            &line,
            Stroke {
                style: canvas::Style::Gradient(canvas::Gradient::Linear(gradient)),
                width: if self.mini { 1.5 } else { 2.5 },
                ..Stroke::default()
            },
        );

        if !self.mini {
            for (band, name) in self.bands.iter().zip(HANDLE_LABELS) {
                let centre = handle_point(plot, band);
                let t = (centre.x - plot.x) / plot.width;
                let colour = with_alpha(mix_colour(p::ARC_END, p::ARC_START, t), alpha);
                frame.fill(&Path::circle(centre, HANDLE_RADIUS), p::CHROME);
                frame.stroke(
                    &Path::circle(centre, HANDLE_RADIUS),
                    Stroke::default().with_color(colour).with_width(2.0),
                );
                frame.fill_text(Text {
                    content: name.to_string(),
                    position: centre,
                    color: with_alpha(p::TEXT, alpha),
                    size: 9.0.into(),
                    font: SEMIBOLD,
                    align_x: Alignment::Center,
                    align_y: Vertical::Center,
                    ..Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if state.dragging.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let over_handle = !self.mini
            && self.on_change.is_some()
            && cursor
                .position_in(bounds)
                .is_some_and(|at| self.handle_at(self.plot(bounds.size()), at).is_some());
        if over_handle {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

fn with_alpha(c: Color, alpha: f32) -> Color {
    Color {
        a: c.a * alpha,
        ..c
    }
}

impl<'a, Message: 'a> From<EqGraph<'a, Message>> for Element<'a, Message> {
    fn from(graph: EqGraph<'a, Message>) -> Self {
        let height = if graph.mini { MINI_HEIGHT } else { FULL_HEIGHT };
        canvas::Canvas::new(graph)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_mapping_round_trips() {
        let plot = Rectangle::new(Point::new(30.0, 12.0), Size::new(500.0, 180.0));
        for hz in [20.0, 100.0, 1_000.0, 20_000.0] {
            assert!(
                (hz_at(plot, x_of(plot, hz)) / hz - 1.0).abs() < 1e-3,
                "{hz}"
            );
        }
        for db in [-6.0, 0.0, 3.5, 6.0] {
            assert!((db_at(plot, y_of(plot, db)) - db).abs() < 1e-3, "{db}");
        }
        assert_eq!(db_at(plot, plot.y), GAIN_LIMIT_DB);
    }
}
