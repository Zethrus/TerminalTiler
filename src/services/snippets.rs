use std::error::Error;
use std::fmt;

use crate::model::assets::{CliSnippet, TemplateVariableValues};
use crate::services::template_variables::{
    TemplateRenderError, TemplateVariableContext, render_variables,
};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub enum SnippetResolveError {
    Template(TemplateRenderError),
}

impl fmt::Display for SnippetResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Template(error) => error.fmt(formatter),
        }
    }
}

impl Error for SnippetResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Template(error) => Some(error),
        }
    }
}

impl From<TemplateRenderError> for SnippetResolveError {
    fn from(error: TemplateRenderError) -> Self {
        Self::Template(error)
    }
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn resolve_snippet(
    snippet: &CliSnippet,
    variables: &TemplateVariableValues,
) -> Result<String, SnippetResolveError> {
    let rendered = render_variables(
        &snippet.command,
        variables,
        TemplateVariableContext::Snippet,
    )?;
    Ok(if rendered.ends_with('\n') {
        rendered
    } else {
        format!("{rendered}\n")
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{SnippetResolveError, resolve_snippet};
    use crate::model::assets::{CliSnippet, SnippetVariable};
    use crate::services::template_variables::{TemplateRenderError, TemplateVariableContext};

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

    #[test]
    fn reports_missing_variables_with_a_typed_error() {
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

        let error =
            resolve_snippet(&snippet, &HashMap::new()).expect_err("missing variable should fail");

        assert_eq!(
            error,
            SnippetResolveError::Template(TemplateRenderError::MissingVariable {
                context: TemplateVariableContext::Snippet,
                key: "service".into(),
            })
        );
    }
}
