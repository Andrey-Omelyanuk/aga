# aga — LLM Agent Framework

## Overview
Фреймворк для создания и запуска LLM-агентов. Предоставляет HTTP API, цикл агента (LLM → parse → exec → trace), SQLite-трассировку и human-in-the-loop. Спроектирован для слабых (маломощных) LLM — минимум шагов, простые промпты, никаких оркестраторов.

## Boundaries
- **Делает:** предоставляет HTTP API для отправки задач агентам; управляет жизненным циклом агента; валидирует команды по белому списку; хранит трассировку в SQLite (WAL); поддерживает human-in-the-loop через `[ASK_HUMAN]`; управляет проектами (docker-compose) и ролями через API.
- **Не делает:** не управляет Docker-контейнерами напрямую (только через shell); не предоставляет готовых агентов — агенты конфигурируются через `roles/`; не включает аутентификацию; не является оркестратором (NATS, Redis, S3).

## Tech Stack
- Rust 2021, Tokio, Axum 0.7, reqwest 0.11, sqlx 0.7 (SQLite), serde, tracing, thiserror, regex

## Architecture
```
aga/
├── src/           # Ядро на Rust
├── roles/         # YAML-пресеты ролей агентов
├── static/        # Веб-интерфейс (SPA, index.html)
├── prompts/       # Системные промпты для агентов
├── examples/      # Примеры проектов (game-xo, game-2d)
├── config/        # Основной конфиг (roles.yaml, example)
├── data/          # Runtime-данные (trace.db, work/)
├── Cargo.toml
├── Dockerfile
└── docker-compose.yml
```

## Patterns
- Всё async через Tokio.
- Ошибки в agent-цикле — `Box<dyn Error + Send + Sync>`; в остальных модулях — `thiserror` или直接 return.
- sqlx с raw-запросами (без ORM), WAL-режим.
- LLM-запросы через OpenAI-compatible API (один LlmClient на все роли).
- Парсинг ответов LLM через regex (извлекает команды из markdown-код-блоков).
- Human-in-the-loop через маркер `[ASK_HUMAN]...[/ASK_HUMAN]`.

## Non-Obvious Rules
- `roles/` — это библиотека пресетов; runtime загружает `config/roles.yaml` (сборка из одного или нескольких пресетов).
- Команды выполняются через `sh -c` (dev) или `docker compose exec` (prod). Inline-конструкции (`|`, `>`, `<`, `;`, `&`) запрещены на уровне `is_command_allowed`.
- `[ASK_HUMAN]` — единственный протокол human-in-the-loop. При его обнаружении выполнение задачи останавливается до ответа.
- Проекты и роли проектов хранятся в SQLite вместе с трассировкой (в TraceStore). Это осознанное упрощение: не плодить отдельный слой.
- Каждый task создаёт новый Agent (легковесный, без state между задачами).

## Verification
- **Сборка:** `cargo build` / `cargo clippy` — без ошибок.
- **Интеграционный тест:** `docker compose up --build` и `curl POST /tasks/<role>` — сервер отвечает, агент выполняет задачу.
- **Критерий готовности:** фреймворк компилируется, запускается, отвечает на HTTP-запросы, выполняет цикл агента и сохраняет трассировку.

## Dependencies
- Rust (edition 2021)
- LLM API (OpenAI-compatible: Ollama, vLLM, OpenAI, LocalAI)
- Docker (для production-режима)
