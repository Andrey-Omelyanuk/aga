use crate::agent::{execute_via_executor, Executor};
use crate::project_files::PROJECT_ROOT;

/// Перезаписать каталог проекта воркстейшна кодом указанного (другого)
/// проекта. Выполняется в уже работающем ws (`kubectl exec`/`docker exec`),
/// поэтому сам ws (под/сервис) не пересоздаётся — в отличие от `create_pod`.
pub fn switch_project_command(git_url: &str, branch: &str) -> String {
    format!(
        "rm -rf '{root}' && git clone '{url}' '{root}' && git -C '{root}' checkout -B '{branch}'",
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

/// Восстановить на воркстейшне файлы проекта из ветки прерванной сессии.
pub async fn restore_workspace(
    executor: &Executor,
    branch: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    execute_via_executor(executor, &restore_branch_command(branch)).await?;
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
        assert!(cmd.contains(&format!("rm -rf '{PROJECT_ROOT}'")));
        assert!(cmd.contains("git clone 'https://example.com/new.git'"));
        assert!(cmd.contains("checkout -B 'ws-5'"));
    }

    #[test]
    fn restore_command_checks_out_interrupted_session_branch() {
        let cmd = restore_branch_command("ws-3");
        assert!(cmd.contains("git fetch origin 'ws-3'"));
        assert!(cmd.contains("git checkout 'ws-3'"));
    }
}
