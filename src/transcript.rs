use std::collections::VecDeque;

const MAX_TRANSCRIPT_OUTPUT_LINES: usize = 4_000;
const MAX_TRANSCRIPT_INPUT_LINES: usize = 100;

#[derive(Default)]
pub struct TranscriptBuffer {
    output_lines: VecDeque<String>,
    input_lines: VecDeque<String>,
}

impl TranscriptBuffer {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub fn replace_output(&mut self, snapshot: &str) {
        self.output_lines = snapshot
            .replace('\r', "")
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .rev()
            .take(MAX_TRANSCRIPT_OUTPUT_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn push_output(&mut self, text: &str) {
        for line in text
            .replace('\r', "")
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
        {
            self.output_lines.push_back(line.to_string());
            while self.output_lines.len() > MAX_TRANSCRIPT_OUTPUT_LINES {
                self.output_lines.pop_front();
            }
        }
    }

    pub fn push_input(&mut self, text: &str) {
        for line in text
            .replace('\r', "")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            self.input_lines.push_back(line.to_string());
            while self.input_lines.len() > MAX_TRANSCRIPT_INPUT_LINES {
                self.input_lines.pop_front();
            }
        }
    }

    pub fn recent_output(&self, row_count: usize) -> String {
        let skip_count = self.output_lines.len().saturating_sub(row_count);
        let mut text = String::new();
        for line in self.output_lines.iter().skip(skip_count) {
            append_line(&mut text, line);
        }
        text
    }

    pub fn recent_transcript(&self, line_count: usize) -> String {
        let input_line_count = if self.input_lines.is_empty() {
            0
        } else {
            1 + self.input_lines.len()
        };
        let total_line_count = self.output_lines.len() + input_line_count;
        let first_index = total_line_count.saturating_sub(line_count);
        let mut current_index = 0;
        let mut text = String::new();

        for line in &self.output_lines {
            if current_index >= first_index {
                append_line(&mut text, line);
            }
            current_index += 1;
        }

        if !self.input_lines.is_empty() {
            if current_index >= first_index {
                append_line(&mut text, "[input]");
            }
            current_index += 1;
            for line in &self.input_lines {
                if current_index >= first_index {
                    append_prefixed_line(&mut text, "> ", line);
                }
                current_index += 1;
            }
        }

        text
    }
}

fn append_line(text: &mut String, line: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(line);
}

fn append_prefixed_line(text: &mut String, prefix: &str, line: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(prefix);
    text.push_str(line);
}

#[cfg(test)]
mod tests {
    use super::TranscriptBuffer;

    #[test]
    fn recent_output_keeps_original_order_after_truncation() {
        let mut buffer = TranscriptBuffer::default();
        buffer.push_output("one\ntwo\nthree\n");

        assert_eq!(buffer.recent_output(2), "two\nthree");
    }

    #[test]
    fn recent_transcript_includes_input_marker_and_truncates_from_front() {
        let mut buffer = TranscriptBuffer::default();
        buffer.push_output("out-1\nout-2\n");
        buffer.push_input("cmd-1\ncmd-2\n");

        assert_eq!(buffer.recent_transcript(3), "[input]\n> cmd-1\n> cmd-2");
        assert_eq!(
            buffer.recent_transcript(5),
            "out-1\nout-2\n[input]\n> cmd-1\n> cmd-2"
        );
    }
}
