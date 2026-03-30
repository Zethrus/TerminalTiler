use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VtColor {
    DefaultForeground,
    DefaultBackground,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTrackingMode {
    Disabled,
    Click,
    Drag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellIntegrationPhase {
    PromptStart,
    PromptEnd,
    CommandStart,
    CommandEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtStyle {
    pub fg: VtColor,
    pub bg: VtColor,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hyperlink_id: Option<u32>,
}

impl Default for VtStyle {
    fn default() -> Self {
        Self {
            fg: VtColor::DefaultForeground,
            bg: VtColor::DefaultBackground,
            bold: false,
            underline: false,
            inverse: false,
            hyperlink_id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtCell {
    pub ch: char,
    pub style: VtStyle,
}

impl Default for VtCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: VtStyle::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VtPosition {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Debug)]
struct SavedScreen {
    cells: Vec<VtCell>,
    history: VecDeque<Vec<VtCell>>,
    viewport_offset: usize,
    cursor_col: usize,
    cursor_row: usize,
    saved_cursor_col: usize,
    saved_cursor_row: usize,
    scroll_top: usize,
    scroll_bottom: usize,
}

#[derive(Clone, Debug)]
pub struct VtBuffer {
    columns: usize,
    rows: usize,
    cells: Vec<VtCell>,
    history: VecDeque<Vec<VtCell>>,
    viewport_offset: usize,
    cursor_col: usize,
    cursor_row: usize,
    saved_cursor_col: usize,
    saved_cursor_row: usize,
    style: VtStyle,
    cursor_visible: bool,
    application_cursor_keys: bool,
    autowrap: bool,
    origin_mode: bool,
    bracketed_paste: bool,
    alternate_screen: bool,
    mouse_tracking: MouseTrackingMode,
    sgr_mouse_mode: bool,
    focus_reporting: bool,
    window_title: Option<String>,
    title_stack: Vec<String>,
    current_working_directory: Option<String>,
    shell_integration_phase: Option<ShellIntegrationPhase>,
    shell_integration_command: Option<String>,
    last_command_status: Option<i32>,
    hyperlinks: Vec<String>,
    current_hyperlink: Option<u32>,
    pending_input: Vec<u8>,
    pending_clipboard_write: Option<String>,
    primary_screen: Option<SavedScreen>,
    scroll_top: usize,
    scroll_bottom: usize,
    parser_state: ParserState,
}

#[derive(Clone, Debug, Default)]
enum ParserState {
    #[default]
    Ground,
    Escape,
    Csi(String),
    Osc(String),
    OscEscape(String),
}

impl VtBuffer {
    pub fn new(columns: usize, rows: usize) -> Self {
        let columns = columns.max(1);
        let rows = rows.max(1);
        Self {
            columns,
            rows,
            cells: vec![VtCell::default(); columns * rows],
            history: VecDeque::new(),
            viewport_offset: 0,
            cursor_col: 0,
            cursor_row: 0,
            saved_cursor_col: 0,
            saved_cursor_row: 0,
            style: VtStyle::default(),
            cursor_visible: true,
            application_cursor_keys: false,
            autowrap: true,
            origin_mode: false,
            bracketed_paste: false,
            alternate_screen: false,
            mouse_tracking: MouseTrackingMode::Disabled,
            sgr_mouse_mode: false,
            focus_reporting: false,
            window_title: None,
            title_stack: Vec::new(),
            current_working_directory: None,
            shell_integration_phase: None,
            shell_integration_command: None,
            last_command_status: None,
            hyperlinks: Vec::new(),
            current_hyperlink: None,
            pending_input: Vec::new(),
            pending_clipboard_write: None,
            primary_screen: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            parser_state: ParserState::Ground,
        }
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_col, self.cursor_row)
    }

    pub fn window_title(&self) -> Option<&str> {
        self.window_title.as_deref()
    }

    pub fn current_working_directory(&self) -> Option<&str> {
        self.current_working_directory.as_deref()
    }

    pub fn shell_integration_phase(&self) -> Option<ShellIntegrationPhase> {
        self.shell_integration_phase
    }

    pub fn shell_integration_command(&self) -> Option<&str> {
        self.shell_integration_command.as_deref()
    }

    pub fn last_command_status(&self) -> Option<i32> {
        self.last_command_status
    }

    pub fn hyperlink_at(&self, row: usize, column: usize) -> Option<&str> {
        self.visible_cell(row, column)
            .style
            .hyperlink_id
            .and_then(|id| self.hyperlinks.get(id as usize))
            .map(String::as_str)
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
    }

    pub fn autowrap(&self) -> bool {
        self.autowrap
    }

    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    pub fn mouse_tracking(&self) -> MouseTrackingMode {
        self.mouse_tracking
    }

    pub fn sgr_mouse_mode(&self) -> bool {
        self.sgr_mouse_mode
    }

    pub fn focus_reporting(&self) -> bool {
        self.focus_reporting
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    pub fn take_pending_input(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_input)
    }

    pub fn take_pending_clipboard_write(&mut self) -> Option<String> {
        self.pending_clipboard_write.take()
    }

    pub fn has_selection(&self, start: VtPosition, end: VtPosition) -> bool {
        start != end
    }

    pub fn scroll_viewport(&mut self, delta_rows: isize) -> bool {
        if self.alternate_screen || delta_rows == 0 {
            return false;
        }

        let previous = self.viewport_offset;
        let max_offset = self.history.len();
        if delta_rows > 0 {
            self.viewport_offset = (self.viewport_offset + delta_rows as usize).min(max_offset);
        } else {
            self.viewport_offset = self.viewport_offset.saturating_sub((-delta_rows) as usize);
        }

        self.viewport_offset != previous
    }

    pub fn reset_viewport(&mut self) -> bool {
        if self.viewport_offset == 0 {
            return false;
        }
        self.viewport_offset = 0;
        true
    }

    pub fn word_selection_at(&self, position: VtPosition) -> (VtPosition, VtPosition) {
        let row = position.row.min(self.rows.saturating_sub(1));
        let column = position.column.min(self.columns.saturating_sub(1));
        let current = self.visible_cell(row, column).ch;

        if current == ' ' {
            return (VtPosition { row, column }, VtPosition { row, column });
        }

        let predicate = |ch: char| classify_word_char(ch) == classify_word_char(current);
        let mut start = column;
        while start > 0 {
            let next = self.visible_cell(row, start - 1).ch;
            if !predicate(next) {
                break;
            }
            start -= 1;
        }

        let mut end = column;
        while end + 1 < self.columns {
            let next = self.visible_cell(row, end + 1).ch;
            if !predicate(next) {
                break;
            }
            end += 1;
        }

        (
            VtPosition { row, column: start },
            VtPosition { row, column: end },
        )
    }

    pub fn visible_cell(&self, row: usize, column: usize) -> VtCell {
        let row = row.min(self.rows.saturating_sub(1));
        let column = column.min(self.columns.saturating_sub(1));
        let global_row = self.viewport_start_row() + row;
        self.global_cell(global_row, column)
    }

    pub fn cursor_in_view(&self) -> Option<(usize, usize)> {
        let global_cursor_row = self.history.len() + self.cursor_row;
        let viewport_start = self.viewport_start_row();
        let viewport_end = viewport_start + self.rows;
        if global_cursor_row < viewport_start || global_cursor_row >= viewport_end {
            None
        } else {
            Some((self.cursor_col, global_cursor_row - viewport_start))
        }
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&VtCell> {
        if row >= self.rows || column >= self.columns {
            None
        } else {
            self.cells.get((row * self.columns) + column)
        }
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(1);
        let rows = rows.max(1);
        if columns == self.columns && rows == self.rows {
            return;
        }

        for line in &mut self.history {
            resize_line(line, columns);
        }

        let mut resized = vec![VtCell::default(); columns * rows];
        let copy_rows = self.rows.min(rows);
        let copy_columns = self.columns.min(columns);
        for row in 0..copy_rows {
            for column in 0..copy_columns {
                let source = (row * self.columns) + column;
                let target = (row * columns) + column;
                resized[target] = self.cells[source];
            }
        }

        self.columns = columns;
        self.rows = rows;
        self.cells = resized;
        self.cursor_col = self.cursor_col.min(self.columns - 1);
        self.cursor_row = self.cursor_row.min(self.rows - 1);
        self.saved_cursor_col = self.saved_cursor_col.min(self.columns - 1);
        self.saved_cursor_row = self.saved_cursor_row.min(self.rows - 1);
        self.scroll_top = self.scroll_top.min(self.rows.saturating_sub(1));
        self.scroll_bottom = self
            .scroll_bottom
            .clamp(self.scroll_top, self.rows.saturating_sub(1));
        self.viewport_offset = self.viewport_offset.min(self.history.len());
    }

    pub fn process(&mut self, input: &str) {
        for character in input.chars() {
            match &mut self.parser_state {
                ParserState::Ground => self.process_ground(character),
                ParserState::Escape => self.process_escape(character),
                ParserState::Csi(sequence) => {
                    if ('@'..='~').contains(&character) {
                        sequence.push(character);
                        let sequence = std::mem::take(sequence);
                        self.apply_csi(&sequence);
                        self.parser_state = ParserState::Ground;
                    } else {
                        sequence.push(character);
                    }
                }
                ParserState::Osc(sequence) => match character {
                    '\u{07}' => {
                        let sequence = std::mem::take(sequence);
                        self.apply_osc(&sequence);
                        self.parser_state = ParserState::Ground;
                    }
                    '\u{1b}' => {
                        let sequence = std::mem::take(sequence);
                        self.parser_state = ParserState::OscEscape(sequence);
                    }
                    _ => sequence.push(character),
                },
                ParserState::OscEscape(sequence) => {
                    if character == '\\' {
                        let sequence = std::mem::take(sequence);
                        self.apply_osc(&sequence);
                        self.parser_state = ParserState::Ground;
                    } else {
                        sequence.push('\u{1b}');
                        sequence.push(character);
                        let sequence = std::mem::take(sequence);
                        self.parser_state = ParserState::Osc(sequence);
                    }
                }
            }
        }
    }

    pub fn selection_text(&self, start: VtPosition, end: VtPosition) -> String {
        let (start, end) = normalize_positions(start, end);
        let mut lines = Vec::new();

        for row in start.row..=end.row {
            let first_column = if row == start.row { start.column } else { 0 };
            let last_column = if row == end.row {
                end.column.min(self.columns.saturating_sub(1))
            } else {
                self.columns.saturating_sub(1)
            };
            let mut line = String::new();
            for column in first_column..=last_column {
                line.push(self.visible_cell(row, column).ch);
            }
            while line.ends_with(' ') {
                line.pop();
            }
            lines.push(line);
        }

        lines.join("\r\n")
    }

    fn process_ground(&mut self, character: char) {
        match character {
            '\u{1b}' => self.parser_state = ParserState::Escape,
            '\n' => self.line_feed(),
            '\r' => self.cursor_col = 0,
            '\u{8}' => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            '\t' => {
                let next_stop = ((self.cursor_col / 8) + 1) * 8;
                self.cursor_col = next_stop.min(self.columns.saturating_sub(1));
            }
            character if !character.is_control() => self.write_character(character),
            _ => {}
        }
    }

    fn process_escape(&mut self, character: char) {
        match character {
            '[' => self.parser_state = ParserState::Csi(String::new()),
            ']' => self.parser_state = ParserState::Osc(String::new()),
            '7' => {
                self.saved_cursor_col = self.cursor_col;
                self.saved_cursor_row = self.cursor_row;
                self.parser_state = ParserState::Ground;
            }
            '8' => {
                self.cursor_col = self.saved_cursor_col.min(self.columns - 1);
                self.cursor_row = self.saved_cursor_row.min(self.rows - 1);
                self.parser_state = ParserState::Ground;
            }
            'D' => {
                self.line_feed();
                self.parser_state = ParserState::Ground;
            }
            'E' => {
                self.line_feed();
                self.cursor_col = 0;
                self.parser_state = ParserState::Ground;
            }
            'M' => {
                self.reverse_index();
                self.parser_state = ParserState::Ground;
            }
            'c' => {
                self.hard_reset();
                self.parser_state = ParserState::Ground;
            }
            _ => {
                self.parser_state = ParserState::Ground;
            }
        }
    }

    fn apply_csi(&mut self, sequence: &str) {
        let Some(command) = sequence.chars().last() else {
            return;
        };
        let params = &sequence[..sequence.len().saturating_sub(command.len_utf8())];
        let values = parse_csi_params(params);
        let prefix = csi_prefix(params);

        match command {
            'A' => self.cursor_row = self.cursor_row.saturating_sub(param_or(&values, 0, 1)),
            'B' => {
                self.cursor_row =
                    (self.cursor_row + param_or(&values, 0, 1)).min(self.rows.saturating_sub(1))
            }
            'C' => {
                self.cursor_col =
                    (self.cursor_col + param_or(&values, 0, 1)).min(self.columns.saturating_sub(1))
            }
            'D' => self.cursor_col = self.cursor_col.saturating_sub(param_or(&values, 0, 1)),
            'E' => {
                self.cursor_row =
                    (self.cursor_row + param_or(&values, 0, 1)).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
            }
            'F' => {
                self.cursor_row = self.cursor_row.saturating_sub(param_or(&values, 0, 1));
                self.cursor_col = 0;
            }
            'G' => {
                self.cursor_col = param_or(&values, 0, 1)
                    .saturating_sub(1)
                    .min(self.columns.saturating_sub(1));
            }
            'H' | 'f' => {
                let row = self.resolve_row_param(param_or(&values, 0, 1));
                let col = param_or(&values, 1, 1).saturating_sub(1);
                self.cursor_row = row;
                self.cursor_col = col.min(self.columns.saturating_sub(1));
            }
            'J' => match values.first().copied().unwrap_or(0) {
                0 => self.clear_from_cursor_to_end(),
                1 => self.clear_from_start_to_cursor(),
                2 => {
                    self.clear_screen();
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                }
                _ => {}
            },
            'K' => match values.first().copied().unwrap_or(0) {
                0 => self.clear_line_from_cursor(),
                1 => self.clear_line_to_cursor(),
                2 => self.clear_line(self.cursor_row),
                _ => {}
            },
            'L' => self.insert_lines(param_or(&values, 0, 1)),
            'M' => self.delete_lines(param_or(&values, 0, 1)),
            '@' => self.insert_chars(param_or(&values, 0, 1)),
            'P' => self.delete_chars(param_or(&values, 0, 1)),
            'X' => self.erase_chars(param_or(&values, 0, 1)),
            'd' => self.cursor_row = self.resolve_row_param(param_or(&values, 0, 1)),
            'S' => self.scroll_up(param_or(&values, 0, 1)),
            'T' => self.scroll_down(param_or(&values, 0, 1)),
            'a' => {
                self.cursor_col =
                    (self.cursor_col + param_or(&values, 0, 1)).min(self.columns.saturating_sub(1))
            }
            'b' => self.repeat_last_character(param_or(&values, 0, 1)),
            'e' => {
                self.cursor_row =
                    (self.cursor_row + param_or(&values, 0, 1)).min(self.rows.saturating_sub(1))
            }
            'm' => self.apply_sgr(&values),
            'c' => self.apply_device_attributes(prefix, &values),
            'n' => self.apply_device_status_report(prefix, &values),
            's' => {
                self.saved_cursor_col = self.cursor_col;
                self.saved_cursor_row = self.cursor_row;
            }
            'u' => {
                self.cursor_col = self.saved_cursor_col.min(self.columns - 1);
                self.cursor_row = self.saved_cursor_row.min(self.rows - 1);
            }
            'r' => self.set_scroll_region(&values),
            'p' if prefix == Some('!') => self.soft_reset(),
            'h' if prefix == Some('?') => self.apply_private_mode(&values, true),
            'l' if prefix == Some('?') => self.apply_private_mode(&values, false),
            _ => {}
        }
    }

    fn apply_osc(&mut self, sequence: &str) {
        let Some((command, value)) = sequence.split_once(';') else {
            return;
        };

        match command {
            "0" | "1" | "2" => {
                let title = value.trim();
                self.window_title = (!title.is_empty()).then(|| title.to_string());
            }
            "8" => self.apply_osc8(value),
            "22" => self.push_title(),
            "23" => self.pop_title(),
            "52" => self.apply_osc52(value),
            "7" => {
                let cwd = value.trim();
                self.current_working_directory = parse_osc7_path(cwd);
            }
            "133" => self.apply_osc133(value),
            _ => {}
        }
    }

    fn apply_osc8(&mut self, value: &str) {
        let Some((_, uri)) = value.split_once(';') else {
            return;
        };
        let uri = uri.trim();
        if uri.is_empty() {
            self.current_hyperlink = None;
            self.style.hyperlink_id = None;
            return;
        }

        let hyperlink_id = self.hyperlinks.len() as u32;
        self.hyperlinks.push(uri.to_string());
        self.current_hyperlink = Some(hyperlink_id);
        self.style.hyperlink_id = Some(hyperlink_id);
    }

    fn push_title(&mut self) {
        if let Some(title) = self.window_title.as_ref() {
            self.title_stack.push(title.clone());
        }
    }

    fn pop_title(&mut self) {
        self.window_title = self.title_stack.pop();
    }

    fn apply_osc52(&mut self, value: &str) {
        let Some((selection, payload)) = value.split_once(';') else {
            return;
        };
        let selection = selection.trim();
        if !selection.is_empty() && !selection.contains(['c', 'p', 's', '0']) {
            return;
        }

        if payload == "?" {
            return;
        }

        if payload.is_empty() {
            self.pending_clipboard_write = Some(String::new());
            return;
        }

        if let Some(decoded) = decode_base64(payload.trim()) {
            self.pending_clipboard_write = String::from_utf8(decoded).ok();
        }
    }

    fn apply_osc133(&mut self, value: &str) {
        let mut parts = value.split(';');
        let marker = parts.next().unwrap_or_default().trim();
        match marker {
            "A" => {
                self.shell_integration_phase = Some(ShellIntegrationPhase::PromptStart);
                self.shell_integration_command = None;
            }
            "B" => self.shell_integration_phase = Some(ShellIntegrationPhase::CommandStart),
            "C" => self.shell_integration_phase = Some(ShellIntegrationPhase::CommandEnd),
            "D" => {
                self.shell_integration_phase = Some(ShellIntegrationPhase::PromptEnd);
                self.last_command_status = parts.next().and_then(|status| status.parse().ok());
                self.shell_integration_command = None;
            }
            "L" => {
                let command = parts.collect::<Vec<_>>().join(";");
                self.shell_integration_command = (!command.trim().is_empty()).then_some(command);
            }
            _ => {}
        }
    }

    fn apply_sgr(&mut self, values: &[usize]) {
        if values.is_empty() {
            self.style = VtStyle::default();
            self.style.hyperlink_id = self.current_hyperlink;
            return;
        }

        let mut index = 0usize;
        while let Some(value) = values.get(index).copied() {
            index += 1;
            match value {
                0 => self.style = VtStyle::default(),
                1 => self.style.bold = true,
                22 => self.style.bold = false,
                4 => self.style.underline = true,
                24 => self.style.underline = false,
                7 => self.style.inverse = true,
                27 => self.style.inverse = false,
                30..=37 => self.style.fg = VtColor::Indexed((value - 30) as u8),
                39 => self.style.fg = VtStyle::default().fg,
                40..=47 => self.style.bg = VtColor::Indexed((value - 40) as u8),
                49 => self.style.bg = VtStyle::default().bg,
                90..=97 => self.style.fg = VtColor::Indexed((value - 90 + 8) as u8),
                100..=107 => self.style.bg = VtColor::Indexed((value - 100 + 8) as u8),
                38 => {
                    if let Some((color, consumed)) =
                        parse_extended_sgr_color(&values[index..], true)
                    {
                        self.style.fg = color;
                        index += consumed;
                    }
                }
                48 => {
                    if let Some((color, consumed)) =
                        parse_extended_sgr_color(&values[index..], false)
                    {
                        self.style.bg = color;
                        index += consumed;
                    }
                }
                _ => {}
            }

            self.style.hyperlink_id = self.current_hyperlink;
        }
    }

    fn apply_private_mode(&mut self, values: &[usize], enabled: bool) {
        for value in values {
            match *value {
                1 => self.application_cursor_keys = enabled,
                6 => {
                    self.origin_mode = enabled;
                    self.cursor_to_home();
                }
                7 => self.autowrap = enabled,
                25 => self.cursor_visible = enabled,
                2004 => self.bracketed_paste = enabled,
                1000 => {
                    if enabled {
                        self.mouse_tracking = MouseTrackingMode::Click;
                    } else if self.mouse_tracking == MouseTrackingMode::Click {
                        self.mouse_tracking = MouseTrackingMode::Disabled;
                    }
                }
                1002 | 1003 => {
                    if enabled {
                        self.mouse_tracking = MouseTrackingMode::Drag;
                    } else if self.mouse_tracking == MouseTrackingMode::Drag {
                        self.mouse_tracking = MouseTrackingMode::Disabled;
                    }
                }
                1004 => self.focus_reporting = enabled,
                1006 => self.sgr_mouse_mode = enabled,
                1048 => {
                    if enabled {
                        self.saved_cursor_col = self.cursor_col;
                        self.saved_cursor_row = self.cursor_row;
                    } else {
                        self.cursor_col = self.saved_cursor_col.min(self.columns - 1);
                        self.cursor_row = self.saved_cursor_row.min(self.rows - 1);
                    }
                }
                47 | 1047 | 1049 => self.set_alternate_screen(enabled),
                _ => {}
            }
        }
    }

    fn apply_device_attributes(&mut self, prefix: Option<char>, values: &[usize]) {
        let primary_request = prefix.is_none() && values.is_empty();
        let secondary_request =
            prefix == Some('>') && (values.is_empty() || values.first().copied() == Some(0));

        if primary_request {
            self.queue_response("\u{1b}[?1;2c");
        } else if secondary_request {
            self.queue_response("\u{1b}[>0;10;1c");
        }
    }

    fn apply_device_status_report(&mut self, prefix: Option<char>, values: &[usize]) {
        let Some(code) = values.first().copied() else {
            return;
        };

        match (prefix, code) {
            (None, 5) => self.queue_response("\u{1b}[0n"),
            (None, 6) => self.queue_response(&format!(
                "\u{1b}[{};{}R",
                self.cursor_row + 1,
                self.cursor_col + 1
            )),
            _ => {}
        }
    }

    fn queue_response(&mut self, response: &str) {
        self.pending_input.extend_from_slice(response.as_bytes());
    }

    fn soft_reset(&mut self) {
        self.style = VtStyle::default();
        self.style.hyperlink_id = self.current_hyperlink;
        self.cursor_visible = true;
        self.application_cursor_keys = false;
        self.autowrap = true;
        self.origin_mode = false;
        self.bracketed_paste = false;
        self.mouse_tracking = MouseTrackingMode::Disabled;
        self.sgr_mouse_mode = false;
        self.focus_reporting = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.cursor_to_home();
    }

    fn hard_reset(&mut self) {
        self.cells.fill(VtCell::default());
        self.history.clear();
        self.viewport_offset = 0;
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.saved_cursor_col = 0;
        self.saved_cursor_row = 0;
        self.window_title = None;
        self.title_stack.clear();
        self.current_working_directory = None;
        self.shell_integration_phase = None;
        self.shell_integration_command = None;
        self.last_command_status = None;
        self.hyperlinks.clear();
        self.current_hyperlink = None;
        self.pending_input.clear();
        self.pending_clipboard_write = None;
        self.primary_screen = None;
        self.alternate_screen = false;
        self.soft_reset();
    }

    fn set_alternate_screen(&mut self, enabled: bool) {
        if enabled == self.alternate_screen {
            return;
        }

        if enabled {
            self.primary_screen = Some(SavedScreen {
                cells: std::mem::take(&mut self.cells),
                history: std::mem::take(&mut self.history),
                viewport_offset: self.viewport_offset,
                cursor_col: self.cursor_col,
                cursor_row: self.cursor_row,
                saved_cursor_col: self.saved_cursor_col,
                saved_cursor_row: self.saved_cursor_row,
                scroll_top: self.scroll_top,
                scroll_bottom: self.scroll_bottom,
            });
            self.cells = vec![VtCell::default(); self.columns * self.rows];
            self.history.clear();
            self.viewport_offset = 0;
            self.cursor_col = 0;
            self.cursor_row = 0;
            self.saved_cursor_col = 0;
            self.saved_cursor_row = 0;
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        } else if let Some(saved) = self.primary_screen.take() {
            self.cells = saved.cells;
            self.history = saved.history;
            self.viewport_offset = saved.viewport_offset.min(self.history.len());
            self.cursor_col = saved.cursor_col.min(self.columns.saturating_sub(1));
            self.cursor_row = saved.cursor_row.min(self.rows.saturating_sub(1));
            self.saved_cursor_col = saved.saved_cursor_col.min(self.columns.saturating_sub(1));
            self.saved_cursor_row = saved.saved_cursor_row.min(self.rows.saturating_sub(1));
            self.scroll_top = saved.scroll_top.min(self.rows.saturating_sub(1));
            self.scroll_bottom = saved
                .scroll_bottom
                .clamp(self.scroll_top, self.rows.saturating_sub(1));
        }

        self.alternate_screen = enabled;
    }

    fn write_character(&mut self, character: char) {
        let index = (self.cursor_row * self.columns) + self.cursor_col;
        if let Some(cell) = self.cells.get_mut(index) {
            *cell = VtCell {
                ch: character,
                style: self.style,
            };
        }

        if self.cursor_col + 1 >= self.columns {
            if self.autowrap {
                self.cursor_col = 0;
                self.line_feed();
            } else {
                self.cursor_col = self.columns.saturating_sub(1);
            }
        } else {
            self.cursor_col += 1;
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_row >= self.scroll_top && self.cursor_row <= self.scroll_bottom {
            if self.cursor_row == self.scroll_bottom {
                self.scroll_region_up(self.scroll_top, self.scroll_bottom, 1);
            } else {
                self.cursor_row += 1;
            }
        } else if self.cursor_row + 1 >= self.rows {
            self.scroll_up(1);
        } else {
            self.cursor_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.cursor_row >= self.scroll_top
            && self.cursor_row <= self.scroll_bottom
            && self.cursor_row == self.scroll_top
        {
            self.scroll_region_down(self.scroll_top, self.scroll_bottom, 1);
        } else if self.cursor_row == 0 {
            self.scroll_down(1);
        } else {
            self.cursor_row -= 1;
        }
    }

    fn insert_lines(&mut self, lines: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }

        let lines = lines.min(self.scroll_bottom.saturating_sub(self.cursor_row) + 1);
        if lines == 0 {
            return;
        }

        for row in (self.cursor_row..=(self.scroll_bottom - lines)).rev() {
            self.copy_row(row, row + lines);
        }
        for row in self.cursor_row..(self.cursor_row + lines) {
            self.clear_line(row);
        }
    }

    fn delete_lines(&mut self, lines: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }

        let lines = lines.min(self.scroll_bottom.saturating_sub(self.cursor_row) + 1);
        if lines == 0 {
            return;
        }

        for row in self.cursor_row..=(self.scroll_bottom - lines) {
            self.copy_row(row + lines, row);
        }
        for row in (self.scroll_bottom + 1 - lines)..=self.scroll_bottom {
            self.clear_line(row);
        }
    }

    fn insert_chars(&mut self, count: usize) {
        let count = count.min(self.columns.saturating_sub(self.cursor_col));
        if count == 0 {
            return;
        }

        let row = self.cursor_row;
        for column in (self.cursor_col..(self.columns - count)).rev() {
            self.cells[(row * self.columns) + column + count] =
                self.cells[(row * self.columns) + column];
        }
        for column in self.cursor_col..(self.cursor_col + count) {
            self.cells[(row * self.columns) + column] = VtCell::default();
        }
    }

    fn delete_chars(&mut self, count: usize) {
        let count = count.min(self.columns.saturating_sub(self.cursor_col));
        if count == 0 {
            return;
        }

        let row = self.cursor_row;
        for column in self.cursor_col..(self.columns - count) {
            self.cells[(row * self.columns) + column] =
                self.cells[(row * self.columns) + column + count];
        }
        for column in (self.columns - count)..self.columns {
            self.cells[(row * self.columns) + column] = VtCell::default();
        }
    }

    fn erase_chars(&mut self, count: usize) {
        let end = (self.cursor_col + count).min(self.columns);
        for column in self.cursor_col..end {
            self.cells[(self.cursor_row * self.columns) + column] = VtCell::default();
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll_region_up(0, self.rows.saturating_sub(1), lines);
    }

    fn scroll_region_up(&mut self, top: usize, bottom: usize, lines: usize) {
        let lines = lines.min(bottom.saturating_sub(top) + 1);
        if lines == 0 {
            return;
        }

        if !self.alternate_screen && top == 0 && bottom + 1 == self.rows {
            for row in 0..lines {
                self.push_history_row(top + row);
            }
            if self.viewport_offset > 0 {
                self.viewport_offset = (self.viewport_offset + lines).min(self.history.len());
            }
        }

        for row in top..=(bottom - lines) {
            self.copy_row(row + lines, row);
        }
        for row in (bottom + 1 - lines)..=bottom {
            self.clear_line(row);
        }
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_region_down(0, self.rows.saturating_sub(1), lines);
    }

    fn scroll_region_down(&mut self, top: usize, bottom: usize, lines: usize) {
        let lines = lines.min(bottom.saturating_sub(top) + 1);
        if lines == 0 {
            return;
        }

        for row in (top..=(bottom - lines)).rev() {
            self.copy_row(row, row + lines);
        }
        for row in top..(top + lines) {
            self.clear_line(row);
        }
    }

    fn set_scroll_region(&mut self, values: &[usize]) {
        let top = values.first().copied().unwrap_or(1).saturating_sub(1);
        let bottom = values
            .get(1)
            .copied()
            .unwrap_or(self.rows)
            .saturating_sub(1);
        if top >= self.rows || bottom >= self.rows || top >= bottom {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        } else {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        }
        self.cursor_to_home();
    }

    fn cursor_to_home(&mut self) {
        self.cursor_col = 0;
        self.cursor_row = if self.origin_mode { self.scroll_top } else { 0 };
    }

    fn resolve_row_param(&self, row: usize) -> usize {
        let row = row.saturating_sub(1);
        if self.origin_mode {
            (self.scroll_top + row).min(self.scroll_bottom)
        } else {
            row.min(self.rows.saturating_sub(1))
        }
    }

    fn copy_row(&mut self, source_row: usize, target_row: usize) {
        for column in 0..self.columns {
            let source = (source_row * self.columns) + column;
            let target = (target_row * self.columns) + column;
            self.cells[target] = self.cells[source];
        }
    }

    fn clear_screen(&mut self) {
        self.cells.fill(VtCell::default());
    }

    fn clear_from_start_to_cursor(&mut self) {
        for row in 0..=self.cursor_row {
            let end_column = if row == self.cursor_row {
                self.cursor_col
            } else {
                self.columns.saturating_sub(1)
            };
            for column in 0..=end_column {
                self.cells[(row * self.columns) + column] = VtCell::default();
            }
        }
    }

    fn clear_from_cursor_to_end(&mut self) {
        for row in self.cursor_row..self.rows {
            let start_column = if row == self.cursor_row {
                self.cursor_col
            } else {
                0
            };
            for column in start_column..self.columns {
                self.cells[(row * self.columns) + column] = VtCell::default();
            }
        }
    }

    fn clear_line_to_cursor(&mut self) {
        for column in 0..=self.cursor_col {
            self.cells[(self.cursor_row * self.columns) + column] = VtCell::default();
        }
    }

    fn clear_line_from_cursor(&mut self) {
        for column in self.cursor_col..self.columns {
            self.cells[(self.cursor_row * self.columns) + column] = VtCell::default();
        }
    }

    fn clear_line(&mut self, row: usize) {
        for column in 0..self.columns {
            self.cells[(row * self.columns) + column] = VtCell::default();
        }
    }

    fn repeat_last_character(&mut self, count: usize) {
        let index = self.cursor_col.saturating_sub(1);
        let character = self
            .cells
            .get((self.cursor_row * self.columns) + index)
            .copied()
            .unwrap_or_default();
        if character.ch == ' ' {
            return;
        }

        for _ in 0..count {
            self.write_character(character.ch);
        }
    }

    fn push_history_row(&mut self, row: usize) {
        let start = row * self.columns;
        let end = start + self.columns;
        self.history.push_back(self.cells[start..end].to_vec());
        while self.history.len() > 10_000 {
            self.history.pop_front();
        }
    }

    fn viewport_start_row(&self) -> usize {
        let total_rows = self.history.len() + self.rows;
        total_rows.saturating_sub(self.rows + self.viewport_offset)
    }

    fn global_cell(&self, global_row: usize, column: usize) -> VtCell {
        if global_row < self.history.len() {
            self.history
                .get(global_row)
                .and_then(|line| line.get(column))
                .copied()
                .unwrap_or_default()
        } else {
            let screen_row = global_row.saturating_sub(self.history.len());
            self.cell(screen_row, column).copied().unwrap_or_default()
        }
    }
}

fn parse_csi_params(params: &str) -> Vec<usize> {
    if params.is_empty() {
        return Vec::new();
    }

    let params = params.trim_start_matches(['?', '>', '!']);
    params
        .split(';')
        .map(|part| part.parse::<usize>().unwrap_or(0))
        .collect()
}

fn csi_prefix(params: &str) -> Option<char> {
    params
        .chars()
        .next()
        .filter(|ch| matches!(ch, '?' | '>' | '!'))
}

fn parse_extended_sgr_color(params: &[usize], foreground: bool) -> Option<(VtColor, usize)> {
    let default = if foreground {
        VtColor::DefaultForeground
    } else {
        VtColor::DefaultBackground
    };

    match params.first().copied()? {
        5 => {
            let color = params.get(1).copied().map(|value| value.min(255) as u8)?;
            Some((VtColor::Indexed(color), 2))
        }
        2 => {
            let red = params.get(1).copied().map(|value| value.min(255) as u8)?;
            let green = params.get(2).copied().map(|value| value.min(255) as u8)?;
            let blue = params.get(3).copied().map(|value| value.min(255) as u8)?;
            Some((VtColor::Rgb(red, green, blue), 4))
        }
        0 => Some((default, 1)),
        _ => None,
    }
}

fn param_or(values: &[usize], index: usize, default: usize) -> usize {
    values.get(index).copied().unwrap_or(default).max(1)
}

fn normalize_positions(start: VtPosition, end: VtPosition) -> (VtPosition, VtPosition) {
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

fn resize_line(line: &mut Vec<VtCell>, columns: usize) {
    if line.len() > columns {
        line.truncate(columns);
    } else if line.len() < columns {
        line.resize(columns, VtCell::default());
    }
}

fn parse_osc7_path(value: &str) -> Option<String> {
    let value = value.strip_prefix("file://")?;
    let path_start = value.find('/')?;
    let path = &value[path_start..];
    (!path.is_empty()).then(|| path.to_string())
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity((value.len() * 3) / 4);
    let mut quartet = [0u8; 4];
    let mut quartet_len = 0usize;
    let mut padding = 0usize;

    for ch in value.chars() {
        let decoded = match ch {
            'A'..='Z' => ch as u8 - b'A',
            'a'..='z' => ch as u8 - b'a' + 26,
            '0'..='9' => ch as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            '=' => {
                padding += 1;
                0
            }
            '\r' | '\n' => continue,
            _ => return None,
        };
        quartet[quartet_len] = decoded;
        quartet_len += 1;

        if quartet_len == 4 {
            output.push((quartet[0] << 2) | (quartet[1] >> 4));
            if padding < 2 {
                output.push((quartet[1] << 4) | (quartet[2] >> 2));
            }
            if padding == 0 {
                output.push((quartet[2] << 6) | quartet[3]);
            }
            quartet_len = 0;
            padding = 0;
        }
    }

    (quartet_len == 0).then_some(output)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordClass {
    Space,
    Symbol,
    Word,
}

fn classify_word_char(ch: char) -> WordClass {
    if ch.is_whitespace() {
        WordClass::Space
    } else if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '~') {
        WordClass::Word
    } else {
        WordClass::Symbol
    }
}

#[cfg(test)]
mod tests {
    use super::{MouseTrackingMode, ShellIntegrationPhase, VtBuffer, VtColor, VtPosition};

    fn row_text(buffer: &VtBuffer, row: usize) -> String {
        (0..buffer.columns())
            .map(|column| buffer.cell(row, column).unwrap().ch)
            .collect()
    }

    #[test]
    fn writes_text_and_wraps_lines() {
        let mut buffer = VtBuffer::new(4, 2);
        buffer.process("abcdz");

        assert_eq!(buffer.cell(0, 0).unwrap().ch, 'a');
        assert_eq!(buffer.cell(0, 3).unwrap().ch, 'd');
        assert_eq!(buffer.cell(1, 0).unwrap().ch, 'z');
    }

    #[test]
    fn handles_cursor_moves_and_clear_screen() {
        let mut buffer = VtBuffer::new(6, 2);
        buffer.process("hello");
        buffer.process("\u{1b}[2J");
        buffer.process("\u{1b}[2;3Hok");

        assert_eq!(buffer.cell(1, 2).unwrap().ch, 'o');
        assert_eq!(buffer.cell(1, 3).unwrap().ch, 'k');
        assert_eq!(buffer.cell(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn applies_sgr_colors() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}[31mR\u{1b}[0mN");

        let red = buffer.cell(0, 0).unwrap();
        let normal = buffer.cell(0, 1).unwrap();

        assert_eq!(red.ch, 'R');
        assert_eq!(red.style.fg, VtColor::Indexed(1));
        assert_eq!(normal.ch, 'N');
        assert_eq!(normal.style.fg, VtColor::DefaultForeground);
    }

    #[test]
    fn supports_insert_delete_and_osc_title() {
        let mut buffer = VtBuffer::new(6, 2);
        buffer.process("abcdef");
        buffer.process("\u{1b}[1;1H\u{1b}[2PXY");
        buffer.process("\u{1b}]0;Build Shell\u{7}");

        assert_eq!(
            buffer.selection_text(
                VtPosition { row: 0, column: 0 },
                VtPosition { row: 0, column: 5 }
            ),
            "XYef"
        );
        assert_eq!(buffer.window_title(), Some("Build Shell"));
    }

    #[test]
    fn extracts_multiline_selection_text() {
        let mut buffer = VtBuffer::new(6, 2);
        buffer.process("ab  \r\ncd  ");

        let text = buffer.selection_text(
            VtPosition { row: 0, column: 0 },
            VtPosition { row: 1, column: 3 },
        );

        assert_eq!(text, "ab\r\ncd");
    }

    #[test]
    fn supports_extended_colors_and_cursor_visibility() {
        let mut buffer = VtBuffer::new(8, 1);
        buffer.process("\u{1b}[38;5;202;48;2;1;2;3mX\u{1b}[?25l");

        let styled = buffer.cell(0, 0).unwrap();
        assert_eq!(styled.style.fg, VtColor::Indexed(202));
        assert_eq!(styled.style.bg, VtColor::Rgb(1, 2, 3));
        assert!(!buffer.cursor_visible());
    }

    #[test]
    fn tracks_sgr_underline_state() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}[4mU\u{1b}[24mN");

        assert!(buffer.cell(0, 0).unwrap().style.underline);
        assert!(!buffer.cell(0, 1).unwrap().style.underline);
    }

    #[test]
    fn tracks_private_modes_and_reports_terminal_status() {
        let mut buffer = VtBuffer::new(8, 2);
        buffer.process("\u{1b}[?1h\u{1b}[?2004h");
        assert!(buffer.application_cursor_keys());
        assert!(buffer.bracketed_paste());

        buffer.process("\u{1b}[2;4H\u{1b}[5n\u{1b}[6n\u{1b}[c\u{1b}[>c");
        let response =
            String::from_utf8(buffer.take_pending_input()).expect("terminal response bytes");
        assert_eq!(response, "\u{1b}[0n\u{1b}[2;4R\u{1b}[?1;2c\u{1b}[>0;10;1c");
    }

    #[test]
    fn restores_primary_screen_after_alternate_screen() {
        let mut buffer = VtBuffer::new(4, 2);
        buffer.process("main");
        buffer.process("\u{1b}[?1049h");
        buffer.process("alt");
        assert_eq!(buffer.cell(0, 0).unwrap().ch, 'a');
        buffer.process("\u{1b}[?1049l");
        assert_eq!(buffer.cell(0, 0).unwrap().ch, 'm');
        assert_eq!(buffer.cell(0, 3).unwrap().ch, 'n');
    }

    #[test]
    fn expands_word_selection_for_paths() {
        let mut buffer = VtBuffer::new(24, 1);
        buffer.process("cd ~/src/project-name");
        let (start, end) = buffer.word_selection_at(VtPosition { row: 0, column: 8 });
        assert_eq!(buffer.selection_text(start, end), "~/src/project-name");
    }

    #[test]
    fn keeps_primary_scrollback_and_supports_viewport_scrolling() {
        let mut buffer = VtBuffer::new(3, 2);
        buffer.process("abcde");
        buffer.process("fgh");

        assert!(buffer.scroll_viewport(1));
        assert_eq!(buffer.visible_cell(0, 0).ch, 'a');
        assert_eq!(buffer.visible_cell(0, 1).ch, 'b');
        assert_eq!(buffer.visible_cell(1, 0).ch, 'd');
        assert!(buffer.reset_viewport());
    }

    #[test]
    fn parses_osc7_current_directory() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}]7;file://wsl.localhost/Ubuntu/home/user/project\u{7}");
        assert_eq!(
            buffer.current_working_directory(),
            Some("/Ubuntu/home/user/project")
        );
    }

    #[test]
    fn tracks_mouse_modes() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}[?1000h\u{1b}[?1006h");
        assert_eq!(buffer.mouse_tracking(), MouseTrackingMode::Click);
        assert!(buffer.sgr_mouse_mode());
        buffer.process("\u{1b}[?1002h");
        assert_eq!(buffer.mouse_tracking(), MouseTrackingMode::Drag);
        buffer.process("\u{1b}[?1002l\u{1b}[?1006l");
        assert_eq!(buffer.mouse_tracking(), MouseTrackingMode::Disabled);
        assert!(!buffer.sgr_mouse_mode());
    }

    #[test]
    fn tracks_focus_reporting_mode() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}[?1004h");
        assert!(buffer.focus_reporting());
        buffer.process("\u{1b}[?1004l");
        assert!(!buffer.focus_reporting());
    }

    #[test]
    fn decodes_osc52_clipboard_writes() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}]52;c;aGVsbG8gd29ybGQ=\u{7}");
        assert_eq!(
            buffer.take_pending_clipboard_write().as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn tracks_autowrap_mode() {
        let mut buffer = VtBuffer::new(3, 2);
        buffer.process("\u{1b}[?7labcX");

        assert!(!buffer.autowrap());
        assert_eq!(row_text(&buffer, 0), "abX");
        assert_eq!(buffer.cursor(), (2, 0));
    }

    #[test]
    fn applies_scroll_region_and_origin_mode() {
        let mut buffer = VtBuffer::new(4, 4);
        buffer.process("\u{1b}[1;1H1\u{1b}[2;1H2\u{1b}[3;1H3\u{1b}[4;1H4");
        buffer.process("\u{1b}[2;3r\u{1b}[?6h\u{1b}[1;1H\u{1b}M");

        assert_eq!(buffer.cell(0, 0).unwrap().ch, '1');
        assert_eq!(buffer.cell(1, 0).unwrap().ch, ' ');
        assert_eq!(buffer.cell(2, 0).unwrap().ch, '2');
        assert_eq!(buffer.cell(3, 0).unwrap().ch, '4');
        assert_eq!(buffer.cursor(), (0, 1));
    }

    #[test]
    fn saves_and_restores_window_title_stack() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}]0;main\u{7}");
        buffer.process("\u{1b}]22;\u{7}");
        buffer.process("\u{1b}]0;build\u{7}");
        buffer.process("\u{1b}]23;\u{7}");

        assert_eq!(buffer.window_title(), Some("main"));
    }

    #[test]
    fn tracks_osc133_shell_integration_markers() {
        let mut buffer = VtBuffer::new(4, 1);
        buffer.process("\u{1b}]133;A\u{7}");
        assert_eq!(
            buffer.shell_integration_phase(),
            Some(ShellIntegrationPhase::PromptStart)
        );
        buffer.process("\u{1b}]133;L;cargo test\u{7}");
        assert_eq!(buffer.shell_integration_command(), Some("cargo test"));
        buffer.process("\u{1b}]133;B\u{7}");
        assert_eq!(
            buffer.shell_integration_phase(),
            Some(ShellIntegrationPhase::CommandStart)
        );
        buffer.process("\u{1b}]133;C\u{7}");
        assert_eq!(
            buffer.shell_integration_phase(),
            Some(ShellIntegrationPhase::CommandEnd)
        );
        buffer.process("\u{1b}]133;D;23\u{7}");
        assert_eq!(
            buffer.shell_integration_phase(),
            Some(ShellIntegrationPhase::PromptEnd)
        );
        assert_eq!(buffer.last_command_status(), Some(23));
        assert_eq!(buffer.shell_integration_command(), None);
    }

    #[test]
    fn tracks_osc8_hyperlinks_on_cells() {
        let mut buffer = VtBuffer::new(6, 1);
        buffer.process("\u{1b}]8;;https://example.com\u{7}go\u{1b}]8;;\u{7}!");

        assert_eq!(buffer.hyperlink_at(0, 0), Some("https://example.com"));
        assert_eq!(buffer.hyperlink_at(0, 1), Some("https://example.com"));
        assert_eq!(buffer.hyperlink_at(0, 2), None);
    }

    #[test]
    fn supports_soft_reset() {
        let mut buffer = VtBuffer::new(4, 2);
        buffer.process("\u{1b}[?7l\u{1b}[?1000h\u{1b}]8;;https://example.com\u{7}\u{1b}[4mX");
        buffer.process("\u{1b}[!p");
        buffer.process("Y");

        assert!(buffer.autowrap());
        assert_eq!(buffer.mouse_tracking(), MouseTrackingMode::Disabled);
        assert_eq!(buffer.hyperlink_at(0, 0), Some("https://example.com"));
        assert!(!buffer.cell(0, 0).unwrap().style.underline);
        assert_eq!(buffer.hyperlink_at(0, 0), Some("https://example.com"));
    }

    #[test]
    fn supports_hard_reset() {
        let mut buffer = VtBuffer::new(4, 2);
        buffer.process("\u{1b}]0;shell\u{7}\u{1b}]133;L;make build\u{7}ok");
        buffer.process("\u{1b}c");

        assert_eq!(buffer.window_title(), None);
        assert_eq!(buffer.shell_integration_command(), None);
        assert_eq!(buffer.cell(0, 0).unwrap().ch, ' ');
        assert_eq!(buffer.history_len(), 0);
    }
}
