use std::collections::HashMap;

use regex::Regex;

use crate::model::assets::CliSnippet;

pub fn resolve_snippet(
    snippet: &CliSnippet,
    variables: &HashMap<String, String>,
) -> Result<String, String> {
    let rendered = render_variables(&snippet.command, variables)?;
    Ok(if rendered.ends_with('\n') {
        rendered
    } else {
        format!("{rendered}\n")
    })
}

fn render_variables(command: &str, variables: &HashMap<String, String>) -> Result<String, String> {
    let variable_pattern =
        Regex::new(r"\{\{\s*([a-zA-Z0-9_-]+)\s*\}\}").map_err(|error| error.to_string())?;
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
            .ok_or_else(|| format!("Missing snippet variable '{key}'."))?;
        rendered.push_str(value);
        last_end = variable_match.end();
    }
    rendered.push_str(&command[last_end..]);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::resolve_snippet;
    use crate::model::assets::{CliSnippet, SnippetVariable};

    #[test]
    fn resolves_variables_and_appends_newline() {
        let snippet = CliSnippet {
            id: "restart".into(),
            name: "Restart service".into(),
            description: String::new(),
            command: "sudo systemctl restart {{service}}".into(),
            variables: vec![SnippetVariable {
                id: "service".into(),
                label: "Service".into(),
                description: String::new(),
                default_value: "nginx".into(),
            }],
            tags: Vec::new(),
        };

        let variables = HashMap::from([(String::from("service"), String::from("postgresql"))]);

        let resolved = resolve_snippet(&snippet, &variables).unwrap();

        assert_eq!(resolved, "sudo systemctl restart postgresql\n");
    }
}