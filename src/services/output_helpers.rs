use regex::Regex;

use crate::model::assets::{OutputHelperRule, OutputSeverity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperMatch {
    pub rule_id: String,
    pub label: String,
    pub severity: OutputSeverity,
    pub toast_on_match: bool,
}

#[derive(Clone, Debug)]
pub struct CompiledOutputHelpers {
    rules: Vec<CompiledOutputHelperRule>,
}

#[derive(Clone, Debug)]
struct CompiledOutputHelperRule {
    regex: Regex,
    rule_id: String,
    label: String,
    severity: OutputSeverity,
    toast_on_match: bool,
}

impl CompiledOutputHelpers {
    pub fn new(rules: &[OutputHelperRule]) -> Self {
        let rules = rules
            .iter()
            .filter_map(|rule| {
                Regex::new(&rule.regex)
                    .ok()
                    .map(|regex| CompiledOutputHelperRule {
                        regex,
                        rule_id: rule.id.clone(),
                        label: rule.label.clone(),
                        severity: rule.severity,
                        toast_on_match: rule.toast_on_match,
                    })
            })
            .collect();
        Self { rules }
    }

    pub fn scan(&self, text: &str) -> Vec<HelperMatch> {
        self.rules
            .iter()
            .filter(|rule| rule.regex.is_match(text))
            .map(|rule| HelperMatch {
                rule_id: rule.rule_id.clone(),
                label: rule.label.clone(),
                severity: rule.severity,
                toast_on_match: rule.toast_on_match,
            })
            .collect()
    }
}

#[allow(dead_code)]
pub fn scan_output(rules: &[OutputHelperRule], text: &str) -> Vec<HelperMatch> {
    CompiledOutputHelpers::new(rules).scan(text)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, regex: &str, label: &str, severity: OutputSeverity) -> OutputHelperRule {
        OutputHelperRule {
            id: id.into(),
            label: label.into(),
            regex: regex.into(),
            severity,
            toast_on_match: true,
        }
    }

    #[test]
    fn compiled_helpers_match_scan_output_wrapper() {
        let rules = vec![
            rule("warn", "warning", "Warning", OutputSeverity::Warning),
            rule("err", "error|failed", "Error", OutputSeverity::Error),
            rule("info", "started", "Started", OutputSeverity::Info),
        ];
        let text = "process warning: retry failed";

        assert_eq!(
            CompiledOutputHelpers::new(&rules).scan(text),
            scan_output(&rules, text)
        );
    }

    #[test]
    fn invalid_regex_rules_are_ignored() {
        let rules = vec![
            rule("bad", "(", "Bad", OutputSeverity::Error),
            rule("good", "ready", "Ready", OutputSeverity::Info),
        ];

        let matches = CompiledOutputHelpers::new(&rules).scan("service ready");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_id, "good");
    }
}
