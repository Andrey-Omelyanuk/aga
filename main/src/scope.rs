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
    /// уже нормализованный (без `..`). В работе границу держит песочница
    /// (`sandbox_command`); здесь — то же правило словами, для тестов.
    #[cfg(test)]
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
}

/// Территория агента по его узлу в дереве набора: папка узла минус папки
/// наследников (имена агентов — пути папок проекта).
pub fn territory_for(set: &AgentSet, agent: &AgentDef) -> Territory {
    territory_for_list(&set.agents, agent)
}

/// То же по списку агентов набора (без заимствования всего набора).
pub fn territory_for_list(agents: &[AgentDef], agent: &AgentDef) -> Territory {
    // Корень дерева набора — корень проекта (папка ""), а не имя агента:
    // дерево повторяет иерархию папок проекта, а у корня папки-родителя нет.
    // Имя корня — метка слоя проекта (например, `ui` библиотеки), папкой в
    // репозитории оно не является; у наследников папка — их имя.
    let folder = if agent.parent_id.is_none() {
        String::new()
    } else {
        agent.name.clone()
    };
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
pub fn wrap_command_in(root: &str, folder: &str, command: &str) -> String {
    let cwd = if folder.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{folder}")
    };
    format!("cd {} && {command}", shell_quote(&cwd))
}

// === Граница территории — песочница владения ===
//
// Команда агента (и его stdio MCP-сервер) выполняется в отдельном mount
// namespace станции: весь проект смонтирован только на чтение, своя папка —
// на запись, папки наследников — снова только на чтение. Запись вне своей
// зоны сразу падает с «Read-only file system»; чтение не ограничено, чужие
// изменения видны сразу (файлы те же). `.git` лежит в корне — коммитит
// владелец корня, остальные просят его. Песочницу строит root (под станции
// привилегированный — DinD), сама команда идёт от пользователя станции `aga`
// (uid 1000). Вне `/tmp`/`$HOME` и проекта ФС не меняется. Не удалось
// смонтировать — команда не выполняется (`set -e`).

/// Пользователь станции, от которого выполняются команды агента.
pub const WORKSTATION_USER: &str = "aga";

/// Команда в песочнице владения: cwd — папка агента, запись — только в его
/// территории. Запускается от root (`docker exec -u 0` / `kubectl exec`).
pub fn sandbox_command(root: &str, territory: &Territory, command: &str) -> String {
    let path = |rel: &str| {
        if rel.is_empty() {
            root.to_string()
        } else {
            format!("{root}/{rel}")
        }
    };
    let own = path(&territory.folder);
    let excludes: Vec<String> = territory.excludes.iter().map(|e| path(e)).collect();
    let bind = |p: &str, mode: &str| {
        let q = shell_quote(p);
        format!("mount --bind {q} {q}; mount -o remount,{mode},bind {q} {q}")
    };

    // Папки, на которые вешаются монтирования, должны существовать: создаём
    // их от пользователя станции (владелец проекта), иначе граница не встанет.
    let mut dirs = vec![shell_quote(&own)];
    dirs.extend(excludes.iter().map(|e| shell_quote(e)));
    let mkdir = format!("mkdir -p -- {}", dirs.join(" "));
    let mut steps = vec![
        "set -e".to_string(),
        format!(
            "su -s /bin/sh {WORKSTATION_USER} -c {}",
            shell_quote(&mkdir)
        ),
    ];
    if !territory.folder.is_empty() {
        steps.push(bind(root, "ro"));
        steps.push(bind(&own, "rw"));
    }
    for e in &excludes {
        steps.push(bind(e, "ro"));
    }
    steps.push(format!("exec su -s /bin/sh {WORKSTATION_USER} -c \"$1\""));
    format!(
        "unshare -m sh -c {} aga-sandbox {}",
        shell_quote(&steps.join("\n")),
        shell_quote(&wrap_command_in(root, &territory.folder, command))
    )
}

/// Строка в одинарных кавычках для sh.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
            wrap_command_in(PROJECT_ROOT, "src", "touch x.py"),
            "cd '/work/project/src' && touch x.py"
        );
        assert_eq!(
            wrap_command_in(PROJECT_ROOT, "", "make test"),
            "cd '/work/project' && make test"
        );
    }

    #[test]
    fn sandbox_mounts_project_ro_and_own_folder_rw() {
        let script = sandbox_command("/work/project", &terr(), "touch x");
        // Проект — только чтение, своя папка — запись, наследники — снова чтение.
        assert!(script.starts_with("unshare -m sh -c "));
        let ro_root = "mount -o remount,ro,bind '\\''/work/project'\\'' ";
        let rw_own = "mount -o remount,rw,bind '\\''/work/project/src'\\'' ";
        let ro_child = "mount -o remount,ro,bind '\\''/work/project/src/backend'\\'' ";
        let (a, b, c) = (
            script.find(ro_root).unwrap(),
            script.find(rw_own).unwrap(),
            script.find(ro_child).unwrap(),
        );
        assert!(a < b && b < c, "{script}");
        // Команда — отдельным аргументом, от пользователя станции, в папке агента.
        assert!(script.contains("exec su -s /bin/sh aga -c \"$1\""));
        assert!(script.ends_with(" aga-sandbox 'cd '\\''/work/project/src'\\'' && touch x'"));
    }

    #[test]
    fn sandbox_of_root_agent_closes_only_descendants() {
        let t = Territory {
            folder: String::new(),
            excludes: vec!["ui".to_string()],
        };
        let script = sandbox_command("/work/project", &t, "make");
        assert!(!script.contains("remount,ro,bind '\\''/work/project'\\'' "));
        assert!(script.contains("remount,ro,bind '\\''/work/project/ui'\\'' "));
        assert!(!script.contains("remount,rw"));
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    /// Живая проверка песочницы в образе воркстейшна (нужны docker и образ
    /// `aga-workstation:dev`): `cargo test -- --ignored sandbox_in_workstation`.
    #[test]
    #[ignore]
    fn sandbox_in_workstation_blocks_writes_outside_territory() {
        let image =
            std::env::var("AGA_WS_IMAGE").unwrap_or_else(|_| "aga-workstation:dev".to_string());
        let command = "echo x > ok.txt && echo own-ok; \
            echo y > ../README.md 2>/dev/null || echo readme-denied; \
            echo z > backend/b.txt 2>/dev/null || echo child-denied; \
            git add ok.txt 2>/dev/null || echo git-denied; \
            cat ../README.md; id -u";
        let setup = format!(
            "set -e; mkdir -p /work/project/src && cd /work/project && git init -q \
             && echo orig > README.md && chown -R 1000:1000 /work\n{}\n\
             echo outside:; ls /work/project/src | tr '\\n' ' '; echo; grep -c ' /work' /proc/mounts || true",
            sandbox_command("/work/project", &terr(), command)
        );
        let out = std::process::Command::new("docker")
            .args([
                "run",
                "--rm",
                "--privileged",
                "--entrypoint",
                "sh",
                &image,
                "-c",
                &setup,
            ])
            .output()
            .expect("docker недоступен");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{text}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            vec![
                "own-ok",
                "readme-denied",
                "child-denied",
                "git-denied",
                "orig",
                "1000",
                "outside:",
                "backend frontend ok.txt ",
                "0"
            ]
        );
    }
}
