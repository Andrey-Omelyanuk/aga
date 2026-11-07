//! Типы хранилища: задачи и трасса, human-запросы, проекты, наборы агентов,
//! каталог способностей, подключения к LLM и MCP-серверы. Поведение
//! (`impl TraceStore`) — в соседних файлах модуля, по доменам.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TraceEntry {
    pub id: String,
    pub task_id: String,
    pub step: i32,
    pub entry_type: String,
    pub content: String,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskTrace {
    pub task_id: String,
    pub role: String,
    pub status: String,
    /// Токены LLM за задачу (сумма `usage` ответов; 0 — сервер их не отдал).
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub entries: Vec<TraceEntry>,
}

/// Pending-запрос человека, привязанный к сообщению-вопросу в чате: ответ —
/// «ответ на это сообщение», продолжение выполняет агент `agent_name` от
/// своего имени в чате `chat_id`.
#[derive(Debug, Clone, Serialize)]
pub struct HumanRequest {
    pub id: String,
    pub task_id: String,
    pub question: String,
    pub agent_name: String,
    pub chat_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub git_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Скилл из каталога, данный агенту набора: ссылка
/// по имени на запись целиком. Версий нет — агент всегда берёт её последнее
/// содержимое.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub name: String,
}

/// Агент из AgentSet-а. Способности — скиллы из общего каталога
/// (имя записи; агент всегда использует последнее содержимое); инструменты —
/// отдельный список исполняемого в консоли воркстейшна, без версий. LLM —
/// выбранное подключение (llm_id); своей модели и температуры у агента нет.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: u32,
    /// Подключение к LLM (см. llm_connections). Нет — дефолтная LLM из env.
    pub llm_id: Option<i64>,
    /// Указание на родителя в дереве набора: агент наследует под-уровень папки.
    pub parent_id: Option<i64>,
    /// Данные агенту скиллы (имя из каталога).
    pub skills: Vec<AgentCapability>,
    /// Территория по узлу в дереве набора: папка узла минус папки наследников.
    pub territory: crate::scope::Territory,
    /// Привязка к пользователю чата: агент слушает его сообщения и отвечает от
    /// его имени. Нет — агент ни на что не реагирует (настройка на странице
    /// агента; использует `runtime.rs`).
    pub listen_user_id: Option<i64>,
    /// MCP-серверы каталога, данные агенту (имена).
    pub mcp: Vec<String>,
}

/// Набор агентов с их деревом. Прикрепляется к одному или нескольким проектам.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSet {
    pub id: i64,
    pub name: String,
    pub agents: Vec<AgentDef>,
}

/// Спек агента при создании/обновлении набора: parent — имя родителя в дереве.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: u32,
    /// Подключение к LLM (llm_connections). Нет — дефолтная LLM из env.
    pub llm_id: Option<i64>,
    pub parent: Option<String>,
    pub skills: Vec<AgentCapability>,
    /// Привязка к пользователю чата (слушать и отвечать от его имени).
    #[serde(default)]
    pub listen_user_id: Option<i64>,
    /// MCP-серверы каталога по имени; неизвестные имена пропускаются.
    #[serde(default)]
    pub mcp: Vec<String>,
}

/// Подключение к LLM: название, url API, ключ доступа и модель. Агент набора
/// ссылается на подключение (llm_id); одно подключение отмечается дефолтным —
/// к нему ходят агенты без своего подключения. Имя поля model_name в API —
/// «model» зарезервировано базовым классом mobx-модели веб-клиента.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConnection {
    pub id: i64,
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
    pub is_default: bool,
    /// Нативный function calling (`tools` в API). false — запасной текстовый
    /// режим: команды в блоках ```bash, вопрос — маркер `[ASK_HUMAN]`.
    pub native_tools: bool,
    /// Предел токенов ответа (`max_tokens`); None — дефолт клиента.
    pub max_tokens: Option<i64>,
    /// Температура; None — дефолт клиента.
    pub temperature: Option<f64>,
}

/// Спек подключения при создании/обновлении: название, url API, ключ, модель.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConnectionSpec {
    pub name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
    #[serde(default = "default_native_tools")]
    pub native_tools: bool,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

pub(crate) fn default_native_tools() -> bool {
    true
}

/// MCP-сервер из каталога: агенты набора получают его инструменты по имени
/// (`agent_mcp_servers`). Транспорт `http` — Streamable HTTP по `url` (ключ —
/// `Authorization: Bearer`); `stdio` — процесс `command`, запускаемый в
/// воркстейшне агента (cwd — папка агента).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub transport: String,
    pub url: String,
    pub command: String,
    pub api_key: Option<String>,
}

/// Спек MCP-сервера при создании/обновлении.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSpec {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl McpServerSpec {
    /// Ошибка заполнения: имя непустое, транспорт `http` (нужен url) или
    /// `stdio` (нужна команда).
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("имя MCP-сервера пустое".into());
        }
        match self.transport.as_str() {
            "http" if self.url.trim().is_empty() => Err("у http-сервера нет url".into()),
            "stdio" if self.command.trim().is_empty() => Err("у stdio-сервера нет команды".into()),
            "http" | "stdio" => Ok(()),
            other => Err(format!("неизвестный транспорт `{other}` (http или stdio)")),
        }
    }
}

/// Вид записи каталога: скилл или сокращение. Скиллы даются агентам;
/// сокращения агентам не даются — это текст, который вставляется
/// в скрытую часть сообщения по слову `/имя`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityKind {
    Skill,
    Shortcut,
}

impl CapabilityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityKind::Skill => "skill",
            CapabilityKind::Shortcut => "shortcut",
        }
    }
}

/// Действие в истории изменений записи каталога.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityAction {
    Create,
    Update,
    Rename,
    Delete,
}

impl CapabilityAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityAction::Create => "create",
            CapabilityAction::Update => "update",
            CapabilityAction::Rename => "rename",
            CapabilityAction::Delete => "delete",
        }
    }
}

/// Одна запись истории изменения записи каталога: кто, когда и что сделал.
/// `content` — содержимое записи после действия: по соседним записям строится
/// дифф на странице истории.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityHistoryEntry {
    pub id: i64,
    pub action: CapabilityAction,
    pub actor_id: i64,
    pub actor_name: String,
    pub created_at: DateTime<Utc>,
    pub detail: Option<String>,
    pub content: String,
}

/// Запись каталога способностей с единственным текущим содержимым.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityItem {
    pub id: i64,
    pub kind: CapabilityKind,
    pub name: String,
    pub content: String,
    /// Удалена ли запись (мягкое удаление): остаётся в списке «Удалённые».
    pub deleted: bool,
}
