//! Reverb envelope on a canvas: early reflections followed by the late tail.

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{color, Color, Element, Length, Point, Rectangle, Renderer, Theme};

use crate::theme::palette as p;

const EARLY: Color = color!(0xb0475a);
const LATE: Color = color!(0x3fa371);
const FULL_HEIGHT: f32 = 160.0;
const MINI_HEIGHT: f32 = 60.0;
const DIM: f32 = 0.35;
const GRID_LINES: usize = 12;

pub struct ReverbGraph {
    decay: f32,
    blend: f32,
    enabled: bool,
    mini: bool,
}

impl ReverbGraph {
    /// `decay` and `blend` are both 0..1; blend runs from early to late.
    pub fn new(decay: f32, blend: f32) -> Self {
        Self {
            decay: decay.clamp(0.0, 1.0),
            blend: blend.clamp(0.0, 1.0),
            enabled: true,
            mini: false,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Strip thumbnail: envelope only.
    pub fn mini(mut self) -> Self {
        self.mini = true;
        self
    }
}

impl<Message> canvas::Program<Message> for ReverbGraph {
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
        let alpha = if self.enabled { 1.0 } else { DIM };
        let colour = |c: Color, a: f32| Color {
            a: c.a * a * alpha,
            ..c
        };
        let pad = if self.mini { 5.0 } else { 10.0 };
        let (left, top) = (pad, pad);
        let (width, height) = (bounds.width - 2.0 * pad, bounds.height - 2.0 * pad);
        let base = top + height;
        let at = |t: f32, level: f32| Point::new(left + width * t, base - height * level);

        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()),
            p::CHROME,
        );
        let grid = Stroke::default()
            .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            .with_width(1.0);
        for i in 0..=GRID_LINES {
            let t = i as f32 / GRID_LINES as f32;
            frame.stroke(&Path::line(at(t, 0.0), at(t, 1.0)), grid);
        }

        // Early triangle rises at once and falls; the late one ramps up then
        // tails off. Times are fractions of the width.
        let (e_on, e_end, e_h) = (0.03, 0.22, 0.9 - 0.5 * self.blend);
        let (l_on, l_peak, l_h) = (0.20, 0.30, 0.3 + 0.6 * self.blend);
        let l_end = l_peak + 0.22 + 0.5 * self.decay;
        let late = Path::new(|b| {
            b.move_to(at(l_on, 0.0));
            b.line_to(at(l_peak, l_h));
            b.line_to(at(l_end, 0.0));
            b.close();
        });
        let early = Path::new(|b| {
            b.move_to(at(e_on, 0.0));
            b.line_to(at(e_on, e_h));
            b.line_to(at(e_end, 0.0));
            b.close();
        });
        for (shape, tone) in [(&late, LATE), (&early, EARLY)] {
            frame.fill(shape, colour(tone, 0.55));
            frame.stroke(
                shape,
                Stroke::default()
                    .with_color(colour(tone, 1.0))
                    .with_width(1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}

impl<'a, Message: 'a> From<ReverbGraph> for Element<'a, Message> {
    fn from(graph: ReverbGraph) -> Self {
        let height = if graph.mini { MINI_HEIGHT } else { FULL_HEIGHT };
        canvas::Canvas::new(graph)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }
}
