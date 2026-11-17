//! Palette, fonts and widget styles.

use iced::widget::{button, container, pick_list, text, Text};
use iced::{
    border, font, gradient, Background, Border, Color, Font, Radians, Shadow, Theme, Vector,
};

pub mod palette {
    use iced::{color, Color};
    pub const WINDOW: Color = color!(0x0e1319);
    pub const CHROME: Color = color!(0x0a0e13);
    pub const STRIP: Color = color!(0x151c25);
    pub const STRIP_LINE: Color = color!(0x263241);
    pub const SECTION: Color = color!(0x1b2430);
    pub const BUTTON_TOP: Color = color!(0x222d3b);
    pub const BUTTON_BOTTOM: Color = color!(0x1b2430);
    pub const LIT_TOP: Color = color!(0xf5b942);
    pub const TEXT: Color = color!(0xe6ebf2);
    pub const DARK_TEXT: Color = color!(0x10141a);
    pub const MUTED: Color = color!(0x8a97a8);
    pub const FAINT: Color = color!(0x56627a);
    pub const MIC: Color = color!(0xb79cff);
    pub const OPTICAL: Color = color!(0xf5b942);
    pub const COMPUTER: Color = color!(0x36d6e7);
    pub const REVERB: Color = color!(0xe6ebf2);
    pub const ARC_START: Color = color!(0x36d6e7);
    pub const ARC_END: Color = color!(0xb79cff);
    pub const MONITORING: Color = color!(0x9be564);
    pub const CLOCK: Color = color!(0x9be564);
    pub const SELECTOR: Color = color!(0x36d6e7);
    pub const METER_LOW: Color = color!(0x9be564);
    pub const METER_MID: Color = color!(0xf5b942);
    pub const METER_HIGH: Color = color!(0xff6b57);
    pub const MUTE_ON: Color = color!(0xff6b57);
    pub const SOLO_ON: Color = color!(0xf5b942);
}
use palette as p;

pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};
pub const SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..Font::DEFAULT
};

pub fn theme() -> Theme {
    Theme::custom(
        "Mixer".to_string(),
        iced::theme::Palette {
            background: p::WINDOW,
            text: p::TEXT,
            primary: p::ARC_END,
            success: p::CLOCK,
            warning: p::SOLO_ON,
            danger: p::MUTE_ON,
        },
    )
}

/// Wide-tracked upper-case label.
pub fn caption<'a>(s: &str, size: f32, color: Color) -> Text<'a> {
    let tracked: String = s
        .to_uppercase()
        .chars()
        .flat_map(|c| [c, '\u{2009}'])
        .collect::<String>()
        .trim_end()
        .to_string();
    text(tracked)
        .size(size)
        .font(SEMIBOLD)
        .color(color)
        .wrapping(text::Wrapping::None)
}

fn vertical(top: Color, bottom: Color) -> Background {
    Background::Gradient(
        gradient::Linear::new(Radians(std::f32::consts::PI))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom)
            .into(),
    )
}

fn lighten(c: Color, by: f32) -> Color {
    Color::from_rgb(
        c.r + (1.0 - c.r) * by,
        c.g + (1.0 - c.g) * by,
        c.b + (1.0 - c.b) * by,
    )
}

/// Hardware-style key. `lit` keys fill with `accent` (amber for selectors).
pub fn key(lit: bool, accent: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hover = if matches!(status, button::Status::Hovered) {
            0.06
        } else {
            0.0
        };
        let pressed = matches!(status, button::Status::Pressed);
        let (top, bottom, text_color) = if lit {
            (lighten(accent, 0.12 + hover), accent, p::DARK_TEXT)
        } else {
            (
                lighten(p::BUTTON_TOP, hover),
                lighten(p::BUTTON_BOTTOM, hover),
                p::TEXT,
            )
        };
        let (top, bottom) = if pressed {
            (bottom, top)
        } else {
            (top, bottom)
        };
        button::Style {
            background: Some(vertical(top, bottom)),
            text_color,
            border: Border {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, if pressed { 0.0 } else { 0.4 }),
                offset: Vector::new(0.0, 1.5),
                blur_radius: 2.0,
            },
            snap: true,
        }
    }
}

/// Round power switch on a processor card: filled in `accent` when on.
pub fn power(on: bool, accent: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hover = if matches!(status, button::Status::Hovered) {
            0.08
        } else {
            0.0
        };
        let (fill, edge) = if on {
            (lighten(accent, hover), accent)
        } else {
            (lighten(p::BUTTON_BOTTOM, hover), p::FAINT)
        };
        button::Style {
            background: Some(fill.into()),
            text_color: if on { p::DARK_TEXT } else { p::MUTED },
            border: Border {
                color: edge,
                width: 1.5,
                radius: 9.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// Flat icon button (arrows, pop-out, swap).
pub fn flat(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        text_color: if matches!(status, button::Status::Hovered) {
            p::LIT_TOP
        } else {
            p::MUTED
        },
        ..button::Style::default()
    }
}

/// Outlined pill (clock and sample-rate badges).
pub fn pill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: Some(color),
        border: Border {
            color,
            width: 1.5,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

/// Channel strip column with a hairline divider on the right.
pub fn strip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(p::STRIP.into()),
        border: Border {
            color: p::STRIP_LINE,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Recessed card inside a strip (the INPUT block).
pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(p::SECTION.into()),
        border: border::rounded(4)
            .color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))
            .width(1),
        ..container::Style::default()
    }
}

pub fn tab(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(if active {
                p::LIT_TOP.into()
            } else if hovered {
                p::STRIP.into()
            } else {
                p::CHROME.into()
            }),
            text_color: if active { p::DARK_TEXT } else { p::MUTED },
            border: Border {
                color: p::STRIP_LINE,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..button::Style::default()
        }
    }
}

pub fn select(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    let hovered = !matches!(status, pick_list::Status::Active);
    pick_list::Style {
        text_color: p::TEXT,
        placeholder_color: p::MUTED,
        handle_color: p::TEXT,
        background: vertical(
            lighten(p::BUTTON_TOP, if hovered { 0.05 } else { 0.0 }),
            p::BUTTON_BOTTOM,
        ),
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            width: 1.0,
            radius: 4.0.into(),
        },
    }
}

pub fn menu(_: &Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: p::BUTTON_BOTTOM.into(),
        border: Border {
            color: p::STRIP_LINE,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: p::TEXT,
        selected_text_color: p::DARK_TEXT,
        selected_background: p::LIT_TOP.into(),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 8.0,
        },
    }
}

/// Inset selector frame (input mode).
pub fn well(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb8(0x0a, 0x0e, 0x13).into()),
        border: border::rounded(6)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.12))
            .width(1),
        ..container::Style::default()
    }
}
