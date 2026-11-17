//! Modal confirmation dialogs.

use iced::widget::{column, container, opaque, row, text};
use iced::{Color, Element, Length};

use super::key;
use crate::theme::{self, palette as p, BOLD};
use crate::Message;

/// Ask before phantom power goes onto a mic input.
pub fn phantom<'a>(ch: u8) -> Element<'a, Message> {
    let panel = container(
        column![
            text(format!("Switch on +48V for MIC {}?", ch + 1))
                .size(18)
                .font(BOLD)
                .color(p::TEXT),
            text("Phantom power can damage ribbon microphones and some other equipment. Check what is connected first.")
                .size(13)
                .color(p::MUTED),
            row![
                key("Cancel", false, p::LIT_TOP, Message::PhantomAnswer(false)),
                key("Switch on", true, p::MUTE_ON, Message::PhantomAnswer(true)),
            ]
            .spacing(10),
        ]
        .spacing(14)
        .width(380),
    )
    .padding(20)
    .style(theme::card);
    opaque(
        container(panel)
            .center(Length::Fill)
            .style(theme::fill(Color {
                a: 0.7,
                ..p::CHROME
            })),
    )
}
