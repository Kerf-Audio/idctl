//! Settings page: digital I/O modes and Comfort Reverb.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};

use idctl_protocol::params::{reverb, system};

use super::key;
use crate::model::Model;
use crate::theme::{self, caption, palette as p};
use crate::widgets::{Knob, Taper};
use crate::Message;

pub fn view(m: &Model) -> Element<'_, Message> {
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
    let unit = |label: &'static str, addr| {
        let v = m.unit(addr);
        column![
            caption(label, 11.0, p::MUTED),
            Knob::new(v, (0.0, 1.0), move |x| Message::Unit(addr, x))
                .step(0.01)
                .readout(format!("{:.0}", v * 100.0))
                .labels("0", "100", "%"),
        ]
        .align_x(Alignment::Center)
    };
    let damping = m.hz(reverb::DAMPING).clamp(20.0, 20_000.0);

    let card = |title: &str, body: Element<'static, Message>| {
        container(column![caption(title, 12.0, p::TEXT), body].spacing(18))
            .padding(20)
            .style(theme::card)
    };
    container(
        column![
            card(
                "Digital I/O",
                column![
                    optical("Optical input", system::OPTICAL_INPUT_MODE),
                    optical("Optical output", system::OPTICAL_OUTPUT_MODE)
                ]
                .spacing(18)
                .into(),
            ),
            card(
                "Comfort reverb",
                row![
                    unit("Size", reverb::SIZE),
                    column![
                        caption("Damping", 11.0, p::MUTED),
                        Knob::new(damping, (20.0, 20_000.0), |x| Message::Hz(
                            reverb::DAMPING,
                            x
                        ))
                        .step(10.0)
                        .taper(Taper::Power(0.35))
                        .readout(format!("{:.1}", damping / 1000.0))
                        .labels("20", "20k", "kHz"),
                    ]
                    .align_x(Alignment::Center),
                    unit("Level", reverb::LEVEL),
                ]
                .spacing(28)
                .into(),
            ),
            text("Reverb is fed by the Reverb mix: choose it in the mix list to set sends.")
                .size(12)
                .color(p::MUTED),
        ]
        .spacing(20),
    )
    .padding(28)
    .width(Length::Fill)
    .into()
}
