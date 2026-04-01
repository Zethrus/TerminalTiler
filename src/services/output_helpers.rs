use regex::Regex;

use crate::model::assets::{OutputHelperRule, OutputSeverity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperMatch {
    pub rule_id: String,
    pub label: String,
    pub severity: OutputSeverity,
    pub toast_on_match: bool,
}

pub fn scan_output(rules: &[OutputHelperRule], text: &str) -> Vec<HelperMatch> {
    let mut matches = Vec::new();
    for rule in rules {
        let Ok(regex) = Regex::new(&rule.regex) else {
            continue;
        };
        if regex.is_match(text) {
            matches.push(HelperMatch {
                rule_id: rule.id.clone(),
                label: rule.label.clone(),
                severity: rule.severity,
                toast_on_match: rule.toast_on_match,
            });
        }
    }
    matches
}

pub fn helper_summary_text(matches: &[HelperMatch]) -> (String, Option<OutputSeverity>) {
    let Some(first) = matches.first() else {
        return (String::new(), None);
    };

    if matches.len() == 1 {
        (first.label.clone(), Some(first.severity))
    } else {
        (
            format!("{} alerts", matches.len()),
            Some(max_severity(matches.iter().map(|item| item.severity))),
        )
    }
}

fn max_severity(values: impl IntoIterator<Item = OutputSeverity>) -> OutputSeverity {
    values
        .into_iter()
        .max_by_key(|value| match value {
            OutputSeverity::Info => 0,
            OutputSeverity::Warning => 1,
            OutputSeverity::Error => 2,
        })
        .unwrap_or(OutputSeverity::Info)
}
