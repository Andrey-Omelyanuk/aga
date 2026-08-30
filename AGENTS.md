# aga — LLM Agent Framework

## Overview
Монорепо фреймворка для создания и запуска LLM-агентов. Состоит из двух
сервисов: ядро (`main/` — Rust: HTTP API, цикл агента, SQLite-трассировка,
human-in-the-loop, модель чата, SSO) и веб-клиент (`front/` — SPA на nginx).
Стенд — в Kubernetes (`infra/`). Спроектирован для слабых (маломощных) LLM —
минимум шагов, простые промпты, никаких оркестраторов.

## Boundaries
- **Делает (платформа):** REST API задач агентам и чата; управление жизненным
  циклом агента; валидация команд по белому списку; трассировка в SQLite (WAL);
  human-in-the-loop через `[ASK_HUMAN]`; проекты (git-репозиторий) и наборы
  агентов (AgentSet) через API; воркстейшны — поды в Kubernetes (`kubectl`) или
  контейнеры в dev (`docker`, `AGA_WS_BACKEND=docker`); модель чата (`/users`,
  `/chats`, `/messages`, `/workstations`); SSO (Keycloak): JWKS-проверка JWT,
  роли participant/admin, вход веб-клиента через `/auth/login` + `/auth/callback`.
- **Не делает:** не управляет Docker-контейнерами напрямую в k8s-стенде (в
  dev-режиме воркстейшны — контейнеры, которыми ядро управляет через docker
  CLI); не предоставляет готовых агентов — агенты настраиваются наборами
  (AgentSet) через API; не является оркестратором (NATS, Redis, S3); не управляет
  воркстейшнами из веб-интерфейса (создание — только суперпользователь API);
  не редактирует персонал внутри aga (учётки и роли — в Keycloak).
- **Модель доступа (веб):** участники из SSO видят все проекты и сессии;
  участник создаёт проекты и открывает сессии на готовых воркстейшнах (одна
  активная сессия на воркстейшн); закрыть сессию может только её владелец;
  админ — внешняя сущность (SSO + k8s), в aga его нет.
- **Модель чата (минимальная реализация):** реализация — `main/src/chat.rs`
  (дерево сообщений, единый `parent_id`, шаринг `#share <chat_id>` как
  копия-шар со ссылкой на оригинал, сессия как корневой чат воркстейшна,
  реактивные агенты) + `main/AGENTS.md`. Ещё не сделано: DinD-изоляция
  воркстейшнов.

## Tech Stack
- Ядро: Rust 2021, Tokio, Axum 0.7, reqwest 0.11 (rustls), sqlx 0.7 (SQLite, WAL),
  serde, regex, tracing, thiserror, base64.
- Фронт: React SPA (Vite, mobx-model-ui, Tailwind, shadcn/ui, Storybook),
  собирается в `dist/`, раздаётся nginx.
- Инфра: Kubernetes (minikube), Keycloak, Docker.

## Architecture
```
aga/
├── AGENTS.md           # этот уровень (монорепо)
├── makefile            # единственный интерфейс команд (см. Development)
├── main/               # ядро — Rust-сервис (см. main/AGENTS.md)
│   ├── src/            # модули ядра
│   ├── prompts/        # общая инструкция для агентов
│   ├── config/         # sso-конфиг (runtime) + config.example.yml
│   ├── data/           # runtime-данные (trace.db, work/) — в .gitignore
│   ├── Cargo.toml
│   └── Dockerfile      # образ ядра (kubectl + docker CLI + бинарь)
├── front/              # веб-клиент — React SPA-сервис (см. front/AGENTS.md)
│   ├── src/            # models / api / components / pages / styles
│   ├── stories/        # Storybook
│   └── Dockerfile      # образ nginx (раздаёт dist/)
├── infra/              # .env.example, dev-compose (ядро + фронт + воркстейшны), k8s-стенд, AGENTS.md
└── stories/            # истории разработки
```

## Patterns
- Всё async через Tokio.
- Ошибки в agent-цикле — `Box<dyn Error + Send + Sync>`; в остальных модулях — `thiserror`.
- sqlx с raw-запросами (без ORM), WAL-режим; БД создаётся через
  `create_if_missing(true)`.
- LLM-запросы через OpenAI-compatible API (один LlmClient на все роли).
- Парсинг ответов LLM через regex (команды из markdown-код-блоков).
- Human-in-the-loop через маркер `[ASK_HUMAN]...[/ASK_HUMAN]`.
- Модель чата отделена от трассировки: `main/src/chat.rs` (ChatStore) рядом с
  `main/src/trace.rs` (TraceStore), БД одна.

## Development
- Все команды — через `make` в корне.
- `make init` — создаёт `.env` (из `infra/.env.example`) и `main/config/roles.yaml`
  (из `main/config.example.yml`).
- Локальная разработка: `make build`, `make run` (ядро, cargo в `main/`),
  `make run-front` (vite dev, `front/`), `make test`, `make lint`,
  `make fmt`.
- Dev-стенд без кластера (ядро + веб-клиент + 2 воркстейшна в docker compose):
  `make dev-prepare`, `make dev-up`, `make dev-down`, `make dev-logs`,
  `make dev-ps`, `make dev-reset`, `make dev-verify`. Воркстейшны — контейнеры
  `ws-1`/`ws-2` с пустыми git-репо (проект агент наполняет сам); ядро в
  docker-режиме (`AGA_WS_BACKEND=docker`) переиспользует их; фронт — сервис
  `front` (nginx, `:8081`).
- Тестовый стенд — в k8s (minikube): `make k8s-up`, `make k8s-build`, `make k8s-load`,
  `make k8s-deploy`, `make k8s-wait`, `make k8s-web`, `make k8s-verify`; ручной
  доступ по `*.localhost` (dev/api/auth) — `make k8s-dev` (локальный nginx-прокси
  в Docker, без tunnel) и остановка — `make k8s-dev-stop`.
- Переменные окружения — `.env` в корне (см. `infra/.env.example`).

## Non-Obvious Rules
- Агенты проекта определяет набор (AgentSet) через API (`/agent-sets`,
  привязка к проекту), а не глобальный конфиг ролей.
- Команды выполняются через `sh -c` (dev, без воркстейшна), `kubectl exec`
  в под воркстейшна (Kubernetes) или `docker exec` в контейнер воркстейшна
  (dev, `AGA_WS_BACKEND=docker`). Inline-конструкции (`|`, `>`, `<`, `;`, `&`)
  запрещены на уровне `is_command_allowed`.
- Проект регистрируется git-URL; воркстейшн — под/контейнер `ws-<id>` с
  собственным Docker (DinD) и копией проекта; кластером/контейнерами управляет
  только ядро. Dev-compose поднимает контейнеры заранее — ядро их переиспользует.
- `[ASK_HUMAN]` — единственный протокол human-in-the-loop. При обнаружении
  выполнение задачи останавливается до ответа.
- Модель чата: команды (`#invite`/`#kick`/`#start`/`#end`/`#share`) — это обычные
  сообщения с дополнительной реакцией; реактивные агенты по `@Agent.<имя>`
  сериализуются per-workstation.
- Каждый task создаёт новый Agent (легковесный, без state между задачами).
- В стенде SPA и API разнесены по сервисам: `dev.localhost` → `front/`,
  `api.localhost` → `main/`. Ядро статику не раздаёт.

## Verification
- Сборка и линт ядра: `make build`, `make lint` — без ошибок.
- Тесты ядра: `make test` (cargo test в `main/`).
- Фронт: `make run-front` — страница грузится без ошибок консоли; Storybook
  и unit-тесты строятся без ошибок.
- Интеграционный тест стенда: `make k8s-verify` — ядро, фронт и Keycloak
  поднимаются в кластере (minikube), проверяются воркстейшны-поды, SSO и
  персистентность; локально `make run` отвечает на `/users`, `/chats/:id/messages`.
- Критерий готовности: фреймворк компилируется, ядро запускается и отвечает на
  HTTP, фронт раздаётся отдельно, цикл агента выполняется, трассировка
  сохраняется.

## Dependencies
- Rust (edition 2021), nginx
- LLM API (OpenAI-compatible: Ollama, vLLM, OpenAI, LocalAI)
- Docker (для сборки образов ядра, фронта и воркстейшна; dev-стенд — `docker compose`)
- Kubernetes (`kubectl`) — стенд и воркстейшны как поды; локально — minikube (`make k8s-up`)

## Markdown Style
- Заголовки — `## <Section>`; списки через `-`; Код в блоках с указанием языка.