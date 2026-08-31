use crate::chat::ChatStore;
use crate::trace::{AgentCapability, AgentSpec, CapabilityKind, CapabilityVersion, TraceStore};

/// Тестовый набор: очищает БД и восстанавливает детерминированную фикстуру
/// (фиксированные ID) для демо и отладки. Запуск — `aga seed` (см. main.rs),
/// из make — `make dev-seed` / `make k8s-seed`.
pub async fn seed(db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let trace = TraceStore::new(db_path).await?;
    let chat = ChatStore::new(db_path).await?;

    trace.clear_all().await?;
    chat.clear_all().await?;

    // --- Юзеры: anonymous создаёт ChatStore, участники и агент — фикстура.
    // Участники alice/bob — те же учётки, что в Keycloak (см.
    // infra/k8s/core/keycloak-realm.json): фиксированные sso_subject = id из
    // realm, пароли alice-pass / bob-pass. Вход через SSO находит этих юзеров,
    // поэтому сессии/чаты сида принадлежат реальным учёткам.
    let anonymous = chat
        .insert_user("anonymous", "anonymous", true, None, None)
        .await?;
    let alice = chat
        .insert_user(
            "alice",
            "human",
            false,
            Some("1a1a1a1a-1a1a-4a1a-8a1a-1a1a1a1a1a1a"),
            Some("participant"),
        )
        .await?;
    let bob = chat
        .insert_user(
            "bob",
            "human",
            true, // admin в Keycloak (realmRoles: admin + participant)
            Some("2b2b2b2b-2b2b-4b2b-8b2b-2b2b2b2b2b2b"),
            Some("participant"),
        )
        .await?;
    let bot = chat
        .insert_user("Agent.Bot", "agent", false, None, Some("dev"))
        .await?;
    let _ = anonymous;

    // --- Каталог способностей: скиллы и команды с версиями.
    let review = trace
        .create_capability(
            CapabilityKind::Skill,
            "code-review",
            &[
                CapabilityVersion {
                    version: "v1".into(),
                    content: "Ревью по чеклисту: архитектура, безопасность, тесты.".into(),
                },
                CapabilityVersion {
                    version: "v2".into(),
                    content: "Ревью по чеклисту: архитектура, безопасность, тесты, перфора".into(),
                },
            ],
        )
        .await?;
    let _ = review;
    trace
        .create_capability(
            CapabilityKind::Skill,
            "git-workflow",
            &[CapabilityVersion {
                version: "v1".into(),
                content: "Ветки feature/*, коммиты по conventional-commits.".into(),
            }],
        )
        .await?;
    trace
        .create_capability(
            CapabilityKind::Command,
            "run-tests",
            &[CapabilityVersion {
                version: "v1".into(),
                content: "Запуск юнит-тестов и линтера пакета.".into(),
            }],
        )
        .await?;
    trace
        .create_capability(
            CapabilityKind::Command,
            "deploy",
            &[CapabilityVersion {
                version: "v1".into(),
                content: "Деплой сервиса в staging-окружение.".into(),
            }],
        )
        .await?;

    // --- Набор агентов: дерево backend → api.
    let set_id = trace
        .create_agent_set(
            "dev-team",
            &[
                AgentSpec {
                    name: "backend".into(),
                    description: "Бэкенд-разработчик: API, БД, интеграции.".into(),
                    tools: vec!["cat".into(), "ls".into(), "grep".into(), "find".into()],
                    max_iterations: 5,
                    model: None,
                    temperature: 0.7,
                    parent: None,
                    skills: vec![
                        AgentCapability {
                            name: "code-review".into(),
                            pinned_version: None,
                        },
                        AgentCapability {
                            name: "git-workflow".into(),
                            pinned_version: None,
                        },
                    ],
                    commands: vec![AgentCapability {
                        name: "run-tests".into(),
                        pinned_version: None,
                    }],
                },
                AgentSpec {
                    name: "api".into(),
                    description: "Агент API-слоя: эндпоинты и контракты.".into(),
                    tools: vec!["cat".into(), "grep".into()],
                    max_iterations: 3,
                    model: None,
                    temperature: 0.7,
                    parent: Some("backend".into()),
                    skills: Vec::new(),
                    commands: vec![AgentCapability {
                        name: "deploy".into(),
                        pinned_version: None,
                    }],
                },
            ],
        )
        .await?;

    // --- Проекты (git-URL) + привязка набора.
    let p1 = trace
        .upsert_project("https://example.com/backend.git")
        .await?;
    let p2 = trace
        .upsert_project("https://example.com/mobile.git")
        .await?;
    trace.attach_agent_set(p1, set_id).await?;

    // --- Воркстейшны: готовые станции (контейнеры ws-1/ws-2 dev-стенда).
    let ws1 = chat.create_workstation(p1, "ws-1", None).await?;
    let ws2 = chat.create_workstation(p2, "ws-2", None).await?;
    chat.set_workstation_state(ws1.id, "ready").await?;
    chat.set_workstation_state(ws2.id, "ready").await?;

    // --- Сессия на ws-1: корневой чат воркстейшна.
    let session = chat
        .open_workstation_session(ws1.id, Some("Сессия: backend"), alice)
        .await?;
    let task_msg = chat
        .send_message(
            session.id,
            alice,
            "Проверь пул-реквест #42 в ветке feature/auth",
            None,
            None,
        )
        .await?
        .ok_or("message failed")?;
    let review_msg = chat
        .send_message(
            session.id,
            bot,
            "Ревью сделал: конфликт в auth.rs, тесты проваливаются — см. артефакт.",
            Some(task_msg.id),
            None,
        )
        .await?
        .ok_or("message failed")?;
    chat.add_artifact(
        review_msg.id,
        "review",
        Some("review.md"),
        "# Ревью PR #42\n- конфликт в auth.rs\n- тесты: 2 падают",
    )
    .await?;

    // Тред в сессии.
    let thread = chat
        .create_chat(Some(session.id), Some("Обсуждение ревью"), alice, None)
        .await?;
    chat.send_message(thread.id, alice, "Что именно падает в тестах?", None, None)
        .await?;
    chat.send_message(
        thread.id,
        bob,
        "Гоняю run-tests, приложу вывод.",
        None,
        None,
    )
    .await?;

    chat.send_message(session.id, bob, "Отправил фикс в ветку.", None, None)
        .await?;

    // --- Общий чат (без воркстейшна) с зашаренным сообщением.
    let general = chat.create_chat(None, Some("Общий чат"), bob, None).await?;
    chat.add_participant(general.id, alice).await?;
    chat.send_message(
        general.id,
        bob,
        "Привет! Кто возьмёт мобильный клиент?",
        None,
        None,
    )
    .await?;
    chat.share_message(general.id, review_msg.id, bob).await?;

    // --- Трассировка: выполненная задача + pending human-запрос.
    trace.create_task("seed-task-001", "dev").await?;
    trace
        .add_entry(
            "seed-task-001",
            0,
            "prompt",
            "Проверь пул-реквест #42",
            None,
        )
        .await?;
    trace
        .add_entry(
            "seed-task-001",
            1,
            "llm",
            "План: ревью auth.rs, прогнать тесты.",
            None,
        )
        .await?;
    trace
        .add_entry("seed-task-001", 2, "command", "git diff --stat", None)
        .await?;
    trace
        .add_entry(
            "seed-task-001",
            3,
            "result",
            "Конфликт в auth.rs, тесты падают.",
            None,
        )
        .await?;
    trace.complete_task("seed-task-001", "success").await?;
    let hr = trace
        .create_human_request("seed-task-001", "Какой порт открыть для API?")
        .await?;
    trace.answer_human_request(&hr, "8080").await?;

    trace.create_task("seed-task-002", "dev").await?;
    trace
        .create_human_request("seed-task-002", "Разрешить деплой на прод?")
        .await?;

    tracing::info!(
        "Тестовый набор восстановлен: users={}, set={}, projects={}/{} ws, chats={}; \
         вход в Keycloak: alice/alice-pass, bob/bob-pass",
        4,
        set_id,
        p1,
        p2,
        session.id
    );
    Ok(())
}
