use serde::{Deserialize, Serialize};

use crate::trace::{AgentDef, AgentSet};

/// Корень проекта в воркстейшне: относительно него живут папки агентов.
pub const PROJECT_ROOT: &str = "/work/project";

/// Территория агента в дереве набора: папка его узла (имя агента как путь) и
/// папки наследников, в которые агент писать не может. Чтение не ограничено:
/// граница только на изменения.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Territory {
    /// Папка узла агента. Пустая строка — корень проекта.
    pub folder: String,
    /// Папки наследников: территория агента заканчивается перед ними.
    pub excludes: Vec<String>,
}

impl Territory {
    /// Принадлежит ли путь территории. `path` — repo-относительный,
    /// уже нормализованный (без `..`).
    pub fn contains(&self, path: &str) -> bool {
        if !self.folder.is_empty() && path != self.folder {
            let prefix = format!("{}/", self.folder);
            if !path.starts_with(&prefix) {
                return false;
            }
        }
        !self
            .excludes
            .iter()
            .any(|e| path == *e || path.starts_with(&format!("{}/", e)))
    }

    /// Может ли команда выполняться без изменения файлов вне территории.
    /// Инструменты только для чтения не ограничены; остальным достаточно, чтобы
    /// каждый похожий на путь аргумент оказался внутри территории.
    pub fn write_allowed(&self, command: &str) -> bool {
        let mut words = command.split_whitespace();
        let Some(base) = words.next() else {
            return true;
        };
        if read_only_tool(base) {
            return true;
        }
        for token in words {
            if token.starts_with('-') {
                continue;
            }
            let token = token.trim_matches(|c| c == '\'' || c == '"');
            if token.is_empty() {
                continue;
            }
            if let Some(path) = resolve_path(&self.folder, token) {
                if !self.contains(&path) {
                    return false;
                }
            }
        }
        true
    }
}

/// Инструменты, которые только читают: им разрешён любой путь, чтение
/// вне территории не ограничено. Всё остальное — способное писать.
fn read_only_tool(base: &str) -> bool {
    matches!(
        base,
        "cat"
            | "ls"
            | "find"
            | "grep"
            | "head"
            | "tail"
            | "wc"
            | "file"
            | "du"
            | "stat"
            | "diff"
            | "echo"
            | "which"
            | "true"
            | "false"
    )
}

/// Нормализовать аргумент команды в repo-относительный путь от папки узла.
/// Абсолютный путь трактуется как repo-относительный (ведущий `/` срезается);
/// `..` за пределы папки просто поднимается вверх по пути.
fn resolve_path(folder: &str, token: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(rest) = token.strip_prefix('/') {
        parts.extend(rest.split('/'));
    } else {
        if !folder.is_empty() {
            parts.push(folder);
        }
        parts.extend(token.split('/'));
    }
    let mut out: Vec<&str> = Vec::new();
    for p in parts {
        match p {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(p),
        }
    }
    Some(out.join("/"))
}

/// Территория агента по его узлу в дереве набора: папка узла минус папки
/// наследников (имена агентов — пути папок проекта).
pub fn territory_for(set: &AgentSet, agent: &AgentDef) -> Territory {
    territory_for_list(&set.agents, agent)
}

/// То же по списку агентов набора (без заимствования всего набора).
pub fn territory_for_list(agents: &[AgentDef], agent: &AgentDef) -> Territory {
    let folder = agent.name.clone();
    // Территория заканчивается перед папками наследников — только ближайших
    // (у подпапок — свои наследники, их папка уже покрыта папкой ребёнка).
    let excludes: Vec<String> = agents
        .iter()
        .filter(|a| a.id != agent.id)
        .filter(|a| is_descendant(&folder, &a.name))
        .filter(|a| {
            !agents.iter().any(|other| {
                other.id != agent.id
                    && other.id != a.id
                    && is_descendant(&folder, &other.name)
                    && is_descendant(&other.name, &a.name)
            })
        })
        .map(|a| a.name.clone())
        .collect();
    Territory { folder, excludes }
}

fn is_descendant(folder: &str, other: &str) -> bool {
    folder.is_empty() || other.starts_with(&format!("{}/", folder))
}

/// Обёртка команды: cwd агента — его папка внутри проекта воркстейшна,
/// чтобы относительные записи ложились в его территорию.
pub fn wrap_command(folder: &str, command: &str) -> String {
    let cwd = if folder.is_empty() {
        PROJECT_ROOT.to_string()
    } else {
        format!("{}/{}", PROJECT_ROOT, folder)
    };
    format!("cd {cwd} && {command}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terr() -> Territory {
        // Агент "src": владеет src/ кроме src/backend и src/frontend.
        Territory {
            folder: "src".to_string(),
            excludes: vec!["src/backend".to_string(), "src/frontend".to_string()],
        }
    }

    #[test]
    fn territory_owns_its_node_excluding_descendants() {
        let t = terr();
        assert!(t.contains("src"));
        assert!(t.contains("src/main.rs"));
        assert!(t.contains("src/lib/mod.rs"));
        assert!(!t.contains("src/backend"));
        assert!(!t.contains("src/backend/api"));
        assert!(!t.contains("src/frontend/App.tsx"));
        assert!(!t.contains("README.md"));
        assert!(!t.contains("other/x"));
    }

    #[test]
    fn agent_tool_cannot_modify_file_outside_territory() {
        let t = terr();
        // Инструмент-писатель, цель — вне территории: не проходит.
        assert!(!t.write_allowed("touch ../README.md"));
        assert!(!t.write_allowed("touch /README.md"));
        assert!(!t.write_allowed("rm -rf backend"));
        assert!(!t.write_allowed("sed -i 's/a/b/' /src/backend/api/x.py"));
        // Внутри территории — проходит.
        assert!(t.write_allowed("touch main.rs"));
        assert!(t.write_allowed("touch src/main.rs"));
        assert!(t.write_allowed("sed -i 's/a/b/' lib/mod.rs"));
        // Без похожих на путь аргументов — проходит (работает в cwd агента).
        assert!(t.write_allowed("make build"));
        assert!(t.write_allowed("git commit -m \"fix\""));
    }

    #[test]
    fn agent_tool_can_read_file_outside_territory() {
        let t = terr();
        // Инструменты чтения не ограничены: любой путь.
        assert!(t.write_allowed("cat ../README.md"));
        assert!(t.write_allowed("grep -r 'TODO' src/backend"));
        assert!(t.write_allowed("ls /src/frontend"));
        assert!(t.write_allowed("find /work/project -name '*.rs'"));
    }

    #[test]
    fn territory_of_root_node_covers_project_minus_descendants() {
        // Корневой узел (пустая папка) владеет проектом кроме наследников.
        let t = Territory {
            folder: String::new(),
            excludes: vec!["ws".to_string()],
        };
        assert!(t.contains("README.md"));
        assert!(t.contains("src/main.rs"));
        assert!(!t.contains("ws/1"));
    }

    #[test]
    fn command_wraps_into_territory_cwd() {
        assert_eq!(
            wrap_command("src", "touch x.py"),
            "cd /work/project/src && touch x.py"
        );
        assert_eq!(
            wrap_command("", "make test"),
            "cd /work/project && make test"
        );
    }
}
