//! MCP-клиент (Model Context Protocol): инструменты MCP-серверов для агента.
//!
//! Протокол — JSON-RPC 2.0: `initialize` → `notifications/initialized` →
//! `tools/list` → `tools/call`. Транспорты:
//! - `http` — Streamable HTTP: POST JSON-RPC на url, ответ JSON или SSE-поток;
//!   сессия — заголовок `Mcp-Session-Id` из ответа на `initialize`.
//! - `stdio` — процесс сервера, сообщения построчно в stdin/stdout. Процесс
//!   запускается executor'ом агента (`docker exec -i` / `kubectl exec -i` в
//!   воркстейшн, локально — `sh -c`) и живёт один прогон агента: закрытие stdin
//!   завершает сервер.
//!
//! Клиент держится одного прогона (агент без состояния между задачами).

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::trace::McpServer;

/// Версия протокола, которую предлагает клиент (сервер может ответить своей).
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Таймаут подключения (initialize + список инструментов), сек.
const CONNECT_TIMEOUT_SECS: u64 = 30;
/// Таймаут одного вызова инструмента, сек.
const CALL_TIMEOUT_SECS: u64 = 300;

pub type McpResult<T> = Result<T, String>;

/// Инструмент MCP-сервера: имя, описание и JSON Schema аргументов.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema", default = "empty_schema")]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

enum Transport {
    Http {
        client: reqwest::Client,
        url: String,
        api_key: Option<String>,
        session: Option<String>,
        protocol: Option<String>,
    },
    // Процесс и его потоки — в куче: вариант иначе в разы крупнее Http.
    Stdio {
        _child: Box<Child>,
        stdin: Box<ChildStdin>,
        lines: Box<Lines<BufReader<ChildStdout>>>,
    },
}

pub struct McpClient {
    /// Имя сервера из каталога.
    pub name: String,
    transport: Transport,
    next_id: i64,
}

impl McpClient {
    /// Подключиться к серверу каталога и пройти `initialize`. Stdio-сервер
    /// запускается процессом из `launch` (агент даёт `exec -i` в свою
    /// песочницу владения на станции).
    pub async fn connect(
        server: &McpServer,
        launch: impl Fn(&str) -> tokio::process::Command,
    ) -> McpResult<Self> {
        let transport = match server.transport.as_str() {
            "http" => Transport::Http {
                client: reqwest::Client::new(),
                url: server.url.clone(),
                api_key: server.api_key.clone().filter(|k| !k.is_empty()),
                session: None,
                protocol: None,
            },
            "stdio" => {
                let mut child = launch(&server.command)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| format!("не удалось запустить `{}`: {e}", server.command))?;
                let stdin = child.stdin.take().ok_or("нет stdin процесса")?;
                let stdout = child.stdout.take().ok_or("нет stdout процесса")?;
                Transport::Stdio {
                    _child: Box::new(child),
                    stdin: Box::new(stdin),
                    lines: Box::new(BufReader::new(stdout).lines()),
                }
            }
            other => return Err(format!("неизвестный транспорт `{other}`")),
        };
        let mut client = Self {
            name: server.name.clone(),
            transport,
            next_id: 0,
        };
        tokio::time::timeout(
            Duration::from_secs(CONNECT_TIMEOUT_SECS),
            client.initialize(),
        )
        .await
        .map_err(|_| "таймаут подключения".to_string())??;
        Ok(client)
    }

    async fn initialize(&mut self) -> McpResult<()> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "aga", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .await?;
        if let Transport::Http { protocol, .. } = &mut self.transport {
            *protocol = result["protocolVersion"].as_str().map(str::to_string);
        }
        self.notify("notifications/initialized").await
    }

    /// Все инструменты сервера (с пагинацией `nextCursor`).
    pub async fn list_tools(&mut self) -> McpResult<Vec<McpTool>> {
        let fut = async {
            let mut tools = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let params = match &cursor {
                    Some(c) => json!({"cursor": c}),
                    None => json!({}),
                };
                let result = self.request("tools/list", params).await?;
                let page: Vec<McpTool> = serde_json::from_value(result["tools"].clone())
                    .map_err(|e| format!("кривой список инструментов: {e}"))?;
                tools.extend(page);
                match result["nextCursor"].as_str() {
                    Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                    _ => return Ok(tools),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS), fut)
            .await
            .map_err(|_| "таймаут списка инструментов".to_string())?
    }

    /// Вызвать инструмент: текст результата и флаг ошибки инструмента
    /// (`isError`). Err — сбой протокола/транспорта.
    pub async fn call_tool(&mut self, tool: &str, arguments: Value) -> McpResult<(String, bool)> {
        let result = tokio::time::timeout(
            Duration::from_secs(CALL_TIMEOUT_SECS),
            self.request("tools/call", json!({"name": tool, "arguments": arguments})),
        )
        .await
        .map_err(|_| format!("таймаут вызова ({CALL_TIMEOUT_SECS} с)"))??;
        Ok((
            result_text(&result),
            result["isError"].as_bool().unwrap_or(false),
        ))
    }

    async fn request(&mut self, method: &str, params: Value) -> McpResult<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let response = self
            .exchange(&msg, Some(id))
            .await?
            .ok_or_else(|| format!("нет ответа на `{method}`"))?;
        if let Some(err) = response.get("error").filter(|e| !e.is_null()) {
            let text = err["message"].as_str().unwrap_or("ошибка без текста");
            return Err(format!("`{method}`: {text}"));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn notify(&mut self, method: &str) -> McpResult<()> {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        self.exchange(&msg, None).await.map(|_| ())
    }

    /// Отправить сообщение и, если `id` задан, дождаться ответа с этим id.
    async fn exchange(&mut self, msg: &Value, id: Option<i64>) -> McpResult<Option<Value>> {
        match &mut self.transport {
            Transport::Http {
                client,
                url,
                api_key,
                session,
                protocol,
            } => {
                let mut req = client
                    .post(url.as_str())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .json(msg);
                if let Some(key) = api_key {
                    req = req.header("Authorization", format!("Bearer {key}"));
                }
                if let Some(sid) = session {
                    req = req.header("Mcp-Session-Id", sid.as_str());
                }
                if let Some(v) = protocol {
                    req = req.header("MCP-Protocol-Version", v.as_str());
                }
                let resp = req.send().await.map_err(|e| format!("HTTP: {e}"))?;
                if let Some(sid) = resp
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    *session = Some(sid.to_string());
                }
                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(format!("HTTP {status}: {body}"));
                }
                let Some(id) = id else {
                    return Ok(None);
                };
                let is_sse = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.starts_with("text/event-stream"));
                let body = resp.text().await.map_err(|e| format!("HTTP: {e}"))?;
                let candidates: Vec<Value> = if is_sse {
                    sse_messages(&body)
                } else {
                    match serde_json::from_str::<Value>(&body) {
                        Ok(Value::Array(batch)) => batch,
                        Ok(v) => vec![v],
                        Err(e) => return Err(format!("ответ не JSON: {e}")),
                    }
                };
                candidates
                    .into_iter()
                    .find(|m| is_response_to(m, id))
                    .map(Some)
                    .ok_or_else(|| "в ответе нет результата запроса".to_string())
            }
            Transport::Stdio { stdin, lines, .. } => {
                let mut line = msg.to_string();
                line.push('\n');
                stdin
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|e| format!("stdin: {e}"))?;
                stdin.flush().await.map_err(|e| format!("stdin: {e}"))?;
                let Some(id) = id else {
                    return Ok(None);
                };
                loop {
                    let Some(line) = lines
                        .next_line()
                        .await
                        .map_err(|e| format!("stdout: {e}"))?
                    else {
                        return Err("сервер закрыл соединение".to_string());
                    };
                    // Посторонний вывод (логи в stdout) — пропускаем.
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if is_response_to(&message, id) {
                        return Ok(Some(message));
                    }
                    // Запрос сервера к клиенту (roots, sampling...) — не
                    // поддерживаем, отвечаем ошибкой, чтобы сервер не ждал.
                    if message.get("method").is_some() && message.get("id").is_some() {
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": message["id"],
                            "error": {"code": -32601, "message": "method not supported by aga"}
                        });
                        let mut out = reply.to_string();
                        out.push('\n');
                        let _ = stdin.write_all(out.as_bytes()).await;
                    }
                }
            }
        }
    }
}

fn is_response_to(message: &Value, id: i64) -> bool {
    message.get("id").and_then(Value::as_i64) == Some(id)
        && (message.get("result").is_some() || message.get("error").is_some())
}

/// JSON-сообщения из тела SSE: `data:`-строки события склеиваются.
fn sse_messages(body: &str) -> Vec<Value> {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|d| d.strip_prefix(' ').unwrap_or(d))
                .collect();
            if data.is_empty() {
                return None;
            }
            serde_json::from_str(&data.join("\n")).ok()
        })
        .collect()
}

/// Текст результата `tools/call`: текстовые блоки как есть, прочие — пометкой;
/// без блоков — структурированный результат JSON-ом.
fn result_text(result: &Value) -> String {
    let parts: Vec<String> = result["content"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| match item["type"].as_str().unwrap_or("") {
                    "text" => item["text"].as_str().unwrap_or("").to_string(),
                    "resource" => item["resource"]["text"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format!(
                                "[ресурс {}]",
                                item["resource"]["uri"].as_str().unwrap_or("")
                            )
                        }),
                    "resource_link" => format!("[ссылка {}]", item["uri"].as_str().unwrap_or("")),
                    other => format!("[{other} {}]", item["mimeType"].as_str().unwrap_or("")),
                })
                .collect()
        })
        .unwrap_or_default();
    if parts.is_empty() {
        if let Some(structured) = result.get("structuredContent") {
            return structured.to_string();
        }
    }
    parts.join("\n")
}

/// Имя инструмента для LLM: `<сервер>__<инструмент>`, только `[A-Za-z0-9_-]`,
/// не длиннее 64 символов (ограничение OpenAI-compatible API).
pub fn llm_tool_name(server: &str, tool: &str) -> String {
    format!("{server}__{tool}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Детерминированный stdio MCP-сервер на sh: id запросов клиента идут
    /// подряд (1 — initialize, 2 — tools/list, дальше — вызовы), уведомление
    /// `initialized` id не имеет. На вызов сервер пишет файл `$MCP_OUT`
    /// (по умолчанию `mcp-touched.txt` в cwd) и отвечает текстом. Перед ответом — строка-лог и запрос к клиенту
    /// (их клиент должен пропустить).
    pub(crate) const FAKE_STDIO_SERVER: &str = r#"read l; echo 'log: starting'; echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}'; read l; read l; echo '{"jsonrpc":"2.0","id":"srv-1","method":"roots/list"}'; echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"add","description":"Сложить числа","inputSchema":{"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}}}}]}}'; read l; n=3; while read l; do case "$l" in *srv-1*) continue;; esac; echo touched > "${MCP_OUT:-mcp-touched.txt}"; echo "{\"jsonrpc\":\"2.0\",\"id\":$n,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"5\"}]}}"; n=$((n+1)); done"#;

    /// Локальный запуск stdio-сервера (`sh -c`).
    pub(crate) fn local(command: &str) -> tokio::process::Command {
        crate::agent::exec_command(
            &crate::agent::Executor::Sh,
            command,
            true,
            crate::agent::RunAs::Station,
        )
    }

    pub(crate) fn stdio_server(command: &str) -> McpServer {
        McpServer {
            id: 1,
            name: "fake".into(),
            transport: "stdio".into(),
            url: String::new(),
            command: command.into(),
            api_key: None,
        }
    }

    #[tokio::test]
    async fn stdio_server_lists_and_calls_tools() {
        let dir = std::env::temp_dir().join(format!("aga_mcp_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_str().unwrap().to_string();
        let mut client = McpClient::connect(&stdio_server(FAKE_STDIO_SERVER), |c| {
            local(&format!("cd '{cwd}' && {c}"))
        })
        .await
        .unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "add");
        assert_eq!(tools[0].input_schema["properties"]["a"]["type"], "number");
        let (text, is_error) = client
            .call_tool("add", json!({"a": 2, "b": 3}))
            .await
            .unwrap();
        assert_eq!((text.as_str(), is_error), ("5", false));
        // Процесс работал в cwd из обёртки.
        assert!(dir.join("mcp-touched.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stdio_server_that_exits_reports_error() {
        let err = McpClient::connect(&stdio_server("true"), local)
            .await
            .err()
            .unwrap();
        // Смотря когда процесс умер: EOF на чтении или broken pipe на записи.
        assert!(
            err.contains("закрыл соединение") || err.contains("stdin"),
            "{err}"
        );
    }

    pub(crate) type Seen =
        std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>, Option<String>)>>>;

    /// Streamable HTTP MCP-сервер: initialize выдаёт сессию; tools/list
    /// отвечает JSON, tools/call — SSE-потоком (с уведомлением перед ответом).
    /// Записывает (метод, Mcp-Session-Id, Authorization).
    pub(crate) async fn http_server() -> (String, tokio::task::JoinHandle<()>, Seen) {
        use axum::response::IntoResponse;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Seen = Default::default();
        let seen2 = seen.clone();
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                    let seen = seen2.clone();
                    async move {
                        let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(String::from);
                        let method = body["method"].as_str().unwrap_or("").to_string();
                        seen.lock().unwrap().push((method.clone(), h("mcp-session-id"), h("authorization")));
                        let id = body["id"].clone();
                        match method.as_str() {
                            "initialize" => (
                                [("mcp-session-id", "sess-1")],
                                axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": {
                                    "protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                                    "serverInfo": {"name": "h", "version": "1"}}})),
                            )
                                .into_response(),
                            "notifications/initialized" => axum::http::StatusCode::ACCEPTED.into_response(),
                            "tools/list" => axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": {
                                "tools": [{"name": "echo", "inputSchema": {"type": "object"}}]}}))
                                .into_response(),
                            _ => {
                                let note = json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {}});
                                let resp = json!({"jsonrpc": "2.0", "id": id, "result": {
                                    "content": [{"type": "text", "text": body["params"]["arguments"]["text"]}],
                                    "isError": true}});
                                (
                                    [("content-type", "text/event-stream")],
                                    format!("event: message\ndata: {note}\n\nevent: message\ndata: {resp}\n\n"),
                                )
                                    .into_response()
                            }
                        }
                    }
                },
            ),
        );
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/mcp"), handle, seen)
    }

    #[tokio::test]
    async fn http_server_keeps_session_and_reads_sse_result() {
        let (url, server, seen) = http_server().await;
        let cfg = McpServer {
            id: 1,
            name: "h".into(),
            transport: "http".into(),
            url,
            command: String::new(),
            api_key: Some("k".into()),
        };
        let mut client = McpClient::connect(&cfg, local).await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "");
        let (text, is_error) = client
            .call_tool("echo", json!({"text": "boom"}))
            .await
            .unwrap();
        assert_eq!((text.as_str(), is_error), ("boom", true));
        let seen = seen.lock().unwrap().clone();
        let methods: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "initialize",
                "notifications/initialized",
                "tools/list",
                "tools/call"
            ]
        );
        // Сессия из ответа на initialize идёт во все следующие запросы, ключ — всегда.
        assert_eq!(seen[0].1, None);
        assert!(seen[1..].iter().all(|s| s.1.as_deref() == Some("sess-1")));
        assert!(seen.iter().all(|s| s.2.as_deref() == Some("Bearer k")));
        server.abort();
    }

    #[test]
    fn tool_result_text_covers_content_kinds() {
        let r = json!({"content": [
            {"type": "text", "text": "a"},
            {"type": "image", "data": "...", "mimeType": "image/png"},
            {"type": "resource", "resource": {"uri": "file:///x", "text": "body"}},
            {"type": "resource_link", "uri": "file:///y"}
        ]});
        assert_eq!(
            result_text(&r),
            "a\n[image image/png]\nbody\n[ссылка file:///y]"
        );
        assert_eq!(
            result_text(&json!({"structuredContent": {"n": 1}})),
            r#"{"n":1}"#
        );
    }

    #[test]
    fn llm_tool_names_are_api_safe() {
        assert_eq!(llm_tool_name("git hub", "list.prs"), "git_hub__list_prs");
        assert_eq!(llm_tool_name(&"x".repeat(70), "t").len(), 64);
    }
}
