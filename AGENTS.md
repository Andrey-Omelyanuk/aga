# aga — LLM Agent Framework

## Overview
Монорепо фреймворка для создания и запуска LLM-агентов. Один Rust-бинарь ядра
(`main/`) запускается в двух процессах: HTTP-сервер (`aga` — модель чата, SSO,
трассировка, воркстейшны, публикация событий в Centrifugo) и агент-рантайм
(`aga agent` — подписчик Centrifugo, цикл агента). Плюс веб-клиент (`front/` —
SPA на nginx). Стенд — в Kubernetes (`infra/`). Чат — единственная шина:
агенты не вызывают друг друга напрямую, оркестратора нет; состояние работы —
в чате, процесс агента его не хранит. Инструменты агента — CLI воркстейшна
и MCP-серверы из каталога.

## Boundaries
- **Делает (платформа):** REST API задач агентам и чата; управление жизненным
  циклом агента; валидация команд по белому списку; трассировка в SQLite (WAL);
  human-in-the-loop через инструмент `ask_human`; проекты (git-репозиторий) и наборы
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
  участник создаёт проекты и открывает сессии на готовых пустых воркстейшнах
  (сессия задаёт проект и разворачивает его на станции; одна
  активная сессия на воркстейшн); закрыть сессию может только её владелец;
  админ — внешняя сущность (SSO + k8s), в aga его нет.
- **Модель чата (минимальная реализация):** реализация — `main/src/chat.rs`
  (сообщения с `parent_id`, нити — дочерние чаты от сообщения с заголовком на
  первом сообщении нити, шаринг `#share <chat_id>` как
  копия-шар со ссылкой на оригинал, сессия (Session) — запись с проектом и
  воркстейшном, чат-сессия 1:1 с ней, публикация изменений в Centrifugo)
  + `main/AGENTS.md`. Ещё не сделано: DinD-изоляция
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
│   ├── prompts/        # общая инструкция агентам и протокол инструментов (в бинаре)
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
- Инструменты агента — нативный function calling (`shell`, `ask_human`);
  запасной текстовый режим (по флагу LLM-подключения) — команды из блоков
  ```` ```bash ````, вопрос — маркер `[ASK_HUMAN]...[/ASK_HUMAN]`. MCP-серверы
  (каталог `/mcp-servers`, http или stdio в воркстейшне) дают агенту свои
  инструменты — только в нативном режиме (`main/src/mcp.rs`).
- Модель чата отделена от трассировки: `main/src/chat.rs` (ChatStore) рядом с
  `main/src/trace.rs` (TraceStore), БД одна.

## Development
- Все команды — через `make` в корне.
- `make init` — создаёт `.env` (из `infra/.env.example`) и `main/config/roles.yaml`
  (из `main/config.example.yml`).
- Локальная разработка: `make build`, `make run` (ядро, cargo в `main/`),
  `make run-front` (vite dev, `front/`), `make test`, `make lint`,
  `make fmt`.
- Dev-стенд без кластера (ядро + маленькая LLM + Keycloak + веб-клиент +
  2 воркстейшна в docker compose; SSO включён, как в стенде):
  `make dev-roles`, `make dev-up`, `make dev-down`,
  `make dev-logs`, `make dev-ps`, `make dev-reset`, `make dev-verify`.
  Воркстейшны — контейнеры `ws-1`/`ws-2` с пустыми git-репо в отдельных named
  volumes (проект агент наполняет сам); ядро в docker-режиме
  (`AGA_WS_BACKEND=docker`) переиспользует
  их; маленькая LLM — контейнер `ollama` с моделью до 1B (`qwen3:0.6b`),
  подключение к нему в БД создаёт сид (`make dev-seed`, адрес `ollama:11434/v1`)
  или настраивается вручную на странице «LLM» — из env LLM не читается;
  фронт — сервис `front` (vite, `:8081`); прокси `*.localhost` на `:80`
  (`dev.localhost` → front, `api.localhost` → core, `auth.localhost` → Keycloak)
  — как в k8s-стенде. `make dev-roles` генерирует `infra/dev-roles.yaml`
  (roles.yaml со включённым SSO для dev-Keycloak).
- Тестовый стенд — в k8s (minikube): `make k8s-up`, `make k8s-build`, `make k8s-load`,
  `make k8s-deploy`, `make k8s-wait`, `make k8s-web`, `make k8s-verify`; ручной
  доступ по `*.localhost` (dev/api/auth) — `make k8s-dev` (локальный nginx-прокси
  в Docker, без tunnel) и остановка — `make k8s-dev-stop`.
- Тестовый набор в БД ядра (сброс + детерминированная фикстура): `make dev-seed`
  (dev-стенд) / `make k8s-seed` (кластер); локально — `cargo run -- seed` в `main/`
  (см. `main/src/seed.rs`). Участники набора — учётки Keycloak: `alice`/`alice-pass`
  (participant) и `bob`/`bob-pass` (admin) с фиксированными `sso_subject`
  (см. `infra/k8s/core/keycloak-realm.json`) — после сида вход через SSO находит
  этих юзеров, и сессии/чаты принадлежат реальным учёткам.
- Переменные окружения — `.env` в корне (см. `infra/.env.example`).

## Non-Obvious Rules
- Агенты проекта определяет набор (AgentSet) через API (`/agent-sets`,
  привязка к проекту), а не глобальный конфиг ролей.
- Команды выполняются через `sh -c` (dev, без воркстейшна), `kubectl exec`
  в под воркстейшна (Kubernetes) или `docker exec` в контейнер воркстейшна
  (dev, `AGA_WS_BACKEND=docker`). Пайпы, перенаправления и цепочки разрешены;
  список инструментов агента проверяется по каждой команде скрипта. Граница
  территории — объявленное владение: читают все, пишет только владелец своей
  папки — команда идёт в песочнице станции (mount namespace: проект только на
  чтение, своя папка на запись), запись вне зоны падает сразу. `.git` — у
  корневого агента, общие артефакты сборки — у владельца их папки; остальные
  просят их в чате.
- Проект регистрируется git-URL; воркстейшн — под/контейнер `ws-<id>` с
  собственным Docker (DinD) и копией проекта; кластером/контейнерами управляет
  только ядро. Dev-compose поднимает контейнеры заранее — ядро их переиспользует.
- `ask_human` (`[ASK_HUMAN]` в текстовом режиме) — единственный протокол human-in-the-loop, живёт в чате: текст вопроса
  публикуется сообщением агента, задача ждёт (`waiting_human`); ответ — сообщение с
  `parent_id` на вопрос от любого участника, оно закрывает запрос и запускает продолжение.
- Модель чата: команды (`#invite`/`#kick`/`#end`/`#share`) — это обычные
  сообщения с дополнительной реакцией.
- Чат ничего не знает про агентов: HTTP-сервер пишет сообщения и публикует
  события в Centrifugo (каналы чата, пользователя-автора и общий). Агенты —
  отдельный процесс того же бинаря (`aga agent`), подписанный на общий канал.
  Агент привязан к пользователю X (`agents.listen_user_id`, страница набора) и
  обслуживает его — отвечает от его имени, когда X спрашивают: упоминание
  `@X` в чужом сообщении, любое чужое сообщение в нити, начатой X, ответ на
  вопрос агента. Агенты общаются через чат как обычные участники и не знают,
  кто стоит за другими (человек или агент). Переписки ограничены нитями:
  глубина и число сообщений в нити — env `AGA_MAX_THREAD_DEPTH` (10) и
  `AGA_MAX_THREAD_MESSAGES` (100), нужны и ядру, и агент-рантайму.
- Нити в чате — дочерние чаты от конкретного сообщения (`start_message_id`),
  заголовок нити живёт на её первом сообщении (`messages.title`); в списке
  чатов нитей нет, они свёрнуты у сообщения в родителе; из нити сообщение
  можно отправить в родительский чат (`thread_of_id` на сообщение-источник).
  `#start` больше нить не создаёт — только действие у сообщения.
- Каждый task создаёт новый Agent (легковесный, без state между задачами).
- LLM агентов — подключения в БД (страница «LLM»): у каждого url, ключ и модель;
  одно подключение дефолтное, к нему ходят агенты без своего. LLM из env не
  читается вовсе (LLM_API_URL/LLM_API_KEY/LLM_MODEL и bootstrap нет) — всё
  настраивается в БД.
- В стенде SPA и API разнесены по сервисам: `dev.localhost` → `front/`,
  `api.localhost` → `main/`. Ядро статику не раздаёт.

## Verification
- Сборка и линт ядра: `make build`, `make lint` — без ошибок.
- Тесты ядра: `make test` (cargo test в `main/`).
- Фронт: `make run-front` — страница грузится без ошибок консоли; Storybook
  и unit-тесты строятся без ошибок.
- E2E: `make dev-e2e` (dev-стенд) и `make k8s-verify` (кластер); что
  покрывает e2e и почему — `infra/E2E.md`.
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