use chrono::{DateTime, Utc};
use sqlx::Row;

use super::*;

fn parse_dt(v: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(v)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
pub(crate) fn user_from_row(r: &sqlx::sqlite::SqliteRow) -> ChatUser {
    ChatUser {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        is_super_user: r.get::<i32, _>("is_super_user") != 0,
        sso_subject: r.get("sso_subject"),
        role: r.get("role"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
    }
}

pub(crate) fn chat_from_row(r: &sqlx::sqlite::SqliteRow) -> Chat {
    Chat {
        id: r.get("id"),
        root_id: r.get("root_id"),
        parent_id: r.get("parent_id"),
        level: r.get("level"),
        title: r.get("title"),
        start_message_id: r.get("start_message_id"),
        created_by_id: r.get("created_by_id"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
        updated_at: parse_dt(&r.get::<String, _>("updated_at")),
        state: r.get("state"),
        workstation_id: r.get("workstation_id"),
        project_id: r.get("project_id"),
    }
}

pub(crate) fn workstation_from_row(r: &sqlx::sqlite::SqliteRow) -> Workstation {
    Workstation {
        id: r.get("id"),
        name: r.get("name"),
        project_id: r.get("project_id"),
        state: r.get("state"),
        secret: r.get("secret"),
        current_session_id: r.get("current_session_id"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
    }
}

pub(crate) fn message_from_row(r: &sqlx::sqlite::SqliteRow) -> Message {
    Message {
        id: r.get("id"),
        chat_id: r.get("chat_id"),
        parent_id: r.get("parent_id"),
        author_id: r.get("author_id"),
        shared_by_id: r.get("shared_by_id"),
        share_of_id: r.get("share_of_id"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
        last_message_id: r.get("last_message_id"),
        title: r.get("title"),
        thread_of_id: r.get("thread_of_id"),
        body: r.get("body"),
        hidden: r.get("hidden"),
        origin: r.get("origin"),
        kind: r.get("kind"),
        tool_call: r.get("tool_call"),
    }
}

pub(crate) fn artifact_from_row(r: &sqlx::sqlite::SqliteRow) -> Artifact {
    Artifact {
        id: r.get("id"),
        message_id: r.get("message_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        content: r.get("content"),
        created_at: parse_dt(&r.get::<String, _>("created_at")),
    }
}
