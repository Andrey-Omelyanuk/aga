# aga — LLM Agent Framework

## Overview
Фреймворк для создания и запуска LLM-агентов. Предоставляет HTTP API, цикл агента
(LLM → parse → exec → trace), SQLite-трассировку, human-in-the-loop и модель чата
(сессии, участники, сообщения-дерево, шаринг). Спроектирован для слабых
(маломощных) LLM — минимум шагов, простые промпты, никаких оркестраторов.

## Boundaries
- **Делает:** предоставляет HTTP API для отправки задач агентам и чата; управляет
  жизненным циклом агента; валидирует команды по белому списку; хранит трассировку
  в SQLite (WAL); поддерживает human-in-the-loop через `[ASK_HUMAN]`; управляет
  проектами (git-репозиторий) и ролями через API; воркстейшны — как поды в
  Kubernetes (`kubectl`); модель чата (`/users`, `/chats`, `/messages`,
  `/workstations`); SSO (Keycloak): JWKS-проверка JWT, роли participant/admin,
  вход веб-клиента через `/auth/login` + `/auth/callback`.
- **Не делает:** не управляет Docker-контейнерами напрямую (только через shell);
  не предоставляет готовых агентов — агенты конфигурируются через `roles/`;
  не является оркестратором (NATS, Redis, S3); не управляет воркстейшнами из
  веб-интерфейса (их поднимает админ через SSO и k8s); не редактирует персонал
  внутри aga (учётки и роли — в Keycloak).
- **Модель доступа (веб):** участники из SSO видят все проекты и сессии;
  участник создаёт проекты и открывает сессии на готовых воркстейшнах (одна
  активная сессия на воркстейшн); закрыть сессию может только её владелец;
  админ — внешняя сущность (SSO + k8s), в aga его нет.
- **Модель чата (минимальная реализация):** дизайн — `oos/` (дерево сообщений,
  единый `parent_id`, шаринг `#share <chat_id>` как копия-шар со ссылкой на
  оригинал, сессия как корневой чат воркстейшна, реактивные агенты); реализация —
  `src/chat.rs` + API. Ещё не сделано: DinD-изоляция воркстейшнов.

## Tech Stack
- Rust 2021, Tokio, Axum 0.7, reqwest 0.11 (rustls), sqlx 0.7 (SQLite, WAL),
  serde, regex, tracing, thiserror, base64.

## Architecture
```
aga/
├── src/           # Ядро на Rust (ядро фреймворка)
├── static/        # Веб-клиент (SPA, index.html)
├── roles/         # Библиотека YAML-пресетов ролей агентов
├── prompts/       # Системные промпты для агентов
├── config/        # Runtime-конфиг ролей (roles.yaml, из config.example.yml)
├── examples/      # Примеры проектов (game-xo, game-2d)
├── data/          # Runtime-данные (trace.db, work/) — в .gitignore
├── infra/         # Образ ядра (Dockerfile), .env.example, k8s-стенд (ядро + Keycloak + воркстейшны), AGENTS.md
├── oos/           # Дизайн-модель (object-oriented design, документы-объекты)
├── stories/       # Истории разработки
├── makefile       # Единственный интерфейс команд (см. Development)
├── Cargo.toml
└── Dockerfile     # Мультистейдж-сборка
```

## Patterns
- Всё async через Tokio.
- Ошибки в agent-цикле — `Box<dyn Error + Send + Sync>`; в остальных модулях — `thiserror`.
- sqlx с raw-запросами (без ORM), WAL-режим; БД создаётся через
  `create_if_missing(true)`.
- LLM-запросы через OpenAI-compatible API (один LlmClient на все роли).
- Парсинг ответов LLM через regex (команды из markdown-код-блоков).
- Human-in-the-loop через маркер `[ASK_HUMAN]...[/ASK_HUMAN]`.
- Модель чата отделена от трассировки: `src/chat.rs` (ChatStore) рядом с
  `src/trace.rs` (TraceStore), БД одна.

## Development
- Все команды — через `make` в корне.
- `make init` — создаёт `.env` (из `infra/.env.example`) и `config/roles.yaml`
  (из `config.example.yml`).
- Локальная разработка: `make build`, `make run`, `make test`, `make lint`, `make fmt`.
- Тестовый стенд — в k8s (minikube): `make k8s-up`, `make k8s-build`, `make k8s-load`,
  `make k8s-deploy`, `make k8s-wait`, `make k8s-web`, `make k8s-verify`; ручной
  доступ по `*.localhost` (dev/api/auth) — `make k8s-dev` (локальный nginx-прокси
  в Docker, без tunnel) и остановка — `make k8s-dev-stop`.
- Переменные окружения — `.env` в корне (см. `infra/.env.example`).

## Non-Obvious Rules
- `roles/` — библиотека пресетов; runtime загружает `config/roles.yaml` (сборка
  из одного или нескольких пресетов, валидная структура — ключ `roles:`).
- Команды выполняются через `sh -c` (dev, без воркстейшна) или `kubectl exec`
  в под воркстейшна (Kubernetes). Inline-конструкции (`|`, `>`, `<`, `;`, `&`)
  запрещены на уровне `is_command_allowed`.
- Проект регистрируется git-URL; воркстейшн — под `ws-<id>` с собственным Docker
  (DinD) и копией проекта; кластером управляет только ядро.
- `[ASK_HUMAN]` — единственный протокол human-in-the-loop. При обнаружении
  выполнение задачи останавливается до ответа.
- Модель чата: команды (`#invite`/`#kick`/`#start`/`#end`/`#share`) — это обычные
  сообщения с дополнительной реакцией; реактивные агенты по `@Agent.<role>`
  сериализуются per-workstation.
- Каждый task создаёт новый Agent (легковесный, без state между задачами).

## Verification
- Сборка и линт: `make build`, `make lint` — без ошибок.
- Тесты: `make test` (cargo test).
- Интеграционный тест стенда: `make k8s-verify` — ядро и Keycloak поднимаются в
  кластере (minikube), проверяются воркстейшны-поды, SSO и персистентность;
  локально `make run` отвечает на `/users`, `/chats/:id/messages`.
- Критерий готовности: фреймворк компилируется, запускается, отвечает на HTTP,
  выполняет цикл агента и сохраняет трассировку.

## Dependencies
- Rust (edition 2021)
- LLM API (OpenAI-compatible: Ollama, vLLM, OpenAI, LocalAI)
- Docker (для сборки образов ядра и воркстейшна)
- Kubernetes (`kubectl`) — стенд и воркстейшны как поды; локально — minikube (`make k8s-up`)

## Markdown Style
- Заголовки — `## <Section>`; списки через `-`; Код в блоках с указанием языка.
- Документы-объекты дизайна — в `oos/`, каждая сущность отдельным `*.md`.
