pub const DEFAULT_TERMINAL_HISTORY_LINES: u32 = 2_000;
pub const MAX_TERMINAL_HISTORY_LINES: u32 = 20_000;

pub fn normalize_saved_terminal_history_line_limit(lines: u32) -> u32 {
    lines.min(MAX_TERMINAL_HISTORY_LINES)
}

pub fn normalize_terminal_history_lines(text: &str, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    if lines.len() > max_lines {
        lines.split_off(lines.len() - max_lines)
    } else {
        lines
    }
}

pub fn restored_terminal_history_text(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let mut rendered = format!(
        "\r\n[terminaltiler] restored previous terminal history ({} line{})\r\n",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" }
    );
    for line in lines {
        rendered.push_str(line);
        rendered.push_str("\r\n");
    }
    rendered.push_str("[terminaltiler] end restored terminal history\r\n");
    rendered
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TERMINAL_HISTORY_LINES, normalize_saved_terminal_history_line_limit,
        normalize_terminal_history_lines, restored_terminal_history_text,
    };

    #[test]
    fn zero_history_limit_disables_capture() {
        assert!(normalize_terminal_history_lines("one\ntwo", 0).is_empty());
    }

    #[test]
    fn trims_to_last_requested_lines() {
        assert_eq!(
            normalize_terminal_history_lines("one\ntwo\nthree\nfour", 2),
            vec!["three".to_string(), "four".to_string()]
        );
    }

    #[test]
    fn normalizes_line_endings_and_trailing_blank_rows() {
        assert_eq!(
            normalize_terminal_history_lines("one\r\ntwo\r\n\r\n", 10),
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn restored_history_text_has_terminal_markers_and_final_newline() {
        let rendered = restored_terminal_history_text(&["one".into(), "two".into()]);
        assert!(rendered.contains("restored previous terminal history (2 lines)"));
        assert!(rendered.contains("one\r\ntwo\r\n"));
        assert!(rendered.ends_with("\r\n"));
    }

    #[test]
    fn saved_line_limit_is_clamped_to_supported_maximum() {
        assert_eq!(
            normalize_saved_terminal_history_line_limit(MAX_TERMINAL_HISTORY_LINES + 1),
            MAX_TERMINAL_HISTORY_LINES
        );
    }
}
