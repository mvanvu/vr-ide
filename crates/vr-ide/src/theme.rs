//! Colours and text styles. Styles start from the terminal's own colours (`Color::Reset`)
//! so the editor blends into the user's terminal theme.

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub text: Style,
    pub gutter: Style,
    pub gutter_current: Style,
    pub selection: Style,
    /// Placeholder drawn for control characters.
    pub control: Style,
    pub tab_bar: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub tab_modified: Style,
    pub tab_warning: Style,
    pub status: Style,
    pub info: Style,
    pub warning: Style,
    pub error: Style,
    pub dim: Style,
}

impl Theme {
    /// The named theme, or `None` for an unknown name.
    pub fn named(name: &str) -> Option<Theme> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }

    /// The named theme; unknown names fall back to dark (the startup notice reports them).
    pub fn from_name(name: &str) -> Theme {
        Self::named(name).unwrap_or_else(Self::dark)
    }

    pub fn dark() -> Theme {
        Theme {
            text: Style::default(),
            gutter: Style::default().fg(Color::DarkGray),
            gutter_current: Style::default().fg(Color::Gray),
            selection: Style::default().bg(Color::Indexed(239)),
            control: Style::default().fg(Color::Red),
            tab_bar: Style::default().bg(Color::Indexed(235)),
            tab_active: Style::default()
                .bg(Color::Reset)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().bg(Color::Indexed(235)).fg(Color::Gray),
            tab_modified: Style::default().fg(Color::Yellow),
            tab_warning: Style::default().fg(Color::Red),
            status: Style::default().bg(Color::Indexed(236)).fg(Color::Gray),
            info: Style::default().fg(Color::Cyan),
            warning: Style::default().fg(Color::Yellow),
            error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            dim: Style::default().fg(Color::DarkGray),
        }
    }

    pub fn light() -> Theme {
        Theme {
            text: Style::default(),
            gutter: Style::default().fg(Color::Gray),
            gutter_current: Style::default().fg(Color::Black),
            selection: Style::default().bg(Color::Indexed(153)),
            control: Style::default().fg(Color::Red),
            tab_bar: Style::default().bg(Color::Indexed(254)),
            tab_active: Style::default()
                .bg(Color::Reset)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().bg(Color::Indexed(254)).fg(Color::DarkGray),
            tab_modified: Style::default().fg(Color::Indexed(130)),
            tab_warning: Style::default().fg(Color::Red),
            status: Style::default().bg(Color::Indexed(252)).fg(Color::Black),
            info: Style::default().fg(Color::Blue),
            warning: Style::default().fg(Color::Indexed(130)),
            error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            dim: Style::default().fg(Color::Gray),
        }
    }
}
