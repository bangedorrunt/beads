//! TUI color theme — hacker-night neon-on-void (Ghostty v2). governed-by: ADR-0003.
//!
//! Single source of truth for bare-`br` TUI. Ghostty v2 palette:
//! void #08080e, fg #c8d0e8, muted #565f89, cyan #0db9d7, lav #bb9af7,
//! mint #73daca, amber #e0af68, red #f7768e, ice #89ddff, selection #1a1a3e.
//! 3-tier degrade: truecolor (COLORTERM=truecolor/24bit) → 256 → 16 ANSI.
//! NO_COLOR / TERM=dumb → plain (skill §4 accessibility).

use ratatui::style::{Color, Modifier, Style};

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
        || std::env::var("TERM").is_ok_and(|v| v == "dumb")
}

fn truecolor() -> bool {
    if no_color() {
        return false;
    }
    std::env::var("COLORTERM").ok().is_some_and(|v| {
        let l = v.to_ascii_lowercase();
        l == "truecolor" || l == "24bit"
    })
}

fn is_256() -> bool {
    if no_color() || truecolor() {
        return false;
    }
    std::env::var("TERM").is_ok_and(|v| v.contains("256color"))
}

// — raw hacker-night constants —

const VOID: Color = Color::Rgb(0x08, 0x08, 0x0e);
#[allow(dead_code)]
const SURFACE: Color = Color::Rgb(0x12, 0x12, 0x2a);
#[allow(dead_code)]
const OVERLAY: Color = Color::Rgb(0x1a, 0x1a, 0x3e);
const SELECTION_BG: Color = Color::Rgb(0x24, 0x24, 0x3f);
const FG: Color = Color::Rgb(0xc8, 0xd0, 0xe8);
const MUTED: Color = Color::Rgb(0x56, 0x5f, 0x89);
#[allow(dead_code)]
const EMPHASIS: Color = Color::Rgb(0xed, 0xed, 0xed);
const CYAN: Color = Color::Rgb(0x0d, 0xb9, 0xd7);
#[allow(dead_code)]
const LAV: Color = Color::Rgb(0xbb, 0x9a, 0xf7);
const MINT: Color = Color::Rgb(0x73, 0xda, 0xca);
const ICE: Color = Color::Rgb(0x89, 0xdd, 0xff);
const AMBER: Color = Color::Rgb(0xe0, 0xaf, 0x68);
const RED: Color = Color::Rgb(0xf7, 0x76, 0x8e);

fn resolve(rgb: Color, idx256: u8, ansi16: Color) -> Color {
    if no_color() {
        return Color::Reset;
    }
    if truecolor() {
        return rgb;
    }
    if is_256() {
        Color::Indexed(idx256)
    } else {
        ansi16
    }
}

fn rgb_or_reset(c: Color) -> Color {
    if no_color() { Color::Reset } else { c }
}

/// Primary accent — headers, selection border, help titles (void bg + cyan fg logic lives in callers).
#[must_use]
pub fn primary() -> Style {
    if no_color() {
        return Style::new();
    }
    // hacker-night: vivid cyan on void — neon-on-void header
    Style::new()
        .fg(resolve(CYAN, 44, Color::Cyan))
        .bg(resolve(VOID, 233, Color::Black))
        .add_modifier(Modifier::BOLD)
}

/// Like primary but without bg (for header spans that sit on default bg).
#[must_use]
pub fn primary_fg() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new()
        .fg(resolve(CYAN, 44, Color::Cyan))
        .add_modifier(Modifier::BOLD)
}

/// Dim secondary text (hints, page info).
#[must_use]
pub fn dim() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new()
        .fg(resolve(MUTED, 60, Color::DarkGray))
        .add_modifier(Modifier::DIM)
}

/// Highlighted list row — selection bg + bold fg.
#[must_use]
pub fn selected_row() -> Style {
    if no_color() {
        return Style::new().add_modifier(Modifier::REVERSED);
    }
    Style::new()
        .bg(resolve(SELECTION_BG, 236, Color::DarkGray))
        .fg(resolve(FG, 189, Color::White))
        .add_modifier(Modifier::BOLD)
}

/// Normal list row.
#[must_use]
pub fn row() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(FG, 189, Color::White))
}

/// Muted row variant (secondary text inside a row).
#[must_use]
pub fn row_muted() -> Style {
    dim()
}

/// Focused panel border — vivid cyan.
#[must_use]
pub fn border_focused() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(CYAN, 44, Color::Cyan))
}

/// Unfocused panel border — muted.
#[must_use]
pub fn border_unfocused() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(MUTED, 60, Color::DarkGray))
}

/// Selection bar accent (left border of selected row) — use primary_fg.
#[must_use]
pub fn selection_bar() -> Style {
    primary_fg()
}

// — status badges —

/// Open status badge — neon mint.
#[must_use]
pub fn status_open() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(MINT, 79, Color::Green))
}

/// In-progress status badge — ice cyan (working).
#[must_use]
pub fn status_in_progress() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(ICE, 117, Color::Cyan))
}

/// Closed/tombstone status badge — muted.
#[must_use]
pub fn status_closed() -> Style {
    dim()
}

/// Blocked/deferred status badge — hot pink-red + bold.
#[must_use]
pub fn status_blocked() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new()
        .fg(resolve(RED, 210, Color::LightRed))
        .add_modifier(Modifier::BOLD)
}

/// Warning badge — amber gold.
#[must_use]
pub fn status_warning() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(AMBER, 180, Color::Yellow))
}

/// Lavender secondary accent (for secondary highlights).
#[must_use]
pub fn accent_lav() -> Style {
    if no_color() {
        return Style::new();
    }
    Style::new().fg(resolve(LAV, 141, Color::Magenta))
}

/// Quit-confirm modal border — blocked-red.
#[must_use]
pub fn danger_border() -> Style {
    if no_color() {
        return Style::new().add_modifier(Modifier::BOLD);
    }
    Style::new()
        .fg(resolve(RED, 210, Color::LightRed))
        .add_modifier(Modifier::BOLD)
}

/// Expose raw palette for places that need Color values directly.
pub mod palette {
    use super::*;
    pub const VOID_RGB: Color = VOID;
    pub const FG_RGB: Color = FG;
    pub const MUTED_RGB: Color = MUTED;
    pub const CYAN_RGB: Color = CYAN;
    pub const SELECTION_BG_RGB: Color = SELECTION_BG;
    pub fn cyan() -> Color {
        rgb_or_reset(CYAN)
    }
    pub fn fg() -> Color {
        rgb_or_reset(FG)
    }
    pub fn muted() -> Color {
        rgb_or_reset(MUTED)
    }
}
