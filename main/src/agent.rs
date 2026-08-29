use crate::config::RoleConfig;
use crate::llm::LlmClient;
use crate::trace::TraceStore;
use regex::Regex;
use tokio::process::Command;

/// Способ исполнения команд.
#[derive(Debug, Clone, Default)]
pub enum Executor {
    /// Локальное исполнение `sh -c` (dev-режим, без воркстейшна).
    #[default]
    Sh,
    /// Исполнение внутри пода воркстейшна через `kubectl exec`.
    KubectlExec { namespace: String, pod: String },
    /// Исполнение внутри контейнера воркстейшна через `docker exec` (dev).
    DockerExec { container: String },
}

/// Аргументы для `kubectl exec` в под воркстейшна.
pub fn kubectl_exec_args(namespace: &str, pod: &str, command: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        "-n".to_string(),
        namespace.to_string(),
        pod.to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-c".to_string(),
        command.to_string(),
    ]
}

/// Аргументы для `docker exec` в контейнер воркстейшна. В отличие от kubectl,
/// docker exec разделитель `--` не понимает — команда идёт сразу после имени.
pub fn docker_exec_args(container: &str, command: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        container.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        command.to_string(),
    ]
}

pub struct Agent {
    role_config: RoleConfig,
    llm_client: LlmClient,
    trace_store: TraceStore,
    command_regex: Regex,
    ask_human_regex: Regex,
    executor: Executor,
}

impl Agent {
    pub fn with_executor(
        role_config: RoleConfig,
        llm_client: LlmClient,
        trace_store: TraceStore,
        executor: Executor,
    ) -> Self {
        let command_regex = Regex::new(r"```(?:bash|sh)?\n(.*?)\n```").unwrap();
        let ask_human_regex = Regex::new(r"\[ASK_HUMAN\](.*?)\[/ASK_HUMAN\]").unwrap();

        Self {
            role_config,
            llm_client,
            trace_store,
            command_regex,
            ask_human_regex,
            executor,
        }
    }

    pub async fn run(
        &self,
        task_id: &str,
        task: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.trace_store
            .create_task(
                task_id,
                self.role_config
                    .prompt
                    .split_whitespace()
                    .next()
                    .unwrap_or("unknown"),
            )
            .await?;

        let mut history: Vec<String> = Vec::new();
        let mut step = 0;
        let mut result = String::new();

        while step < self.role_config.max_iterations as i32 {
            step += 1;

            // Запрос к LLM
            let response = self
                .llm_client
                .chat(
                    &self.role_config.llm,
                    &self.role_config.prompt,
                    task,
                    &history,
                )
                .await?;

            self.trace_store
                .add_entry(task_id, step, "llm_response", &response, None)
                .await?;
            history.push(task.to_string());
            history.push(response.clone());

            // Проверяем на запрос к человеку
            if let Some(captures) = self.ask_human_regex.captures(&response) {
                if let Some(question) = captures.get(1) {
                    let question_text = question.as_str().trim().to_string();
                    let request_id = self
                        .trace_store
                        .create_human_request(task_id, &question_text)
                        .await?;

                    self.trace_store
                        .add_entry(
                            task_id,
                            step,
                            "human_request",
                            &question_text,
                            Some(&request_id),
                        )
                        .await?;

                    return Ok(format!("[WAITING_FOR_HUMAN] Request ID: {}", request_id));
                }
            }

            // Извлекаем команды из ответа
            let commands = self.extract_commands(&response);

            if commands.is_empty() {
                // Если нет команд, считаем что это финальный ответ
                result = response.clone();
                break;
            }

            // Выполняем команды
            for cmd in commands {
                if !self.is_command_allowed(&cmd) {
                    let error = format!("Command '{}' is not allowed", cmd);
                    self.trace_store
                        .add_entry(task_id, step, "error", &error, None)
                        .await?;
                    return Err(error.into());
                }

                self.trace_store
                    .add_entry(task_id, step, "command", &cmd, None)
                    .await?;

                // Выполняем команду выбранным executor'ом
                let output = self.execute_command(&cmd).await?;
                self.trace_store
                    .add_entry(task_id, step, "command_output", &output, None)
                    .await?;
                history.push(format!("$ {}\n{}", cmd, output));
            }
        }

        let status = if step >= self.role_config.max_iterations as i32 {
            "max_iterations_reached"
        } else {
            "completed"
        };

        self.trace_store.complete_task(task_id, status).await?;

        if result.is_empty() {
            result = "Task completed. Check trace for details.".to_string();
        }

        Ok(result)
    }

    async fn execute_command(
        &self,
        command: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match &self.executor {
            Executor::Sh => {
                let output = Command::new("sh").arg("-c").arg(command).output().await?;
                Self::collect(output)
            }
            Executor::KubectlExec { namespace, pod } => {
                let output = Command::new("kubectl")
                    .args(kubectl_exec_args(namespace, pod, command))
                    .output()
                    .await?;
                Self::collect(output)
            }
            Executor::DockerExec { container } => {
                let output = Command::new("docker")
                    .args(docker_exec_args(container, command))
                    .output()
                    .await?;
                Self::collect(output)
            }
        }
    }

    fn collect(
        output: std::process::Output,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(format!("Command failed: {}\n{}", stderr, stdout).into())
        }
    }

    fn extract_commands(&self, text: &str) -> Vec<String> {
        let mut commands = Vec::new();
        for captures in self.command_regex.captures_iter(text) {
            if let Some(cmd) = captures.get(1) {
                let cmd_text = cmd.as_str().trim().to_string();
                // Разбиваем на отдельные команды если их несколько
                for line in cmd_text.split('\n') {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        commands.push(trimmed.to_string());
                    }
                }
            }
        }
        commands
    }

    fn is_command_allowed(&self, command: &str) -> bool {
        // Извлекаем первую часть команды (до пробелов и аргументов)
        let base_cmd = command.split_whitespace().next().unwrap_or("");

        // Проверка на опасные конструкции
        if command.contains('|')
            || command.contains('>')
            || command.contains('<')
            || command.contains(';')
            || command.contains('&')
        {
            return false;
        }

        self.role_config
            .allowed_commands
            .iter()
            .any(|allowed| base_cmd == allowed || base_cmd.starts_with(&format!("{}-", allowed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_commands_run_in_workstation_pod() {
        assert_eq!(
            kubectl_exec_args("aga", "ws-7", "ls -la"),
            vec!["exec", "-n", "aga", "ws-7", "--", "sh", "-c", "ls -la"]
        );
    }

    #[test]
    fn agent_commands_run_in_workstation_container() {
        assert_eq!(
            docker_exec_args("ws-7", "ls -la"),
            vec!["exec", "ws-7", "sh", "-c", "ls -la"]
        );
    }
}
