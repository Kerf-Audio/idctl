//! Window layout: top bar, side navigation, channel strips, monitor panel.

mod cards;
mod editor;
mod monitor;
mod routing;
mod settings;
mod strips;

use iced::widget::{button, column, container, mouse_area, row, space, stack, text};
use iced::{mouse, Alignment, Color, Element, Length};

use crate::theme::{self, caption, palette as p, BOLD};
use crate::{App, Message, Tab};

/// Height budget. Everything above the faders is fixed apart from the rack
/// of cards, which the user can resize; the faders take the rest and must
/// keep at least `FADER_MIN` at the minimum window height.
pub mod layout {
    pub const DEFAULT_WIDTH: f32 = 1500.0;
    pub const DEFAULT_HEIGHT: f32 = 1000.0;
    pub const MIN_WIDTH: f32 = 1100.0;
    pub const MIN_HEIGHT: f32 = 830.0;

    pub const TOP_BAR: f32 = 58.0;
    pub const STRIP_HEADER: f32 = 80.0;
    /// Default rack height; dragging the monitoring bar changes it.
    pub const SECTIONS: f32 = 338.0;
    pub const RACK_MIN: f32 = 80.0;
    pub const MONITORING_BAR: f32 = 26.0;
    pub const PAN_ROW: f32 = 60.0;
    pub const LINK_ROW: f32 = 22.0;
    pub const BUTTON_ROW: f32 = 34.0;
    pub const FADER_MIN: f32 = 200.0;
    pub const SCROLLBAR: f32 = 10.0;

    pub const CHROME_HEIGHT: f32 = TOP_BAR
        + STRIP_HEADER
        + SECTIONS
        + MONITORING_BAR
        + PAN_ROW
        + LINK_ROW
        + BUTTON_ROW
        + SCROLLBAR;

    /// Faders keep room at the minimum window height; checked at compile time.
    const _: () = assert!(CHROME_HEIGHT + FADER_MIN <= MIN_HEIGHT);

    /// Strip height below the rack, faders at their minimum.
    pub const BELOW_RACK: f32 = CHROME_HEIGHT - TOP_BAR - SECTIONS + FADER_MIN;

    pub const SIDEBAR: f32 = 112.0;
    pub const STRIP: f32 = 137.0;
    pub const MONITOR_PANEL: f32 = 236.0;
}

pub fn root(app: &App) -> Element<'_, Message> {
    let body: Element<'_, Message> = match app.tab {
        Tab::Mixer => strips::view(app),
        Tab::Routing => routing::view(&app.model, &app.routing),
        Tab::Settings => settings::view(app),
    };
    let mut main = row![
        sidebar(app),
        container(body).width(Length::Fill).height(Length::Fill)
    ]
    .height(Length::Fill);
    if app.tab != Tab::Routing {
        main = main.push(monitor::view(&app.model, app.mix));
    }
    let window: Element<'_, Message> = if app.resizing_rack {
        // The whole window follows the drag so a release anywhere ends it.
        mouse_area(column![top_bar(app), main])
            .on_move(|at| {
                Message::RackDrag(
                    at.y - layout::TOP_BAR - layout::STRIP_HEADER - layout::MONITORING_BAR / 2.0,
                )
            })
            .on_release(Message::RackRelease)
            .interaction(mouse::Interaction::ResizingVertically)
            .into()
    } else {
        column![top_bar(app), main].into()
    };
    match app.editor {
        Some(e) => stack![window, editor::view(app, e)].into(),
        None => window,
    }
}

fn top_bar(app: &App) -> Element<'_, Message> {
    let status = match &app.offline {
        None => caption("iD2+ connected", 11.0, p::CLOCK),
        Some(why) => caption(&format!("offline: {why}"), 11.0, p::MUTE_ON),
    };
    container(
        row![
            container(text("iD Mixer").size(22).font(BOLD).color(p::TEXT)).width(Length::Fill),
            container(status)
                .width(Length::Fill)
                .align_x(Alignment::End),
        ]
        .align_y(Alignment::Center)
        .padding([0, 24]),
    )
    .height(layout::TOP_BAR)
    .center_y(layout::TOP_BAR)
    .style(theme::fill(p::CHROME))
    .into()
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let tab = |label: &str, t: Tab| {
        let active = app.tab == t;
        button(
            container(caption(
                label,
                12.0,
                if active { p::DARK_TEXT } else { p::MUTED },
            ))
            .center(Length::Fill),
        )
        .width(Length::Fill)
        .height(76)
        .style(theme::tab(active))
        .on_press(Message::Tab(t))
    };
    let rate = app.model.sample_rate();
    let rate = if rate == 0 {
        "-".to_string()
    } else {
        format!("{}KHZ", trim_khz(rate))
    };
    let clock = if app
        .model
        .flag(idctl_protocol::params::system::clock_locked(0))
    {
        "LOCKED"
    } else {
        "INTERNAL"
    };
    let badge = |s: String| {
        container(caption(&s, 10.0, p::CLOCK))
            .padding([3, 10])
            .style(theme::pill(p::CLOCK))
    };
    container(
        column![
            tab("Mixer", Tab::Mixer),
            tab("Routing", Tab::Routing),
            tab("Settings", Tab::Settings),
            space::vertical(),
            column![
                caption("Clock", 10.0, p::MUTED),
                badge(clock.into()),
                space().height(8),
                caption("Sample rate", 10.0, p::MUTED),
                badge(rate),
            ]
            .spacing(5)
            .align_x(Alignment::Center)
            .padding([20, 0]),
        ]
        .align_x(Alignment::Center),
    )
    .width(layout::SIDEBAR)
    .height(Length::Fill)
    .style(theme::fill(p::CHROME))
    .into()
}

fn trim_khz(hz: u32) -> String {
    let khz = f64::from(hz) / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}")
    }
}

/// A hardware-style key with a tracked caption.
fn key<'a>(label: &str, lit: bool, accent: Color, msg: Message) -> button::Button<'a, Message> {
    button(
        container(caption(
            label,
            10.0,
            if lit { p::DARK_TEXT } else { p::TEXT },
        ))
        .center_x(Length::Fill),
    )
    .padding([6, 4])
    .width(Length::Fill)
    .style(theme::key(lit, accent))
    .on_press(msg)
}

#[cfg(test)]
mod tests {

    #[test]
    fn khz_labels() {
        assert_eq!(super::trim_khz(44_100), "44.1");
        assert_eq!(super::trim_khz(48_000), "48");
    }
}
