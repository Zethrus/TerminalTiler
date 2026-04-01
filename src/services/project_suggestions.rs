use std::fs;
use std::path::Path;

use crate::model::assets::ProjectSuggestion;

pub fn detect_project_suggestions(workspace_root: &Path) -> Vec<ProjectSuggestion> {
    let mut suggestions = Vec::new();
    let has = |name: &str| workspace_root.join(name).exists();
    let cargo = has("Cargo.toml");
    let package_json = has("package.json");
    let docker_compose = has("docker-compose.yml") || has("compose.yaml");
    let terraform = workspace_root.join(".terraform").exists()
        || collect_dir_entries(workspace_root, |path| {
            path.extension().is_some_and(|ext| ext == "tf")
        });
    let ansible = has("ansible.cfg")
        || has("playbook.yml")
        || has("playbook.yaml")
        || workspace_root.join("inventory").exists();
    let pnpm_workspace = has("pnpm-workspace.yaml");
    let turbo = has("turbo.json");

    if cargo {
        suggestions.push(ProjectSuggestion {
            id: "rust-delivery".into(),
            title: "Rust Delivery Workspace".into(),
            description: "Planner, build, and verification terminals tuned for Rust work."
                .into(),
            role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some("cargo check --all-targets".into()),
                Some("cargo test".into()),
            ],
            tags: vec!["rust".into(), "delivery".into()],
        });
    }

    if package_json {
        suggestions.push(ProjectSuggestion {
            id: "node-app".into(),
            title: if pnpm_workspace || turbo {
                "JavaScript Monorepo Workspace".into()
            } else {
                "Node Application Workspace".into()
            },
            description: "Separate install, dev server, and test terminals for JavaScript projects."
                .into(),
            role_ids: vec!["planner".into(), "implementer".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some(if has("pnpm-lock.yaml") {
                    "pnpm install && pnpm dev".into()
                } else if has("bun.lockb") || has("bun.lock") {
                    "bun install && bun run dev".into()
                } else {
                    "npm install && npm run dev".into()
                }),
                Some(if has("pnpm-lock.yaml") {
                    "pnpm test".into()
                } else if has("bun.lockb") || has("bun.lock") {
                    "bun test".into()
                } else {
                    "npm test".into()
                }),
            ],
            tags: vec!["javascript".into(), "web".into()],
        });
    }

    if docker_compose || terraform || ansible {
        suggestions.push(ProjectSuggestion {
            id: "ops-stack".into(),
            title: "Infrastructure Workspace".into(),
            description: "One pane for control, one for plan/apply, and one for logs or checks."
                .into(),
            role_ids: vec!["planner".into(), "ops".into(), "reviewer".into()],
            tile_count: 3,
            startup_commands: vec![
                Some("codex".into()),
                Some(if terraform {
                    "terraform plan".into()
                } else if ansible {
                    "ansible-playbook --check playbook.yml".into()
                } else {
                    "docker compose ps".into()
                }),
                Some(if docker_compose {
                    "docker compose logs -f".into()
                } else {
                    "bash".into()
                }),
            ],
            tags: vec!["ops".into(), "infra".into()],
        });
    }

    suggestions
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

    use super::detect_project_suggestions;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("terminaltiler-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn detects_rust_workspace() {
        let root = temp_root("rust");
        fs::write(root.join("Cargo.toml"), "[package]\nname='app'\nversion='0.1.0'\n")
            .expect("cargo manifest");
        let suggestions = detect_project_suggestions(&root);
        assert!(suggestions.iter().any(|item| item.id == "rust-delivery"));
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
