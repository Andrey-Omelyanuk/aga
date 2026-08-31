use crate::agent::{execute_via_executor, Executor};
use crate::project_files::PROJECT_ROOT;

/// Перезаписать каталог проекта воркстейшна кодом указанного (другого)
/// проекта. Выполняется в уже работающем ws (`kubectl exec`/`docker exec`),
/// поэтому сам ws (под/сервис) не пересоздаётся — в отличие от `create_pod`.
/// Клонируем во временный каталог и подменяем содержимое только при успехе:
/// сбой clone (например, недоступный git_url) не уничтожает текущий проект.
/// Временный каталог — `$$` (PID шелла exec): два параллельных switch одного
/// ws не дерутся за общий `.new`. Каталог как таковой не удаляем (в dev
/// `/work/project` — точка бинд-маунта), только его содержимое — так команда
/// работает и в поде, и в контейнере.
pub fn switch_project_command(git_url: &str, branch: &str) -> String {
    // Временный каталог — `/work/project.new.<pid>`: `$$` вне кавычек, иначе
    // шелл его не раскроет. PID у каждого `sh -c` свой — параллельные switch
    // одного ws не дерутся за общий `.new`.
    format!(
        "rm -rf '{root}.new.'$$ && git clone '{url}' '{root}.new.'$$ && git -C '{root}.new.'$$ checkout -B '{branch}' && find '{root}' -mindepth 1 -delete && cp -a '{root}.new.'$$/. '{root}/' && rm -rf '{root}.new.'$$",
        root = PROJECT_ROOT,
        url = git_url,
        branch = branch
    )
}

/// Восстановить файлы проекта из ветки прерванной сессии (`ws-<id>` упавшей
/// станции) на текущем воркстейшне.
pub fn restore_branch_command(branch: &str) -> String {
    format!(
        "cd '{root}' && git fetch origin '{branch}' && git checkout '{branch}'",
        root = PROJECT_ROOT,
        branch = branch
    )
}

/// Переключить проект воркстейшна: переписать `/work/project` кодом нового
/// проекта без пересоздания самого ws.
pub async fn replace_project(
    executor: &Executor,
    git_url: &str,
    branch: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    execute_via_executor(executor, &switch_project_command(git_url, branch)).await?;
    Ok(())
}

/// Очистить каталог проекта воркстейшна до пустого git-репо: станция
/// отпущена, кода прежнего проекта на ней быть не должно (следующая занятие
/// клонирует свой проект заново). Удаляем только содержимое — в dev
/// `/work/project` это точка бинд-маунта, её саму удалить нельзя.
pub fn release_workspace_command() -> String {
    format!(
        "find '{root}' -mindepth 1 -delete && git -C '{root}' init -q",
        root = PROJECT_ROOT
    )
}

/// Восстановить на воркстейшне файлы проекта из ветки прерванной сессии.
pub async fn restore_workspace(
    executor: &Executor,
    branch: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    execute_via_executor(executor, &restore_branch_command(branch)).await?;
    Ok(())
}

/// Очистить проект отпущенного воркстейшна (`release_workspace_command`).
pub async fn release_workspace(
    executor: &Executor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    execute_via_executor(executor, &release_workspace_command()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_command_replaces_project_contents_without_recreate() {
        let cmd = switch_project_command("https://example.com/new.git", "ws-5");
        // Директория проекта перезаписывается кодом нового проекта через
        // git (exec в работающий ws), а не через пересоздание пода.
        assert!(cmd.contains("git clone 'https://example.com/new.git'"));
        assert!(cmd.contains("checkout -B 'ws-5'"));
        // Клонирование идёт во временный каталог, содержимое текущего проекта
        // очищается только после успешного clone — сбой не оставляет ws без
        // проекта. Корень `/work/project` не удаляется (bind-mount в dev).
        assert!(
            cmd.find("git clone").unwrap()
                < cmd
                    .find(&format!("find '{PROJECT_ROOT}' -mindepth 1 -delete"))
                    .unwrap()
        );
        // Временный каталог уникален на вызов (`$$` — PID шелла exec, вне
        // кавычек): два параллельных switch не дерутся за общий путь.
        assert!(cmd.contains(&format!(
            "cp -a '{PROJECT_ROOT}.new.'$$/. '{PROJECT_ROOT}/'"
        )));
        assert!(cmd.contains(&format!("rm -rf '{PROJECT_ROOT}.new.'$$")));
    }

    #[test]
    fn restore_command_checks_out_interrupted_session_branch() {
        let cmd = restore_branch_command("ws-3");
        assert!(cmd.contains("git fetch origin 'ws-3'"));
        assert!(cmd.contains("git checkout 'ws-3'"));
    }

    #[test]
    fn release_command_clears_project_to_empty_repo() {
        let cmd = release_workspace_command();
        // Очищается содержимое, а не сам каталог (bind-mount в dev).
        assert!(cmd.contains(&format!("find '{PROJECT_ROOT}' -mindepth 1 -delete")));
        assert!(cmd.contains(&format!("git -C '{PROJECT_ROOT}' init -q")));
    }
}
