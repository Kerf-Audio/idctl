//! Latency page: what the audio graph is doing, and who is late.

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use idctl_audio::graph::Interface;
use idctl_audio::stats::{buffer_ms, Driving, Party, Snapshot, HISTORY_SECS};

use super::{card, key, trim_khz};
use crate::latency::{Msg, State};
use crate::theme::{self, caption, palette as p};
use crate::Message;

const NAME: f32 = 220.0;
const BAR: f32 = 260.0;
const STRIP_HEIGHT: f32 = 56.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let body: Element<'_, Message> = if state.down {
        note("PipeWire is not running. This page needs it.")
    } else if state.interface.is_none() {
        note("PipeWire does not list an iD2+. Check that it is plugged in.")
    } else {
        match &state.snapshot {
            None => note("Listening to PipeWire."),
            Some(s) => match &s.driving {
                Driving::Idle => {
                    note("The interface is idle. Play something through it to see xruns and load.")
                }
                Driving::Other(name) => note(&format!(
                    "The interface is not driving the graph (\"{name}\" is). Counts below \
                     would point at the wrong thing, so none are shown."
                )),
                Driving::Interface => running(state, s),
            },
        }
    };
    container(scrollable(body))
        .padding(28)
        .width(Length::Fill)
        .into()
}

fn note<'a>(message: &str) -> Element<'a, Message> {
    card("Latency", text(message.to_owned()).color(p::MUTED).into())
}

fn running<'a>(state: &'a State, s: &'a Snapshot) -> Element<'a, Message> {
    column![
        card("Now", now(state.interface.as_ref(), s)),
        card("Xruns", xruns(s, state.now)),
        card("Load", load(s)),
        card(
            "Xruns per second, last 5 minutes",
            canvas::Canvas::new(History(state.history().collect()))
                .width(Length::Fill)
                .height(STRIP_HEIGHT)
                .into()
        ),
    ]
    .spacing(20)
    .into()
}

fn now<'a>(interface: Option<&Interface>, s: &Snapshot) -> Element<'a, Message> {
    let headroom = |capture: bool| {
        interface
            .into_iter()
            .flat_map(|i| &i.nodes)
            .find(|n| n.capture == capture)
            .map(|n| n.headroom)
    };
    let out = buffer_ms(s.quantum, headroom(false).unwrap_or(0), s.rate);
    let mut line = row![
        value(format!("{} kHz", trim_khz(s.rate))),
        value(format!("{} frames", s.quantum)),
        value(format!("{out:.1} ms out")),
    ]
    .spacing(28)
    .align_y(Alignment::Center);
    if let Some(input) = headroom(true) {
        let round = out + buffer_ms(s.quantum, input, s.rate);
        line = line.push(value(format!("{round:.1} ms in + out")));
    }
    line.push(text("(buffers only)").size(12).color(p::MUTED))
        .into()
}

fn value<'a>(s: String) -> Element<'a, Message> {
    text(s).size(18).color(p::TEXT).into()
}

fn xruns<'a>(s: &'a Snapshot, now: u64) -> Element<'a, Message> {
    let line = |who: &Party| {
        let colour = if who.xruns == 0 { p::TEXT } else { p::MUTE_ON };
        let last = match who.last_xrun {
            None => "none".to_owned(),
            Some(at) => ago(now.saturating_sub(at)),
        };
        row![
            text(who.name.clone()).color(p::TEXT).width(NAME),
            text(who.xruns.to_string()).size(18).color(colour).width(90),
            text(format!("last: {last}")).size(12).color(p::MUTED),
        ]
        .align_y(Alignment::Center)
    };
    let mut lines = column![line(&s.interface)].spacing(8);
    for app in &s.apps {
        lines = lines.push(line(app));
    }
    if s.apps.is_empty() {
        lines = lines.push(
            text("No application is playing through the interface.")
                .size(12)
                .color(p::MUTED),
        );
    }
    column![
        lines,
        key(
            "Reset counts",
            false,
            p::CLOCK,
            Message::Latency(Msg::ResetCounts)
        )
        .width(160),
    ]
    .spacing(16)
    .into()
}

fn ago(secs: u64) -> String {
    match secs {
        0..=1 => "just now".to_owned(),
        2..=119 => format!("{secs} s ago"),
        _ => format!("{} min ago", secs / 60),
    }
}

/// Share of each period spent processing; past 100 % the deadline is missed.
fn load<'a>(s: &'a Snapshot) -> Element<'a, Message> {
    let line = |who: &Party| {
        let over = who.load >= 1.0;
        let filled = (who.load.clamp(0.0, 1.0) * 100.0).round() as u16;
        let colour = if over { p::MUTE_ON } else { p::CLOCK };
        let bar = row![
            container(space())
                .width(Length::FillPortion(filled.max(1)))
                .height(8)
                .style(theme::fill(colour)),
            container(space())
                .width(Length::FillPortion((100 - filled).max(1)))
                .height(8)
                .style(theme::fill(p::CHROME)),
        ]
        .width(BAR);
        let mut line = row![
            text(who.name.clone()).color(p::TEXT).width(NAME),
            bar,
            text(format!("{:.0} %", who.load * 100.0))
                .color(colour)
                .width(70),
            text(format!("peak {:.0} %", who.peak * 100.0))
                .size(12)
                .color(if who.peak >= 1.0 {
                    p::MUTE_ON
                } else {
                    p::MUTED
                }),
        ]
        .spacing(14)
        .align_y(Alignment::Center);
        if over {
            line = line.push(caption("over the period", 10.0, p::MUTE_ON));
        }
        line
    };
    let mut lines = column![line(&s.interface)].spacing(10);
    for app in &s.apps {
        lines = lines.push(line(app));
    }
    lines.into()
}

/// One column per second, newest at the right.
struct History(Vec<u32>);

impl<Message> canvas::Program<Message> for History {
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
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()),
            p::CHROME,
        );
        let column = bounds.width / HISTORY_SECS as f32;
        let tallest = self.0.iter().copied().max().unwrap_or(0).max(1) as f32;
        let first = HISTORY_SECS - self.0.len();
        for (i, count) in self.0.iter().enumerate().filter(|(_, c)| **c > 0) {
            let height = (bounds.height - 8.0) * *count as f32 / tallest;
            frame.fill_rectangle(
                Point::new((first + i) as f32 * column, bounds.height - 4.0 - height),
                Size::new(column.max(1.0), height.max(2.0)),
                p::MUTE_ON,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn times_since_the_last_xrun() {
        assert_eq!(super::ago(0), "just now");
        assert_eq!(super::ago(14), "14 s ago");
        assert_eq!(super::ago(600), "10 min ago");
    }
}
