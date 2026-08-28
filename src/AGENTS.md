# src — Rust Core

## Overview
Реализация ядра фреймворка: HTTP-сервер, цикл агента (LLM → parse → exec → trace), LLM-клиент, SQLite-трассировка с управлением проектами и ролями.

## Boundaries
- **Делает:** обрабатывает HTTP-запросы (Axum); запускает цикл агента (LLM, парсинг, валидация, выполнение команд); хранит/отдаёт трассировку; управляет проектами и ролями в SQLite; предоставляет Web UI из `static/`.
- **Не делает:** не определяет роли и промпты агентов (это `roles/`); не управляет Docker напрямую; не реализует аутентификацию; не предоставляет CLI.

## Tech Stack
- Rust 2021, Axum 0.7, Tokio 1, sqlx 0.7 (SQLite), reqwest 0.11, regex, serde, chrono, uuid, tracing, thiserror

## Architecture
```
src/
├── main.rs       # Точка входа: env, init Config, TraceStore, ChatStore, Cluster, LlmClient→AppState
├── config.rs     # Config, RoleConfig, LlmConfig, SsoConfig — загрузка из YAML
├── llm.rs        # LlmClient — OpenAI-compatible HTTP-клиент
├── agent.rs      # Agent — цикл LLM → extract → exec → trace; Executor (Sh / KubectlExec)
├── trace.rs      # TraceStore — SQLite: tasks, trace_entries, human_requests, projects, project_roles
├── chat.rs       # ChatStore — модель чата: chat_users, chats, participants, messages, artifacts, workstations
├── cluster.rs    # Cluster — воркстейшн как под: рендер манифеста, kubectl apply/delete, wait_ready
├── workstation.rs# executor_for_workstation: воркстейшн → kubectl exec в его под
├── reactive.rs   # ReactiveRunner: реактивные агенты по @Agent.<role>, очередь на воркстейшн
├── auth.rs       # resolve_user: аноним-суперюзер или Bearer JWT `sub` → chat_user
└── server.rs     # Axum Router + хендлеры API + SSE для human-запросов
```

## Patterns
- **Изоляция модулей:** `agent` зависит от `config`, `llm`, `trace`; `chat`/`workstation`/`reactive`/`auth` зависят друг от друга и от `trace`; `server` зависит от всех; `main` инициализирует и связывает.
- **Состояние:** `AppState { config, trace_store, chat_store, llm_client, reactive }` передаётся через Axum State.
- **Ошибки:** `thiserror` для типизированных ошибок; `Box<dyn Error + Send + Sync>` в agent-цикле (потенциально разные типы ошибок).
- **LLM-клиент:** один `LlmClient` на все роли; модель и температура из `RoleConfig`.
- **SQLite:** CREATE TABLE IF NOT EXISTS при старте; WAL-режим; raw-запросы. Обязательно `SqliteConnectOptions::create_if_missing(true)` — `SqlitePool::connect` файл НЕ создаёт.
- **SSE:** `/human/pending` — SSE-стрим для получения pending-запросов в реальном времени.

## Non-Obvious Rules
- **TraceStore шире трассировки:** в нём же живут `projects` и `project_roles`. ChatStore — отдельный модуль с таблицами модели чата, БД одна (тот же файл).
- **Проект = git-URL:** воркстейшн-под клонирует репозиторий; `compose_path` из старых БД мигрируется в `git_url` (см. `migrate_projects_git_url`).
- **Воркстейшн = под:** `ws-<id>` в неймспейсе кластера, команды агента — `kubectl exec`. Pod-манифест рендерит `cluster.rs` из шаблона (встроенный дефолт совпадает с `infra/k8s/workstation-pod.yaml`); под привилегированный (DinD) и без доступа к k8s API (`automountServiceAccountToken: false`).
- **Безопасность команд:** `is_command_allowed()` банит пайпы (`|`), редиректы (`>`, `<`) и конкатенацию (`;`, `&`) на уровне строки — это поверх проверки `allowed_commands`.
- **Agent per task / per reactive-сообщение:** Agent создаётся на каждую задачу и каждый реактивный запуск; state не хранится между вызовами.
- **Команды из LLM:** извлекаются из markdown-блоков <code>```bash</code>. Код-блок может содержать несколько команд (построчно). Пустые строки и комментарии (`#`) игнорируются.
- **Команды чата** (`#invite`/`#kick`/`#start`/`#end`/`#share`) — это обычные сообщения с дополнительной реакцией; разбираются в `chat::parse_command`, исполняются в server.rs.
- **Реактивные агенты:** `@Agent.<role>` в сообщении триггерит ReactiveRunner.enqueue; очередь сериализуется per-workstation (ключ 0 = локальный хост). Ответ пишется сообщением от учётки агента + артефакт.
- **Текущий пользователь:** без SSO — аноним-суперюзер; если SSO включён и есть Bearer JWT — `sub` → chat_user (создаётся при необходимости). Подпись токена пока не проверяется (минимально).
- **Error handling в API:** большинство хендлеров при ошибке возвращают `500 INTERNAL_SERVER_ERROR` без деталей. Детали — в логах (tracing) и в БД (trace_entries с entry_type = "error").

## Verification
- `make test` (cargo test) — unit-тесты для: `extract_commands`, `is_command_allowed`, `Config::load`, `parse_command`, `mentioned_roles`, `project_registered_by_git_url`, `migrates_compose_path_to_git_url`, `deleted_workstation_disappears_from_list`, `workstation_renders_pod_with_git_url_and_branch`, `each_workstation_gets_its_own_pod`, `workstation_pod_has_no_k8s_api_access`, `agent_commands_run_in_workstation_pod`, `workstation_executor_targets_its_pod`, JWT-субъекта, открытия БД (`chatstore_opens_db`).
- `make lint` (cargo clippy --all-targets) — статический анализ без предупреждений.
- **Составные тесты:** `chatstore_opens_db` проверяет открытие TraceStore+ChatStore на временной БД.
- **Интеграция (кластер):** `make k8s-verify` (см. `infra/k8s/AGENTS.md`) — воркстейшн поднимает под в локальном кластере.
- **Критерий готовности:** все unit-тесты проходят; публичные API компилируются без ошибок; сервер отвечает на `/users`, `/chats`, `/chats/:id/messages`.

## Dependencies
- Стандартная библиотека Rust
- Внешние крейты: axum, tokio, tower-http, reqwest (rustls-tls), serde, serde_yaml, serde_json, sqlx (sqlite + uuid + chrono), uuid, chrono, tracing, thiserror, regex, base64, futures, futures-util, bytes
