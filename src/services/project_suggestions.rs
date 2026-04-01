use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::model::assets::ProjectSuggestion;
use crate::model::workspace_config::SuggestionOverride;
use crate::storage::workspace_config_store::WorkspaceConfigStore;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedService {
    pub id: String,
    pub title: String,
    pub working_directory: PathBuf,
    pub stack: String,
    pub startup_command: Option<String>,
    pub test_command: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoIntrospectionReport {
    pub tags: Vec<String>,
    pub services: Vec<DetectedService>,
    pub suggestions: Vec<ProjectSuggestion>,
}

pub fn detect_project_suggestions(workspace_root: &Path) -> Vec<ProjectSuggestion> {
    introspect_workspace(workspace_root).suggestions
}

pub fn introspect_workspace(workspace_root: &Path) -> RepoIntrospectionReport {
    let workspace_config = WorkspaceConfigStore::new()
        .load_for_root(workspace_root)
        .config;
    let mut report = RepoIntrospectionReport::default();
    let mut tags = BTreeSet::new();

    let cargo = workspace_root.join("Cargo.toml");
    if cargo.exists() {
        tags.insert("rust".to_string());
        let services = detect_cargo_services(workspace_root, &cargo);
        report.services.extend(services.clone());
        report.suggestions.push(ProjectSuggestion {
            id: "rust-delivery".into(),
            title: if services.len() > 1 {
                "Rust Workspace Delivery".into()
            } else {
                "Rust Delivery Workspace".into()
            },
            description: suggestion_description(
                "Planner, build, and verification terminals tuned for Rust work.",
                &services,
            ),
            role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some("cargo check --workspace --all-targets".into()),
                Some("cargo test --workspace".into()),
            ],
            tags: vec!["rust".into(), "delivery".into()],
        });
    }

    let package_json = workspace_root.join("package.json");
    if package_json.exists() {
        let services = detect_node_services(workspace_root, &package_json);
        if !services.is_empty() {
            tags.insert("javascript".to_string());
            if services.len() > 1 {
                tags.insert("monorepo".to_string());
            }
            report.services.extend(services.clone());
            let monorepo = workspace_root.join("pnpm-workspace.yaml").exists()
                || workspace_root.join("turbo.json").exists()
                || workspace_root.join("nx.json").exists()
                || services.len() > 1;
            report.suggestions.push(ProjectSuggestion {
                id: "node-app".into(),
                title: if monorepo {
                    "JavaScript Monorepo Workspace".into()
                } else {
                    "Node Application Workspace".into()
                },
                description: suggestion_description(
                    "Separate install, dev server, and test terminals for JavaScript projects.",
                    &services,
                ),
                role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
                tile_count: 3,
                startup_commands: vec![
                    Some("codex".into()),
                    services
                        .first()
                        .and_then(|service| service.startup_command.clone())
                        .or_else(|| detect_node_start_command(workspace_root)),
                    services
                        .first()
                        .and_then(|service| service.test_command.clone())
                        .or_else(|| detect_node_test_command(workspace_root)),
                ],
                tags: vec![
                    "javascript".into(),
                    if monorepo { "monorepo" } else { "web" }.into(),
                ],
            });
        }
    }

    let pyproject = workspace_root.join("pyproject.toml");
    if pyproject.exists() || workspace_root.join("requirements.txt").exists() {
        tags.insert("python".to_string());
        let services = detect_python_services(workspace_root, &pyproject);
        report.services.extend(services.clone());
        report.suggestions.push(ProjectSuggestion {
            id: "python-app".into(),
            title: "Python Application Workspace".into(),
            description: suggestion_description(
                "Planner, app shell, and test terminals for Python services.",
                &services,
            ),
            role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                services
                    .first()
                    .and_then(|service| service.startup_command.clone())
                    .or_else(|| Some("python -m uvicorn app:app --reload".into())),
                Some("python -m pytest".into()),
            ],
            tags: vec!["python".into(), "service".into()],
        });
    }

    let go_mod = workspace_root.join("go.mod");
    if go_mod.exists() {
        tags.insert("go".to_string());
        let services = detect_go_services(workspace_root, &go_mod);
        report.services.extend(services.clone());
        report.suggestions.push(ProjectSuggestion {
            id: "go-service".into(),
            title: "Go Service Workspace".into(),
            description: suggestion_description(
                "Planner, build, and test terminals for Go services.",
                &services,
            ),
            role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some("go test ./...".into()),
                Some("go test ./...".into()),
            ],
            tags: vec!["go".into(), "service".into()],
        });
    }

    let docker_compose = workspace_root.join("docker-compose.yml").exists()
        || workspace_root.join("docker-compose.yaml").exists()
        || workspace_root.join("compose.yml").exists()
        || workspace_root.join("compose.yaml").exists();
    let terraform = workspace_root.join(".terraform").exists()
        || collect_dir_entries(workspace_root, |path| {
            path.extension().is_some_and(|ext| ext == "tf")
        });
    let ansible = workspace_root.join("ansible.cfg").exists()
        || workspace_root.join("playbook.yml").exists()
        || workspace_root.join("playbook.yaml").exists()
        || workspace_root.join("inventory").exists();
    let helm = workspace_root.join("Chart.yaml").exists();
    let kubernetes = collect_dir_entries(workspace_root, |path| {
        path.extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
            && fs::read_to_string(path)
                .map(|content| content.contains("apiVersion:") && content.contains("kind:"))
                .unwrap_or(false)
    });

    if docker_compose || terraform || ansible || helm || kubernetes {
        tags.insert("ops".to_string());
        report.suggestions.push(ProjectSuggestion {
            id: "ops-stack".into(),
            title: if kubernetes || helm {
                "Platform Operations Workspace".into()
            } else {
                "Infrastructure Workspace".into()
            },
            description: "One pane for control, one for plan or apply, and one for logs or checks."
                .into(),
            role_ids: vec!["planner".into(), "ops".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some(if terraform {
                    "terraform plan".into()
                } else if ansible {
                    "ansible-playbook --check playbook.yml".into()
                } else if helm {
                    "helm list -A".into()
                } else if kubernetes {
                    "kubectl get pods -A".into()
                } else {
                    "docker compose ps".into()
                }),
                Some(if docker_compose {
                    "docker compose logs -f".into()
                } else if kubernetes {
                    "kubectl get events -A --watch".into()
                } else {
                    "bash".into()
                }),
            ],
            tags: vec!["ops".into(), "infra".into()],
        });
    }

    report.tags = tags.into_iter().collect();
    report.suggestions = apply_overrides(
        report.suggestions,
        &workspace_config.introspection.suggestion_overrides,
    );
    report
}

fn apply_overrides(
    suggestions: Vec<ProjectSuggestion>,
    overrides: &[SuggestionOverride],
) -> Vec<ProjectSuggestion> {
    let mut suggestions = suggestions;
    for override_item in overrides {
        if let Some(suggestion) = suggestions
            .iter_mut()
            .find(|item| item.id == override_item.id)
        {
            if override_item.disabled {
                suggestion.tags.push("disabled".into());
                continue;
            }
            if let Some(title) = override_item.title.as_deref() {
                suggestion.title = title.to_string();
            }
            if let Some(description) = override_item.description.as_deref() {
                suggestion.description = description.to_string();
            }
            if let Some(startup_commands) = override_item.startup_commands.as_ref() {
                suggestion.startup_commands = startup_commands.clone();
            }
        }
    }
    suggestions
        .into_iter()
        .filter(|suggestion| {
            !overrides
                .iter()
                .any(|override_item| override_item.id == suggestion.id && override_item.disabled)
        })
        .collect()
}

fn detect_cargo_services(workspace_root: &Path, cargo_toml: &Path) -> Vec<DetectedService> {
    let mut services = Vec::new();
    let Ok(contents) = fs::read_to_string(cargo_toml) else {
        return services;
    };
    let Ok(value) = toml::from_str::<TomlValue>(&contents) else {
        return services;
    };

    if let Some(workspace) = value.get("workspace").and_then(|value| value.as_table())
        && let Some(members) = workspace.get("members").and_then(|value| value.as_array())
    {
        for member in members.iter().filter_map(|item| item.as_str()) {
            services.push(DetectedService {
                id: member.to_string(),
                title: member.to_string(),
                working_directory: workspace_root.join(member),
                stack: "rust".into(),
                startup_command: Some(format!("cargo check -p {}", sanitize_package_name(member))),
                test_command: Some(format!("cargo test -p {}", sanitize_package_name(member))),
            });
        }
    } else if let Some(package_name) = value
        .get("package")
        .and_then(|value| value.get("name"))
        .and_then(|value| value.as_str())
    {
        services.push(DetectedService {
            id: package_name.to_string(),
            title: package_name.to_string(),
            working_directory: workspace_root.to_path_buf(),
            stack: "rust".into(),
            startup_command: Some("cargo check --all-targets".into()),
            test_command: Some("cargo test".into()),
        });
    }

    services
}

fn detect_node_services(workspace_root: &Path, package_json: &Path) -> Vec<DetectedService> {
    let mut services = Vec::new();
    if let Some(root_service) = parse_node_service(package_json, workspace_root) {
        services.push(root_service);
    }

    for bucket in ["apps", "packages", "services"] {
        let bucket_root = workspace_root.join(bucket);
        let Ok(entries) = fs::read_dir(&bucket_root) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let service_root = entry.path();
            let service_manifest = service_root.join("package.json");
            if service_manifest.exists()
                && let Some(service) = parse_node_service(&service_manifest, &service_root)
            {
                services.push(service);
            }
        }
    }

    dedupe_services(services)
}

fn parse_node_service(package_json: &Path, service_root: &Path) -> Option<DetectedService> {
    let contents = fs::read_to_string(package_json).ok()?;
    let value = serde_json::from_str::<JsonValue>(&contents).ok()?;
    let package_name = value
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| {
            service_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("node-app")
        });
    let scripts = value.get("scripts").and_then(|value| value.as_object());
    let startup_command = scripts.and_then(|scripts| {
        if scripts.contains_key("dev") {
            Some(command_for_node_script(service_root, "dev"))
        } else if scripts.contains_key("start") {
            Some(command_for_node_script(service_root, "start"))
        } else {
            None
        }
    });
    let test_command = scripts.and_then(|scripts| {
        if scripts.contains_key("test") {
            Some(command_for_node_script(service_root, "test"))
        } else {
            None
        }
    });

    Some(DetectedService {
        id: package_name.to_string(),
        title: package_name.to_string(),
        working_directory: service_root.to_path_buf(),
        stack: "javascript".into(),
        startup_command,
        test_command,
    })
}

fn detect_node_start_command(workspace_root: &Path) -> Option<String> {
    Some(command_for_node_script(workspace_root, "dev"))
}

fn detect_node_test_command(workspace_root: &Path) -> Option<String> {
    Some(command_for_node_script(workspace_root, "test"))
}

fn command_for_node_script(workspace_root: &Path, script: &str) -> String {
    if workspace_root.join("pnpm-lock.yaml").exists() {
        format!("pnpm {script}")
    } else if workspace_root.join("bun.lock").exists() || workspace_root.join("bun.lockb").exists()
    {
        format!("bun run {script}")
    } else {
        format!("npm run {script}")
    }
}

fn detect_python_services(workspace_root: &Path, pyproject: &Path) -> Vec<DetectedService> {
    let mut services = Vec::new();
    if pyproject.exists()
        && let Ok(contents) = fs::read_to_string(pyproject)
        && let Ok(value) = toml::from_str::<TomlValue>(&contents)
    {
        let project_name = value
            .get("project")
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str())
            .or_else(|| {
                value
                    .get("tool")
                    .and_then(|value| value.get("poetry"))
                    .and_then(|value| value.get("name"))
                    .and_then(|value| value.as_str())
            })
            .unwrap_or("python-app");
        services.push(DetectedService {
            id: project_name.to_string(),
            title: project_name.to_string(),
            working_directory: workspace_root.to_path_buf(),
            stack: "python".into(),
            startup_command: Some("python -m uvicorn app:app --reload".into()),
            test_command: Some("python -m pytest".into()),
        });
    }
    services
}

fn detect_go_services(workspace_root: &Path, go_mod: &Path) -> Vec<DetectedService> {
    let Ok(contents) = fs::read_to_string(go_mod) else {
        return Vec::new();
    };
    let module_name = contents
        .lines()
        .find_map(|line| line.strip_prefix("module "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("go-service");
    vec![DetectedService {
        id: module_name.to_string(),
        title: module_name.to_string(),
        working_directory: workspace_root.to_path_buf(),
        stack: "go".into(),
        startup_command: Some("go run ./...".into()),
        test_command: Some("go test ./...".into()),
    }]
}

fn suggestion_description(base: &str, services: &[DetectedService]) -> String {
    if services.is_empty() {
        return base.into();
    }
    let names = services
        .iter()
        .take(3)
        .map(|service| service.title.clone())
        .collect::<Vec<_>>()
        .join(", ");
    if services.len() <= 3 {
        format!("{base} Detected: {names}.")
    } else {
        format!("{base} Detected: {names}, and {} more.", services.len() - 3)
    }
}

fn dedupe_services(services: Vec<DetectedService>) -> Vec<DetectedService> {
    let mut seen = BTreeSet::new();
    services
        .into_iter()
        .filter(|service| seen.insert(service.id.clone()))
        .collect()
}

fn sanitize_package_name(value: &str) -> String {
    value.replace('/', "-")
}

fn collect_dir_entries(root: &Path, predicate: impl Fn(&Path) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .any(|path| predicate(&path))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{detect_project_suggestions, introspect_workspace};

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("terminaltiler-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn detects_rust_workspace() {
        let root = temp_root("rust");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"api\", \"worker\"]\n",
        )
        .expect("cargo manifest");
        let report = introspect_workspace(&root);
        assert!(
            report
                .suggestions
                .iter()
                .any(|item| item.id == "rust-delivery")
        );
        assert_eq!(report.services.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_node_workspace() {
        let root = temp_root("node");
        fs::write(
            root.join("package.json"),
            r#"{"name":"web","scripts":{"dev":"vite","test":"vitest"}}"#,
        )
        .expect("package json");
        let suggestions = detect_project_suggestions(&root);
        assert!(suggestions.iter().any(|item| item.id == "node-app"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_infra_workspace() {
        let root = temp_root("infra");
        fs::write(root.join("docker-compose.yml"), "services: {}\n").expect("compose file");
        let suggestions = detect_project_suggestions(&root);
        assert!(suggestions.iter().any(|item| item.id == "ops-stack"));
        let _ = fs::remove_dir_all(root);
    }
}
