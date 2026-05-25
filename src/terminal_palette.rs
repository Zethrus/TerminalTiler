#[derive(Clone, Copy)]
pub(crate) struct TerminalPalette {
    pub foreground: &'static str,
    pub background: &'static str,
    pub cursor: &'static str,
    pub cursor_foreground: &'static str,
    pub highlight_background: &'static str,
    pub highlight_foreground: &'static str,
    pub palette: &'static [&'static str; 16],
}

const DARK_TERMINAL_PALETTE: [&str; 16] = [
    "#0f1724", "#c9575f", "#78a062", "#d6a04b", "#6b8cff", "#b28cf0", "#5eb8c8", "#d7dde8",
    "#334155", "#ef7c86", "#91be78", "#e6bb6a", "#8fa7ff", "#c8a6f6", "#7ccad7", "#f8fafc",
];

const LIGHT_TERMINAL_PALETTE: [&str; 16] = [
    "#24313f", "#b24f45", "#617d43", "#9b6d11", "#4168b5", "#8b61a8", "#2f7f8a", "#d6dde8",
    "#516172", "#cf685d", "#78975a", "#b38622", "#5e81ca", "#a47dc1", "#4f97a2", "#f7f2e8",
];

pub(crate) fn terminal_palette(use_dark_palette: bool) -> TerminalPalette {
    if use_dark_palette {
        TerminalPalette {
            foreground: "#d7dde8",
            background: "#0f1724",
            cursor: "#f2b35f",
            cursor_foreground: "#101923",
            highlight_background: "#27405f",
            highlight_foreground: "#f8fafc",
            palette: &DARK_TERMINAL_PALETTE,
        }
    } else {
        TerminalPalette {
            foreground: "#223041",
            background: "#f4efe4",
            cursor: "#cb7a2b",
            cursor_foreground: "#fffaf1",
            highlight_background: "#d7e2f2",
            highlight_foreground: "#16202b",
            palette: &LIGHT_TERMINAL_PALETTE,
        }
    }
}
