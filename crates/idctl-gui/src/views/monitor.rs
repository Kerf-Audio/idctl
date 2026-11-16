//! Right-hand panel: monitor controller, headphones, mix selection and the
//! selected mix's master fader.

use std::fmt;

use iced::widget::{column, container, pick_list, row, space, Column};
use iced::{Alignment, Element, Length};

use idctl_protocol::params::{mixer, monitor, output};

use super::strips::db_text;
use super::{key, layout};
use crate::model::Model;
use crate::theme::{self, caption, palette as p};
use crate::watch::PHONES;
use crate::widgets::knob::Style;
use crate::widgets::{Fader, Knob, Taper};
use crate::Message;

/// Volume knob taper: -36 dB sits a third of the way round.
pub(super) const VOLUME_TAPER: Taper = Taper::Power(3.4);

pub const MIX_NAMES: [&str; 4] = ["Main mix", "Mix A", "Mix B", "Reverb"];

#[derive(Clone, Copy, PartialEq, Eq)]
struct Source(&'static str, u8);

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

const SOURCES: [Source; 4] = [
    Source("Main Mix", 44),
    Source("Mix A", 46),
    Source("Mix B", 48),
    Source("DAW 1-2", 12),
];

pub fn view(m: &Model, mix: usize) -> Element<'_, Message> {
    let volume = m.db(monitor::VOLUME);
    let flag = |label: &str, addr| {
        let on = m.flag(addr);
        key(label, on, p::LIT_TOP, Message::Flag(addr, !on))
    };
    let source = m.byte(monitor::SOURCE);
    let phones = m.db(output::level(PHONES[0]));

    let monitor = column![
        caption("Monitor", 13.0, p::MUTED),
        Knob::new(volume, (-128.0, 0.0), |v| Message::Db(monitor::VOLUME, v))
            .default(-128.0)
            .step(1.0)
            .taper(VOLUME_TAPER)
            .style(Style::Large)
            .readout(format!("{volume:.0}"))
            .labels("-128", "0", "dB"),
        row![
            flag("Mute", monitor::MUTE),
            flag("Dim", monitor::DIM),
            key(
                "Ref",
                false,
                p::LIT_TOP,
                Message::Flag(monitor::SET_TO_REF, true)
            )
        ]
        .spacing(8),
        row![
            flag("\u{25cb}", monitor::MONO),
            flag("M/S", monitor::MID_SIDE),
            talkback(m)
        ]
        .spacing(8),
        caption("Input source", 10.0, p::MUTED),
        pick_list(SOURCES, SOURCES.into_iter().find(|s| s.1 == source), |s| {
            Message::Byte(monitor::SOURCE, s.1)
        })
        .style(theme::select)
        .menu_style(theme::menu)
        .text_size(12)
        .width(Length::Fill),
        caption("Headphones", 10.0, p::MUTED),
        Knob::new(phones, (-128.0, 0.0), Message::Phones)
            .default(-128.0)
            .step(1.0)
            .taper(VOLUME_TAPER)
            .readout(format!("{phones:.0}"))
            .labels("-128", "0", "dB"),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .padding([12, 16]);

    container(column![monitor, mixes(m, mix)].height(Length::Fill))
        .width(layout::MONITOR_PANEL)
        .height(Length::Fill)
        .style(theme::fill(p::CHROME))
        .into()
}

/// TB lights for the hardware button as well as for our own switch; the unit
/// dims the monitors itself while talkback is on.
fn talkback(m: &Model) -> Element<'_, Message> {
    let on = m.flag(monitor::TALKBACK_STATE) || m.flag(monitor::TALKBACK);
    key("TB", on, p::LIT_TOP, Message::Flag(monitor::TALKBACK, !on)).into()
}

/// Mix list; the selected mix shows its master fader and bus meters.
fn mixes(m: &Model, selected: usize) -> Element<'_, Message> {
    let mut list = Column::new().height(Length::Fill);
    for (mix, name) in MIX_NAMES.into_iter().enumerate() {
        let active = mix == selected;
        let label = caption(name, 12.0, if active { p::MONITORING } else { p::MUTED });
        let title = iced::widget::button(container(label).padding([6, 16]).width(Length::Fill))
            .padding(0)
            .style(theme::tab(false))
            .on_press(Message::SelectMix(mix));
        if !active {
            list = list.push(title);
            continue;
        }
        let master = m.db(mixer::master(2 * mix as u8));
        let meter = vec![m.bus_level(2 * mix), m.bus_level(2 * mix + 1)];
        list = list
            .push(
                container(space())
                    .width(Length::Fill)
                    .height(4)
                    .style(theme::fill(p::MONITORING)),
            )
            .push(title)
            .push(container(caption("Mix master", 9.0, p::MUTED)).padding([0, 16]))
            .push(
                container(Fader::new(master, meter, move |v| Message::Master(mix, v)))
                    .center_x(Length::Fill)
                    .height(Length::Fill)
                    .padding([4, 0]),
            )
            .push(
                container(caption(&db_text(master), 10.0, p::MUTED))
                    .center_x(Length::Fill)
                    .padding(iced::Padding::ZERO.bottom(8)),
            );
    }
    container(list.spacing(2).align_x(Alignment::Start))
        .height(Length::Fill)
        .into()
}
