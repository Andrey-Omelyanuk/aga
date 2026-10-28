//! Типы модели чата: пользователи, чаты, сообщения, артефакты, воркстейшны,
//! команды чата и ошибки сессий. Поведение (`impl ChatStore`) — в соседних
//! файлах модуля, по доменам.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Глубина дерева чатов (вложенность нитей) по умолчанию; env
/// `AGA_MAX_THREAD_DEPTH`.
pub const DEFAULT_MAX_CHAT_LEVEL: i32 = 10;
/// Сообщений в одной нити по умолчанию (корневые чаты не ограничены); env
/// `AGA_MAX_THREAD_MESSAGES`. Вместе с глубиной — предохранитель от
/// бесконечных переписок участников в нитях.
pub const DEFAULT_MAX_THREAD_MESSAGES: i64 = 100;

/// Положительное число из env-значения, иначе — дефолт (нет, пусто, мусор, ≤0).
pub(super) fn limit_from(value: Option<&str>, default: i64) -> i64 {
    value
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// Предел вложенности нитей (`AGA_MAX_THREAD_DEPTH`, читается один раз).
pub fn max_chat_level() -> i32 {
    static LEVEL: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *LEVEL.get_or_init(|| {
        let v = std::env::var("AGA_MAX_THREAD_DEPTH").ok();
        limit_from(v.as_deref(), DEFAULT_MAX_CHAT_LEVEL as i64).min(i32::MAX as i64) as i32
    })
}

/// Предел сообщений в нити (`AGA_MAX_THREAD_MESSAGES`, читается один раз).
pub fn max_thread_messages() -> i64 {
    static MESSAGES: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *MESSAGES.get_or_init(|| {
        let v = std::env::var("AGA_MAX_THREAD_MESSAGES").ok();
        limit_from(v.as_deref(), DEFAULT_MAX_THREAD_MESSAGES)
    })
}

/// Вид сообщения: обычный текст или шаг работы (вызов инструмента: тело —
/// команда, скрытая часть — вывод, `tool_call` — вызов JSON-ом).
pub const KIND_TEXT: &str = "text";
pub const KIND_STEP: &str = "step";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUser {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub is_super_user: bool,
    pub sso_subject: Option<String>,
    pub role: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub level: i32,
    pub title: Option<String>,
    /// Сообщение, от которого началась нить (дочерний чат). У корневых чатов
    /// (сессий, общих чатов) — None; заголовок нити живёт не здесь, а на её
    /// первом сообщении.
    pub start_message_id: Option<i64>,
    pub created_by_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: String,
    /// Производные от сессии поля (в БД живут в `sessions`): воркстейшн и
    /// проект корневого чата-сессии. У общих чатов и нитей — None/0.
    pub workstation_id: Option<i64>,
    pub project_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ChatParticipant {
    pub chat_id: i64,
    pub chat_user_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub author_id: i64,
    pub shared_by_id: Option<i64>,
    pub share_of_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub last_message_id: Option<i64>,
    /// Заголовок нити: задаётся на первом сообщении нити при её начале.
    /// Обычные сообщения заголовка не несут.
    pub title: Option<String>,
    /// Сообщение в родительском чате, отправленное из нити в родителя: копия
    /// сообщения нити со ссылкой на сообщение, от которого нить началась.
    pub thread_of_id: Option<i64>,
    pub body: String,
    /// Скрытая часть сообщения — заметка, которую не видно в ленте, пока её не
    /// развернут. Заполняется не вводом, а сокращениями: при отправке каждое
    /// слово `/имя` добавляет в `hidden` текст, привязанный к имени. В контекст
    /// агентов (LLM) не уходит.
    pub hidden: String,
    /// Источник сообщения: 'user' — набрано человеком, иначе — кем сообщение
    /// записано (ответ агент-процесса пишет 'agent'). Чат это поле не
    /// интерпретирует — оно нейтральное; различает источники подписчик
    /// (`runtime.rs`), чтобы не реагировать на записи, сделанные не человеком.
    pub origin: String,
    /// Вид сообщения: `text` или `step` (шаг работы с инструментом).
    pub kind: String,
    /// Вызов инструмента шага JSON-ом (`{"name", "arguments"}`); у текста — None.
    pub tool_call: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: i64,
    pub message_id: i64,
    pub kind: String,
    pub title: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstation {
    pub id: i64,
    pub name: String,
    /// Производное от активной сессии (в БД проект живёт в `sessions`):
    /// 0 — станция свободна. Станция обслуживает сессии, своего проекта она
    /// не хранит.
    pub project_id: i64,
    pub state: String,
    /// Имя k8s-Secret, который при подъёме монтируется в ws (секреты для
    /// сторонних CLI). Живёт в кластере, в БД храним только имя.
    pub secret: Option<String>,
    /// Активная сессия воркстейшна (корневой чат), если она открыта.
    pub current_session_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

/// Прерванная (незакрытая) сессия на упавшем воркстейшне — кандидат на
/// восстановление при открытии сессии на свободном ws того же проекта.
#[derive(Debug, Clone)]
pub struct InterruptedSession {
    pub session_id: i64,
}

/// Команда, распознанная в теле сообщения. Команды — это обычные сообщения,
/// на которые есть дополнительная реакция.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatCommand {
    Invite(String),
    Kick(String),
    End,
}

/// Ошибки открытия/закрытия сессии воркстейшна.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("воркстейшн не найден")]
    NotFound,
    #[error("воркстейшн не готов")]
    WorkstationNotReady,
    #[error("на воркстейшне уже открыта сессия")]
    WorkstationBusy,
    #[error("закрыть сессию может только её владелец")]
    Forbidden,
    #[error("база данных: {0}")]
    Db(#[from] sqlx::Error),
}
