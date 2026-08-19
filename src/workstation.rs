use std::path::Path;

use crate::agent::Executor;
use crate::chat::ChatStore;
use crate::trace::TraceStore;

/// Разрешить способ исполнения команд для воркстейшна.
///
/// Если у проекта воркстейшна есть docker-compose.yml — исполняем команды
/// через `docker compose exec` внутрь первого сервиса. Иначе — локальный `sh -c`.
pub async fn executor_for_workstation(
    chat_store: &ChatStore,
    trace_store: &TraceStore,
    workstation_id: Option<i64>,
) -> Executor {
    let Some(ws_id) = workstation_id else {
        return Executor::Sh;
    };

    let Ok(Some(ws)) = chat_store.get_workstation(ws_id).await else {
        return Executor::Sh;
    };

    let Ok(Some(project)) = trace_store.get_project(ws.project_id).await else {
        return Executor::Sh;
    };

    compose_executor(&project.compose_path).unwrap_or(Executor::Sh)
}

/// По пути к compose-файлу собирает `Executor::DockerCompose`, выбирая первый сервис.
fn compose_executor(compose_path: &str) -> Option<Executor> {
    let path = Path::new(compose_path);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let service = first_service(&content)?;
    Some(Executor::DockerCompose {
        compose_path: compose_path.to_string(),
        service,
    })
}

/// Первый сервис из docker-compose (под `services:`), по отступу в 2 пробела.
fn first_service(content: &str) -> Option<String> {
    let mut in_services = false;
    for line in content.lines() {
        let t = line.trim_end();
        if t == "services:" {
            in_services = true;
            continue;
        }
        if !in_services {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        // Пустая строка или комментарий внутри services-блока — пропускаем.
        if t.trim().is_empty() || t.trim_start().starts_with('#') {
            continue;
        }
        // Сервис — `  <name>:`, отступ ровно 2 пробела.
        if indent == 2 && t.ends_with(':') {
            let name = t[..t.len() - 1].trim();
            if !name.is_empty() && !name.contains(' ') {
                return Some(name.to_string());
            }
        }
        // Отступ меньше 2 (и это не комментарий) — services-блок закончился.
        if indent < 2 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_first_service() {
        let yaml = "version: \"3\"\nservices:\n  api:\n    image: x\n  worker:\n    image: y\n";
        assert_eq!(first_service(yaml), Some("api".to_string()));
    }

    #[test]
    fn no_services() {
        assert_eq!(first_service("foo: bar\n"), None);
    }
}
