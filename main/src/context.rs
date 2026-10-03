//! Контекст агента из чата: сообщения чата → диалог для LLM.
//!
//! Агент — участник чата наравне с остальными и не знает, кто стоит за
//! другими участниками (человек или агент): все чужие сообщения — `user` с
//! именем автора, без скрытой части. Свои сообщения (автор — пользователь,
//! которого обслуживает агент) — `assistant`, а свои шаги работы
//! (`kind='step'`) восстанавливаются вызовами инструментов с их выводом (вывод
//! — из скрытой части шага; у старых шагов — только вызов). Сообщение, на
//! которое агент отвечает, — последнее и помечено. В нити впереди — исходное
//! сообщение из родительского чата и заголовок нити. Объём ограничен бюджетом
//! символов: берутся самые свежие сообщения, остальное отрезается пометкой.

use std::collections::HashMap;

use crate::chat::{ChatStore, Message as ChatMessage, KIND_STEP};
use crate::llm::{FunctionCall, Message, ToolCall};

/// Бюджет контекста в символах (без системного промпта).
pub const BUDGET_CHARS: usize = 24_000;
/// Сколько последних своих шагов несут вывод; у более старых — только вызов.
pub const STEPS_WITH_OUTPUT: usize = 5;
/// Предел длины исходного сообщения нити в шапке.
const SOURCE_CHARS: usize = 2_000;

/// Собрать контекст: сообщения чата `chat_id` до `trigger_id` включительно
/// глазами участника `self_user`. `native` — шаги восстанавливаются нативными
/// вызовами инструментов, иначе — блоками ```bash и выводом текстом.
pub async fn build_context(
    store: &ChatStore,
    chat_id: i64,
    self_user: i64,
    trigger_id: i64,
    native: bool,
) -> Result<Vec<Message>, sqlx::Error> {
    let chat = store
        .get_chat(chat_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let names: HashMap<i64, String> = store
        .list_users()
        .await?
        .into_iter()
        .map(|u| (u.id, u.name))
        .collect();
    let name = |id: i64| names.get(&id).cloned().unwrap_or_else(|| "unknown".into());
    let messages: Vec<ChatMessage> = store
        .list_messages(chat_id)
        .await?
        .into_iter()
        .filter(|m| m.id <= trigger_id)
        .collect();

    // От свежих к старым, пока помещается в бюджет; сообщение-триггер — всегда.
    let mut units: Vec<Vec<Message>> = Vec::new();
    let mut used = 0;
    let mut cut = false;
    let mut own_steps = 0;
    for m in messages.iter().rev() {
        let unit = if m.author_id == self_user {
            own_unit(m, native, &mut own_steps)
        } else {
            let mark = if m.id == trigger_id {
                " → тебе"
            } else {
                ""
            };
            vec![Message::user(&format!(
                "{}{mark}: {}",
                name(m.author_id),
                visible_text(m)
            ))]
        };
        let size: usize = unit.iter().map(message_size).sum();
        if used + size > BUDGET_CHARS && !units.is_empty() {
            cut = true;
            break;
        }
        used += size;
        units.push(unit);
    }
    units.reverse();

    let mut out = Vec::new();
    if let Some(source_id) = chat.start_message_id {
        let title = messages
            .first()
            .and_then(|m| m.title.clone())
            .unwrap_or_default();
        let source = match store.get_message(source_id).await? {
            Some(src) => {
                let text: String = visible_text(&src).chars().take(SOURCE_CHARS).collect();
                format!("{}: {text}", name(src.author_id))
            }
            None => "(сообщение недоступно)".to_string(),
        };
        out.push(Message::user(&format!(
            "Это нить «{title}». Она начата от сообщения в родительском чате — {source}"
        )));
    }
    if cut {
        out.push(Message::user("(более ранние сообщения чата опущены)"));
    }
    out.extend(units.into_iter().flatten());
    Ok(out)
}

/// Видимый текст сообщения: заголовок (у первого сообщения нити) и тело.
fn visible_text(m: &ChatMessage) -> String {
    match m.title.as_deref().filter(|t| !t.is_empty()) {
        Some(title) => format!("«{title}»\n{}", m.body),
        None => m.body.clone(),
    }
}

/// Своё сообщение: шаг — вызов инструмента и его вывод, иначе — ответ ассистента.
/// `own_steps` — сколько своих шагов уже встречено (от свежих к старым).
fn own_unit(m: &ChatMessage, native: bool, own_steps: &mut usize) -> Vec<Message> {
    let call = (m.kind == KIND_STEP)
        .then_some(m.tool_call.as_deref())
        .flatten()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
    let Some(call) = call else {
        return vec![Message::assistant(&visible_text(m))];
    };
    *own_steps += 1;
    let output = if *own_steps <= STEPS_WITH_OUTPUT {
        m.hidden.clone()
    } else {
        "(вывод опущен)".to_string()
    };
    let tool = call["name"].as_str().unwrap_or("shell").to_string();
    if native {
        let id = format!("h{}", m.id);
        vec![
            Message {
                tool_calls: vec![ToolCall {
                    id: id.clone(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: tool,
                        arguments: call["arguments"].to_string(),
                    },
                }],
                ..Message::assistant("")
            },
            Message::tool(&id, &output),
        ]
    } else {
        let request = if tool == "shell" {
            format!("```bash\n{}\n```", m.body)
        } else {
            m.body.clone()
        };
        vec![
            Message::assistant(&request),
            Message::user(&format!("$ {}\n{output}", m.body)),
        ]
    }
}

fn message_size(m: &Message) -> usize {
    m.content.chars().count()
        + m.tool_calls
            .iter()
            .map(|c| c.function.arguments.chars().count())
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn stores() -> (ChatStore, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("aga_ctx_test_{}.db", uuid::Uuid::new_v4()));
        let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
            .await
            .unwrap();
        let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
        (store, path)
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(path);
    }

    fn roles(ctx: &[Message]) -> Vec<&str> {
        ctx.iter().map(|m| m.role.as_str()).collect()
    }

    #[tokio::test]
    async fn own_messages_are_assistant_others_are_named_users() {
        let (store, path) = stores().await;
        let alice = store
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let bob = store
            .insert_user("bob", "human", false, None, None)
            .await
            .unwrap();
        let carol = store
            .insert_user("carol", "human", false, None, None)
            .await
            .unwrap();
        let chat = store.create_chat(None, Some("s"), alice).await.unwrap();
        let send = |author: i64, body: &'static str| {
            let store = store.clone();
            async move {
                store
                    .send_message(chat.id, author, body, "заметка", None, None)
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        send(alice, "@bob сделай отчёт").await;
        send(bob, "Сейчас").await;
        send(carol, "Я тоже посмотрю").await;
        let trigger = send(alice, "@bob готово?").await;
        send(alice, "после триггера — не входит").await;

        let ctx = build_context(&store, chat.id, bob, trigger.id, true)
            .await
            .unwrap();
        // Свои — assistant, все остальные (кто бы ни стоял за ними) — user с именем.
        assert_eq!(roles(&ctx), vec!["user", "assistant", "user", "user"]);
        assert_eq!(ctx[0].content, "alice: @bob сделай отчёт");
        assert_eq!(ctx[1].content, "Сейчас");
        assert_eq!(ctx[2].content, "carol: Я тоже посмотрю");
        // Сообщение-триггер — последнее и помечено; скрытая часть не входит.
        assert_eq!(ctx[3].content, "alice → тебе: @bob готово?");
        assert!(ctx.iter().all(|m| !m.content.contains("заметка")));
        cleanup(&path);
    }

    #[tokio::test]
    async fn own_steps_become_tool_calls_old_ones_without_output() {
        let (store, path) = stores().await;
        let alice = store
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let bot = store
            .insert_user("bot", "human", false, None, None)
            .await
            .unwrap();
        let chat = store.create_chat(None, Some("s"), alice).await.unwrap();
        for i in 0..(STEPS_WITH_OUTPUT + 1) {
            let call =
                serde_json::json!({"name": "shell", "arguments": {"command": format!("ls {i}")}});
            store
                .send_step(
                    chat.id,
                    bot,
                    &format!("ls {i}"),
                    &format!("out {i}"),
                    &call.to_string(),
                    "agent",
                )
                .await
                .unwrap();
        }
        // Чужой шаг — просто сообщение участника, без вывода.
        let foreign = serde_json::json!({"name": "shell", "arguments": {"command": "pwd"}});
        store
            .send_step(
                chat.id,
                alice,
                "pwd",
                "/secret",
                &foreign.to_string(),
                "agent",
            )
            .await
            .unwrap();
        let trigger = store
            .send_message(chat.id, alice, "@bot дальше", "", None, None)
            .await
            .unwrap()
            .unwrap();

        let ctx = build_context(&store, chat.id, bot, trigger.id, true)
            .await
            .unwrap();
        let steps = STEPS_WITH_OUTPUT + 1;
        assert_eq!(ctx.len(), steps * 2 + 2);
        let first_call = &ctx[0].tool_calls[0];
        assert_eq!(first_call.function.name, "shell");
        assert_eq!(first_call.function.arguments, r#"{"command":"ls 0"}"#);
        assert_eq!(ctx[1].tool_call_id.as_deref(), Some(first_call.id.as_str()));
        // Самый старый шаг — без вывода, свежие — с выводом.
        assert_eq!(ctx[1].content, "(вывод опущен)");
        assert_eq!(ctx[steps * 2 - 1].content, format!("out {}", steps - 1));
        assert_eq!(ctx[steps * 2].content, "alice: pwd");
        assert!(!ctx[steps * 2].content.contains("/secret"));

        // Текстовый режим: шаг — блок ```bash и вывод user-сообщением.
        let ctx = build_context(&store, chat.id, bot, trigger.id, false)
            .await
            .unwrap();
        assert_eq!(ctx[0].content, "```bash\nls 0\n```");
        assert_eq!(ctx[1].role, "user");
        assert_eq!(ctx[1].content, "$ ls 0\n(вывод опущен)");
        cleanup(&path);
    }

    #[tokio::test]
    async fn thread_context_starts_with_source_message_and_title() {
        let (store, path) = stores().await;
        let alice = store
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let bot = store
            .insert_user("bot", "human", false, None, None)
            .await
            .unwrap();
        let chat = store.create_chat(None, Some("s"), alice).await.unwrap();
        let source = store
            .send_message(chat.id, alice, "Нужен новый API", "", None, None)
            .await
            .unwrap()
            .unwrap();
        let (thread, first) = store
            .start_thread(
                chat.id,
                source.id,
                "API заказов",
                "@alice какие поля?",
                "",
                bot,
            )
            .await
            .unwrap()
            .unwrap();
        let trigger = store
            .send_message(thread.id, alice, "id и сумма", "", None, None)
            .await
            .unwrap()
            .unwrap();
        let ctx = build_context(&store, thread.id, bot, trigger.id, true)
            .await
            .unwrap();
        assert_eq!(
            ctx[0].content,
            "Это нить «API заказов». Она начата от сообщения в родительском чате — alice: Нужен новый API"
        );
        assert_eq!(ctx[1].role, "assistant");
        assert_eq!(ctx[1].content, "«API заказов»\n@alice какие поля?");
        assert_eq!(ctx[2].content, "alice → тебе: id и сумма");
        let _ = first;
        cleanup(&path);
    }

    #[tokio::test]
    async fn budget_keeps_newest_messages_and_marks_cut() {
        let (store, path) = stores().await;
        let alice = store
            .insert_user("alice", "human", false, None, None)
            .await
            .unwrap();
        let bot = store
            .insert_user("bot", "human", false, None, None)
            .await
            .unwrap();
        let chat = store.create_chat(None, Some("s"), alice).await.unwrap();
        let big = "x".repeat(BUDGET_CHARS / 3);
        for i in 0..5 {
            store
                .send_message(chat.id, alice, &format!("{i}{big}"), "", None, None)
                .await
                .unwrap();
        }
        let trigger = store
            .send_message(chat.id, alice, "@bot итог?", "", None, None)
            .await
            .unwrap()
            .unwrap();
        let ctx = build_context(&store, chat.id, bot, trigger.id, true)
            .await
            .unwrap();
        assert_eq!(ctx[0].content, "(более ранние сообщения чата опущены)");
        assert!(ctx.iter().map(message_size).sum::<usize>() <= BUDGET_CHARS + 100);
        // Самые свежие — на месте, старые отрезаны.
        assert!(ctx[ctx.len() - 2].content.starts_with("alice: 4"));
        assert!(!ctx.iter().any(|m| m.content.starts_with("alice: 0")));
        assert_eq!(ctx.last().unwrap().content, "alice → тебе: @bot итог?");
        cleanup(&path);
    }
}
