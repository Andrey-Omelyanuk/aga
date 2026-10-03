use crate::config::RoleConfig;
use crate::llm::{LlmClient, Message};
use crate::mcp::{self, McpClient};
use crate::scope;
use crate::trace::TraceStore;
use regex::Regex;
use tokio::process::Command;

/// Общая инструкция агентам и протокол инструментов по режиму LLM-подключения.
const COMMON_PROMPT: &str = include_str!("../prompts/all-agents.md");
const NATIVE_TOOLS_PROMPT: &str = include_str!("../prompts/tools-native.md");
const TEXT_TOOLS_PROMPT: &str = include_str!("../prompts/tools-text.md");

/// Слова оболочки, которым не нужен список инструментов агента: встроенные
/// команды и ключевые слова sh.
const SHELL_BUILTINS: &[&str] = &[
    "cd", "pwd", "echo", "printf", "true", "false", "test", "[", "exit", "export", "read",
];
/// Ключевые слова, после которых снова идёт команда (`if make; then ...`).
const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "while", "until", "do", "done", "{", "}", "!",
];

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
/// Команды агента выполняются от uid/gid 1000 (владелец bind-mount на хосте dev),
/// чтобы файлы проекта принадлежали хостовому пользователю, а не root. uid 1000
/// в образе воркстейшна — пользователь `aga` с home `/home/aga` (Dockerfile),
/// поэтому у агента есть свой `~/.ssh` с ключом от `inject_ssh_key`.
pub fn docker_exec_args(container: &str, command: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        "-u".to_string(),
        "1000:1000".to_string(),
        container.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        command.to_string(),
    ]
}

/// От чьего имени идёт команда в воркстейшне: пользователь станции (служебные
/// команды ядра) или root — песочница владения (`scope::sandbox_command`)
/// строится от root и сама переходит на пользователя станции.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunAs {
    Station,
    Root,
}

/// Процесс `command` executor'ом. `interactive` — stdin открыт (`exec -i`) для
/// долгоживущих stdio-серверов (MCP); пайпы и время жизни настраивает
/// вызывающий. В k8s `kubectl exec` и так идёт от root пода; в docker
/// пользователь задаётся явно.
pub fn exec_command(
    executor: &Executor,
    command: &str,
    interactive: bool,
    run_as: RunAs,
) -> Command {
    match executor {
        Executor::Sh => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        }
        Executor::KubectlExec { namespace, pod } => {
            let mut args = kubectl_exec_args(namespace, pod, command);
            if interactive {
                args.insert(1, "-i".to_string());
            }
            let mut c = Command::new("kubectl");
            c.args(args);
            c
        }
        Executor::DockerExec { container } => {
            let mut args = docker_exec_args(container, command);
            if run_as == RunAs::Root {
                args[2] = "0:0".to_string();
            }
            if interactive {
                args.insert(1, "-i".to_string());
            }
            let mut c = Command::new("docker");
            c.args(args);
            c
        }
    }
}

/// Исполнить `command` выбранным executor'ом и вернуть stdout строкой.
/// Общий путь для служебных команд ядра (просмотр файлов, git, ws_ops):
/// ненулевой код выхода — ошибка.
pub async fn execute_via_executor(
    executor: &Executor,
    command: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    collect_output(run_via_executor(executor, command).await?)
}

/// Исполнить `command` и вернуть процесс целиком (код выхода, stdout, stderr).
/// Ошибка — только если не удалось запустить сам executor.
pub async fn run_via_executor(
    executor: &Executor,
    command: &str,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    run_as(executor, command, RunAs::Station).await
}

/// То же от выбранного пользователя.
pub async fn run_as(
    executor: &Executor,
    command: &str,
    run_as: RunAs,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    Ok(exec_command(executor, command, false, run_as)
        .output()
        .await?)
}

fn collect_output(
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

/// Вывод команды агента для модели: stdout, затем stderr; ненулевой код выхода
/// дописывается строкой — модель видит провал и может исправиться.
fn format_output(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if !output.status.success() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        let code = output
            .status
            .code()
            .map_or("signal".to_string(), |c| c.to_string());
        text.push_str(&format!("[exit code {code}]"));
    }
    text
}

/// Описания нативных инструментов агента (OpenAI-compatible `tools`).
/// MCP-инструменты, когда появятся, добавятся в этот же список.
pub fn tool_specs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Выполнить команду POSIX sh в своей папке проекта воркстейшна. \
                    Пайпы, перенаправления и heredoc разрешены. Возвращает stdout, stderr и код выхода.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Команда или скрипт sh"}
                    },
                    "required": ["command"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "ask_human",
                "description": "Задать вопрос людям в чате и остановиться до ответа.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {"type": "string", "description": "Текст вопроса"}
                    },
                    "required": ["question"]
                }
            }
        }),
    ]
}

/// Инструмент `start_thread`: начать нить от сообщения, на которое отвечает
/// агент. Ответы в нити придут агенту отдельными запусками.
fn start_thread_spec() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "start_thread",
            "description": "Начать нить (отдельное обсуждение) от сообщения, на которое ты отвечаешь. \
                Упомяни в первом сообщении @участника, которого просишь. Все ответы в нити придут \
                тебе отдельно — после вызова закончи текущий ответ.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Заголовок нити"},
                    "message": {"type": "string", "description": "Первое сообщение нити"}
                },
                "required": ["title", "message"]
            }
        }
    })
}

/// Действие, которое модель попросила выполнить на шаге.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    /// Команда sh; `call_id` — id нативного вызова (в текстовом режиме пусто).
    Shell {
        call_id: String,
        command: String,
    },
    /// Инструмент MCP-сервера: индекс инструмента в `McpSession::tools`.
    Mcp {
        call_id: String,
        tool: usize,
        arguments: serde_json::Value,
    },
    AskHuman(String),
    StartThread {
        call_id: String,
        title: String,
        message: String,
    },
    /// Нативный вызов, который не разобрать: ошибка уходит модели результатом.
    Invalid {
        call_id: String,
        error: String,
    },
}

/// Действия из нативных вызовов инструментов.
fn native_actions(reply: &Message, mcp: &McpSession) -> Vec<Action> {
    reply
        .tool_calls
        .iter()
        .map(|call| {
            let args: serde_json::Value =
                serde_json::from_str(&call.function.arguments).unwrap_or_default();
            let arg = |name: &str| {
                args.get(name)
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            };
            let call_id = call.id.clone();
            match call.function.name.as_str() {
                "shell" => match arg("command") {
                    Some(command) => Action::Shell { call_id, command },
                    None => Action::Invalid {
                        call_id,
                        error: "Ошибка: у shell нет аргумента command".to_string(),
                    },
                },
                "start_thread" if mcp.start_thread => match (arg("title"), arg("message")) {
                    (Some(title), Some(message)) => Action::StartThread {
                        call_id,
                        title,
                        message,
                    },
                    _ => Action::Invalid {
                        call_id,
                        error: "Ошибка: у start_thread нужны title и message".to_string(),
                    },
                },
                "ask_human" => match arg("question") {
                    Some(q) => Action::AskHuman(q),
                    None => Action::Invalid {
                        call_id,
                        error: "Ошибка: у ask_human нет аргумента question".to_string(),
                    },
                },
                other => match mcp.tools.iter().position(|t| t.llm_name == other) {
                    Some(tool) => Action::Mcp {
                        call_id,
                        tool,
                        arguments: if args.is_object() {
                            args.clone()
                        } else {
                            serde_json::json!({})
                        },
                    },
                    None => Action::Invalid {
                        call_id,
                        error: format!("Ошибка: неизвестный инструмент `{other}`"),
                    },
                },
            }
        })
        .collect()
}

/// MCP-инструмент прогона: клиент сервера, имя у сервера и имя для LLM.
struct McpTool {
    client: usize,
    name: String,
    llm_name: String,
    spec: serde_json::Value,
}

/// Подключённые на прогон MCP-серверы и их инструменты. Клиенты закрываются
/// вместе с сессией (stdio-процесс получает EOF и завершается).
#[derive(Default)]
struct McpSession {
    clients: Vec<McpClient>,
    tools: Vec<McpTool>,
    /// Доступен ли встроенный `start_thread` (агент запущен из чата).
    start_thread: bool,
}

/// Первое слово каждой простой команды скрипта sh на верхнем уровне:
/// пайпы, `&&`/`||`/`;`/`&`, подоболочки и `$(...)`/обратные кавычки
/// вне двойных кавычек. Пропускаются присваивания `X=y`, цели
/// перенаправлений, тела heredoc и комментарии. Это защита от ошибок модели,
/// а не граница безопасности: граница — изолированный воркстейшн и проверка
/// территории по факту.
fn command_words(script: &str) -> Vec<String> {
    #[derive(Debug)]
    enum Tok {
        Word(String),
        /// Разделитель команд (`|`, `&&`, `;`, перевод строки, `(`...).
        Sep,
        /// Перенаправление: следующее слово — цель, не команда.
        Redir,
        /// Начало подстановки `$(` / обратной кавычки и её конец.
        Open,
        Close,
    }
    let chars: Vec<char> = script.chars().collect();
    let n = chars.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut cur = String::new();
    let mut has_word = false;
    let mut heredocs: Vec<String> = Vec::new();
    let mut in_backtick = false;
    let mut subst_depth: Vec<bool> = Vec::new(); // true — `$(`, false — `(`
    let flush = |toks: &mut Vec<Tok>, cur: &mut String, has_word: &mut bool| {
        if *has_word {
            toks.push(Tok::Word(std::mem::take(cur)));
            *has_word = false;
        }
    };
    let mut i = 0;
    while i < n {
        let c = chars[i];
        match c {
            '\'' => {
                has_word = true;
                i += 1;
                while i < n && chars[i] != '\'' {
                    cur.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                has_word = true;
                i += 1;
                while i < n && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < n {
                        i += 1;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                // `\<перевод строки>` — продолжение строки.
                if i + 1 < n && chars[i + 1] != '\n' {
                    cur.push(chars[i + 1]);
                    has_word = true;
                }
                i += 2;
            }
            '#' if !has_word => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' => {
                flush(&mut toks, &mut cur, &mut has_word);
                i += 1;
            }
            '\n' => {
                flush(&mut toks, &mut cur, &mut has_word);
                toks.push(Tok::Sep);
                i += 1;
                // Тела heredoc — данные, не команды: пропускаем до разделителя.
                for delim in std::mem::take(&mut heredocs) {
                    while i < n {
                        let end = (i..n).find(|&j| chars[j] == '\n').unwrap_or(n);
                        let line: String = chars[i..end].iter().collect();
                        i = (end + 1).min(n);
                        if line.trim_start_matches('\t') == delim {
                            break;
                        }
                    }
                }
            }
            '`' => {
                flush(&mut toks, &mut cur, &mut has_word);
                toks.push(if in_backtick { Tok::Close } else { Tok::Open });
                in_backtick = !in_backtick;
                i += 1;
            }
            '(' => {
                let subst = cur.ends_with('$');
                if subst {
                    cur.pop();
                }
                flush(&mut toks, &mut cur, &mut has_word);
                subst_depth.push(subst);
                toks.push(if subst { Tok::Open } else { Tok::Sep });
                i += 1;
            }
            ')' => {
                flush(&mut toks, &mut cur, &mut has_word);
                toks.push(if subst_depth.pop().unwrap_or(false) {
                    Tok::Close
                } else {
                    Tok::Sep
                });
                i += 1;
            }
            '|' | '&' | ';' => {
                flush(&mut toks, &mut cur, &mut has_word);
                toks.push(Tok::Sep);
                i += 1;
                while i < n && matches!(chars[i], '|' | '&' | ';') {
                    i += 1;
                }
            }
            '<' if i + 1 < n && chars[i + 1] == '<' => {
                // heredoc: `<<EOF`, `<<-EOF`, `<< 'EOF'`.
                if !cur.is_empty() && cur.chars().all(|d| d.is_ascii_digit()) {
                    cur.clear();
                    has_word = false;
                }
                flush(&mut toks, &mut cur, &mut has_word);
                i += 2;
                if i < n && chars[i] == '-' {
                    i += 1;
                }
                while i < n && matches!(chars[i], ' ' | '\t') {
                    i += 1;
                }
                let mut delim = String::new();
                while i < n && !matches!(chars[i], ' ' | '\t' | '\n' | ';' | '|' | '&' | ')') {
                    if !matches!(chars[i], '\'' | '"' | '\\') {
                        delim.push(chars[i]);
                    }
                    i += 1;
                }
                toks.push(Tok::Redir);
                toks.push(Tok::Word(delim.clone()));
                heredocs.push(delim);
            }
            '>' | '<' => {
                // Номер дескриптора (`2>`) — часть перенаправления.
                if !cur.is_empty() && cur.chars().all(|d| d.is_ascii_digit()) {
                    cur.clear();
                    has_word = false;
                }
                flush(&mut toks, &mut cur, &mut has_word);
                i += 1;
                while i < n && matches!(chars[i], '>' | '&' | '|') {
                    i += 1;
                }
                // `2>&1`, `>&2`: цель — номер дескриптора сразу за `&`.
                toks.push(Tok::Redir);
            }
            _ => {
                cur.push(c);
                has_word = true;
                i += 1;
            }
        }
    }
    flush(&mut toks, &mut cur, &mut has_word);

    let mut words = Vec::new();
    let mut at_start = true;
    let mut skip_next = false;
    let mut saved: Vec<bool> = Vec::new();
    for tok in toks {
        match tok {
            Tok::Sep => at_start = true,
            Tok::Redir => skip_next = true,
            Tok::Open => {
                saved.push(at_start);
                at_start = true;
            }
            Tok::Close => at_start = saved.pop().unwrap_or(false),
            Tok::Word(w) => {
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if !at_start {
                    continue;
                }
                let is_assignment = w.split_once('=').is_some_and(|(name, _)| {
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                });
                if is_assignment {
                    continue;
                }
                if SHELL_KEYWORDS.contains(&w.as_str()) {
                    continue;
                }
                at_start = false;
                words.push(w);
            }
        }
    }
    words
}

/// Разрешена ли команда списком инструментов агента: каждая простая команда
/// скрипта — встроенная команда sh или инструмент из списка (с подкомандами
/// вида `docker-compose` для `docker`). Err — первое неразрешённое слово.
fn command_allowed(tools: &[String], script: &str) -> Result<(), String> {
    for word in command_words(script) {
        let ok = SHELL_BUILTINS.contains(&word.as_str())
            || tools
                .iter()
                .any(|t| word == *t || word.starts_with(&format!("{t}-")));
        if !ok {
            return Err(word);
        }
    }
    Ok(())
}

pub struct Agent {
    role_config: RoleConfig,
    llm_client: LlmClient,
    trace_store: TraceStore,
    command_regex: Regex,
    ask_human_regex: Regex,
    executor: Executor,
    /// Территория агента в воркстейшне: None — вне воркстейшна (границы нет).
    scope: Option<scope::Territory>,
    /// Корень проекта в воркстейшне (`/work/project`; тесты — временное репо).
    project_root: String,
    /// Инструмент `start_thread` (только при заданном — в чате рантайма).
    thread_starter: Option<ThreadStarter>,
}

/// Итог прогона агента: финальный ответ или вопрос человеку (задача ставится
/// на `waiting_human`, ответ приходит в чат «ответом на сообщение-вопрос»).
#[derive(Debug, Clone)]
pub enum AgentOutcome {
    Answer(String),
    Question {
        request_id: String,
        question: String,
    },
}

/// Шаг работы для чата: вызов инструмента (тело — команда, скрытая часть —
/// вывод, `tool_call` — вызов `{"name", "arguments"}`) либо пояснение модели
/// между вызовами (`tool_call` нет, скрытая часть пустая).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub body: String,
    pub hidden: String,
    pub tool_call: Option<serde_json::Value>,
}

impl Step {
    fn note(text: String) -> Self {
        Self {
            body: text,
            hidden: String::new(),
            tool_call: None,
        }
    }

    fn call(body: String, hidden: String, name: &str, arguments: serde_json::Value) -> Self {
        Self {
            body,
            hidden,
            tool_call: Some(serde_json::json!({"name": name, "arguments": arguments})),
        }
    }
}

/// Наблюдатель шагов работы (рантайм публикует их в чат).
pub type StepSender = tokio::sync::mpsc::UnboundedSender<Step>;

/// Начать нить от сообщения, на которое отвечает агент: (заголовок, первое
/// сообщение) → текст результата для модели. Даёт рантайм — агент про чат не
/// знает.
pub type ThreadStarter = std::sync::Arc<
    dyn Fn(
            String,
            String,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn with_executor(
        role_config: RoleConfig,
        llm_client: LlmClient,
        trace_store: TraceStore,
        executor: Executor,
        scope: Option<scope::Territory>,
    ) -> Self {
        let command_regex = Regex::new(r"(?s)```(?:bash|sh|shell)[ \t]*\n(.*?)\n?```").unwrap();
        let ask_human_regex = Regex::new(r"(?s)\[ASK_HUMAN\](.*?)\[/ASK_HUMAN\]").unwrap();

        Self {
            role_config,
            llm_client,
            trace_store,
            command_regex,
            ask_human_regex,
            executor,
            scope,
            project_root: scope::PROJECT_ROOT.to_string(),
            thread_starter: None,
        }
    }

    /// Дать агенту инструмент `start_thread`.
    pub fn with_thread_starter(mut self, starter: ThreadStarter) -> Self {
        self.thread_starter = Some(starter);
        self
    }

    /// Системный промпт: правила агента (описание + скиллы), общая инструкция,
    /// протокол инструментов по режиму и список разрешённых инструментов.
    fn system_prompt(&self) -> String {
        let protocol = if self.role_config.llm.native_tools {
            NATIVE_TOOLS_PROMPT
        } else {
            TEXT_TOOLS_PROMPT
        };
        let tools = if self.role_config.tools.is_empty() {
            "нет (только встроенные команды sh)".to_string()
        } else {
            self.role_config.tools.join(", ")
        };
        format!(
            "{}\n\n{}\n{}\nРазрешённые инструменты: {tools}.",
            self.role_config.prompt.trim(),
            COMMON_PROMPT.trim(),
            protocol.trim()
        )
    }

    /// Прогон цикла. `steps` — канал наблюдателя: после каждой выполненной
    /// команды уходит `(команда, вывод)`, а пояснение модели между вызовами —
    /// `(текст, "")`, чтобы рантайм публиковал ход работы в чат. Агент про чат
    /// ничего не знает.
    ///
    /// Режим берётся из LLM-подключения: нативный function calling (инструменты
    /// `shell`/`ask_human`) или запасной текстовый (блоки ```bash и маркер
    /// `[ASK_HUMAN]`). Ответ без действий — финальный.
    pub async fn run(
        &self,
        task_id: &str,
        context: Vec<Message>,
        steps: Option<StepSender>,
    ) -> Result<AgentOutcome, Box<dyn std::error::Error + Send + Sync>> {
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

        let native = self.role_config.llm.native_tools;
        // MCP — только в нативном режиме: в текстовом вызывать их нечем.
        let mut mcp = if native {
            self.connect_mcp(task_id, steps.as_ref()).await?
        } else {
            McpSession::default()
        };
        mcp.start_thread = native && self.thread_starter.is_some();
        let tools = if native {
            let mut specs = tool_specs();
            if mcp.start_thread {
                specs.push(start_thread_spec());
            }
            specs.extend(mcp.tools.iter().map(|t| t.spec.clone()));
            specs
        } else {
            Vec::new()
        };
        let mut messages = vec![Message::system(&self.system_prompt())];
        messages.extend(context);
        let max_steps = self.role_config.max_iterations as i32;
        let mut answer: Option<String> = None;

        for step in 1..=max_steps {
            let mut reply = self
                .llm_client
                .chat(&self.role_config.llm, &messages, &tools)
                .await?;
            // id вызова обязателен для ответа на него; часть серверов его не даёт.
            for (i, call) in reply.tool_calls.iter_mut().enumerate() {
                if call.id.is_empty() {
                    call.id = format!("call_{step}_{i}");
                }
            }
            let mut logged = reply.content.clone();
            if !reply.tool_calls.is_empty() {
                logged.push_str(&format!(
                    "\n[tool_calls] {}",
                    serde_json::to_string(&reply.tool_calls).unwrap_or_default()
                ));
            }
            self.trace_store
                .add_entry(task_id, step, "llm_response", &logged, None)
                .await?;

            let (actions, note) = if native {
                (
                    native_actions(&reply, &mcp),
                    reply.content.trim().to_string(),
                )
            } else {
                self.text_actions(&reply.content)
            };
            if actions.is_empty() {
                answer = Some(reply.content.trim().to_string());
                break;
            }
            if !note.is_empty() {
                if let Some(tx) = &steps {
                    let _ = tx.send(Step::note(note));
                }
            }
            messages.push(if native {
                reply.clone()
            } else {
                Message::assistant(&reply.content)
            });

            let mut text_results: Vec<String> = Vec::new();
            for action in actions {
                match action {
                    Action::StartThread {
                        call_id,
                        title,
                        message,
                    } => {
                        self.trace_store
                            .add_entry(
                                task_id,
                                step,
                                "command",
                                &format!("start_thread «{title}»: {message}"),
                                None,
                            )
                            .await?;
                        let result = match &self.thread_starter {
                            Some(start) => match start(title, message).await {
                                Ok(text) => text,
                                Err(e) => format!("Ошибка: {e}"),
                            },
                            None => "Ошибка: нити здесь недоступны".to_string(),
                        };
                        self.trace_store
                            .add_entry(task_id, step, "command_output", &result, None)
                            .await?;
                        messages.push(Message::tool(&call_id, &result));
                    }
                    Action::AskHuman(question) => {
                        return self.ask_human(task_id, step, &question).await;
                    }
                    Action::Shell { call_id, command } => {
                        let output = self.shell(task_id, step, &command, steps.as_ref()).await?;
                        if native {
                            messages.push(Message::tool(&call_id, &output));
                        } else {
                            text_results.push(format!("$ {command}\n{output}"));
                        }
                    }
                    Action::Mcp {
                        call_id,
                        tool,
                        arguments,
                    } => {
                        let output = self
                            .mcp_call(task_id, step, &mut mcp, tool, arguments, steps.as_ref())
                            .await?;
                        messages.push(Message::tool(&call_id, &output));
                    }
                    Action::Invalid { call_id, error } => {
                        self.trace_store
                            .add_entry(task_id, step, "error", &error, None)
                            .await?;
                        messages.push(Message::tool(&call_id, &error));
                    }
                }
            }
            if !text_results.is_empty() {
                messages.push(Message::user(&text_results.join("\n\n")));
            }
        }

        let (status, text) = match answer {
            Some(text) if !text.is_empty() => ("completed", text),
            Some(_) => (
                "completed",
                "Готово (агент закончил без текста ответа).".to_string(),
            ),
            None => (
                "max_iterations_reached",
                format!(
                    "Лимит шагов ({max_steps}) исчерпан, задача не завершена. \
                     Ход работы — в чате и трассе."
                ),
            ),
        };
        self.trace_store.complete_task(task_id, status).await?;
        Ok(AgentOutcome::Answer(text))
    }

    /// Подключить MCP-серверы агента и собрать их инструменты. Недоступный
    /// сервер не валит прогон: ошибка — в трассу и пояснением в чат, агент
    /// работает без его инструментов.
    async fn connect_mcp(
        &self,
        task_id: &str,
        steps: Option<&StepSender>,
    ) -> Result<McpSession, Box<dyn std::error::Error + Send + Sync>> {
        let mut session = McpSession::default();
        let builtin: Vec<String> = tool_specs()
            .into_iter()
            .chain(std::iter::once(start_thread_spec()))
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
            .collect();
        for server in &self.role_config.mcp_servers {
            // stdio-сервер живёт в воркстейшне агента — в той же песочнице
            // владения, что и его команды (cwd — папка агента).
            let launch = |cmd: &str| self.process(cmd, true);
            let connected = match McpClient::connect(server, launch).await {
                Ok(mut client) => client.list_tools().await.map(|tools| (client, tools)),
                Err(e) => Err(e),
            };
            let (client, tools) = match connected {
                Ok(pair) => pair,
                Err(e) => {
                    let error = format!("MCP-сервер {} недоступен: {e}", server.name);
                    self.trace_store
                        .add_entry(task_id, 0, "error", &error, None)
                        .await?;
                    if let Some(tx) = steps {
                        let _ = tx.send(Step::note(error));
                    }
                    continue;
                }
            };
            let idx = session.clients.len();
            session.clients.push(client);
            for tool in tools {
                let llm_name = mcp::llm_tool_name(&server.name, &tool.name);
                if builtin.contains(&llm_name)
                    || session.tools.iter().any(|t| t.llm_name == llm_name)
                {
                    continue;
                }
                let description = format!("[MCP {}] {}", server.name, tool.description);
                session.tools.push(McpTool {
                    client: idx,
                    spec: serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": llm_name,
                            "description": description.trim(),
                            "parameters": tool.input_schema,
                        }
                    }),
                    name: tool.name,
                    llm_name,
                });
            }
        }
        Ok(session)
    }

    /// Вызов инструмента MCP: трасса, шаг в чат (тело — `mcp <сервер>.<инструмент>
    /// <аргументы>`, скрытая часть — результат). stdio-сервер и так заперт в
    /// песочнице владения агента. Ошибка инструмента или транспорта уходит
    /// модели текстом.
    async fn mcp_call(
        &self,
        task_id: &str,
        step: i32,
        mcp: &mut McpSession,
        tool: usize,
        arguments: serde_json::Value,
        steps: Option<&StepSender>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let (client, name, llm_name) = (
            mcp.tools[tool].client,
            mcp.tools[tool].name.clone(),
            mcp.tools[tool].llm_name.clone(),
        );
        let mcp_client = &mut mcp.clients[client];
        let label = format!("mcp {}.{name} {arguments}", mcp_client.name);
        self.trace_store
            .add_entry(task_id, step, "command", &label, None)
            .await?;
        let output = match mcp_client.call_tool(&name, arguments.clone()).await {
            Ok((text, false)) => text,
            Ok((text, true)) => format!("Ошибка инструмента: {text}"),
            Err(e) => format!("Ошибка MCP: {e}"),
        };
        self.trace_store
            .add_entry(task_id, step, "command_output", &output, None)
            .await?;
        if let Some(tx) = steps {
            let _ = tx.send(Step::call(label, output.clone(), &llm_name, arguments));
        }
        Ok(output)
    }

    /// Действия текстового режима и пояснение (текст ответа без блоков команд).
    /// Вопрос человеку важнее команд того же ответа.
    fn text_actions(&self, text: &str) -> (Vec<Action>, String) {
        if let Some(q) = self
            .ask_human_regex
            .captures(text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|q| !q.is_empty())
        {
            return (vec![Action::AskHuman(q)], String::new());
        }
        let actions: Vec<Action> = self
            .command_regex
            .captures_iter(text)
            .filter_map(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|cmd| !cmd.is_empty())
            .map(|command| Action::Shell {
                call_id: String::new(),
                command,
            })
            .collect();
        let note = self.command_regex.replace_all(text, "").trim().to_string();
        (actions, note)
    }

    /// Вопрос человеку: запрос в БД, задача — `waiting_human`.
    async fn ask_human(
        &self,
        task_id: &str,
        step: i32,
        question: &str,
    ) -> Result<AgentOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let request_id = self
            .trace_store
            .create_human_request(task_id, question)
            .await?;
        self.trace_store
            .add_entry(task_id, step, "human_request", question, Some(&request_id))
            .await?;
        self.trace_store.wait_human_task(task_id).await?;
        Ok(AgentOutcome::Question {
            request_id,
            question: question.to_string(),
        })
    }

    /// Инструмент `shell`: проверка списка инструментов, исполнение, трасса и
    /// шаг в чат. Отказ и провал команды — не ошибка прогона: текст уходит
    /// модели результатом, она может исправиться.
    async fn shell(
        &self,
        task_id: &str,
        step: i32,
        command: &str,
        steps: Option<&StepSender>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let output = match command_allowed(&self.role_config.tools, command) {
            Err(word) => {
                let error = format!(
                    "Команда отклонена: `{word}` нет в списке разрешённых инструментов ({}).",
                    self.role_config.tools.join(", ")
                );
                self.trace_store
                    .add_entry(task_id, step, "error", &error, None)
                    .await?;
                error
            }
            Ok(()) => {
                self.trace_store
                    .add_entry(task_id, step, "command", command, None)
                    .await?;
                let output = self.execute_command(command).await?;
                self.trace_store
                    .add_entry(task_id, step, "command_output", &output, None)
                    .await?;
                output
            }
        };
        if let Some(tx) = steps {
            let _ = tx.send(Step::call(
                command.to_string(),
                output.clone(),
                "shell",
                serde_json::json!({"command": command}),
            ));
        }
        Ok(output)
    }

    /// Процесс команды агента: в воркстейшне — в песочнице владения (запись
    /// только в территории агента, cwd — его папка, от root с переходом на
    /// пользователя станции), вне воркстейшна — как есть.
    fn process(&self, command: &str, interactive: bool) -> Command {
        match &self.scope {
            Some(territory) => exec_command(
                &self.executor,
                &scope::sandbox_command(&self.project_root, territory, command),
                interactive,
                RunAs::Root,
            ),
            None => exec_command(&self.executor, command, interactive, RunAs::Station),
        }
    }

    /// Исполнить команду агента и вернуть вывод для модели.
    async fn execute_command(
        &self,
        command: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(format_output(&self.process(command, false).output().await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmConfig;
    use crate::trace::TraceStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn role(tools: &[&str], api_url: Option<String>, native: bool) -> RoleConfig {
        RoleConfig {
            prompt: "промпт".to_string(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            max_iterations: 4,
            llm: LlmConfig {
                model: Some("m".into()),
                temperature: 0.7,
                api_url,
                api_key: None,
                native_tools: native,
            },
            mcp_servers: vec![],
        }
    }

    async fn store() -> (TraceStore, std::path::PathBuf) {
        let file = std::env::temp_dir().join(format!("aga_agent_test_{}.db", uuid::Uuid::new_v4()));
        let store = TraceStore::new(&file.to_string_lossy()).await.unwrap();
        (store, file)
    }

    /// Мок LLM с последовательностью ответов (после исчерпания — последний).
    /// Строка — ответ текстом, объект — сообщение ассистента целиком (с
    /// tool_calls). Тела запросов записываются.
    type Bodies = Arc<std::sync::Mutex<Vec<serde_json::Value>>>;
    async fn mock_llm_seq(
        answers: Vec<serde_json::Value>,
    ) -> (String, tokio::task::JoinHandle<()>, Bodies) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let idx = Arc::new(AtomicUsize::new(0));
        let bodies: Bodies = Arc::default();
        let bodies2 = bodies.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let answers = answers.clone();
                let idx = Arc::clone(&idx);
                bodies2.lock().unwrap().push(body);
                async move {
                    let i = idx.fetch_add(1, Ordering::SeqCst).min(answers.len() - 1);
                    let message = match &answers[i] {
                        serde_json::Value::String(text) => {
                            serde_json::json!({"role": "assistant", "content": text})
                        }
                        other => other.clone(),
                    };
                    axum::Json(serde_json::json!({ "choices": [{ "message": message }] }))
                }
            }),
        );
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), handle, bodies)
    }

    fn task() -> Vec<Message> {
        vec![Message::user("задача")]
    }

    fn shell_call(id: &str, command: &str) -> serde_json::Value {
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": id, "type": "function",
                "function": {"name": "shell", "arguments": serde_json::json!({"command": command}).to_string()}}]
        })
    }

    async fn cleanup(file: &std::path::PathBuf) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    }

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
            vec!["exec", "-u", "1000:1000", "ws-7", "sh", "-c", "ls -la"]
        );
    }

    #[test]
    fn agent_executes_only_tools_from_its_list() {
        let tools: Vec<String> = ["git", "make", "docker"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Инструменты списка — можно, с подкомандами тоже.
        assert!(command_allowed(&tools, "git status").is_ok());
        assert!(command_allowed(&tools, "make build").is_ok());
        assert!(command_allowed(&tools, "docker-compose up -d").is_ok());
        // Всё остальное — нельзя.
        assert_eq!(command_allowed(&tools, "rm -rf src"), Err("rm".into()));
        assert_eq!(command_allowed(&tools, "cargo test"), Err("cargo".into()));
    }

    #[test]
    fn every_command_of_pipeline_and_chain_is_checked() {
        let tools: Vec<String> = ["git", "head", "make", "cat", "grep", "ls"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Пайпы, цепочки и перенаправления разрешены — проверяется каждая команда.
        assert!(command_allowed(&tools, "git log | head -5").is_ok());
        assert!(command_allowed(&tools, "cd src && make build; ls 2>/dev/null").is_ok());
        assert!(command_allowed(&tools, "FOO=1 make test > out.txt 2>&1").is_ok());
        assert!(command_allowed(&tools, "if test -f x; then make; fi").is_ok());
        assert_eq!(command_allowed(&tools, "git log | wc -l"), Err("wc".into()));
        assert_eq!(command_allowed(&tools, "ls && rm x"), Err("rm".into()));
        // Подстановка команды тоже проверяется, её аргументы — нет.
        assert_eq!(command_allowed(&tools, "ls $(rm x) foo"), Err("rm".into()));
        assert!(command_allowed(&tools, "ls $(git ls-files) foo").is_ok());
        // Операторы внутри кавычек — не разделители.
        assert!(command_allowed(&tools, "grep 'a|b; rm' file").is_ok());
        assert!(command_allowed(&tools, "git commit -m \"fix && rm\"").is_ok());
        // Тело heredoc — данные, не команды; комментарии пропускаются.
        assert!(command_allowed(
            &tools,
            "cat > f.rs <<'EOF'\nfn main() {}\nrm all\nEOF\nls # rm"
        )
        .is_ok());
        assert_eq!(
            command_allowed(&tools, "cat > f <<EOF\nx\nEOF\nrm f"),
            Err("rm".into())
        );
    }

    #[tokio::test]
    async fn native_tool_call_runs_command_and_returns_output_to_model() {
        let (url, llm, bodies) = mock_llm_seq(vec![
            shell_call("c1", "echo hi | cat"),
            serde_json::json!("Готово"),
        ])
        .await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&["echo", "cat"], Some(url), true),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = agent.run("t-native", task(), Some(tx)).await.unwrap();
        match outcome {
            AgentOutcome::Answer(text) => assert_eq!(text, "Готово"),
            other => panic!("ожидался ответ, получен {other:?}"),
        }
        let step = rx.recv().await.expect("шаг не опубликован");
        assert_eq!(step.body, "echo hi | cat");
        assert_eq!(step.hidden, "hi\n");
        // Вызов JSON-ом — по нему следующий запуск восстановит этот шаг.
        assert_eq!(
            step.tool_call,
            Some(serde_json::json!({"name": "shell", "arguments": {"command": "echo hi | cat"}}))
        );
        // Модели ушли описания инструментов, а результат — tool-сообщением на вызов.
        let bodies = bodies.lock().unwrap().clone();
        assert_eq!(bodies[0]["tools"][0]["function"]["name"], "shell");
        let last = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(last["role"], "tool");
        assert_eq!(last["tool_call_id"], "c1");
        assert_eq!(last["content"], "hi\n");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn rejected_and_failed_commands_go_back_to_model() {
        let (url, llm, bodies) = mock_llm_seq(vec![
            shell_call("c1", "rm -rf /"),
            shell_call("c2", "ls /definitely-missing"),
            serde_json::json!("Понял"),
        ])
        .await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&["ls"], Some(url), true),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let outcome = agent.run("t-reject", task(), None).await.unwrap();
        assert!(matches!(outcome, AgentOutcome::Answer(t) if t == "Понял"));
        let bodies = bodies.lock().unwrap().clone();
        let rejected = bodies[1]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(rejected.contains("`rm` нет в списке"), "{rejected}");
        let failed = bodies[2]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(failed.contains("[exit code"), "{failed}");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn native_ask_human_returns_question_and_parks_task() {
        let (url, llm, _) = mock_llm_seq(vec![serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{"id": "q", "type": "function",
                "function": {"name": "ask_human", "arguments": {"question": "Куда катимся?"}}}]
        })])
        .await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&[], Some(url), true),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let outcome = agent.run("t-nq", task(), None).await.unwrap();
        assert!(
            matches!(&outcome, AgentOutcome::Question { question, .. } if question == "Куда катимся?")
        );
        let trace = agent.trace_store.get_trace("t-nq").await.unwrap().unwrap();
        assert_eq!(trace.status, "waiting_human");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn text_mode_runs_whole_bash_block_and_publishes_note() {
        let (url, llm, bodies) = mock_llm_seq(vec![
            serde_json::json!("Смотрю.\n```bash\necho a\necho b | cat\n```"),
            serde_json::json!("Готово"),
        ])
        .await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&["echo", "cat"], Some(url), false),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = agent.run("t-text", task(), Some(tx)).await.unwrap();
        assert!(matches!(outcome, AgentOutcome::Answer(t) if t == "Готово"));
        assert_eq!(rx.recv().await.unwrap(), Step::note("Смотрю.".to_string()));
        let step = rx.recv().await.unwrap();
        assert_eq!(
            (step.body.as_str(), step.hidden.as_str()),
            ("echo a\necho b | cat", "a\nb\n")
        );
        let bodies = bodies.lock().unwrap().clone();
        // В текстовом режиме tools не отправляются, вывод приходит user-сообщением.
        assert!(bodies[0].get("tools").is_none());
        let last = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"], "$ echo a\necho b | cat\na\nb\n");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn text_mode_ask_human_returns_question_and_parks_task() {
        let (url, llm, _) = mock_llm_seq(vec![serde_json::json!(
            "[ASK_HUMAN] Куда\nкатимся?[/ASK_HUMAN]"
        )])
        .await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&[], Some(url), false),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let outcome = agent.run("t-question", task(), None).await.unwrap();
        let request_id = match outcome {
            AgentOutcome::Question {
                request_id,
                question,
            } => {
                assert_eq!(question, "Куда\nкатимся?");
                request_id
            }
            other => panic!("ожидался вопрос, получен {other:?}"),
        };
        assert!(!request_id.is_empty());
        // Задача не остаётся «running» — она переведена в ожидание ответа.
        let trace = agent
            .trace_store
            .get_trace("t-question")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(trace.status, "waiting_human");
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn exhausted_steps_report_unfinished_task() {
        let (url, llm, _) = mock_llm_seq(vec![shell_call("c", "echo again")]).await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            role(&["echo"], Some(url), true),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let outcome = agent.run("t-max", task(), None).await.unwrap();
        assert!(matches!(outcome, AgentOutcome::Answer(t) if t.contains("Лимит шагов (4)")));
        let trace = agent.trace_store.get_trace("t-max").await.unwrap().unwrap();
        assert_eq!(trace.status, "max_iterations_reached");
        llm.abort();
        cleanup(&file).await;
    }

    fn with_mcp(mut role: RoleConfig, command: &str) -> RoleConfig {
        role.mcp_servers = vec![crate::mcp::tests::stdio_server(command)];
        role
    }

    #[tokio::test]
    async fn mcp_tools_are_offered_and_called_natively() {
        let (url, llm, bodies) = mock_llm_seq(vec![
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [{"id": "m1", "type": "function",
                    "function": {"name": "fake__add", "arguments": "{\"a\":2,\"b\":3}"}}]
            }),
            serde_json::json!("Пять"),
        ])
        .await;
        let (store, file) = store().await;
        let dir = std::env::temp_dir().join(format!("aga_mcp_agent_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cmd = format!(
            "cd '{}' && {}",
            dir.display(),
            crate::mcp::tests::FAKE_STDIO_SERVER
        );
        let agent = Agent::with_executor(
            with_mcp(role(&[], Some(url), true), &cmd),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = agent.run("t-mcp", task(), Some(tx)).await.unwrap();
        assert!(matches!(outcome, AgentOutcome::Answer(t) if t == "Пять"));
        // Шаг в чате: вызов инструмента и его результат.
        let step = rx.recv().await.unwrap();
        assert_eq!(step.body, r#"mcp fake.add {"a":2,"b":3}"#);
        assert_eq!(step.hidden, "5");
        assert_eq!(step.tool_call.unwrap()["name"], "fake__add");
        let bodies = bodies.lock().unwrap().clone();
        // Модели предложен инструмент сервера со схемой аргументов.
        let tools = bodies[0]["tools"].as_array().unwrap();
        let add = tools
            .iter()
            .find(|t| t["function"]["name"] == "fake__add")
            .expect("нет fake__add");
        assert_eq!(add["function"]["description"], "[MCP fake] Сложить числа");
        assert_eq!(
            add["function"]["parameters"]["properties"]["b"]["type"],
            "number"
        );
        let last = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(
            (last["role"].as_str(), last["content"].as_str()),
            (Some("tool"), Some("5"))
        );
        llm.abort();
        cleanup(&file).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unavailable_mcp_server_is_reported_and_run_continues() {
        let (url, llm, bodies) = mock_llm_seq(vec![serde_json::json!("ok")]).await;
        let (store, file) = store().await;
        let agent = Agent::with_executor(
            with_mcp(role(&[], Some(url), true), "exit 1"),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = agent.run("t-mcp-down", task(), Some(tx)).await.unwrap();
        assert!(matches!(outcome, AgentOutcome::Answer(t) if t == "ok"));
        let note = rx.recv().await.unwrap().body;
        assert!(note.starts_with("MCP-сервер fake недоступен"), "{note}");
        // Без сервера — только встроенные инструменты.
        let bodies = bodies.lock().unwrap().clone();
        assert_eq!(bodies[0]["tools"].as_array().unwrap().len(), 2);
        llm.abort();
        cleanup(&file).await;
    }

    #[tokio::test]
    async fn text_mode_does_not_start_mcp_servers() {
        let (url, llm, _) = mock_llm_seq(vec![serde_json::json!("ok")]).await;
        let (store, file) = store().await;
        let marker = std::env::temp_dir().join(format!("aga_mcp_text_{}", uuid::Uuid::new_v4()));
        let agent = Agent::with_executor(
            with_mcp(
                role(&[], Some(url), false),
                &format!("touch '{}'", marker.display()),
            ),
            LlmClient::new(),
            store,
            Executor::Sh,
            None,
        );
        agent.run("t-mcp-text", task(), None).await.unwrap();
        assert!(!marker.exists());
        llm.abort();
        cleanup(&file).await;
    }
}
