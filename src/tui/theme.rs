//! TUI color theme (bead gu7ts.6). governed-by: ADR-0003.
//!
//! Indicative palette per bv UX map §1; kept in one place so gu7ts.7/.8
//! views inherit consistent styling instead of hand-rolling colors.

use ratatui::style::{Color, Modifier, Style};

/// Primary accent (headers, selection border).
#[must_use]
pub const fn primary() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// Dim secondary text (hints, page info).
#[must_use]
pub const fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Highlighted list row.
#[must_use]
pub const fn selected_row() -> Style {
    Style::new()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

/// Normal list row.
#[must_use]
pub const fn row() -> Style {
    Style::new()
}

/// Open status badge.
#[must_use]
pub const fn status_open() -> Style {
    Style::new().fg(Color::Green)
}

/// In-progress status badge.
#[must_use]
pub const fn status_in_progress() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Closed/tombstone status badge.
#[must_use]
pub const fn status_closed() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Blocked/deferred status badge.
#[must_use]
pub const fn status_blocked() -> Style {
    Style::new().fg(Color::Red)
}

/// Quit-confirm modal border (bv: blocked-red).
#[must_use]
pub const fn danger_border() -> Style {
    Style::new().fg(Color::Red)
}
