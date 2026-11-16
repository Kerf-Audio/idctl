//! Cards in a strip's scrolling section: input, variable phase, tone,
//! compressor and EQ (in processing order), reverb send, and the reverb
//! return that spans the last pair.

use iced::widget::{button, column, container, mouse_area, row, slider, space, text, Column};
use iced::{mouse, Alignment, Element, Length};

use idctl_protocol::dsp::{self, comp};
use idctl_protocol::params::{input, reverb};

use super::{key, layout};
use crate::channel_dsp::{ChannelDsp, DspEdit, DSP_CHANNELS};
use crate::mix::{NODES, OFF_DB, REVERB_MIX};
use crate::model::{channel, Model};
use crate::theme::{self, caption, palette as p, BOLD};
use crate::widgets::knob::Style;
use crate::widgets::{CompGraph, EqGraph, Knob, ReverbGraph, Taper};
use crate::{App, Editor, Message};

/// Highest sample rate the DSP runs at.
const DSP_MAX_RATE: u32 = 96_000;
/// Send slider: the bottom of the travel is off, then -60 dB up to +6 dB.
const SEND_FLOOR_DB: f32 = -60.0;
const SEND_MAX_DB: f32 = 6.0;

/// ORBIT impedance settings in the order the arrows step through.
const ORBIT: [(u8, &str); 3] = [(0, "Punch"), (1, "Smooth"), (3, "Vintage")];

/// Sample rate the DSP coefficients are drawn at.
pub fn dsp_rate(m: &Model) -> f32 {
    match m.sample_rate() {
        0 => 48_000.0,
        hz => hz as f32,
    }
}

/// The DSP is off above 96 kHz.
pub fn dsp_running(m: &Model) -> bool {
    m.sample_rate() <= DSP_MAX_RATE
}

/// Every card for one node, top to bottom.
pub fn section(app: &App, node: usize) -> Element<'_, Message> {
    let m = &app.model;
    let mut cards = Column::new().spacing(6);
    if let Some(ch) = channel(node).input {
        cards = cards
            .push(input_card(app, ch))
            .push(phase_card(app, ch))
            .push(tone_card(m, ch));
    }
    if node < DSP_CHANNELS {
        let ch = node as u8;
        let swap = container(
            button(text("\u{21c5}").size(14))
                .padding([0, 8])
                .style(theme::flat)
                .on_press(Message::Byte(
                    dsp::order(ch),
                    1 - m.byte(dsp::order(ch)).min(1),
                )),
        )
        .center_x(Length::Fill);
        let (comp, eq) = (comp_card(m, ch), eq_card(m, &app.dsp[node], ch));
        cards = if m.byte(dsp::order(ch)) == 0 {
            cards.push(eq).push(swap).push(comp)
        } else {
            cards.push(comp).push(swap).push(eq)
        };
    }
    if channel(node).group != crate::model::Group::Reverb {
        cards = cards.push(send_card(m, node));
    }
    container(cards)
        .width(layout::STRIP)
        .padding([4, 10])
        .into()
}

/// Card frame: optional power switch, title, optional pop-out.
fn card<'a>(
    title: &str,
    power: Option<(bool, Message)>,
    pop_out: Option<Message>,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut header = row![].spacing(6).align_y(Alignment::Center);
    if let Some((on, msg)) = power {
        header = header.push(
            button(space().width(6).height(6))
                .padding(4)
                .style(theme::power(on, p::CLOCK))
                .on_press(msg),
        );
    }
    header = header
        .push(caption(title, 11.0, p::MUTED))
        .push(space::horizontal());
    let mut body = body.into();
    if let Some(msg) = pop_out {
        header = header.push(
            button(text("\u{2197}").size(13))
                .padding([0, 2])
                .style(theme::flat)
                .on_press(msg.clone()),
        );
        body = mouse_area(body)
            .on_press(msg)
            .interaction(mouse::Interaction::Pointer)
            .into();
    }
    container(column![header, body].spacing(6).align_x(Alignment::Center))
        .padding(8)
        .width(Length::Fill)
        .style(theme::card)
        .into()
}

fn flag<'a>(
    m: &Model,
    label: &str,
    addr: idctl_protocol::ParamAddr,
) -> button::Button<'a, Message> {
    let on = m.flag(addr);
    key(label, on, p::LIT_TOP, Message::Flag(addr, !on))
}

fn input_card(app: &App, ch: u8) -> Element<'_, Message> {
    let m = &app.model;
    let gain = input::gain(ch);
    let hpf_on = m.flag(input::hpf(ch));
    let hpf_hz = app.dsp[usize::from(ch)].hpf_hz;
    let body = column![
        Knob::new(m.db(gain), (0.0, 75.0), move |v| Message::Db(gain, v))
            .step(1.0)
            .readout(format!("{:.0}", m.db(gain).max(0.0)))
            .labels("0", "75", "dB"),
        row![
            flag(m, "+48V", input::phantom(ch)),
            flag(m, "Pad", input::pad(ch))
        ]
        .spacing(6),
        flag(m, "HPF", input::hpf(ch)),
        Knob::new(hpf_hz, (20.0, 250.0), move |hz| {
            Message::Dsp(ch, DspEdit::HpfHz(hz, hpf_on))
        })
        .default(crate::channel_dsp::HPF_DEFAULT_HZ)
        .step(1.0)
        .readout(format!("{hpf_hz:.0}"))
        .labels("20", "250", "Hz"),
    ]
    .spacing(5)
    .align_x(Alignment::Center);
    card("Input", None, None, body)
}

fn phase_card(app: &App, ch: u8) -> Element<'_, Message> {
    let d = app.dsp[usize::from(ch)];
    let body = Knob::new(d.phase_deg, (0.0, 180.0), move |v| {
        Message::Dsp(ch, DspEdit::Phase(v))
    })
    .step(1.0)
    .readout(format!("{:.0}\u{00b0}", d.phase_deg))
    .labels("0", "180", "");
    card(
        "V-Phase",
        Some((d.phase_on, Message::Dsp(ch, DspEdit::PhaseOn(!d.phase_on)))),
        None,
        body,
    )
}

/// A tone key: bold upper-case label, white when lit.
fn tone_key<'a>(label: &str, on: bool, msg: Message) -> button::Button<'a, Message> {
    button(
        container(text(label.to_uppercase()).size(12).font(BOLD).color(if on {
            p::DARK_TEXT
        } else {
            p::TEXT
        }))
        .center_x(Length::Fill),
    )
    .padding([6, 4])
    .width(Length::Fill)
    .style(theme::key(on, p::LIT_TOP))
    .on_press(msg)
}

fn tone_card(m: &Model, ch: u8) -> Element<'_, Message> {
    let toggle = |label: &str, addr| {
        let on = m.flag(addr);
        tone_key(label, on, Message::Flag(addr, !on))
    };
    let glow = m.flag(input::hmx(ch)) && m.flag(input::lift(ch));
    let orbit = m.byte(input::character(ch));
    let at = ORBIT.iter().position(|&(v, _)| v == orbit).unwrap_or(0);
    let step =
        |delta: usize| Message::Byte(input::character(ch), ORBIT[(at + delta) % ORBIT.len()].0);
    let arrow = |glyph: &str, msg| {
        button(text(glyph.to_string()).size(9).color(p::SELECTOR))
            .padding([4, 3])
            .style(theme::flat)
            .on_press(msg)
    };
    let selector = container(
        row![
            arrow("\u{25c0}", step(ORBIT.len() - 1)),
            container(
                text(ORBIT[at].1.to_uppercase())
                    .size(11)
                    .font(BOLD)
                    .color(p::SELECTOR)
                    .wrapping(text::Wrapping::None),
            )
            .center_x(Length::Fill),
            arrow("\u{25b6}", step(1)),
        ]
        .align_y(Alignment::Center),
    )
    .padding(1)
    .style(theme::well);
    let body = column![
        tone_key("Glow", glow, Message::Glow(ch, !glow)),
        row![
            toggle("HMX", input::hmx(ch)),
            toggle("Lift", input::lift(ch))
        ]
        .spacing(6),
        caption("Orbit", 10.0, p::TEXT),
        selector,
    ]
    .spacing(5)
    .align_x(Alignment::Center);
    card("Tone", None, None, body)
}

/// Compressor settings read from the device.
pub struct Comp {
    pub on: bool,
    pub threshold: f32,
    pub ratio: f32,
    pub makeup: f32,
    pub attack: f32,
    pub release: f32,
}

pub fn comp(m: &Model, ch: u8) -> Comp {
    Comp {
        on: m.u16(comp::mix(ch)) != 0,
        threshold: m.db(comp::threshold(ch)).clamp(-60.0, 0.0),
        ratio: (f32::from(m.u16(comp::ratio(ch))) / 256.0).max(1.0),
        makeup: m.db(comp::makeup(ch)).clamp(0.0, 12.0),
        attack: f32::from(m.u16(comp::attack(ch))) / 6.0,
        release: f32::from(m.u16(comp::release(ch))) / 6.0,
    }
}

fn comp_card(m: &Model, ch: u8) -> Element<'_, Message> {
    let c = comp(m, ch);
    let running = dsp_running(m);
    card(
        "Comp",
        Some((c.on, Message::CompOn(ch, !c.on))),
        Some(Message::Open(Editor::Comp(ch))),
        CompGraph::new(c.threshold, c.ratio, c.makeup)
            .enabled(c.on && running)
            .mini(),
    )
}

fn eq_card<'a>(m: &'a Model, d: &ChannelDsp, ch: u8) -> Element<'a, Message> {
    card(
        "EQ",
        Some((d.eq_on, Message::Dsp(ch, DspEdit::EqOn(!d.eq_on)))),
        Some(Message::Open(Editor::Eq(ch))),
        EqGraph::new(d.eq, dsp_rate(m))
            .enabled(d.eq_on && dsp_running(m))
            .mini(),
    )
}

fn send_card(m: &Model, node: usize) -> Element<'_, Message> {
    let s = m.mixer.send(node, REVERB_MIX);
    let on = !s.mute && s.level > OFF_DB;
    let travel = |db: f32| {
        if db <= OFF_DB {
            0.0
        } else {
            ((db - SEND_FLOOR_DB) / (SEND_MAX_DB - SEND_FLOOR_DB)).clamp(0.01, 1.0)
        }
    };
    let level = move |t: f32| {
        if t <= 0.0 {
            OFF_DB
        } else {
            SEND_FLOOR_DB + t * (SEND_MAX_DB - SEND_FLOOR_DB)
        }
    };
    let body = column![
        slider(0.0..=1.0, travel(s.level), move |t| Message::Send(
            node,
            level(t)
        ))
        .step(0.005_f32),
        row![
            caption("-\u{221e}", 9.0, p::FAINT),
            space::horizontal(),
            caption(&super::strips::db_text(s.level), 9.0, p::MUTED),
            space::horizontal(),
            caption("6", 9.0, p::FAINT),
        ],
    ]
    .spacing(2);
    card("Reverb", Some((on, Message::SendOn(node))), None, body)
}

/// Reverb return controls across the width of the last pair.
pub fn reverb_return(m: &Model, mix: usize) -> Element<'_, Message> {
    let body = column![
        ReverbGraph::new(m.unit(reverb::DECAY), m.unit(reverb::BLEND)).mini(),
        reverb_knobs(m, Style::Small),
    ]
    .spacing(8);
    let (on, toggle) = return_power(m, mix);
    container(card(
        "Reverb",
        Some((on, toggle)),
        Some(Message::Open(Editor::Reverb)),
        body,
    ))
    .width(2.0 * layout::STRIP)
    .padding([4, 10])
    .into()
}

/// The reverb return's power switch: it mutes the return in `mix`, as the
/// device has no reverb on/off.
pub fn return_power(m: &Model, mix: usize) -> (bool, Message) {
    let on = [NODES - 2, NODES - 1]
        .iter()
        .all(|&n| !m.mixer.send(n, mix).mute);
    (on, Message::ReturnOn(!on))
}

/// Decay, damping and blend knobs side by side.
pub fn reverb_knobs(m: &Model, style: Style) -> Element<'_, Message> {
    let decay = m.unit(reverb::DECAY);
    let blend = m.unit(reverb::BLEND);
    let damping = m.hz(reverb::DAMPING).clamp(20.0, 20_000.0);
    let knob = |label: &str, k: Knob<'static, Message>| {
        column![k.style(style), caption(label, 10.0, p::MUTED)]
            .spacing(2)
            .align_x(Alignment::Center)
            .width(Length::Fill)
    };
    row![
        knob(
            "Decay",
            Knob::new(decay, (0.0, 1.0), |x| Message::Unit(reverb::DECAY, x))
                .default(0.6)
                .step(0.01)
                .readout(format!("{:.0}", decay * 100.0))
                .labels("Short", "Long", ""),
        ),
        knob(
            "Damping",
            Knob::new(damping, (20.0, 20_000.0), |x| Message::Hz(
                reverb::DAMPING,
                x
            ))
            .default(8_000.0)
            .step(10.0)
            .taper(Taper::Power(0.35))
            .readout(format!("{:.1}k", damping / 1000.0))
            .labels("", "", "Hz"),
        ),
        knob(
            "Blend",
            Knob::new(blend, (0.0, 1.0), |x| Message::Unit(reverb::BLEND, x))
                .default(0.8)
                .step(0.01)
                .readout(format!("{:.0}", blend * 100.0))
                .labels("Early", "Late", ""),
        ),
    ]
    .into()
}
