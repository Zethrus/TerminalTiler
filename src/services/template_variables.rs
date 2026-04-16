use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::assets::TemplateVariableValues;

static VARIABLE_PATTERN: OnceLock<Regex> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) enum TemplateVariableContext {
    Snippet,
    Runbook,
}

impl fmt::Display for TemplateVariableContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snippet => formatter.write_str("snippet"),
            Self::Runbook => formatter.write_str("runbook"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TemplateRenderError {
    MissingVariable {
        context: TemplateVariableContext,
        key: String,
    },
}

impl fmt::Display for TemplateRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVariable { context, key } => {
                write!(formatter, "Missing {context} variable '{key}'.")
            }
        }
    }
}

impl Error for TemplateRenderError {}

pub(super) fn render_variables(
    command: &str,
    variables: &TemplateVariableValues,
    context: TemplateVariableContext,
) -> Result<String, TemplateRenderError> {
    let variable_pattern = VARIABLE_PATTERN.get_or_init(|| {
        Regex::new(r"\{\{\s*([a-zA-Z0-9_-]+)\s*\}\}").expect("valid template variable regex")
    });
    let mut rendered = String::new();
    let mut last_end = 0;
    for captures in variable_pattern.captures_iter(command) {
        let Some(variable_match) = captures.get(0) else {
            continue;
        };
        let Some(key_match) = captures.get(1) else {
            continue;
        };
        rendered.push_str(&command[last_end..variable_match.start()]);
        let key = key_match.as_str();
        let value = variables
            .get(key)
            .ok_or_else(|| TemplateRenderError::MissingVariable {
                context,
                key: key.to_string(),
            })?;
        rendered.push_str(value);
        last_end = variable_match.end();
    }
    rendered.push_str(&command[last_end..]);
    Ok(rendered)
}
