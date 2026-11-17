//! Latency page: buffer settings, what the audio graph is doing, and who is
//! late. Every value sits in a fixed cell, so nothing moves as numbers change.

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::{column, container, row, scrollable, space, text, Column, Row};
use iced::{Alignment, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme};

use idctl_audio::graph::Interface;
use idctl_audio::stats::{buffer_ms, Driving, Party, Snapshot, HISTORY_SECS};
use idctl_audio::sysload::Memory;
use idctl_audio::tuning::{Tuning, BUFFERS};

use super::{key, trim_khz};
use crate::latency::{step_headroom, step_period, Msg, State};
use crate::theme::{self, caption, palette as p};
use crate::Message;

const GAP: f32 = 16.0;
const PADDING: f32 = 20.0;
const TILE_HEIGHT: f32 = 96.0;
const ROW_HEIGHT: f32 = 30.0;
const STRIP_HEIGHT: f32 = 56.0;
const SIZE_KEY: f32 = 120.0;
const STEP_KEY: f32 = 36.0;
const STEP_VALUE: f32 = 84.0;

/// Table columns after the name, which takes what is left.
const XRUNS: f32 = 90.0;
const LAST: f32 = 120.0;
const BAR: f32 = 240.0;
const PERCENT: f32 = 76.0;
const FLAG: f32 = 150.0;

/// Threads per row of the CPU grid, and the width of each cell.
const CORES_PER_ROW: usize = 8;
const CORE_CELL: f32 = 96.0;
const MEMORY_LABEL: f32 = 90.0;
const MEMORY_AMOUNT: f32 = 150.0;

const ABSENT: &str = "-";

pub fn view(state: &State) -> Element<'_, Message> {
    if state.down {
        return page(column![panel(
            "Latency",
            None,
            hint("PipeWire is not running. This page needs it."),
        )]);
    }
    let snapshot = state
        .snapshot
        .as_ref()
        .filter(|s| s.driving == Driving::Interface);
    page(
        column![
            buffer(state),
            tiles(state.interface.as_ref(), snapshot),
            system(state),
            sources(state, snapshot),
            panel(
                "Xruns per second, last 5 minutes",
                None,
                canvas::Canvas::new(History(state.history().collect()))
                    .width(Length::Fill)
                    .height(STRIP_HEIGHT)
                    .into(),
            ),
        ]
        .spacing(GAP),
    )
}

fn page<'a>(content: Column<'a, Message>) -> Element<'a, Message> {
    container(scrollable(content.width(Length::Fill)))
        .padding(28)
        .width(Length::Fill)
        .into()
}

/// A full-width titled panel, with an optional control at the title's right.
fn panel<'a>(
    title: &str,
    trailing: Option<Element<'a, Message>>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let mut head = row![container(caption(title, 12.0, p::TEXT)).width(Length::Fill)]
        .align_y(Alignment::Center)
        .height(28);
    if let Some(trailing) = trailing {
        head = head.push(trailing);
    }
    container(column![head, body].spacing(14))
        .padding(PADDING)
        .width(Length::Fill)
        .style(theme::card)
        .into()
}

fn hint<'a>(message: &str) -> Element<'a, Message> {
    text(message.to_owned()).size(13).color(p::MUTED).into()
}

/// A number that keeps its width whatever its digits.
fn number<'a>(value: String, size: f32, colour: Color) -> iced::widget::Text<'a> {
    text(value).size(size).font(Font::MONOSPACE).color(colour)
}

fn cell<'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
    align: Alignment,
) -> Element<'a, Message> {
    container(content)
        .width(width)
        .height(ROW_HEIGHT)
        .align_x(align)
        .align_y(Alignment::Center)
        .into()
}

// --- Buffer settings -------------------------------------------------------

fn buffer(state: &State) -> Element<'_, Message> {
    let ready = state.interface.is_some() && !state.busy;
    let size_key = |label: String, frames: Option<u32>| {
        let msg = Message::Latency(Msg::Buffer(frames));
        key(&label, state.buffer == frames, p::CLOCK, msg.clone())
            .on_press_maybe(ready.then_some(msg))
            .width(SIZE_KEY)
    };
    let keys = Row::with_children(
        std::iter::once(size_key("Off".into(), None).into()).chain(
            BUFFERS
                .iter()
                .map(|frames| size_key(frames.to_string(), Some(*frames)).into()),
        ),
    )
    .spacing(8);

    let status = match (&state.error, state.busy, state.buffer) {
        (Some(why), _, _) => text(why.clone()).color(p::MUTE_ON),
        (None, true, _) => text("Applying.").color(p::MUTED),
        (None, false, None) => {
            text("Off: each application chooses its own buffer, including your DAW's own setting.")
                .color(p::MUTED)
        }
        (None, false, Some(_)) => text(
            "On: this buffer applies to every application and overrides your DAW's own \
             setting. Off hands it back.",
        )
        .color(p::MUTED),
    };

    // The period and headroom that go with the buffer in use; with none,
    // those of the first size, greyed.
    let tuning = state.presets.get(state.buffer.unwrap_or(BUFFERS[0]));
    let editable = ready && state.buffer.is_some();
    let edit = |t: Tuning| editable.then_some(Message::Latency(Msg::Edit(t)));
    let frames = |v: Option<u32>| v.map_or(ABSENT.to_owned(), |f| f.to_string());
    let with_period = |up| {
        tuning.and_then(|t| {
            edit(Tuning {
                period: Some(step_period(t.period, up)),
                ..t
            })
        })
    };
    let with_headroom = |up| {
        tuning.and_then(|t| {
            edit(Tuning {
                headroom: Some(step_headroom(t.headroom, up)),
                ..t
            })
        })
    };
    let values = row![
        stepper(
            "Period",
            frames(tuning.and_then(|t| t.period)),
            with_period(false),
            with_period(true),
        ),
        stepper(
            "Headroom",
            frames(tuning.and_then(|t| t.headroom)),
            with_headroom(false),
            with_headroom(true),
        ),
        container(
            text(match state.buffer {
                Some(_) => "frames, tuned for the buffer in use",
                None => "frames; pick a buffer size to tune them",
            })
            .size(12)
            .color(p::MUTED)
        )
        .height(ROW_HEIGHT)
        .align_y(Alignment::Center),
    ]
    .spacing(28)
    .align_y(Alignment::End);

    panel(
        "Buffer (frames)",
        None,
        column![
            keys,
            container(status.size(13)).height(20),
            space().height(2),
            values
        ]
        .spacing(10)
        .into(),
    )
}

fn stepper<'a>(
    label: &str,
    value: String,
    down: Option<Message>,
    up: Option<Message>,
) -> Element<'a, Message> {
    let colour = if up.is_some() { p::TEXT } else { p::FAINT };
    let step = |sign: &str, msg: Option<Message>| {
        key(sign, false, p::CLOCK, Message::Tick)
            .on_press_maybe(msg)
            .width(STEP_KEY)
    };
    column![
        caption(label, 10.0, p::MUTED),
        row![
            step("-", down),
            cell(number(value, 16.0, colour), STEP_VALUE, Alignment::Center),
            step("+", up),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

// --- Now -------------------------------------------------------------------

fn tiles<'a>(interface: Option<&Interface>, s: Option<&Snapshot>) -> Element<'a, Message> {
    let headroom = |capture: bool| {
        interface
            .into_iter()
            .flat_map(|i| &i.nodes)
            .find(|n| n.capture == capture)
            .map(|n| n.headroom)
    };
    let ms = |v: f32| format!("{v:.1} ms");
    let out = s.map(|s| buffer_ms(s.quantum, headroom(false).unwrap_or(0), s.rate));
    let round = s
        .zip(out)
        .zip(headroom(true))
        .map(|((s, out), input)| out + buffer_ms(s.quantum, input, s.rate));
    let or_absent = |v: Option<String>| v.unwrap_or_else(|| ABSENT.to_owned());
    row![
        tile(
            "Sample rate",
            or_absent(s.map(|s| format!("{} kHz", trim_khz(s.rate)))),
            "graph rate"
        ),
        tile(
            "Buffer",
            or_absent(s.map(|s| s.quantum.to_string())),
            "frames per cycle"
        ),
        tile("Output", or_absent(out.map(ms)), "buffers only"),
        tile(
            "In + out",
            or_absent(round.map(ms)),
            if headroom(true).is_some() {
                "buffers only"
            } else {
                "no input in this card profile"
            },
        ),
    ]
    .spacing(GAP)
    .into()
}

fn tile<'a>(label: &str, value: String, note: &'a str) -> Element<'a, Message> {
    container(
        column![
            caption(label, 10.0, p::MUTED),
            number(value, 24.0, p::TEXT),
            text(note).size(11).color(p::FAINT),
        ]
        .spacing(8),
    )
    .padding(PADDING)
    .width(Length::Fill)
    .height(TILE_HEIGHT)
    .style(theme::card)
    .into()
}

// --- Who is late -----------------------------------------------------------

fn sources<'a>(state: &'a State, s: Option<&'a Snapshot>) -> Element<'a, Message> {
    let head = |label: &str, width: f32, align: Alignment| {
        cell(caption(label, 10.0, p::MUTED), width, align)
    };
    let header = Row::new()
        .push(
            container(caption("Source", 10.0, p::MUTED))
                .width(Length::Fill)
                .height(ROW_HEIGHT)
                .align_y(Alignment::Center),
        )
        .push(head("Xruns", XRUNS, Alignment::End))
        .push(head("Last", LAST, Alignment::End))
        .push(space().width(28))
        .push(head("Load", BAR, Alignment::Start))
        .push(head("Now", PERCENT, Alignment::End))
        .push(head("Peak", PERCENT, Alignment::End))
        .push(space().width(FLAG));

    let mut table = column![header].spacing(2);
    match s {
        Some(s) => {
            table = table.push(source(&s.interface, state.now));
            for app in &s.apps {
                table = table.push(source(app, state.now));
            }
            if s.apps.is_empty() {
                table = table.push(note_row("No application is playing through the interface."));
            }
        }
        None => table = table.push(note_row(&idle_reason(state))),
    }
    let reset = key(
        "Reset counts",
        false,
        p::CLOCK,
        Message::Latency(Msg::ResetCounts),
    )
    .width(150);
    panel("Who is late", Some(reset.into()), table.into())
}

fn idle_reason(state: &State) -> String {
    if state.interface.is_none() {
        return "PipeWire does not list an iD2+. Check that it is plugged in.".into();
    }
    match state.snapshot.as_ref().map(|s| &s.driving) {
        None => "Listening to PipeWire.".into(),
        Some(Driving::Other(name)) => format!(
            "The interface is not driving the graph (\"{name}\" is), so counts would point at \
             the wrong thing."
        ),
        Some(_) => "The interface is idle. Play something through it to see xruns and load.".into(),
    }
}

fn note_row<'a>(message: &str) -> Element<'a, Message> {
    container(hint(message))
        .height(ROW_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

fn percent(share: f32) -> String {
    format!("{:.0} %", share * 100.0)
}

fn source<'a>(who: &'a Party, now: u64) -> Element<'a, Message> {
    let over = who.load >= 1.0;
    // A listener's lateness is never heard, so it does not get the alarm colour.
    let alarm = if who.listener { p::MUTED } else { p::MUTE_ON };
    let hot = |on: bool, calm: Color| if on { alarm } else { calm };
    let flag = match (who.listener, over) {
        (true, _) => caption("listening only", 10.0, p::MUTED),
        (false, true) => caption("over the period", 10.0, p::MUTE_ON),
        (false, false) => caption("", 10.0, p::MUTED),
    };
    let last = match who.last_xrun {
        None => ABSENT.to_owned(),
        Some(at) => ago(now.saturating_sub(at)),
    };
    Row::new()
        .push(
            container(text(who.name.clone()).size(14).color(p::TEXT))
                .width(Length::Fill)
                .height(ROW_HEIGHT)
                .align_y(Alignment::Center)
                .clip(true),
        )
        .push(cell(
            number(who.xruns.to_string(), 16.0, hot(who.xruns > 0, p::TEXT)),
            XRUNS,
            Alignment::End,
        ))
        .push(cell(number(last, 12.0, p::MUTED), LAST, Alignment::End))
        .push(space().width(28))
        .push(cell(
            bar(who.load, BAR, if over { p::MUTE_ON } else { p::CLOCK }),
            BAR,
            Alignment::Start,
        ))
        .push(cell(
            number(percent(who.load), 14.0, hot(over, p::CLOCK)),
            PERCENT,
            Alignment::End,
        ))
        .push(cell(
            number(percent(who.peak), 12.0, hot(who.peak >= 1.0, p::MUTED)),
            PERCENT,
            Alignment::End,
        ))
        .push(cell(flag, FLAG, Alignment::End))
        .into()
}

/// A filled track `width` wide.
fn bar<'a>(share: f32, width: f32, colour: Color) -> Element<'a, Message> {
    let filled = (share.clamp(0.0, 1.0) * 100.0).round() as u16;
    let part = |portion: u16, colour: Color| {
        container(space())
            .width(Length::FillPortion(portion))
            .height(8)
            .style(theme::fill(colour))
    };
    let mut track = Row::new().width(width);
    if filled > 0 {
        track = track.push(part(filled, colour));
    }
    if filled < 100 {
        track = track.push(part(100 - filled, p::CHROME));
    }
    track.into()
}

/// Calm, busy, or about to run out.
fn heat(share: f32) -> Color {
    match share {
        s if s >= 0.9 => p::METER_HIGH,
        s if s >= 0.7 => p::METER_MID,
        _ => p::METER_LOW,
    }
}

// --- System load -----------------------------------------------------------

fn system(state: &State) -> Element<'_, Message> {
    let cores = state.sampler.cores();
    let body: Element<'_, Message> = if cores.is_empty() {
        hint("Reading the CPU.")
    } else {
        let mut grid = column![].spacing(12);
        for (n, chunk) in cores.chunks(CORES_PER_ROW).enumerate() {
            let mut line = Row::new().spacing(12);
            for (i, share) in chunk.iter().enumerate() {
                line = line.push(core(n * CORES_PER_ROW + i, *share));
            }
            // Short last row: keep the cells where the rows above put them.
            for _ in chunk.len()..CORES_PER_ROW {
                line = line.push(space().width(CORE_CELL));
            }
            grid = grid.push(line);
        }
        column![grid, memory(state.memory)].spacing(18).into()
    };
    let threads = (!cores.is_empty()).then(|| {
        text(format!("{} threads", cores.len()))
            .size(12)
            .color(p::MUTED)
            .into()
    });
    panel("System load", threads, body)
}

fn core<'a>(index: usize, share: f32) -> Element<'a, Message> {
    column![
        row![
            container(caption(&index.to_string(), 10.0, p::MUTED)).width(Length::Fill),
            number(percent(share), 12.0, heat(share)),
        ]
        .align_y(Alignment::Center),
        bar(share, CORE_CELL, heat(share)),
    ]
    .spacing(6)
    .width(CORE_CELL)
    .into()
}

fn gigabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn memory<'a>(memory: Option<Memory>) -> Element<'a, Message> {
    let (share, amount) = match memory {
        Some(m) => (
            m.used_share(),
            format!("{:.1} / {:.1} GB", gigabytes(m.used), gigabytes(m.total)),
        ),
        None => (0.0, ABSENT.to_owned()),
    };
    Row::new()
        .push(cell(
            caption("Memory", 10.0, p::MUTED),
            MEMORY_LABEL,
            Alignment::Start,
        ))
        .push(cell(bar(share, BAR, heat(share)), BAR, Alignment::Start))
        .push(space().width(14))
        .push(cell(
            number(amount, 12.0, p::TEXT),
            MEMORY_AMOUNT,
            Alignment::End,
        ))
        .push(cell(
            number(percent(share), 14.0, heat(share)),
            PERCENT,
            Alignment::End,
        ))
        .align_y(Alignment::Center)
        .into()
}

fn ago(secs: u64) -> String {
    match secs {
        0..=1 => "just now".to_owned(),
        2..=119 => format!("{secs} s ago"),
        _ => format!("{} min ago", secs / 60),
    }
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
