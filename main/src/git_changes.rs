use crate::agent::{execute_via_executor, Executor};
use base64::Engine;
use serde::Serialize;

/// Кандидаты на «начало ветки» воркстейшна: рефы дефолтной ветки репозитория.
/// Воркстейшн клонирует проект и работает на своей ветке `ws-<id>`, созданной
/// из дефолтной ветки при клоне, — дифф против неё и есть «изменения от начала
/// ветки». `origin/HEAD` указывает на дефолтную ветку удалённого репозитория;
/// если его нет (свежий/нестандартный клон), пробуем `main` и `master`.
const DEFAULT_BRANCH_CANDIDATES: &[&str] = &["origin/HEAD", "origin/main", "origin/master"];

/// Отчёт страницы Changes: изменения проекта воркстейшна против начала ветки.
#[derive(Debug, Clone, Serialize)]
pub struct ChangesSummary {
    /// Реф базы (дефолтная ветка репозитория); None — базы нет (пустой репозиторий).
    pub base: Option<String>,
    /// Есть ли изменения.
    pub changed: bool,
    /// Полный unified-дифф текущего состояния против базы, включая новые
    /// (незакоммиченные и неотслеживаемые) файлы как добавленные.
    pub diff: String,
}

/// Ошибка построения диффа изменений.
#[derive(Debug, thiserror::Error)]
pub enum ChangesError {
    #[error("исполнение команды в воркстейшне не удалось: {0}")]
    Exec(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ChangesError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        ChangesError::Exec(e.to_string())
    }
}

/// Выполнить git-команду в корне проекта воркстейшна и вернуть stdout.
/// Корень (`/work/project`) — константа без спецсимволов, поэтому собирается
/// в команду напрямую (как фиксированные find/base64 в project_files.rs).
async fn run(executor: &Executor, root: &str, args: &str) -> Result<String, ChangesError> {
    let command = format!("git -C {root} {args}");
    execute_via_executor(executor, &command)
        .await
        .map_err(ChangesError::from)
}

/// Обернуть строку в одинарные кавычки sh с экранированием внутренних кавычек.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Определить базу диффа — дефолтную ветку репозитория. Ни один кандидат не
/// существует (пустой `git init`, агент наполняет проект с нуля) — базы нет.
async fn resolve_base(executor: &Executor, root: &str) -> Result<Option<String>, ChangesError> {
    let candidates = DEFAULT_BRANCH_CANDIDATES.join(" ");
    let command = format!(
        "for r in {candidates}; do if git -C {root} rev-parse --verify --quiet \"$r\" >/dev/null 2>&1; then echo \"$r\"; exit 0; fi; done; exit 1"
    );
    match execute_via_executor(executor, &command).await {
        Ok(out) => Ok(out
            .lines()
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)),
        Err(e) => {
            // Собственный `exit 1` («кандидатов нет») — базы нет; любой другой
            // сбой исполнения (нет git) — ошибка.
            if e.to_string().starts_with("Command failed:") {
                Ok(None)
            } else {
                Err(ChangesError::from(e))
            }
        }
    }
}

/// Новые файлы проекта (не отслеживаемые git). Учитывается `.gitignore`
/// (`--exclude-standard`): игнорируемое (сборки, артефакты) в изменения не
/// попадает.
async fn list_untracked(executor: &Executor, root: &str) -> Result<Vec<String>, ChangesError> {
    let out = run(executor, root, "ls-files --others --exclude-standard -z").await?;
    Ok(out
        .split('\0')
        .map(|s| s.trim_end_matches('\r').to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Отслеживаемые файлы проекта (`git ls-files`). Нужны только когда базы нет:
/// без неё «добавленным» показывается весь проект, в т.ч. уже закоммиченное.
async fn list_tracked(executor: &Executor, root: &str) -> Result<Vec<String>, ChangesError> {
    let out = run(executor, root, "ls-files -z").await?;
    Ok(out
        .split('\0')
        .map(|s| s.trim_end_matches('\r').to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Прочитать новые файлы батчем: один exec, `base64` каждого файла с маркером
/// пути. Маркер `@@AGA-FILE@@` не пересекается с алфавитом base64 (`@` в нём
/// нет), поэтому содержимое файла не может быть принято за границу записи.
async fn read_untracked(
    executor: &Executor,
    root: &str,
    paths: &[String],
) -> Result<Vec<(String, Vec<u8>)>, ChangesError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let quoted: Vec<String> = paths.iter().map(|p| sh_quote(p)).collect();
    // Пути — относительно корня проекта: читаем из `cd {root}` (cwd executor'а
    // не определён: локально это каталог ядра, в поде/контейнере — WORKDIR).
    let command = format!(
        "cd {} && for f in {}; do echo \"@@AGA-FILE@@$f\"; base64 \"$f\"; done",
        sh_quote(root),
        quoted.join(" ")
    );
    let out = execute_via_executor(executor, &command)
        .await
        .map_err(ChangesError::from)?;
    let mut result = Vec::new();
    let mut current: Option<String> = None;
    let mut b64 = String::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("@@AGA-FILE@@") {
            if let Some(path) = current.take() {
                result.push((path, decode_b64(&b64)?));
                b64.clear();
            }
            current = Some(rest.to_string());
        } else if current.is_some() {
            b64.push_str(line.trim());
        }
    }
    if let Some(path) = current.take() {
        result.push((path, decode_b64(&b64)?));
    }
    Ok(result)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, ChangesError> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .map_err(|e| ChangesError::Exec(format!("base64 decode: {e}")))
}

/// Собрать unified-блок «добавленный файл» в формате `git diff` (как для нового
/// файла). Бинарные файлы — заглушкой, как в git.
fn synthesize_added(path: &str, content: &[u8]) -> String {
    let mut diff = format!("diff --git a/{path} b/{path}\nnew file mode 100644\n");
    if content.contains(&0) {
        diff.push_str(&format!(
            "--- /dev/null\n+++ b/{path}\nBinary files /dev/null and b/{path} differ\n"
        ));
        return diff;
    }
    let text = String::from_utf8_lossy(content);
    let lines: Vec<&str> = text.lines().collect();
    diff.push_str(&format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    ));
    for line in lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

/// Изменения проекта воркстейшна от начала ветки: текущее состояние против
/// дефолтной ветки репозитория. Включает закоммиченное на ветке, незакоммиченные
/// правки и новые файлы. Базы нет (пустой репозиторий) — весь проект показывается
/// как добавленные файлы. Только чтение: никаких коммитов и push'ей.
pub async fn changes(executor: &Executor, root: &str) -> Result<ChangesSummary, ChangesError> {
    let base = resolve_base(executor, root).await?;
    let mut untracked = list_untracked(executor, root).await?;
    if base.is_none() {
        // Базы нет — «изменения» это весь проект: подключаем и отслеживаемые
        // файлы (агент мог успеть закоммитить в пустом репозитории).
        untracked.extend(list_tracked(executor, root).await?);
    }
    untracked.sort();
    untracked.dedup();
    // Пути с переводами строк сломали бы маркер чтения — их пропускаем (край).
    let readable: Vec<String> = untracked
        .into_iter()
        .filter(|p| !p.contains('\n') && !p.contains('\r'))
        .collect();

    let mut diff = String::new();
    if let Some(base_ref) = &base {
        // Дифф рабочего состояния (индекс + рабочее дерево) против базы: и
        // закоммиченные на ветке изменения, и незакоммиченные правки.
        diff.push_str(
            &run(
                executor,
                root,
                &format!("diff --no-color {}", sh_quote(base_ref)),
            )
            .await?,
        );
    }
    for (path, content) in read_untracked(executor, root, &readable).await? {
        diff.push_str(&synthesize_added(&path, &content));
    }

    let changed = !diff.trim().is_empty();
    Ok(ChangesSummary {
        base,
        changed,
        diff,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(root: &std::path::Path) {
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "test@test"]);
        git(root, &["config", "user.name", "test"]);
    }

    fn commit_all(root: &std::path::Path, msg: &str) {
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", msg]);
    }

    /// Поставить origin/main на текущий HEAD: эмуляция клона, при котором ветка
    /// воркстейшна начинается из дефолтной ветки репозитория.
    fn set_origin_main(root: &std::path::Path) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        git(root, &["update-ref", "refs/remotes/origin/main", &sha]);
        git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
    }

    #[tokio::test]
    async fn changes_show_committed_and_uncommitted_changes_against_default_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root);
        std::fs::write(root.join("a.txt"), "v1\n").unwrap();
        commit_all(root, "base");
        set_origin_main(root);
        git(root, &["checkout", "-q", "-b", "ws-7"]);
        // Закоммиченное на ветке.
        std::fs::write(root.join("b.txt"), "new\n").unwrap();
        commit_all(root, "feature");
        // Незакоммиченная правка существующего файла.
        std::fs::write(root.join("a.txt"), "v1 changed\n").unwrap();

        let s = changes(&Executor::Sh, root.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(s.base.as_deref(), Some("origin/HEAD"));
        assert!(s.changed);
        assert!(s.diff.contains("b.txt"));
        assert!(s.diff.contains("+new"));
        assert!(s.diff.contains("v1 changed"));
    }

    #[tokio::test]
    async fn changes_show_untracked_file_as_added() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root);
        std::fs::write(root.join("a.txt"), "v1\n").unwrap();
        commit_all(root, "base");
        set_origin_main(root);
        // Файл, созданный агентом и не закоммиченный.
        std::fs::write(root.join("note.md"), "# Заметка\n").unwrap();

        let s = changes(&Executor::Sh, root.to_str().unwrap())
            .await
            .unwrap();
        assert!(s.changed);
        assert!(s.diff.contains("diff --git a/note.md b/note.md"));
        assert!(s.diff.contains("new file mode"));
        assert!(s.diff.contains("+# Заметка"));
    }

    #[tokio::test]
    async fn changes_show_all_files_as_added_without_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root);
        // Пустой репозиторий: базы нет, весь проект — добавленные файлы.
        std::fs::write(root.join("a.txt"), "A\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main(){}\n").unwrap();

        let s = changes(&Executor::Sh, root.to_str().unwrap())
            .await
            .unwrap();
        assert!(s.base.is_none());
        assert!(s.changed);
        assert!(s.diff.contains("diff --git a/a.txt b/a.txt"));
        assert!(s.diff.contains("+A"));
        assert!(s.diff.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(s.diff.contains("+fn main(){}"));
    }

    #[tokio::test]
    async fn changes_report_no_changes_when_repo_clean() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root);
        std::fs::write(root.join("a.txt"), "v1\n").unwrap();
        commit_all(root, "base");
        set_origin_main(root);

        let s = changes(&Executor::Sh, root.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(s.base.as_deref(), Some("origin/HEAD"));
        assert!(!s.changed);
        assert!(s.diff.trim().is_empty());
    }

    #[test]
    fn sh_quote_escapes_inner_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("a b"), "'a b'");
    }
}
