//! Output routing page: tabs, source columns and one row per output.

use iced::widget::{button, column, container, row, scrollable, space, stack};
use iced::{border, Alignment, Element, Length};

use idctl_protocol::params::output;

use crate::model::Model;
use crate::routing::{self, Msg, Page, Row, State, HEADERS};
use crate::theme::{self, caption, palette as p, BOLD};

const SPACING: f32 = 12.0;
const TOGGLE_WIDTH: f32 = 120.0;
const NAME_WIDTH: f32 = 140.0;
const CELL_WIDTH: f32 = 100.0;
const DESTINATION_WIDTH: f32 = 170.0;
const ROW_HEIGHT: f32 = 52.0;
const DOT: f32 = 28.0;
const DOT_CORE: f32 = 12.0;

/// Width of the controls to the left of the source columns.
const LEFT_WIDTH: f32 = TOGGLE_WIDTH + SPACING + NAME_WIDTH;

pub fn view<'a>(model: &'a Model, state: &'a State) -> Element<'a, crate::Message> {
    let tabs = row(Page::ALL.map(|page| {
        let active = state.page == page;
        button(caption(
            page.title(),
            11.0,
            if active { p::DARK_TEXT } else { p::MUTED },
        ))
        .padding([10, 18])
        .style(theme::tab(active))
        .on_press(Msg::Page(page))
        .into()
    }));

    let rows = routing::rows(state.page, &state.mono)
        .iter()
        .map(|r| output_row(r, model, state))
        .collect::<Vec<_>>();

    let grid = container(
        column![headers(), column(rows).spacing(10)]
            .spacing(14)
            .align_x(Alignment::Start),
    )
    .padding(20)
    .style(theme::card);

    let page: Element<'a, Msg> = container(
        column![
            caption("Output routing", 18.0, p::TEXT).font(BOLD),
            tabs,
            scrollable(grid).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            )),
        ]
        .spacing(22),
    )
    .padding(28)
    .width(Length::Fill)
    .into();
    page.map(crate::Message::Routing)
}

/// SOURCES caption, MONITORING pill and column names above the rows.
fn headers<'a>() -> Element<'a, Msg> {
    let columns = HEADERS.len() as f32 * CELL_WIDTH;
    let monitoring = routing::MONITORING.len() as f32 * CELL_WIDTH;
    let names = row(HEADERS.map(|name| {
        container(caption(name, 10.0, p::TEXT))
            .center_x(CELL_WIDTH)
            .into()
    }));
    let pill = container(caption("Monitoring", 10.0, p::MONITORING))
        .padding([3, 10])
        .style(theme::pill(p::MONITORING));

    let lead = || space().width(LEFT_WIDTH);
    let tail = || space().width(DESTINATION_WIDTH);
    column![
        row![
            lead(),
            container(caption("Sources", 11.0, p::MUTED)).center_x(columns),
            tail(),
        ]
        .spacing(SPACING),
        row![
            lead(),
            container(pill).center_x(monitoring),
            space().width(columns - monitoring),
            tail(),
        ]
        .spacing(SPACING),
        row![
            lead(),
            row![
                space().width(CELL_WIDTH / 2.0),
                main_mix_bracket(monitoring - CELL_WIDTH),
                space().width(columns - monitoring + CELL_WIDTH / 2.0),
            ],
            tail(),
        ]
        .spacing(SPACING),
        row![lead(), names, tail()].spacing(SPACING),
    ]
    .spacing(8)
    .into()
}

/// MAIN MIX caption over a rule spanning the monitoring columns' centres.
fn main_mix_bracket<'a>(width: f32) -> Element<'a, Msg> {
    let rule = || {
        container(
            container(space().height(1.5))
                .width(Length::Fill)
                .style(theme::fill(p::MUTED)),
        )
        .center_y(Length::Fill)
    };
    row![rule(), caption("Main mix", 9.0, p::MUTED), rule()]
        .spacing(6)
        .width(width)
        .height(14)
        .align_y(Alignment::Center)
        .into()
}

fn output_row<'a>(r: &Row, model: &Model, state: &State) -> Element<'a, Msg> {
    let source = model.byte(output::source(r.slot));
    let selected = r.selected(source);
    let toggle: Element<'a, Msg> = match r.toggle {
        Some(pair) => {
            let base = selected.and_then(|i| r.cells[i]).unwrap_or(source);
            mode_toggle(pair, state.mono[pair], base)
        }
        None => space().width(TOGGLE_WIDTH).into(),
    };
    let cells = row(r.cells.iter().enumerate().map(|(i, cell)| {
        let dot = cell.map(|source| {
            dot(
                selected == Some(i),
                Msg::Pick {
                    slot: r.slot,
                    source,
                    stereo: r.stereo,
                },
            )
        });
        stack![
            container(
                container(space().height(2))
                    .width(Length::Fill)
                    .style(theme::fill(p::FAINT))
            )
            .center_y(Length::Fill),
            container(dot).center(Length::Fill),
        ]
        .width(CELL_WIDTH)
        .height(ROW_HEIGHT)
        .into()
    }));
    row![
        row![toggle, name_box(&r.name)]
            .spacing(SPACING)
            .align_y(Alignment::Center),
        cells,
        destination_box(&r.destination, selected == Some(HEADERS.len() - 1)),
    ]
    .spacing(SPACING)
    .align_y(Alignment::Center)
    .into()
}

/// MONO | STEREO segmented switch of one digital pair.
fn mode_toggle<'a>(pair: usize, mono: bool, base: u8) -> Element<'a, Msg> {
    let half = |label: &str, active: bool, msg: Msg| {
        button(
            container(caption(
                label,
                10.0,
                if active { p::DARK_TEXT } else { p::MUTED },
            ))
            .center_x(Length::Fill),
        )
        .padding([7, 4])
        .width(Length::Fill)
        .style(theme::tab(active))
        .on_press(msg)
    };
    row![
        half("Mono", mono, Msg::Mode(pair, None)),
        half("Stereo", !mono, Msg::Mode(pair, Some(base))),
    ]
    .width(TOGGLE_WIDTH)
    .into()
}

fn name_box<'a>(name: &str) -> Element<'a, Msg> {
    container(caption(name, 10.0, p::TEXT))
        .padding([9, 12])
        .width(NAME_WIDTH)
        .style(theme::well)
        .into()
}

/// What Virtual thru feeds; outlined while Virtual thru is the source.
fn destination_box<'a>(label: &str, lit: bool) -> Element<'a, Msg> {
    container(caption(label, 10.0, if lit { p::TEXT } else { p::MUTED }))
        .padding([9, 12])
        .width(DESTINATION_WIDTH)
        .style(move |t| {
            let well = theme::well(t);
            if lit {
                container::Style {
                    border: well.border.color(p::TEXT).width(1.5),
                    ..well
                }
            } else {
                well
            }
        })
        .into()
}

/// A dark round button; the selected source shows a green core.
fn dot<'a>(selected: bool, msg: Msg) -> Element<'a, Msg> {
    let core = container(space())
        .width(DOT_CORE)
        .height(DOT_CORE)
        .style(move |_| container::Style {
            background: selected.then(|| p::CLOCK.into()),
            border: border::rounded(DOT_CORE / 2.0),
            ..container::Style::default()
        });
    button(container(core).center(Length::Fill))
        .padding(0)
        .width(DOT)
        .height(DOT)
        .style(move |_, status| button::Style {
            background: Some(p::CHROME.into()),
            border: border::rounded(DOT / 2.0)
                .color(if matches!(status, button::Status::Hovered) {
                    p::MUTED
                } else {
                    p::FAINT
                })
                .width(1.5),
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}
