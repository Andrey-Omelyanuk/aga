use super::*;

#[test]
fn parses_commands() {
    assert_eq!(
        parse_command("#invite @B"),
        Some(ChatCommand::Invite("B".into()))
    );
    assert_eq!(
        parse_command("#kick @B"),
        Some(ChatCommand::Kick("B".into()))
    );
    assert_eq!(parse_command("#end"), Some(ChatCommand::End));
    assert_eq!(parse_command("hello"), None);
    assert_eq!(parse_command("\n#start X"), None);
    // #start больше не команда: нити начинаются действием у сообщения.
    assert_eq!(parse_command("#start New thread"), None);
}

#[test]
fn finds_shortcut_names_once_in_order() {
    // Слова вида `/имя`, по одному разу, в порядке появления.
    assert_eq!(shortcut_names("/review текст"), vec!["review"]);
    assert_eq!(
        shortcut_names("/first и /second, /first"),
        vec!["first", "second"]
    );
    // Без слэша — не сокращение; путь с внутренним слэшем пропускается.
    assert!(shortcut_names("review /usr/bin").is_empty());
    assert!(shortcut_names("http://example.com/x").is_empty());
}

#[tokio::test]
async fn chatstore_opens_db() {
    let path = std::env::temp_dir().join(format!("aga_chat_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let users = store.list_users().await.unwrap();
    assert!(!users.is_empty());
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn deleted_workstation_disappears_from_list() {
    let path = std::env::temp_dir().join(format!("aga_chat_ws_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws = store.create_workstation("w1", None).await.unwrap();
    assert!(store
        .list_workstations()
        .await
        .unwrap()
        .iter()
        .any(|w| w.id == ws.id));
    assert!(store.delete_workstation(ws.id).await.unwrap());
    assert!(!store
        .list_workstations()
        .await
        .unwrap()
        .iter()
        .any(|w| w.id == ws.id));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

async fn ready_ws(store: &ChatStore) -> i64 {
    let ws = store.create_workstation("w1", None).await.unwrap();
    store.set_workstation_state(ws.id, "ready").await.unwrap();
    ws.id
}

#[tokio::test]
async fn participant_sees_all_sessions() {
    let path = std::env::temp_dir().join(format!("aga_chat_see_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let participant = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let alice = store
        .insert_user("alice", "human", false, None, None)
        .await
        .unwrap();
    let carol = store
        .insert_user("carol", "human", false, None, None)
        .await
        .unwrap();
    let c1 = store.create_chat(None, Some("s1"), alice).await.unwrap();
    let c2 = store.create_chat(None, Some("s2"), carol).await.unwrap();
    let chats = store.list_chats_for_user(participant).await.unwrap();
    assert!(chats.iter().any(|c| c.id == c1.id));
    assert!(chats.iter().any(|c| c.id == c2.id));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn session_binds_to_ready_workstation() {
    let path = std::env::temp_dir().join(format!("aga_chat_open_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let chat = store
        .open_workstation_session(ws_id, 1, Some("s1"), user)
        .await
        .unwrap();
    // Сессия связана с воркстейшном через sessions (workstation_id больше
    // не в chats).
    assert_eq!(
        store.root_workstation_id(chat.id).await.unwrap(),
        Some(ws_id)
    );
    assert_eq!(
        store
            .get_workstation(ws_id)
            .await
            .unwrap()
            .unwrap()
            .current_session_id,
        Some(chat.id)
    );
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn workstation_not_ready_rejects_session() {
    let path = std::env::temp_dir().join(format!("aga_chat_nr_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws = store.create_workstation("w1", None).await.unwrap();
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let err = store
        .open_workstation_session(ws.id, 1, Some("s1"), user)
        .await
        .unwrap_err();
    assert!(matches!(err, SessionError::WorkstationNotReady));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn workstation_holds_single_open_session() {
    let path = std::env::temp_dir().join(format!("aga_chat_busy_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    store
        .open_workstation_session(ws_id, 1, Some("a"), user)
        .await
        .unwrap();
    let err = store
        .open_workstation_session(ws_id, 1, Some("b"), user)
        .await
        .unwrap_err();
    assert!(matches!(err, SessionError::WorkstationBusy));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn session_closed_only_by_owner() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_owner_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let owner = store
        .insert_user("owner", "human", false, None, None)
        .await
        .unwrap();
    let chat = store
        .open_workstation_session(ws_id, 1, Some("s"), owner)
        .await
        .unwrap();
    store
        .close_workstation_session(chat.id, owner)
        .await
        .unwrap();
    assert!(store.active_session_id(ws_id).await.unwrap().is_none());
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn participant_cannot_close_foreign_session() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_foreign_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let owner = store
        .insert_user("owner", "human", false, None, None)
        .await
        .unwrap();
    let other = store
        .insert_user("other", "human", false, None, None)
        .await
        .unwrap();
    let chat = store
        .open_workstation_session(ws_id, 1, Some("s"), owner)
        .await
        .unwrap();
    let err = store
        .close_workstation_session(chat.id, other)
        .await
        .unwrap_err();
    assert!(matches!(err, SessionError::Forbidden));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn closed_session_frees_workstation() {
    let path = std::env::temp_dir().join(format!("aga_chat_free_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let first = store
        .open_workstation_session(ws_id, 1, Some("a"), user)
        .await
        .unwrap();
    store
        .close_workstation_session(first.id, user)
        .await
        .unwrap();
    let second = store
        .open_workstation_session(ws_id, 1, Some("b"), user)
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn workstation_keeps_named_secret() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_secret_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws = store.create_workstation("w1", Some("creds")).await.unwrap();
    assert_eq!(
        store
            .get_workstation(ws.id)
            .await
            .unwrap()
            .unwrap()
            .secret
            .as_deref(),
        Some("creds")
    );
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn session_project_is_direct_and_station_follows_sessions() {
    let path = std::env::temp_dir().join(format!(
        "aga_chat_sessproj_test_{}.db",
        uuid::Uuid::new_v4()
    ));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    // Сессия проекта 1: проект чата достаётся напрямую из sessions.
    let first = store
        .open_workstation_session(ws_id, 1, Some("s1"), user)
        .await
        .unwrap();
    assert_eq!(store.project_id_for_chat(first.id).await.unwrap(), Some(1));
    store
        .close_workstation_session(first.id, user)
        .await
        .unwrap();
    // Станция свободна и может обслуживать сессию другого проекта.
    let second = store
        .open_workstation_session(ws_id, 2, Some("s2"), user)
        .await
        .unwrap();
    assert_eq!(store.project_id_for_chat(second.id).await.unwrap(), Some(2));
    assert_eq!(store.project_id_for_chat(first.id).await.unwrap(), Some(1));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn released_workstation_is_free_after_session_close() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_release_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let session = store
        .open_workstation_session(ws_id, 1, Some("s"), user)
        .await
        .unwrap();
    store
        .close_workstation_session(session.id, user)
        .await
        .unwrap();
    // Закрытая сессия освобождает станцию — release проходит и
    // подтверждает свободу (без активной сессии).
    let released = store.release_workstation(ws_id).await.unwrap();
    assert_eq!(released.current_session_id, None);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn releasing_workstation_rejected_while_session_open() {
    let path = std::env::temp_dir().join(format!(
        "aga_chat_releasebusy_test_{}.db",
        uuid::Uuid::new_v4()
    ));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws_id = ready_ws(&store).await;
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    store
        .open_workstation_session(ws_id, 1, Some("s"), user)
        .await
        .unwrap();
    let err = store.release_workstation(ws_id).await.unwrap_err();
    assert!(matches!(err, SessionError::WorkstationBusy));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn crashed_workstation_marked_down() {
    let path = std::env::temp_dir().join(format!("aga_chat_down_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws = store.create_workstation("w1", None).await.unwrap();
    store.mark_workstation_down(ws.id).await.unwrap();
    assert_eq!(
        store.get_workstation(ws.id).await.unwrap().unwrap().state,
        "down"
    );
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn workstation_down_rejects_new_session() {
    let path = std::env::temp_dir().join(format!(
        "aga_chat_downsess_test_{}.db",
        uuid::Uuid::new_v4()
    ));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let ws = store.create_workstation("w1", None).await.unwrap();
    store.mark_workstation_down(ws.id).await.unwrap();
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let err = store
        .open_workstation_session(ws.id, 1, Some("s"), user)
        .await
        .unwrap_err();
    assert!(matches!(err, SessionError::WorkstationNotReady));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn session_opened_on_free_ws_recovers_interrupted_session() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_recover_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    // Станция 1: проект 1, открыта сессия, станция упала — сессия прервана.
    let ws1 = store.create_workstation("ws1", None).await.unwrap();
    store.set_workstation_state(ws1.id, "ready").await.unwrap();
    let interrupted = store
        .open_workstation_session(ws1.id, 1, Some("s1"), user)
        .await
        .unwrap();
    store.mark_workstation_down(ws1.id).await.unwrap();
    // На свободной станции 2 того же проекта открывают сессию — она
    // распознаёт продолжение прерванной и восстанавливает её.
    let ws2 = store.create_workstation("ws2", None).await.unwrap();
    store.set_workstation_state(ws2.id, "ready").await.unwrap();
    let recovered = store
        .open_workstation_session(ws2.id, 1, Some("s2"), user)
        .await
        .unwrap();
    assert_eq!(
        store.continues_session_id(recovered.id).await.unwrap(),
        Some(interrupted.id)
    );
    assert_eq!(
        store.session_branch(interrupted.id).await.unwrap(),
        Some(crate::cluster::Cluster::branch_name(ws1.id))
    );
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn fresh_session_without_crash_has_no_continuation() {
    let path = std::env::temp_dir().join(format!(
        "aga_chat_norecover_test_{}.db",
        uuid::Uuid::new_v4()
    ));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    let ws_id = ready_ws(&store).await;
    let s = store
        .open_workstation_session(ws_id, 1, Some("new"), user)
        .await
        .unwrap();
    assert!(store.continues_session_id(s.id).await.unwrap().is_none());
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

async fn chat_fixture() -> (ChatStore, std::path::PathBuf, i64) {
    let path =
        std::env::temp_dir().join(format!("aga_chat_thread_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let user = store
        .insert_user("bob", "human", false, None, None)
        .await
        .unwrap();
    (store, path, user)
}

#[tokio::test]
async fn thread_starts_from_message_with_title_on_first_message() {
    let (store, path, user) = chat_fixture().await;
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let msg = store
        .send_message(root.id, user, "задача", "", None, None)
        .await
        .unwrap()
        .unwrap();
    let (thread, first) = store
        .start_thread(root.id, msg.id, "Обсуждение", "Что падает?", "", user)
        .await
        .unwrap()
        .unwrap();
    // Нить — дочерний чат, привязанный к сообщению; у самого чата-нити
    // заголовка нет.
    assert_eq!(thread.parent_id, Some(root.id));
    assert_eq!(thread.start_message_id, Some(msg.id));
    assert_eq!(thread.title, None);
    // Заголовок живёт на первом сообщении нити, оно отвечает на источник.
    assert_eq!(first.chat_id, thread.id);
    assert_eq!(first.title.as_deref(), Some("Обсуждение"));
    assert_eq!(first.body, "Что падает?");
    assert_eq!(first.parent_id, Some(msg.id));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn one_message_spawns_several_threads() {
    let (store, path, user) = chat_fixture().await;
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let msg = store
        .send_message(root.id, user, "задача", "", None, None)
        .await
        .unwrap()
        .unwrap();
    store
        .start_thread(root.id, msg.id, "Разбор демо", "Первое.", "", user)
        .await
        .unwrap()
        .unwrap();
    store
        .start_thread(root.id, msg.id, "Таймеры", "Второе.", "", user)
        .await
        .unwrap()
        .unwrap();
    let threads = store.list_threads(root.id).await.unwrap();
    assert_eq!(threads.len(), 2);
    assert!(threads.iter().all(|t| t.start_message_id == Some(msg.id)));
    // Заголовки нитей — по своим первым сообщениям.
    let mut titles: Vec<String> = Vec::new();
    for t in &threads {
        let first = &store.list_messages(t.id).await.unwrap()[0];
        titles.push(first.title.clone().unwrap());
    }
    assert!(titles.contains(&"Разбор демо".to_string()));
    assert!(titles.contains(&"Таймеры".to_string()));
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn threads_not_listed_for_user() {
    let (store, path, user) = chat_fixture().await;
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let general = store.create_chat(None, Some("g"), user).await.unwrap();
    let msg = store
        .send_message(root.id, user, "задача", "", None, None)
        .await
        .unwrap()
        .unwrap();
    store
        .start_thread(root.id, msg.id, "Обсуждение", "Что падает?", "", user)
        .await
        .unwrap()
        .unwrap();
    let chats = store.list_chats_for_user(user).await.unwrap();
    let ids: Vec<i64> = chats.iter().map(|c| c.id).collect();
    // В списке — только корневые чаты (сессии и общие чаты), нитей нет.
    assert!(ids.contains(&root.id));
    assert!(ids.contains(&general.id));
    assert_eq!(ids.len(), 2);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn post_to_parent_puts_copy_with_thread_link() {
    let (store, path, user) = chat_fixture().await;
    let other = store
        .insert_user("alice", "human", false, None, None)
        .await
        .unwrap();
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let msg = store
        .send_message(root.id, user, "задача", "", None, None)
        .await
        .unwrap()
        .unwrap();
    let (thread, _) = store
        .start_thread(root.id, msg.id, "Обсуждение", "Что падает?", "", user)
        .await
        .unwrap()
        .unwrap();
    let reply = store
        .send_message(thread.id, other, "Гоняю run-tests.", "", None, None)
        .await
        .unwrap()
        .unwrap();
    let copy = store.post_to_parent(reply.id).await.unwrap().unwrap();
    // Копия в ленте родителя со ссылкой на сообщение-источник нити;
    // оригинал остаётся в нити.
    assert_eq!(copy.chat_id, root.id);
    assert_eq!(copy.body, "Гоняю run-tests.");
    assert_eq!(copy.author_id, other);
    assert_eq!(copy.thread_of_id, Some(msg.id));
    assert!(store.get_message(reply.id).await.unwrap().is_some());
    assert_eq!(store.list_messages(thread.id).await.unwrap().len(), 2);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn nested_thread_spawns_from_message_inside_thread() {
    let (store, path, user) = chat_fixture().await;
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let msg = store
        .send_message(root.id, user, "задача", "", None, None)
        .await
        .unwrap()
        .unwrap();
    let (thread, _) = store
        .start_thread(root.id, msg.id, "Первая", "тело", "", user)
        .await
        .unwrap()
        .unwrap();
    let tmsg = store
        .send_message(thread.id, user, "внутри", "", None, None)
        .await
        .unwrap()
        .unwrap();
    // Вложенная нить: начинается от сообщения внутри нити, родитель — нить.
    let (nested, _) = store
        .start_thread(thread.id, tmsg.id, "Вторая", "глубже", "", user)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(nested.parent_id, Some(thread.id));
    assert_eq!(nested.level, 2);
    assert_eq!(nested.start_message_id, Some(tmsg.id));
    assert_eq!(nested.root_id, root.id);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn thread_depth_limited_to_max_level() {
    let (store, path, user) = chat_fixture().await;
    let root = store.create_chat(None, Some("s"), user).await.unwrap();
    let mut parent = root.clone();
    let mut origin = store
        .send_message(root.id, user, "start", "", None, None)
        .await
        .unwrap()
        .unwrap();
    // Допустимо до max_chat_level() уровней вложенности.
    for _ in 0..crate::chat::max_chat_level() {
        let (thread, _) = store
            .start_thread(parent.id, origin.id, "т", "б", "", user)
            .await
            .unwrap()
            .unwrap();
        parent = thread;
        origin = store
            .send_message(parent.id, user, "сообщение", "", None, None)
            .await
            .unwrap()
            .unwrap();
    }
    // Следующий уровень за пределом не создаётся.
    assert!(store
        .start_thread(parent.id, origin.id, "т", "б", "", user)
        .await
        .unwrap()
        .is_none());
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn thread_accepts_at_most_max_messages_root_chat_unlimited() {
    let path =
        std::env::temp_dir().join(format!("aga_chat_limit_test_{}.db", uuid::Uuid::new_v4()));
    let _ = crate::trace::TraceStore::new(path.to_str().unwrap())
        .await
        .unwrap();
    let store = ChatStore::new(path.to_str().unwrap()).await.unwrap();
    let alice = store
        .insert_user("alice", "human", false, None, None)
        .await
        .unwrap();
    let chat = store.create_chat(None, Some("s"), alice).await.unwrap();
    let source = store
        .send_message(chat.id, alice, "тема", "", None, None)
        .await
        .unwrap()
        .unwrap();
    let (thread, _) = store
        .start_thread(chat.id, source.id, "нить", "первое", "", alice)
        .await
        .unwrap()
        .unwrap();
    // Первое сообщение уже есть — дописываем до предела.
    for i in 1..max_thread_messages() {
        assert!(store
            .send_message(thread.id, alice, &format!("m{i}"), "", None, None)
            .await
            .unwrap()
            .is_some());
    }
    assert!(store
        .send_message(thread.id, alice, "сверх предела", "", None, None)
        .await
        .unwrap()
        .is_none());
    // Шаги работы тоже считаются сообщениями нити.
    assert!(store
        .send_step(thread.id, alice, "ls", "", "{}", "agent")
        .await
        .unwrap()
        .is_none());
    // Корневой чат не ограничен.
    for _ in 0..max_thread_messages() {
        store
            .send_message(chat.id, alice, "x", "", None, None)
            .await
            .unwrap()
            .unwrap();
    }
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn thread_limits_come_from_env_or_default() {
    assert_eq!(limit_from(Some("25"), 10), 25);
    assert_eq!(limit_from(Some(" 7 "), 10), 7);
    for bad in [None, Some(""), Some("abc"), Some("0"), Some("-3")] {
        assert_eq!(limit_from(bad, 10), 10, "{bad:?}");
    }
}

#[test]
fn mentions_are_words_starting_with_at() {
    assert_eq!(
        mentioned_names("@alice, глянь; @bob! и @alice ещё раз (@carol) a@b @"),
        vec!["alice", "bob", "carol"]
    );
    assert!(mentioned_names("без упоминаний").is_empty());
}
