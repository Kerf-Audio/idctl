//! Compressor transfer curve on a canvas: input level against output level.

use iced::alignment::Vertical;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::text::Alignment;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::{palette as p, SEMIBOLD};

const FLOOR_DB: f32 = -60.0;
const FULL_HEIGHT: f32 = 220.0;
const MINI_HEIGHT: f32 = 60.0;
const DIM: f32 = 0.35;
const DOTS: [f32; 2] = [2.0, 4.0];

pub struct CompGraph {
    threshold_db: f32,
    ratio: f32,
    makeup_db: f32,
    enabled: bool,
    mini: bool,
}

impl CompGraph {
    pub fn new(threshold_db: f32, ratio: f32, makeup_db: f32) -> Self {
        Self {
            threshold_db,
            ratio,
            makeup_db,
            enabled: true,
            mini: false,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Strip thumbnail: curve only, no labels.
    pub fn mini(mut self) -> Self {
        self.mini = true;
        self
    }

    /// Output level in dB for an input level in dB.
    fn output(&self, input: f32) -> f32 {
        let over = (input - self.threshold_db).max(0.0);
        input - over + over / self.ratio.max(1.0) + self.makeup_db
    }

    /// The square plot area, centred in `size`.
    fn plot(&self, size: Size) -> Rectangle {
        let (left, right, top, bottom) = if self.mini {
            (5.0, 5.0, 5.0, 5.0)
        } else {
            (30.0, 12.0, 14.0, 30.0)
        };
        let side = (size.width - left - right)
            .min(size.height - top - bottom)
            .max(1.0);
        let x = left + (size.width - left - right - side) / 2.0;
        Rectangle::new(Point::new(x, top), Size::new(side, side))
    }
}

fn x_of(plot: Rectangle, db: f32) -> f32 {
    plot.x + plot.width * (db - FLOOR_DB) / -FLOOR_DB
}

fn y_of(plot: Rectangle, db: f32) -> f32 {
    plot.y + plot.height * (-db / -FLOOR_DB)
}

fn clamp_db(db: f32) -> f32 {
    db.clamp(FLOOR_DB, 0.0)
}

fn label(frame: &mut Frame, content: &str, at: Point, x: Alignment, y: Vertical) {
    frame.fill_text(Text {
        content: content.to_string(),
        position: at,
        color: p::MUTED,
        size: 9.0.into(),
        font: SEMIBOLD,
        align_x: x,
        align_y: y,
        ..Text::default()
    });
}

impl<Message> canvas::Program<Message> for CompGraph {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let plot = self.plot(bounds.size());
        let alpha = if self.enabled { 1.0 } else { DIM };
        let colour = |c: Color, a: f32| Color {
            a: c.a * a * alpha,
            ..c
        };
        let right = plot.x + plot.width;
        let bottom = plot.y + plot.height;

        if self.mini {
            frame.fill(
                &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()),
                p::CHROME,
            );
        }
        let hairline = Stroke::default()
            .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            .with_width(1.0);
        if !self.mini {
            for step in 0..=6 {
                let db = FLOOR_DB + 10.0 * step as f32;
                let (x, y) = (x_of(plot, db), y_of(plot, db));
                frame.stroke(
                    &Path::line(Point::new(x, plot.y), Point::new(x, bottom)),
                    hairline,
                );
                frame.stroke(
                    &Path::line(Point::new(plot.x, y), Point::new(right, y)),
                    hairline,
                );
                let text = format!("{db}");
                label(
                    &mut frame,
                    &text,
                    Point::new(x, bottom + 4.0),
                    Alignment::Center,
                    Vertical::Top,
                );
                label(
                    &mut frame,
                    &text,
                    Point::new(plot.x - 5.0, y),
                    Alignment::Right,
                    Vertical::Center,
                );
            }
            label(
                &mut frame,
                "IN",
                Point::new(plot.x + plot.width / 2.0, bottom + 17.0),
                Alignment::Center,
                Vertical::Top,
            );
            label(
                &mut frame,
                "OUT",
                Point::new(plot.x, plot.y - 4.0),
                Alignment::Right,
                Vertical::Bottom,
            );
        } else {
            frame.stroke(&Path::rectangle(plot.position(), plot.size()), hairline);
        }

        let dotted = Stroke {
            line_dash: LineDash {
                segments: &DOTS,
                offset: 0,
            },
            ..Stroke::default()
                .with_color(colour(p::MUTED, 0.8))
                .with_width(1.0)
        };
        let th = clamp_db(self.threshold_db);
        let knee = Point::new(x_of(plot, th), y_of(plot, clamp_db(self.output(th))));
        let unity_end = Point::new(right, y_of(plot, 0.0));
        frame.stroke(
            &Path::line(Point::new(x_of(plot, th), y_of(plot, th)), unity_end),
            dotted,
        );
        frame.stroke(
            &Path::line(Point::new(knee.x, plot.y), Point::new(knee.x, bottom)),
            dotted,
        );

        let steps = if self.mini { 60 } else { 120 };
        let points: Vec<Point> = (0..=steps)
            .map(|i| {
                let input = FLOOR_DB - FLOOR_DB * (steps - i) as f32 / steps as f32;
                Point::new(x_of(plot, input), y_of(plot, clamp_db(self.output(input))))
            })
            .collect();
        let fill = Path::new(|b| {
            b.move_to(Point::new(plot.x, bottom));
            for &pt in &points {
                b.line_to(pt);
            }
            b.line_to(Point::new(right, bottom));
            b.close();
        });
        frame.fill(&fill, colour(p::MIC, 0.22));
        frame.stroke(
            &Path::new(|b| {
                b.move_to(points[0]);
                for &pt in &points[1..] {
                    b.line_to(pt);
                }
            }),
            Stroke::default()
                .with_color(colour(p::ARC_START, 1.0))
                .with_width(if self.mini { 1.5 } else { 2.5 }),
        );

        vec![frame.into_geometry()]
    }
}

impl<'a, Message: 'a> From<CompGraph> for Element<'a, Message> {
    fn from(graph: CompGraph) -> Self {
        let height = if graph.mini { MINI_HEIGHT } else { FULL_HEIGHT };
        canvas::Canvas::new(graph)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }
}
