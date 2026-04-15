#![allow(dead_code)]

use std::collections::VecDeque;

const MAX_TRANSCRIPT_OUTPUT_LINES: usize = 4_000;
const MAX_TRANSCRIPT_INPUT_LINES: usize = 100;

#[derive(Default)]
pub struct TranscriptBuffer {
    output_lines: VecDeque<String>,
    input_lines: VecDeque<String>,
}

impl TranscriptBuffer {
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

    #[allow(dead_code)]
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
        self.output_lines
            .iter()
            .rev()
            .take(row_count)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn recent_transcript(&self, line_count: usize) -> String {
        let mut lines = self.output_lines.iter().cloned().collect::<Vec<_>>();
        if !self.input_lines.is_empty() {
            lines.push(String::from("[input]"));
            lines.extend(self.input_lines.iter().map(|line| format!("> {line}")));
        }
        lines
            .into_iter()
            .rev()
            .take(line_count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }
}
