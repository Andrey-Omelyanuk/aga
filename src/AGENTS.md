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
├── main.rs    # Точка входа: env, init Config, TraceStore, LlmClient → AppState
├── config.rs  # Config, RoleConfig, LlmConfig — загрузка из YAML
├── llm.rs     # LlmClient — OpenAI-compatible HTTP-клиент
├── agent.rs   # Agent — основной цикл: LLM → extract → exec → trace
├── trace.rs   # TraceStore — SQLite: tasks, trace_entries, human_requests, projects, project_roles
└── server.rs  # Axum Router + хендлеры API + SSE для human-запросов
```

## Patterns
- **Изоляция модулей:** `agent` зависит от `config`, `llm`, `trace`; `server` зависит от всех; `main` инициализирует и связывает.
- **Состояние:** `AppState { config, trace_store, llm_client }` передаётся через Axum State.
- **Ошибки:** `thiserror` для типизированных ошибок; `Box<dyn Error + Send + Sync>` в agent-цикле (потенциально разные типы ошибок).
- **LLM-клиент:** один `LlmClient` на все роли; модель и температура из `RoleConfig`.
- **SQLite:** CREATE TABLE IF NOT EXISTS при старте в `TraceStore::new`; WAL-режим; raw-запросы.
- **SSE:** `/human/pending` — SSE-стрим для получения pending-запросов в реальном времени.

## Non-Obvious Rules
- **TraceStore шире трассировки:** в нём же живут `projects` и `project_roles`. Это осознанное упрощение ради одного соединения с БД и одной точки миграции.
- **Безопасность команд:** `is_command_allowed()` банит пайпы (`|`), редиректы (`>`, `<`) и конкатенацию (`;`, `&`) на уровне строки — это поверх проверки `allowed_commands`.
- **Agent per task:** Agent создаётся на каждый вызов `POST /tasks/:role`, не хранит state между вызовами.
- **История в LLM-запросах:** чередование user/assistant (чётные — user, нечётные — assistant). Это упрощение для слабых LLM, не использующих multi-turn корректно.
- **Команды из LLM:** извлекаются из markdown-блоков <code>```bash</code>. Код-блок может содержать несколько команд (построчно). Пустые строки и комментарии (`#`) игнорируются.
- **Error handling в API:** большинство хендлеров при ошибке возвращают `500 INTERNAL_SERVER_ERROR` без деталей. Детали — в логах (tracing) и в БД (trace_entries с entry_type = "error").

## Verification
- `cargo test` — unit-тесты для: `extract_commands`, `is_command_allowed`, `Config::load`, парсинга ответов.
- `cargo clippy` — статический анализ без предупреждений.
- **Критерий готовности:** все unit-тесты проходят; публичные API компилируются без ошибок.

## Dependencies
- Стандартная библиотека Rust
- Внешние крейты: axum, tokio, tower-http, reqwest, serde, serde_yaml, serde_json, sqlx (sqlite + uuid), uuid, chrono, tracing, thiserror, regex, futures, futures-util, bytes
