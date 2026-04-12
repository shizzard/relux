use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

// ── Border ──────────────────────────────────────────────────────────────────

pub const BORDER: Style = Style::new().fg(Color::DarkGray);
pub const BORDER_FOCUSED: Style = Style::new().fg(Color::Green);

// ── Title ───────────────────────────────────────────────────────────────────

pub const TITLE: Style = Style::new().fg(Color::White);

// ── Hotkey ──────────────────────────────────────────────────────────────────

pub const HOTKEY_ACTIVE: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
pub const HOTKEY_LABEL: Style = Style::new().fg(Color::White);
pub const HOTKEY_INACTIVE: Style = Style::new().fg(Color::DarkGray);

// ── Input ───────────────────────────────────────────────────────────────────

pub const INPUT_EDITING: Style = Style::new().fg(Color::Yellow);
pub const INPUT_IDLE: Style = Style::new().fg(Color::White);

// ── Hints ───────────────────────────────────────────────────────────────────

pub const HINT: Style = Style::new().fg(Color::DarkGray);

// ── File tree ───────────────────────────────────────────────────────────────

pub const FILE_CURSOR: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
pub const FILE_NAME: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
pub const DIR_NAME: Style = Style::new().fg(Color::DarkGray);

// ── Details ─────────────────────────────────────────────────────────────────

pub const TEST_NAME: Style = Style::new().fg(Color::Green);
pub const TEST_DOCSTRING: Style = Style::new().fg(Color::DarkGray);

// ── Help overlay ────────────────────────────────────────────────────────────

pub const HELP_BORDER: Style = Style::new().fg(Color::Yellow);
pub const HELP_LAYER_NAME: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const HELP_KEY: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
pub const HELP_DESCRIPTION: Style = Style::new().fg(Color::White);
