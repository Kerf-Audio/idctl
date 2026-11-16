//! Channel strips: header, a scrolling stack of processor cards, then the
//! monitoring section for the selected mix. Strips are laid out in stereo
//! pairs so the link key can sit between them.

use iced::widget::{column, container, row, scrollable, space, text, Column};
use iced::{Alignment, Color, Element, Length};

use super::{cards, key, layout};
use crate::mix::NODES;
use crate::model::{channel, Group, Model};
use crate::theme::{self, caption, palette as p, BOLD};
use crate::widgets::knob::Style;
use crate::widgets::{Fader, Knob};
use crate::{App, Message};

/// The reverb return pair, nodes 24 and 25.
const REVERB_PAIR: usize = NODES / 2 - 1;

pub fn view(app: &App) -> Element<'_, Message> {
    let pairs = row((0..NODES / 2).map(|pair| pair_view(app, pair)));
    column![container(
        scrollable(pairs).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(6).scroller_width(6)
        ))
    )
    .height(Length::Fill),]
    .into()
}

fn group_colour(g: Group) -> Color {
    match g {
        Group::Mic => p::MIC,
        Group::Optical => p::OPTICAL,
        Group::Computer => p::COMPUTER,
        Group::Reverb => p::REVERB,
    }
}

fn pair_view(app: &App, pair: usize) -> Element<'_, Message> {
    let (m, mix) = (&app.model, app.mix);
    let nodes = [2 * pair, 2 * pair + 1];
    let both = |f: &dyn Fn(usize) -> Element<'static, Message>| row(nodes.map(f));
    let linked = m.mixer.linked(pair);
    let link = container(
        key(
            if linked { "Stereo" } else { "Mono" },
            linked,
            p::LIT_TOP,
            Message::Link(pair),
        )
        .width(76),
    )
    .center_x(2.0 * layout::STRIP)
    .height(layout::LINK_ROW);

    let monitoring = column![
        both(&|n| container(pan(m, mix, n))
            .center_x(layout::STRIP)
            .height(layout::PAN_ROW)
            .into()),
        link,
        both(&|n| mute_solo(m, mix, n)),
        row(nodes.map(|n| container(fader(m, mix, n))
            .center_x(layout::STRIP)
            .height(Length::Fill)
            .into())),
    ];
    let sections: Element<'_, Message> = if pair == REVERB_PAIR {
        cards::reverb_return(m, mix)
    } else {
        row(nodes.map(|n| cards::section(app, n))).into()
    };
    container(
        column![
            row(nodes.map(header)),
            container(
                scrollable(sections).direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
            )
            .height(layout::SECTIONS),
            monitoring_bar(pair == 0),
            monitoring.height(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .style(theme::strip)
    .into()
}

fn header(node: usize) -> Element<'static, Message> {
    let ch = channel(node);
    let colour = group_colour(ch.group);
    column![
        container(space())
            .width(Length::Fill)
            .height(5)
            .style(theme::fill(colour)),
        column![
            text(ch.number.to_string())
                .size(26)
                .font(BOLD)
                .color(p::TEXT),
            caption(&format!("iD2+ \u{00b7} {}", ch.source), 10.0, colour),
            space().height(4),
            caption(&ch.name, 12.0, p::TEXT),
        ]
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .padding([4, 0]),
    ]
    .width(layout::STRIP)
    .height(layout::STRIP_HEADER)
    .into()
}

fn monitoring_bar(first: bool) -> Element<'static, Message> {
    let line = container(space())
        .width(Length::Fill)
        .height(1)
        .style(theme::fill(p::MONITORING));
    let content: Element<'static, Message> = if first {
        row![caption("Monitoring", 11.0, p::TEXT), line]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    } else {
        line.into()
    };
    container(content)
        .padding([0, 12])
        .center_y(layout::MONITORING_BAR)
        .width(2.0 * layout::STRIP)
        .style(theme::fill(p::CHROME))
        .into()
}

fn pan(m: &Model, mix: usize, node: usize) -> Element<'static, Message> {
    let pan = m.mixer.send(node, mix).pan;
    let readout = match (pan * 100.0).round() as i32 {
        0 => "C".to_string(),
        v if v < 0 => format!("L{}", -v),
        v => format!("R{v}"),
    };
    Knob::new(pan, (-1.0, 1.0), move |v| Message::Pan(node, v))
        .default(0.0)
        .step(0.01)
        .bipolar()
        .style(Style::Mini)
        .readout(readout)
        .labels("L", "R", "")
        .into()
}

fn mute_solo(m: &Model, mix: usize, node: usize) -> Element<'static, Message> {
    let s = m.mixer.send(node, mix);
    container(
        row![
            key("Mute", s.mute, p::MUTE_ON, Message::Mute(node)),
            key("Solo", s.solo, p::SOLO_ON, Message::Solo(node))
        ]
        .spacing(8),
    )
    .padding([0, 12])
    .width(layout::STRIP)
    .center_y(layout::BUTTON_ROW)
    .into()
}

fn fader(m: &Model, mix: usize, node: usize) -> Element<'static, Message> {
    let level = m.mixer.send(node, mix).level;
    Column::new()
        .push(Fader::new(level, vec![m.source_level(node)], move |v| {
            Message::Level(node, v)
        }))
        .push(
            container(caption(&db_text(level), 10.0, p::MUTED))
                .center_x(Length::Fill)
                .padding([4, 0]),
        )
        .width(layout::STRIP)
        .padding([6, 0])
        .align_x(Alignment::Center)
        .into()
}

pub fn db_text(db: f32) -> String {
    if db <= crate::mix::OFF_DB + 0.01 {
        "-\u{221e}".into()
    } else {
        format!("{db:+.1}")
    }
}
