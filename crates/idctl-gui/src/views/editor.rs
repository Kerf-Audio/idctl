//! Full-size EQ and compressor editors, shown over the window.

use iced::widget::{button, column, container, mouse_area, opaque, row, space, text, Row};
use iced::{Alignment, Color, Element, Length};

use idctl_protocol::dsp::{comp, Band};
use idctl_protocol::params::reverb;

use super::cards::{self, dsp_rate, dsp_running};
use super::key;
use crate::channel_dsp::DspEdit;
use crate::model::channel;
use crate::theme::{self, caption, palette as p, BOLD};
use crate::widgets::knob::Style;
use crate::widgets::{CompGraph, EqGraph, Knob, ReverbGraph, Taper};
use crate::{App, Editor, Message};

const PANEL_WIDTH: f32 = 760.0;
const BAND_NAMES: [&str; 4] = ["Low shelf", "Peak", "Peak", "High shelf"];
const RATIOS: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
/// Gain-reduction meter range, dB.
const GR_RANGE: f32 = 24.0;
const GR_HEIGHT: f32 = 220.0;

pub fn view(app: &App, editor: Editor) -> Element<'_, Message> {
    let (heading, power, body) = match editor {
        Editor::Eq(ch) => {
            let d = app.dsp[usize::from(ch)];
            let power = (d.eq_on, Message::Dsp(ch, DspEdit::EqOn(!d.eq_on)));
            (heading(ch, "EQ"), power, eq(app, ch))
        }
        Editor::Comp(ch) => {
            let on = cards::comp(&app.model, ch).on;
            (
                heading(ch, "Compressor"),
                (on, Message::CompOn(ch, !on)),
                compressor(app, ch),
            )
        }
        Editor::Reverb => (
            "REVERB".to_string(),
            cards::return_power(&app.model, app.mix),
            reverb(app),
        ),
    };
    let header = row![
        button(space().width(8).height(8))
            .padding(5)
            .style(theme::power(power.0, p::CLOCK))
            .on_press(power.1),
        text(heading).size(16).font(BOLD).color(p::TEXT),
        space::horizontal(),
        button(text("\u{2715}").size(16))
            .style(theme::flat)
            .on_press(Message::Close),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let mut panel = column![header].spacing(16);
    if !dsp_running(&app.model) {
        panel = panel.push(caption("DSP is off above 96 kHz", 11.0, p::MUTE_ON));
    }
    let panel = container(panel.push(body))
        .padding(20)
        .width(PANEL_WIDTH)
        .style(theme::card);
    let scrim = mouse_area(
        container(opaque(panel))
            .center(Length::Fill)
            .style(theme::fill(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
    )
    .on_press(Message::Close);
    opaque(scrim)
}

fn heading(ch: u8, title: &str) -> String {
    let name = channel(usize::from(ch)).name;
    format!("{name} \u{00b7} {}", title.to_uppercase())
}

fn reverb(app: &App) -> Element<'_, Message> {
    let m = &app.model;
    column![
        ReverbGraph::new(m.unit(reverb::DECAY), m.unit(reverb::BLEND)),
        cards::reverb_knobs(m, Style::Large),
        caption(
            "The power switch mutes the reverb return in the selected mix.",
            11.0,
            p::MUTED
        ),
    ]
    .spacing(16)
    .into()
}

fn eq(app: &App, ch: u8) -> Element<'_, Message> {
    let d = app.dsp[usize::from(ch)];
    let bands = d.eq;
    let edit = move |i: usize, band: Band| Message::Dsp(ch, DspEdit::Band(i, band));
    let graph = EqGraph::new(bands, dsp_rate(&app.model))
        .enabled(d.eq_on && dsp_running(&app.model))
        .on_change(move |i, hz, gain_db| {
            edit(
                i,
                Band {
                    hz,
                    gain_db,
                    ..bands[i]
                },
            )
        });
    let controls = Row::with_children((0..bands.len()).map(|i| {
        let b = bands[i];
        let hz = if b.hz >= 1000.0 {
            format!("{:.1}k", b.hz / 1000.0)
        } else {
            format!("{:.0}", b.hz)
        };
        column![
            caption(BAND_NAMES[i], 11.0, p::TEXT),
            row![
                Knob::new(b.hz, (20.0, 20_000.0), move |hz| edit(i, Band { hz, ..b }))
                    .step(1.0)
                    .taper(Taper::Power(0.3))
                    .readout(hz)
                    .labels("", "", "Freq"),
                Knob::new(b.gain_db, (-6.0, 6.0), move |gain_db| edit(
                    i,
                    Band { gain_db, ..b }
                ))
                .default(0.0)
                .step(0.1)
                .bipolar()
                .readout(format!("{:+.1}", b.gain_db))
                .labels("", "", "Gain"),
                Knob::new(b.q, (0.01, 1.0), move |q| edit(i, Band { q, ..b }))
                    .default(0.707)
                    .step(0.01)
                    .readout(format!("{:.2}", b.q))
                    .labels("", "", "Q"),
            ]
            .spacing(2),
        ]
        .spacing(6)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .into()
    }))
    .spacing(8);
    column![graph, controls].spacing(16).into()
}

fn compressor(app: &App, ch: u8) -> Element<'_, Message> {
    let m = &app.model;
    let c = cards::comp(m, ch);
    let (threshold, makeup) = (comp::threshold(ch), comp::makeup(ch));
    let (attack, release) = (comp::attack(ch), comp::release(ch));
    let gr = m.gain_reduction(ch).clamp(0.0, GR_RANGE);
    let meter = column![
        caption("GR", 10.0, p::MUTED),
        container(
            container(space())
                .width(Length::Fill)
                .height(GR_HEIGHT * gr / GR_RANGE)
                .style(theme::fill(p::SOLO_ON)),
        )
        .width(14)
        .height(GR_HEIGHT)
        .style(theme::well),
        caption(&format!("{gr:.1}"), 10.0, p::TEXT),
    ]
    .spacing(4)
    .align_x(Alignment::Center);
    let ratio_keys = Row::with_children(RATIOS.map(|r| {
        key(
            &format!("{r:.0}:1"),
            (c.ratio - r).abs() < 0.01,
            p::LIT_TOP,
            Message::Ratio(ch, r),
        )
        .into()
    }))
    .spacing(6);
    let ms = |v: f32| {
        if v >= 1000.0 {
            format!("{:.1}s", v / 1000.0)
        } else {
            format!("{v:.0}")
        }
    };
    let knobs = row![
        Knob::new(c.threshold, (-60.0, 0.0), move |v| Message::Db(
            threshold, v
        ))
        .default(0.0)
        .step(0.5)
        .readout(format!("{:.1}", c.threshold))
        .labels("-60", "0", "Thresh"),
        Knob::new(c.attack, (10.0 / 3.0, 10_000.0), move |v| Message::Time(
            attack, v
        ))
        .default(10.0)
        .step(0.5)
        .taper(Taper::Power(0.3))
        .readout(ms(c.attack))
        .labels("", "", "Attack"),
        Knob::new(c.release, (10.0 / 3.0, 10_000.0), move |v| Message::Time(
            release, v
        ))
        .default(100.0)
        .step(1.0)
        .taper(Taper::Power(0.3))
        .readout(ms(c.release))
        .labels("", "", "Release"),
        Knob::new(c.makeup, (0.0, 12.0), move |v| Message::Db(makeup, v))
            .step(0.5)
            .readout(format!("{:.1}", c.makeup))
            .labels("0", "12", "Makeup"),
    ]
    .spacing(12);
    row![
        container(CompGraph::new(c.threshold, c.ratio, c.makeup).enabled(c.on && dsp_running(m)))
            .width(Length::FillPortion(2)),
        meter,
        column![caption("Ratio", 11.0, p::MUTED), ratio_keys, knobs]
            .spacing(14)
            .width(Length::FillPortion(3)),
    ]
    .spacing(20)
    .align_y(Alignment::Center)
    .into()
}
