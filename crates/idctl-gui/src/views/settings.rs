//! Settings page: sample rate, headphones and digital I/O modes.

use iced::widget::{column, container, row, scrollable, text, Row};
use iced::{Alignment, Element, Length};

use idctl_protocol::params::{output, system};

use super::monitor::VOLUME_TAPER;
use super::{key, trim_khz};
use crate::clock::RATES;
use crate::model::Model;
use crate::theme::{self, caption, palette as p};
use crate::watch::PHONES;
use crate::widgets::Knob;
use crate::{App, Message};

/// Headphone sources: label and the left sample-bus id of the stereo pair.
const PHONES_SOURCES: [(&str, u8); 4] =
    [("Main", 59), ("Mix A", 46), ("Mix B", 48), ("DAW 5-6", 16)];

pub fn view(app: &App) -> Element<'_, Message> {
    let m = &app.model;
    container(scrollable(
        column![
            card("Sample rate", clock(app)),
            card("Headphones", phones(m)),
            card("Digital I/O", digital(m)),
            card("Talkback", talkback(app)),
        ]
        .spacing(20),
    ))
    .padding(28)
    .width(Length::Fill)
    .into()
}

fn card<'a>(title: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![caption(title, 12.0, p::TEXT), body].spacing(18))
        .padding(20)
        .style(theme::card)
        .into()
}

fn clock(app: &App) -> Element<'_, Message> {
    let current = app.model.sample_rate();
    let keys = Row::with_children(RATES.map(|hz| {
        key(
            &trim_khz(hz),
            hz == current,
            p::CLOCK,
            Message::SampleRate(hz),
        )
        .into()
    }))
    .spacing(8)
    .width(480);
    let note = match &app.clock_error {
        Some(why) => text(why.clone()).color(p::MUTE_ON),
        None => {
            text("kHz. Set through PipeWire. EQ and compressor run up to 96 kHz.").color(p::MUTED)
        }
    };
    column![keys, note.size(12)].spacing(10).into()
}

fn phones(m: &Model) -> Element<'_, Message> {
    let level = m.db(output::level(PHONES[0]));
    let source = m.byte(output::source(PHONES[0]));
    let keys = Row::with_children(PHONES_SOURCES.map(|(label, base)| {
        key(
            label,
            source == base,
            p::LIT_TOP,
            Message::PhonesSource(base),
        )
        .into()
    }))
    .spacing(8)
    .width(360);
    row![
        column![
            caption("Level", 11.0, p::MUTED),
            Knob::new(level, (-128.0, 0.0), Message::Phones)
                .default(-128.0)
                .step(1.0)
                .taper(VOLUME_TAPER)
                .readout(format!("{level:.0}"))
                .labels("-128", "0", "dB"),
        ]
        .align_x(Alignment::Center),
        column![caption("Source", 11.0, p::MUTED), keys].spacing(8),
    ]
    .spacing(32)
    .align_y(Alignment::Center)
    .into()
}

fn digital(m: &Model) -> Element<'_, Message> {
    let optical = |label: &str, addr| {
        let mode = m.byte(addr);
        column![
            caption(label, 11.0, p::MUTED),
            row![
                key("ADAT", mode == 0, p::LIT_TOP, Message::Byte(addr, 0)),
                key("S/PDIF", mode == 1, p::LIT_TOP, Message::Byte(addr, 1)),
            ]
            .spacing(8)
            .width(220),
        ]
        .spacing(8)
    };
    column![
        optical("Optical input", system::OPTICAL_INPUT_MODE),
        optical("Optical output", system::OPTICAL_OUTPUT_MODE)
    ]
    .spacing(18)
    .into()
}

/// The mic TB sends to Mix A and Mix B while it is held on.
fn talkback(app: &App) -> Element<'_, Message> {
    let current = app.talkback.source;
    let choice = |label: &str, source: Option<usize>| {
        key(
            label,
            current == source,
            p::LIT_TOP,
            Message::TalkbackSource(source),
        )
    };
    column![
        row![
            choice("Off", None),
            choice("Mic 1", Some(0)),
            choice("Mic 2", Some(1))
        ]
        .spacing(8)
        .width(330),
        text("While TB is on, this mic goes to Mix A and Mix B at 0 dB and the monitors dim.")
            .size(12)
            .color(p::MUTED),
    ]
    .spacing(10)
    .into()
}
