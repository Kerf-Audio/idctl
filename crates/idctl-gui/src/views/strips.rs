//! Channel strips: header, input card (preamps only), then the monitoring
//! section for the selected mix. Strips are laid out in stereo pairs so the
//! link key can sit between them.

use iced::widget::{column, container, row, scrollable, space, text, Column};
use iced::{Alignment, Color, Element, Length};

use idctl_protocol::params::input;

use super::{key, layout};
use crate::mix::NODES;
use crate::model::{channel, Group, Model};
use crate::theme::{self, caption, palette as p, BOLD};
use crate::widgets::knob::Style;
use crate::widgets::{Fader, Knob};
use crate::Message;

pub fn view(m: &Model, mix: usize) -> Element<'_, Message> {
    let pairs = row((0..NODES / 2).map(|pair| pair_view(m, mix, pair)));
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

fn pair_view(m: &Model, mix: usize, pair: usize) -> Element<'_, Message> {
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
    container(
        column![
            row(nodes.map(header)),
            row(nodes.map(|n| input_card(m, n))),
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

fn input_card(m: &Model, node: usize) -> Element<'_, Message> {
    let body: Element<'_, Message> = match channel(node).input {
        Some(ch) => preamp(m, ch),
        None => space().into(),
    };
    container(body)
        .width(layout::STRIP)
        .height(layout::INPUT_CARD)
        .padding([4, 10])
        .into()
}

/// ORBIT impedance settings in the order the arrows step through.
const ORBIT: [(u8, &str); 3] = [(0, "Punch"), (1, "Smooth"), (3, "Vintage")];

fn preamp(m: &Model, ch: u8) -> Element<'_, Message> {
    let gain = input::gain(ch);
    let flag = |label: &str, addr| {
        let on = m.flag(addr);
        key(label, on, p::LIT_TOP, Message::Flag(addr, !on))
    };
    let glow = m.flag(input::hmx(ch)) && m.flag(input::lift(ch));

    let orbit = m.byte(input::character(ch));
    let at = ORBIT.iter().position(|&(v, _)| v == orbit).unwrap_or(0);
    let step =
        |delta: usize| Message::Byte(input::character(ch), ORBIT[(at + delta) % ORBIT.len()].0);
    let arrow = |glyph: &str, msg| {
        iced::widget::button(text(glyph.to_string()).size(10).color(p::TEXT))
            .padding([4, 6])
            .style(|_, _| iced::widget::button::Style::default())
            .on_press(msg)
    };
    let orbit_select = container(
        row![
            arrow("\u{25c0}", step(ORBIT.len() - 1)),
            container(caption(ORBIT[at].1, 11.0, p::ARC_START)).center_x(Length::Fill),
            arrow("\u{25b6}", step(1)),
        ]
        .align_y(Alignment::Center),
    )
    .padding(2)
    .style(theme::well);

    let input_card = container(
        column![
            caption("Input", 11.0, p::MUTED),
            Knob::new(m.db(gain), (0.0, 75.0), move |v| Message::Db(gain, v))
                .step(1.0)
                .readout(format!("{:.0}", m.db(gain).max(0.0)))
                .labels("0", "75", "dB"),
            row![
                flag("+48V", input::phantom(ch)),
                flag("Pad", input::pad(ch))
            ]
            .spacing(6),
            flag("HPF", input::hpf(ch)),
        ]
        .spacing(5)
        .align_x(Alignment::Center),
    )
    .padding(8)
    .style(theme::card);

    let tone_card = container(
        column![
            caption("Tone", 11.0, p::MUTED),
            key("Glow", glow, p::LIT_TOP, Message::Glow(ch, !glow)),
            row![flag("HMX", input::hmx(ch)), flag("Lift", input::lift(ch))].spacing(6),
            caption("Orbit", 10.0, p::TEXT),
            orbit_select,
        ]
        .spacing(5)
        .align_x(Alignment::Center),
    )
    .padding(8)
    .style(theme::card);

    column![input_card, tone_card].spacing(6).into()
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
