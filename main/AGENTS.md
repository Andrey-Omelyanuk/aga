# main — Ядро (Rust-сервис)

## Overview
Реализация ядра фреймворка: HTTP REST API (модель чата, трассировка, проекты,
наборы агентов, SSO) и — вторым процессом того же бинаря (`aga agent`) —
агент-рантайм: подписчик Centrifugo, цикл LLM → parse → exec → trace. Один
бинарь, режимы: `aga` (сервер), `aga agent` (рантайм), `aga seed`. Воркстейшны —
поды Kubernetes (стенд) или контейнеры Docker (dev). Чистый бэкенд — UI в этом
уровне нет.

## Boundaries
- **Делает:** HTTP-сервер: обрабатывает запросы (Axum), хранит/отдаёт трассировку,
  управляет проектами и наборами агентов (AgentSet) в SQLite, поднимает
  воркстейшны-поды через kubectl (стенд) или контейнеры через docker CLI (dev,
  `AGA_WS_BACKEND=docker`), пишет сообщения чата и публикует события в Centrifugo,
  включает/выключает SSO. Агент-рантайм (`aga agent`): слушает общий канал
  Centrifugo; когда спрашивают пользователя, которого обслуживает агент
  (`@имя`, нить, начатая этим пользователем, ответ на вопрос агента), запускает
  цикл агента (контекст из чата, LLM, инструменты, выполнение команд в
  песочнице воркстейшна) и пишет ответ от имени этого пользователя.
- **Не делает:** HTTP-сервер не запускает агентов (только публикует события);
  чат не знает про агентов; не раздаёт веб-клиент (это `front/`); не управляет
  Docker напрямую в k8s-стенде (в dev-режиме воркстейшны — контейнеры, которыми
  ядро управляет через docker CLI); не предоставляет CLI.

## Tech Stack
- Rust 2021, Axum 0.7, Tokio 1, sqlx 0.7 (SQLite), reqwest 0.11, regex, serde,
  chrono, uuid, tracing, thiserror. kubectl и docker CLI в образе (стенд — кластер,
  dev — контейнеры воркстейшнов).

## Architecture
```
main/
├── AGENTS.md            # этот уровень
├── Cargo.toml           # пакет aga
├── Dockerfile           # образ ядра: kubectl + docker CLI + бинарь (без static/)
├── src/                 # модули ядра
├── prompts/             # общая инструкция агентам + протокол инструментов по режиму (встраиваются в бинарь)
├── config/              # sso-конфиг (runtime, gitignored) + config.example.yml
└── data/                # runtime: trace.db, work/ (gitignored)
```
```
src/
├── main.rs       # Точка входа: env, RunMode (aga / aga agent / aga seed); сервер: init Config, stores, Cluster, JwtVerifier→AppState
├── config.rs     # Config, RoleConfig, LlmConfig, SsoConfig (JWKS/authorize/token/клиент), CentrifugeConfig — загрузка из YAML
├── git_changes.rs# Изменения проекта воркстейшна от начала ветки (git diff, страница Changes)
├── mcp.rs        # McpClient — MCP (JSON-RPC): транспорты Streamable HTTP и stdio (процесс через executor, `exec -i`), initialize / tools/list / tools/call
├── llm.rs        # LlmClient — OpenAI-compatible HTTP-клиент: сообщения с ролями, нативные tools/tool_calls
├── agent.rs      # Agent — цикл LLM → действия (tool_calls или блоки ```bash) → exec → trace; инструменты shell/ask_human; проверка списка инструментов по командам скрипта; итог AgentOutcome (Answer|Question); наблюдатель шагов; Executor (Sh / KubectlExec / DockerExec)
├── trace.rs      # TraceStore — SQLite: tasks, trace_entries, human_requests, projects, agent_sets, agents (с listen_user_id)
├── chat.rs       # ChatStore — модель чата: chat_users, chats, sessions, participants, messages (origin, kind, tool_call), artifacts, workstations; лимиты нитей; разбор @упоминаний
├── context.rs    # Контекст агента из чата: свои сообщения — assistant (шаги — tool_calls), чужие — user с именем; шапка нити; бюджет символов
├── cluster.rs    # Cluster — воркстейшн: рендер манифеста пода (k8s) / docker-run (dev), wait_ready, delete
├── workstation.rs# executor_for_workstation: воркстейшн → kubectl exec / docker exec
├── runtime.rs    # Агент-рантайм (`aga agent`): подписка на общий канал (SSE Centrifugo), триггеры (@упоминание, нить пользователя, ответ на вопрос), асинхронный диспетчер (FIFO «станция+агент», семафор, дедуп), цикл Agent, ответы от имени обслуживаемого пользователя, инструмент start_thread, публикация шагов работы в чат
├── centrifuge.rs # CentrifugeClient: connection-JWT (веб) + subscriber_jwt (рантайм) + publish в каналы chat:/user:/common + SSE-поток
├── seed.rs       # Тестовый набор (aga seed): очистка БД + детерминированная фикстура
├── auth.rs       # JwtVerifier (JWKS, RS256) + resolve_user: участник/админ из Keycloak или аноним-супер
└── server.rs     # Axum Router + хендлеры API + /auth/login, /auth/callback, /auth/refresh, /auth/logout, /users/me, /connection-jwt/; события в Centrifugo
```

## Patterns
- **Изоляция модулей:** `agent` зависит от `config`, `llm`, `trace`; `chat`/`workstation`/`runtime`/`auth` зависят друг от друга и от `trace`; `server` зависит от всех кроме `runtime`; `main` инициализирует и связывает. `server` (HTTP) и `runtime` (`aga agent`) — два процесса, не знают друг о друге: связаны только через БД и события Centrifugo.
- **Состояние:** `AppState { config, trace_store, chat_store, cluster, centrifuge, sso_verifier, front_url }` передаётся через Axum State; агентов сервер не запускает.
- **Ошибки:** `thiserror` для типизированных ошибок; `Box<dyn Error + Send + Sync>` в agent-цикле (потенциально разные типы ошибок).
- **LLM-клиент:** один `LlmClient` на все роли; url, ключ и модель — из
  подключения агента или дефолтной LLM, температура 0.7.
- **SQLite:** CREATE TABLE IF NOT EXISTS при старте; WAL-режим; raw-запросы. Обязательно `SqliteConnectOptions::create_if_missing(true)` — `SqlitePool::connect` файл НЕ создаёт.
- **Инструменты агента — нативный function calling, текстовый режим — запасной:**
  режим задаёт LLM-подключение (`llm_connections.native_tools`, по умолчанию 1).
  Нативный: в запрос уходят `tools` (`agent::tool_specs`: `shell(command)`,
  `ask_human(question)`), действия — `tool_calls` ответа, результат команды
  возвращается модели сообщением `role=tool` на id вызова. Текстовый (для
  моделей без function calling): каждый блок ```` ```bash ````/```` ```sh ```` —
  один скрипт целиком, вывод возвращается user-сообщением `$ <команда>\n<вывод>`,
  вопрос — маркер `[ASK_HUMAN]...[/ASK_HUMAN]`; блок без языка не исполняется.
  Ответ без действий — финальный. Системный промпт = правила агента (описание +
  скиллы) + `prompts/all-agents.md` + протокол режима (`prompts/tools-native.md`
  / `tools-text.md`) + список разрешённых инструментов; файлы встраиваются в
  бинарь (`include_str!`). Провал команды (код выхода ≠ 0) и отказ по списку
  инструментов — не ошибка прогона: текст уходит модели результатом (код выхода
  дописывается строкой `[exit code N]`). Исчерпан `max_iterations` — в чат уходит
  «Лимит шагов исчерпан, задача не завершена», статус `max_iterations_reached`.
  MCP-инструменты добавляются к `tool_specs` на прогон (см. ниже).
- **MCP-серверы — каталог, агенту даются по имени:** `mcp_servers` (имя,
  `transport` `http`|`stdio`, `url`, `command`, `api_key`) настраиваются через
  `/mcp-servers` (имя уникально — 409; у http нужен url, у stdio — команда —
  иначе 400); привязка к агенту — `agent_mcp_servers` по id (как скиллы:
  переименование не рвёт, удаление отвязывает каскадом), в API набора —
  `agents[].mcp` списком имён (неизвестные пропускаются). На каждый прогон
  (только нативный режим) агент подключается к своим серверам
  (`mcp::McpClient`: `initialize` → `notifications/initialized` →
  `tools/list` с пагинацией), их инструменты уходят модели как
  `<сервер>__<инструмент>` (`mcp::llm_tool_name`, только `[A-Za-z0-9_-]`, ≤64;
  описание с префиксом `[MCP <сервер>]`, схема — `inputSchema`). Вызов —
  `tools/call`; текстовые блоки результата отдаются модели как есть, `isError`
  — с префиксом «Ошибка инструмента», сбой транспорта — «Ошибка MCP»
  (прогон не падает). Шаг в чат: тело `mcp <сервер>.<инструмент> <аргументы>`,
  скрытая часть — результат. http — Streamable HTTP: POST JSON-RPC, ответ
  JSON или SSE, сессия `Mcp-Session-Id` из ответа на initialize, ключ —
  `Authorization: Bearer`, после initialize — `MCP-Protocol-Version`. stdio —
  процесс `command`, запущенный executor'ом агента (`docker exec -i` /
  `kubectl exec -i` в воркстейшн, cwd — папка агента; локально `sh -c`),
  сообщения построчно; посторонние строки stdout пропускаются, запросы
  сервера к клиенту получают «method not supported»; процесс живёт прогон
  (kill_on_drop; закрытие stdin завершает сервер). stdio-сервер запускается
  в той же песочнице владения, что и команды агента (`Agent::process`).
  Недоступный
  сервер не валит прогон: ошибка — в трассу и пояснением в чат. Проверка
  http-сервера из UI — `GET /mcp-servers/:id/tools` (502 с текстом ошибки;
  stdio — 400: запускается только в воркстейшне). В образе воркстейшна нет
  node/python — stdio-серверы ставит проект или запускает через docker
  (`docker run -i --rm <образ>`). Таймауты: подключение 30 с, вызов 300 с.
- **Human-in-the-loop — через чат:** вызов `ask_human` (или `[ASK_HUMAN]` в текстовом
  режиме) → `human_requests` (pending) + задача `waiting_human`, а текст вопроса
  публикуется в чат сообщением с `origin='agent'` (к запросу привязаны `chat_id`,
  `question_message_id`, `agent_name`). Ответ — обычное сообщение с `parent_id` на
  сообщение-вопрос от любого участника: рантайм в `route` находит pending-запрос по
  `question_message_id` (тот же чат), закрывает его (`answered`, задача `answered`) и
  запускает продолжение того же агента новой задачей (без восстановления истории;
  контекст — чат). Ход работы: после каждой команды в чат публикуется сообщение —
  команда в `body`, обрезанный вывод (`truncate_output`) в `hidden` (в LLM не входит),
  `origin='agent'`; текст модели между вызовами (пояснение) — отдельным сообщением.
  Отдельных HTTP-роутов для human-запросов нет.
- **Centrifugo (события чата):** `CentrifugeClient` (`centrifuge.rs`) —
  публикация через HTTP API (`POST /api`, method `publish`) в три канала: общий
  (`common`, конфиг), `chat:<chat_id>` и `user:<author_id>`; connection-JWT
  (HS256) вебу — на `/connection-jwt/` (право на общий канал в claims);
  агент-рантайму ядро токен не выдаёт — доверенный процесс подписывает
  `subscriber_jwt` сам тем же HMAC-секретом из конфига (серверная подписка
  `subscriptions`). Клиент бывает `disabled()`-заглушкой (центрифуго не настроен).
- **Сборка/запуск:** CWD для cargo-команд — `main/` (makefile делает `cd main`); пути конфига из env (`.env`) относительные оттуда.

## Non-Obvious Rules
- **TraceStore шире трассировки:** в нём же живут `projects`, `agent_sets` и
  `agents` (набор агентов на проект). ChatStore — отдельный модуль с таблицами
  модели чата, БД одна (тот же файл).
- **Проект = git-URL:** воркстейшн-под клонирует репозиторий; `compose_path` из старых БД мигрируется в `git_url` (см. `migrate_projects_git_url`).
- **Воркстейшн = под/контейнер:** в стенде `ws-<id>` — под в неймспейсе кластера,
  команды агента — `kubectl exec`; в dev (`AGA_WS_BACKEND=docker`) — контейнер
  `ws-<id>`, команды — `docker exec`. Pod-манифест рендерит `cluster.rs` из шаблона
  (встроенный дефолт совпадает с `infra/k8s/workstation-pod.yaml`); под
  привилегированный (DinD) и без доступа к k8s API (`automountServiceAccountToken: false`).
  В docker-режиме `create_workstation` переиспользует уже существующий контейнер
  `ws-<id>` (compose-стенд поднимает их заранее), иначе запускает `docker run`
  (локальный путь в git_url — бинд-маунт, git-URL — клонирование как в k8s).
- **Жизненный цикл воркстейшна:** при подъёме в ws может монтироваться именованный
  k8s-Secret (секреты для сторонних CLI, `secret` у воркстейшна) — хранится только
  имя, в pod рендерится volume+volumeMount; docker-бэкенд секрет игнорирует.
  SSH-ключ aga (git+ssh-доступ воркстейшнов) задаёт админ в env
  `AGA_SSH_PRIVATE_KEY` (OpenSSH-формат); ядро само проставляет
  `secret=aga-ssh` (если в запросе не задан явный): в k8s создаёт Secret
  `kubectl apply` (`ensure_ssh_secret`) и монтирует в под, в docker —
  инжектит в `/home/aga/.ssh` контейнера после создания (`inject_ssh_key`):
  команды агента в docker идут от uid 1000, а в образе воркстейшна uid 1000 —
  пользователь `aga` с home `/home/aga` (иначе ssh агента ключ бы не увидел).
  Entrypoint воркстейшна раскладывает ключ из `/etc/secrets/*/id_ed25519` в
  `~/.ssh` до git-клона. Публичный ключ отдаётся на `GET /settings/ssh-key`
  (см. `ssh_key.rs`).
  Станция создаётся ПУСТОЙ (`POST /workstations` без проекта): `create_pod` со
  пустым `GIT_URL`, entrypoint инициализирует пустое git-репо в `/work/project`.
  Код проекта разворачивается на станции открытием сессии — `POST
  /workstations/:id/session` с `project_id`: ядро переписывает `/work/project`
  кодом проекта через exec (см. `ws_ops::replace_project`; clone идёт во
  временный каталог `/work/project.new.<pid>`, подмена — только при успехе;
  в dev (docker) перед клоном инжектит SSH-ключ aga в контейнер — станция
  переиспользуется). В dev-стенде git_url может быть плейсхолдером — сбой клона
  не откатывает сессию, только логируется (проект агент наполняет сам).
  `/work` в образе воркстейшна принадлежит uid 1000 (entrypoint
  `chown 1000:1000 /work`) — временный каталог клона рядом с проектом должен
  создавать именно агент. Отпустить станцию
  (`POST /workstations/:id/release`) — только без открытой сессии: `/work/project`
  очищается до пустого git-репо (`ws_ops::release_workspace`); в БД отпускать
  нечего — свобода станции = нет активной сессии.
  Упавший ws админ отмечает `POST /workstations/:id/down` (state=`down`; сессии на
  `down`-станции открыть нельзя). Восстановление после падения — ручное: открытие
  сессии на свободном ws определяет незакрытую сессию на `down`-станции того же
  проекта (`interrupted_session_for_project`), помечает новую сессию как
  продолжение (`continues_session_id`) и восстанавливает файлы из её ветки
  (`ws_ops::restore_workspace`, checkout `ws-<id>` — после разворота кода
  проекта сессии).
- **Воркстейшнами из интерфейса не управляют:** создание/удаление — только суперпользователь (админ внешний, поднимает станции через k8s); участники видят список и состояние (`GET /workstations`), но `POST`/`DELETE` им запрещены (403). Сессию открывают на готовом свободном воркстейшне (`POST /workstations/:id/session` с `project_id`), один воркстейшн — одна активная сессия.
- **Сессия (Session) — отдельная сущность (`sessions`):** связывает корневой чат (`chat_id`, 1:1), проект (`project_id`), воркстейшн (`workstation_id`) и владельца (`owner_id`); здесь же `result_id`, `continues_session_id`, `closed_at`. Станция обслуживает сессии и своего проекта не хранит — `workstations.project_id` удалён (миграция переносит привязки старых БД в `sessions`), в API `workstations.project_id` и `chats.project_id`/`workstation_id` — производные от активной сессии (read-модель через LEFT JOIN sessions, 0 = свободен/не сессия). Сессия открывается только на `ready`-воркстейшне без активной сессии (`open_workstation_session`); закрытие (`close_workstation_session`, только владелец или суперпользователь локального режима) освобождает станцию (`current_session_id = NULL`, `closed_at`). Проект чата — `project_id_for_chat` (корень → `sessions.project_id`, без транзита через станцию).
- **Видимость открытая:** «все участники видят все проекты и сессии» — `list_chats_for_user` не фильтрует по участнику, `can_read` всегда true. Персональной видимости нет.
- **SSO:** `JwtVerifier` проверяет подпись RS256 по JWKS; роли из realm-ролей Keycloak (`participant`/`admin`). Вход веб-клиента — `/auth/login` (редирект в Keycloak), `/auth/callback` (обмен code→token, веб-клиенту токены возвращаются фрагментом URL `#token=...&refresh=...`), молчаливое обновление токена — `/auth/refresh` (ядро меняет refresh-токен на свежую пару у Keycloak, без браузерных кук — см. `me.refresh()` на фронте), выход — `/auth/logout` (сброс cookie `aga_token`, при заданном `end_session_url` — редирект на end-session Keycloak). Имя учётки — `preferred_username` из JWT (логин), не UUID `sub`. Текущий пользователь — `GET /users/me`. API принимает токен из `Authorization: Bearer` или cookie `aga_token` (см. `auth.rs`). Без SSO — аноним-суперпользователь.
- **События чата — каналы и подписчики:** каждое сообщение публикуется веером в `common`, `chat:<chat_id>` и `user:<author_id>` (payload: `type=message`, chat_id, message_id, author_id); жизненные события (`chat_created`, `session_opened`, `session_closed`) — в `common`. Вебу `GET /connection-jwt/` подписывает connection-JWT с `sub` = `chat_users.id` (не SSO-`sub`) и правом на `common` в claims (`channels`) — отдельного channel-токена нет, гейт — сам факт входа. Публикация best-effort: сбой центрифуго не ломает отправку, только логируется. Конфиг — блок `centrifuge:` в roles.yaml (api_url, api_key, secret, channel); не задан — клиент-заглушка (`disabled()`), `/connection-jwt/` отдаёт 404, чат без автообновления.
- **Команды агента — `sh -c` как есть, без запрета спецсимволов:** пайпы,
  перенаправления, цепочки и heredoc разрешены (образ воркстейшна — alpine, bash
  нет, поэтому POSIX sh). Список инструментов агента (`agents.tools`) проверяется
  по первому слову каждой простой команды скрипта (`agent::command_allowed`:
  разбор кавычек, `|`/`&&`/`;`/`(`/`$(`, пропуск присваиваний, целей
  перенаправлений, тел heredoc и комментариев); встроенные команды sh (`cd`,
  `echo`, `test`...) разрешены всегда. Это защита от ошибок модели, а не граница
  безопасности: аргументы (`find -exec`, `xargs`, `sh -c`) не разбираются,
  граница — изолированный воркстейшн и песочница владения (см. «Территория»).
- **Agent per task / per сообщение-триггер:** Agent создаётся на каждый запуск; в памяти процесса ничего не держится — состояние работы живёт в чате и собирается в контекст при каждом запуске (`context.rs`).
- **Контекст агента — из чата (`context::build_context`):** агент — участник
  чата наравне с другими и не знает, кто стоит за остальными участниками
  (человек или агент). Свои сообщения (автор — обслуживаемый пользователь) —
  `assistant`; свои шаги (`kind='step'`) — вызовы инструментов
  (`tool_calls` из `messages.tool_call`, вывод — из `hidden` шага; у шагов
  старше `STEPS_WITH_OUTPUT`=5 — только вызов; в текстовом режиме — блок
  ```` ```bash ```` и вывод user-сообщением). Все чужие сообщения — `user` вида
  `имя: текст`, без скрытой части (чужие шаги — тоже просто реплика с
  командой). Берутся сообщения чата до сообщения-триггера включительно; оно
  последнее и помечено (`имя → тебе: …`). В нити впереди — шапка: заголовок
  нити и исходное сообщение из родительского чата. Объём — `BUDGET_CHARS`
  (24 000 символов): самые свежие, старшее отрезается пометкой «(более ранние
  сообщения чата опущены)». Скрытая часть сообщений (сокращения) агентам не
  уходит — это заметка людям.
- **Модель параллелизма рантайма:** события Centrifugo диспатчатся асинхронно (`tokio::spawn`), SSE-стрим не блокируется запусками. Слои сериализации: семафор общего параллелизма (`MAX_CONCURRENT_AGENT_RUNS` = 4); FIFO-мьютекс на пару «станция+агент» — один агент никогда не работает в двух экземплярах, сколько бы раз его ни вызвали в сессии; разные агенты одной станции работают параллельно — и LLM-шаги, и команды: каждая команда идёт в песочнице владения агента, пишет только в свою зону, а чужие изменения видит сразу (общих мьютексов станции нет). Дедуп: повторная доставка того же `message_id` не запускает агента (`RuntimeState::seen`). Worktree-изоляции нет намеренно: другая работа — отдельная сессия и отдельный workspace.
- **Команды чата** (`#invite`/`#kick`/`#end`/`#share`) — это обычные сообщения с дополнительной реакцией; разбираются в `chat::parse_command`, исполняются в server.rs. `#start` командой не является: нити начинаются только действием у сообщения (`POST /messages/:id/thread`).
- **Нити — дочерние чаты от сообщения:** `start_thread` создаёт чат-нить с
  `start_message_id` (сообщение, от которого началась нить) и первым
  сообщением с `title` — заголовком нити (у самого чата-нити `title` не
  хранится). От одного сообщения можно начать сколько угодно нитей.
  `list_chats_for_user` отдаёт только корневые чаты (нити в список не
  попадают — они свёрнуты у сообщения в родителе); деталь `GET /chats/:id`
  несёт `threads` рекурсивно (`chat`+`messages`+`participants`+`threads`).
  `post_to_parent` кладёт в родительский чат копию сообщения нити со ссылкой
  `thread_of_id` на сообщение-источник; оригинал остаётся в нити. Первое
  сообщение нити отвечает на источник (`parent_id`). Нить можно начать от
  любого сообщения внутри другой нити; глубина — до `AGA_MAX_THREAD_DEPTH`
  уровней (по умолчанию 10, `chat::max_chat_level`), в одной нити — не больше
  `AGA_MAX_THREAD_MESSAGES` сообщений (по умолчанию 100, включая шаги работы;
  сверх — отказ, HTTP 400; `chat::max_thread_messages`). Env читается один раз
  при старте процесса; пусто/неверно/≤0 — дефолт; задаётся и ядру, и
  агент-рантайму (`.env`, dev-compose, k8s — configmap `aga-env`). Корневые
  чаты не ограничены.
  Это предохранитель от бесконечных переписок участников (в том числе агентов).
- **Тип сообщения:** `messages.kind` — `text` или `step` (шаг работы агента:
  тело — команда/вызов, `hidden` — вывод, `messages.tool_call` — вызов JSON-ом
  `{"name","arguments"}`; `ChatStore::send_step`). Шаги никого не вызывают; по
  ним контекст восстанавливает прошлые вызовы агента.
- **AgentSet заменяет роли:** агентов проекта определяет прикреплённый набор
  (`agent_sets`/`agents`/`project_agent_set`), а не глобальные роли. Набор
  настраивается через API (`/agent-sets`, прикрепляется к проекту
  (`/projects/:id/agent-set`); у проекта один набор (повторная привязка меняет
  его), один набор — на многие проекты (каскад ON DELETE). Агенты набора —
  дерево по иерархии папок проекта: у агента есть `parent_id` на родителя.
- **Способности агента — каталог с единственным содержимым и историей,
  инструменты — список без версий:** скиллы живут в общем каталоге
  (`capabilities` с `kind='skill'` и полем `content`), агент получает
  их по имени и всегда использует последнее содержимое — версий и фиксации нет
  (`agent_capabilities` хранит только имя). Каждая правка каталога (создание,
  изменение содержимого, переименование, удаление) пишется в отдельную
  сущность `capability_history` — «кто (имя фиксируется в момент правки, чтобы
  историю не исказили переименования учёток), когда и что сделал». Запись хранит
  содержимое после действия (`content`) — по соседним записям страница истории
  строит дифф; старые записи (до миграции) имеют пустое содержимое. Удаление —
  мягкое (`deleted=1`): запись остаётся в списке «Удалённые» (`?deleted=1`) и
  её историю можно открыть; имя уникально в пределах вида (`UNIQUE(kind, name)`
  — удалённая запись имя занимает). Занятое имя при создании/переименовании —
  `409 CONFLICT` (`TraceStore::capability_name_taken`), переименование
  выполняется только если имя реально изменилось (правка содержимого с тем же
  именем не пишет в историю «переименовал»). Инструменты (`agents.tools`) — отдельный список
  исполняемого в консоли воркстейшна, версий у них нет.   Промпт агента =
  его `description` + раскрытые данные скиллы (см.
  `TraceStore::agent_prompt`).
- **Сокращения — второй вид каталога (`kind='shortcut'`), скрытая часть
  сообщения — снимок:** сокращение живёт в том же `capabilities` с той же
  историей (`capability_history`), что скиллы, но агентам не даётся —
  это глобальный перечень текста. При отправке сообщения (`send_message` и
  первое сообщение нити `start_thread`) слово `/имя` добавляет `content`
  сокращения в `messages.hidden` (одно имя — один раз, порядок появления;
  неизвестные имена пропускаются). Скрытая часть пишется снимком на момент
  отправки: правка/удаление сокращения задним числом не меняет уже
  отправленные. Задать `hidden` вводом нельзя (в `SendMessageRequest` его нет) и
  после отправки он не меняется. В контекст агентов (`build_context`) `hidden`
  не входит — в LLM идут только `title`/`body`. Имя сокращения — непустое, без
  пробелов (слово `/имя` не набрать иначе), уникально среди сокращений;
  нарушение — `400`, занятое имя — `409`.
- **Территория агента = его узел в дереве набора:** папка узла — имя агента как
  путь проекта, кроме корня дерева: у корня папки-родителя нет, его территория —
  корень проекта (папка `""`), а имя корня — метка слоя (например, `ui`
  библиотеки), папкой в репозитории оно не является (`scope::territory_for_list`).
  Территория заканчивается перед папками ближайших наследников.
  Читают все, пишет только владелец: граница — объявленное владение, без
  исключений. Enforcement — песочница владения (`scope::sandbox_command`,
  только при воркстейшне): каждая команда агента и его stdio MCP-сервер
  выполняются в отдельном mount namespace станции (`unshare -m`): проект
  смонтирован только на чтение, папка агента — на запись, папки наследников —
  снова только на чтение; запись вне зоны сразу падает с «Read-only file
  system». Песочницу строит root (`docker exec -u 0:0`, в k8s `kubectl exec` и
  так от root привилегированного пода), сама команда — от пользователя станции
  `aga` (uid 1000, `su`); недостающие папки зоны создаются от него же; не
  встала граница — команда не выполняется (`set -e`). Монтирования видны только
  внутри команды; чужие изменения видны сразу (файлы те же). `.git` лежит в
  корне — коммитит владелец корня, остальные просят его в чате; общие
  артефакты сборки (`target/`, `node_modules`) — у владельца их папки; общих
  папок на запись нет. Docker станции (DinD) — общий и границей не закрыт.
  Живая проверка — `cargo test -- --ignored sandbox_in_workstation` (нужны
  docker и образ `aga-workstation:dev`). Команды исполняются с cwd = папка
  агента, у корня — `/work/project`. «Команды чата» (`#invite`/`#kick`/`#end`/`#share`) — сообщения, в каталог
  не входят.
- **Правка набора — PATCH `/agent-sets/:id`:** состав (имя, агенты с их
  инструментами, данными скиллами и деревом) заменяется целиком;
  старые агенты удаляются каскадно.
- **LLM — выбранное подключение и дефолтная LLM, env-дефолта нет:** подключения
  к LLM (`llm_connections`: имя, url API, ключ доступа, модель, режим
  инструментов `native_tools`) живут в БД и
  настраиваются через API (`/llms`). Агент набора ссылается на подключение
  (`agents.llm_id`); своей модели и температуры у агента больше нет (старые
  столбцы model/temperature остаются в старых БД, но не используются) — модель
  живёт в подключении, температура 0.7. url, ключ и модель агента берутся из
  подключения в момент запуска (`TraceStore::llm_config_for`); без подключения
  (или при ссылке на удалённое) — из дефолтной LLM: одно подключение отмечается
  дефолтным (`is_default`, уникальный частичный индекс — дефолт один), выбор
  меняется на странице «LLM» (`POST /settings/llm-default`, `null` — снять).
  Ни подключения, ни дефолта нет — конфиг без url, запуск агента не проходит
  (LlmClient: «LLM не настроена»). Правка подключения меняет url/ключ/модель
  сразу у всех агентов, что на него ссылаются; удаление не ломает агентов —
  они падают на дефолтную LLM (на новых БД каскад `ON DELETE SET NULL`, на
  старых ссылка просто не находится); удаление дефолтного сбрасывает дефолт.
  Ключ наружу отдаётся как есть, маскировки нет. LLM из env не читается
  вовсе (LLM_API_URL/LLM_API_KEY/LLM_MODEL и bootstrap нет): подключение в
  dev-стенде создаёт сид или админ вручную на странице «LLM».
- **Агент-рантайм — отдельный процесс (`aga agent`), не HTTP-путь:** сервер чата
  про агентов не знает. Агент привязан к пользователю чата X
  (`agents.listen_user_id`) и обслуживает его: отвечает от имени X
  (`messages.origin='agent'` — служебная пометка, агентам не видна).
  Рантайм подписан на общий канал (`common`: туда публикуется каждое сообщение
  любого чата и нити; серверная подписка из `subscriber_jwt`; без
  обслуживаемых пользователей слушать нечего). Триггеры — в чатах сессии
  проекта, к которому прикреплён набор агента:
  - чужое сообщение упоминает `@X` (`chat::mentioned_names`; имя агента
    триггером не является — только имя пользователя);
  - любое чужое сообщение в нити, которую начал X (`chats.created_by_id`);
  - ответ (`parent_id`) на pending-вопрос агента в том же чате — от любого
    участника (`route_answer`): запрос закрывается, продолжение — от имени X.
  Своё сообщение X своего агента не вызывает; шаги (`kind='step'`) не
  вызывают никого; одно сообщение может вызвать нескольких агентов (каждый
  отвечает от своего пользователя). Сообщения агентов вызывают других агентов
  так же, как сообщения людей, — переписки ограничены лимитами нитей
  (глубина и число сообщений); в корневом чате лимита нет. Инструмент
  `start_thread` (нативный режим) начинает нить от сообщения-триггера от имени
  X — дальше ответы в ней снова вызывают агента. Centrifugo — обязательная
  зависимость: без него агенты молчат (чат работает).
- **Тестовый набор (`aga seed`):** восстанавливает в БД детерминированную
  фикстуру (юзеры, каталог способностей — включая сокращения `review` и
  `deploy-check`; у сообщения задачи в сессии `hidden` заполнен по `/review`,
  наборы агентов `dev-team` и `ui-kit`,
  проекты — в т.ч. UI-библиотеку `git@github.com:Andrey-Omelyanuk/mobx-model-ui.git`
  с набором `ui-kit` (скиллы `ui-review`/`git-workflow`, тулы
  `cat`/`ls`/`grep`/`find`), воркстейшны ws-1/ws-2, сессия с тредом и артефактом,
  общий чат с share, выполненная задача с trace-записями + pending human-запрос).
  Семантика —
  полный сброс: все таблицы очищаются (`clear_all` в TraceStore/ChatStore) и
  `sqlite_sequence` сбрасывается — ID стабильны между запусками. Seed создаёт
  и дефолтное подключение к LLM (`ollama-local`, адрес `http://ollama:11434/v1`,
  модель `qwen3:0.6b` — контейнер ollama dev-стенда; локально `cargo run -- seed`
  для host-ollama адрес правится на странице «LLM»). Запуск:
  `make dev-seed` (контейнер aga-core) / `make k8s-seed` (кластер); локально —
  `cargo run -- seed` в `main/`. `roles.yaml` для сида не нужен — подкоманда
  отрабатывает до загрузки конфига. Участники `alice`/`bob` — учётки Keycloak
  (фиксированные `sso_subject` из `infra/k8s/core/keycloak-realm.json`, пароли
  `alice-pass`/`bob-pass`) — вход через SSO после сида находит именно их.
- **Error handling в API:** большинство хендлеров при ошибке возвращают `500 INTERNAL_SERVER_ERROR` без деталей. Детали — в логах (tracing) и в БД (trace_entries с entry_type = "error").
- **Просмотр содержимого проекта — только чтение:** `GET /workstations/:id/tree` и
  `GET /workstations/:id/file` читают ФС воркстейшна напрямую (exec `find`/`base64`
  через `executor_for_workstation`). Путь — относительный от `/work/project`;
  абсолютный/`..`/кавычки отклоняет `sanitize_rel` (инъекция в команду исключена).
  Записи нет: никакого write-эндпоинта, правки — через чат с LLM. Каждый участник
  видит содержимое любого воркстейшна (персональной видимости нет, как у сессий).
  Текстовые файлы отдаются `text/plain` (подсветка на фронте по расширению),
  медиа — байтами с MIME по расширению (`mime_for`). 404 — только когда команда
  исполнилась, но пути нет («Command failed» + no such file); сбой самого exec
  (нет kubectl/docker) — 500.
- **Changes — изменения проекта от начала ветки, только чтение:**
  `GET /workstations/:id/changes` (`git_changes.rs`) отдаёт unified-дифф текущего
  состояния проекта воркстейшна против дефолтной ветки репозитория — точки, где
  ветка `ws-<id>` началась при клоне. База ищется по `origin/HEAD`,
  `origin/main`, `origin/master` (`git rev-parse --verify`); базы нет (пустой
  `git init`, агент наполняет проект с нуля) — весь проект показывается как
  добавленные файлы (к отслеживаемым добавляются новые из
  `git ls-files --others --exclude-standard`, `.gitignore` уважается). В диффе
  и закоммиченное на ветке, и незакоммиченные правки (`git diff <base>`
  сравнивает базу с рабочим состоянием). Новые файлы `git diff` не показывает —
  они читаются батчем (`cd /work/project && for f in ...; do echo маркер; base64
  "$f"; done`, маркер `@@AGA-FILE@@` не пересекается с алфавитом base64) и
  добавляются блоками `new file mode`; пути экранируются `sh_quote`. Записи нет:
  коммитить и пушить ручка не умеет, страница только показывает. Видимость —
  как у содержимого проекта: любой участник, любого воркстейшна (персональной
  видимости нет).

## Verification
- `make test` — `cargo test` в `main/`: unit-тесты для: `Config::load`, `parse_command`, `project_registered_by_git_url`, `migrates_compose_path_to_git_url`, `deleted_workstation_disappears_from_list`, `workstation_renders_pod_with_git_url_and_branch`, `each_workstation_gets_its_own_pod`, `workstation_pod_has_no_k8s_api_access`, `agent_commands_run_in_workstation_pod`, `agent_commands_run_in_workstation_container`, `docker_run_clones_git_url_like_k8s`, `docker_run_mounts_local_project_path`, `workstation_executor_targets_its_pod`, `docker_backend_executor_targets_its_container`, `chatstore_opens_db`, JWKS-верификации (`verifies_valid_token_and_extracts_sub_and_roles`, `rejects_invalid_token`, `rejects_tampered_payload`), `resolve_user` (`participant_resolves_from_valid_token`, `invalid_token_rejected`, `anonymous_superuser_without_sso`, `admin_role_maps_to_super_user`), сессий воркстейшна (`session_binds_to_ready_workstation`, `workstation_not_ready_rejects_session`, `workstation_holds_single_open_session`, `session_closed_only_by_owner`, `participant_cannot_close_foreign_session`, `closed_session_frees_workstation`), видимости (`participant_sees_all_sessions`, `participant_sees_all_projects`, `created_project_visible_to_all_participants`), нитей (`thread_starts_from_message_with_title_on_first_message`, `one_message_spawns_several_threads`, `threads_not_listed_for_user`, `post_to_parent_puts_copy_with_thread_link`, `nested_thread_spawns_from_message_inside_thread`, `thread_depth_limited_to_max_level`, `thread_created_via_api_appears_collapsed_in_parent`, `multiple_threads_from_one_message_via_api`, `start_command_no_longer_creates_thread_via_api`, `message_posted_to_parent_via_api_carries_thread_link`, `list_chats_via_api_shows_only_root_chats`), Centrifugo (`connection_jwt_has_sub_and_common_channel`, `disabled_client_has_no_jwt`, `config_uses_default_channel`, `authenticated_user_gets_connection_jwt`, `connection_jwt_requires_authentication`, `connection_jwt_missing_when_centrifuge_not_configured`, `channels_are_named_per_chat_and_per_user`, `message_payload_carries_chat_id_message_id_and_author`, `lifecycle_payload_carries_action_and_chat_id`, `publish_message_fans_out_to_common_chat_and_author_channels`, `disabled_client_publishes_nothing`, `subscriber_jwt_has_server_side_subscriptions_for_bound_users`, `sse_frames_carry_publications_and_ignore_pings`), событий чата (`sent_message_event_reaches_chat_channel`, `sent_message_event_reaches_author_user_channel`, `chat_lifecycle_events_reach_common_channel`, `mentioning_agent_by_name_does_not_trigger_agent`), привязки к пользователю (`agent_binds_to_listening_user_survives_update`, `agent_without_binding_listens_to_nobody`, `agent_listen_binding_saved_and_stays_visible_via_api`), агент-рантайма (`mention_of_bound_user_triggers_agent_answering_as_that_user`, `own_reply_of_bound_user_does_not_trigger_again`, `message_without_mention_does_not_trigger`, `agent_without_binding_does_not_react`, `message_in_chat_of_unattached_project_does_not_trigger`, `message_in_general_chat_without_project_does_not_trigger`, `mention_of_unknown_or_unserved_user_does_not_trigger`, `listen_channels_cover_each_bound_user_once`, `without_served_users_there_is_nothing_to_listen`, `agent_steps_do_not_trigger`, `thread_started_by_served_user_triggers_on_every_message`, `agent_reply_mentioning_other_served_user_triggers_their_agent`, `agent_starts_thread_and_replies_there_trigger_it`, `sse_frames_split_on_blank_lines`, `long_command_output_is_truncated_short_is_kept`, human-in-the-loop ASK_HUMAN (`ask_human_posts_question_text_to_chat_and_waits`, `answer_to_question_resumes_agent_from_any_participant`, `thread_started_from_question_is_not_an_answer`, `native_tool_call_runs_command_and_returns_output_to_model`, `native_ask_human_returns_question_and_parks_task`, `text_mode_runs_whole_bash_block_and_publishes_note`, `text_mode_ask_human_returns_question_and_parks_task`, `rejected_and_failed_commands_go_back_to_model`, `exhausted_steps_report_unfinished_task`, `human_request_links_to_question_message_and_resolves`, `migrates_human_request_chat_columns`, `own_messages_are_assistant_others_are_named_users`, `own_steps_become_tool_calls_old_ones_without_output`, `thread_context_starts_with_source_message_and_title`, `budget_keeps_newest_messages_and_marks_cut`, `thread_accepts_at_most_max_messages_root_chat_unlimited`, `thread_limits_come_from_env_or_default`, `mentions_are_words_starting_with_at`), параллелизма (`same_agent_two_messages_serialize`, `different_agents_on_one_workstation_run_in_parallel`, `redelivered_event_triggers_single_run`)), режимов бинаря (`agent_arg_selects_runtime_not_http_server`, `no_arg_selects_http_server`, `seed_arg_selects_seed`), AgentSet (`one_agent_set_attaches_to_many_projects`, `each_agent_keeps_own_rules_and_llm`, `agent_set_agents_form_tree_by_parent`, `replacing_agent_set_changes_project_agents`, `deleted_agent_set_disappears_from_projects`, `bound_agent_resolves_own_rules_and_llm`, `agent_without_connection_and_default_has_no_llm`, `agent_without_connection_uses_default_llm`, `updated_connection_changes_url_and_key_for_agents`, `deleted_default_connection_clears_default_and_agent_has_no_llm`, `own_connection_wins_over_default`, `setting_default_moves_it_and_clearing_removes_it`, `connection_keeps_model_and_default_flag`, `each_agent_owns_territory_by_its_tree_node`, `tools_are_plain_list_capabilities_have_single_content`, `agent_uses_only_assigned_skills`, `agent_always_uses_latest_capability_content`, `capability_actions_written_to_history_with_author`, `capability_renamed_and_deleted_with_history`, `bound_agent_applies_territory_skills_and_tools`, `sandbox_mounts_project_ro_and_own_folder_rw`, `sandbox_of_root_agent_closes_only_descendants`, `sandbox_in_workstation_blocks_writes_outside_territory` (ignored, docker), MCP (`stdio_server_lists_and_calls_tools`, `stdio_server_that_exits_reports_error`, `http_server_keeps_session_and_reads_sse_result`, `tool_result_text_covers_content_kinds`, `llm_tool_names_are_api_safe`, `mcp_tools_are_offered_and_called_natively`, `unavailable_mcp_server_is_reported_and_run_continues`, `text_mode_does_not_start_mcp_servers`, `mcp_servers_catalog_and_agent_links_via_api`, `mcp_http_server_tools_checked_via_api`), `agent_executes_only_tools_from_its_list`, `every_command_of_pipeline_and_chain_is_checked`, `agent_set_detail_includes_territory_skills_and_tools`, `agent_set_update_persists_changes_via_api`), роутера (`participant_sees_all_projects_via_api`, `participant_creates_project_visible_to_others`, `participant_cannot_create_workstation`, `workstations_listed_with_state`, `personnel_listed_from_sso_but_not_editable`, `invalid_token_rejected_by_api`, `refresh_exchanges_token_and_sets_cookie`, `refresh_rejects_invalid_token`, `refresh_requires_sso_config`, `anonymous_superuser_works_without_sso`, `created_agent_set_listed_via_api`, `attached_agent_set_appears_on_project`, `replacing_agent_set_changes_project_agents_via_api`, подключений к LLM (`created_llm_connection_listed_via_api`, `updated_llm_connection_changes_url_and_key_via_api`, `deleted_llm_connection_disappears_from_list_via_api`, `llm_connections_require_authentication`, `default_llm_set_and_cleared_via_api`, `default_llm_requires_authentication`, `chat_uses_config_url_key_and_model`, `chat_sends_tools_and_parses_tool_calls`, `tool_result_message_serializes_with_call_id`, `chat_without_configured_llm_fails`), каталога способностей (`agent_set_detail_includes_territory_skills_and_tools`, `capability_renamed_and_deleted_via_api`, `capability_history_shows_who_when_and_what_via_api`), сокращений (`shortcut_token_adds_text_to_message_hidden`, `unknown_shortcut_leaves_message_hidden_unchanged`, `repeated_shortcut_added_once`, `several_shortcuts_added_to_hidden`, `shortcut_in_thread_first_message_adds_hidden`, `hidden_not_in_agent_context`, `editing_or_deleting_shortcut_does_not_change_sent_message_hidden`, `sent_message_hidden_cannot_be_changed`, `shortcut_name_allows_any_alphabet_without_spaces`, `shortcut_occupied_name_rejected`, `shortcut_changes_written_to_history`, `deleted_shortcut_stays_with_history`, `any_participant_can_edit_shortcuts_and_see_hidden`)), `role_endpoints_removed`, просмотра содержимого (`project_tree_lists_files_and_folders`, `text_file_read_returns_content`, `image_file_read_returns_bytes`, `video_and_audio_file_read_returns_bytes`, `missing_file_returns_error`, `sanitize_rel_rejects_path_traversal_and_shell_breaks`, `mime_detected_by_extension`, `workstation_tree_requires_authentication`, `workstation_file_requires_authentication`, `participant_browses_any_workstation_content`, `missing_workstation_content_returns_not_found`, `no_write_route_for_project_files`), изменений проекта (`changes_show_committed_and_uncommitted_changes_against_default_branch`, `changes_show_untracked_file_as_added`, `changes_show_all_files_as_added_without_base`, `changes_report_no_changes_when_repo_clean`, `sh_quote_escapes_inner_quotes`, `workstation_changes_requires_authentication`, `missing_workstation_changes_returns_not_found`, `participant_browses_any_workstation_changes`, `no_write_route_for_changes`), жизненного цикла воркстейшна (`workstation_pod_mounts_named_secret`, `workstation_keeps_named_secret`, `session_project_is_direct_and_station_follows_sessions`, `released_workstation_is_free_after_session_close`, `switch_command_replaces_project_contents_without_recreate`, `crashed_workstation_marked_down`, `workstation_down_rejects_new_session`, `session_opened_on_free_ws_recovers_interrupted_session`, `fresh_session_without_crash_has_no_continuation`, `restore_command_checks_out_interrupted_session_branch`, `releasing_workstation_rejected_while_session_open`, `releasing_workstation_forbidden_for_participant`, `marking_workstation_down_forbidden_for_participant`).
- `make lint` (cargo clippy --all-targets) — статический анализ без предупреждений.
- **Составные тесты:** `chatstore_opens_db` проверяет открытие TraceStore+ChatStore на временной БД; роутер-тесты гоняют запросы через `tower::ServiceExt::oneshot`.
- **Тесты веб-клиента** (`web_client_*` из старого `src/server.rs`) вынесены в уровень `front/` — ядро UI не раздаёт.
- **Интеграция (кластер):** `make k8s-verify` (см. `infra/k8s/AGENTS.md`) — воркстейшн поднимает под в локальном кластере.
- **Критерий готовности:** все unit-тесты проходят; публичные API компилируются без ошибок; сервер отвечает на `/users`, `/chats`, `/chats/:id/messages`.

## Dependencies
- Стандартная библиотека Rust
- Внешние крейты: axum, tokio, tower-http, reqwest (rustls-tls), serde, serde_yaml, serde_json, sqlx (sqlite + uuid + chrono), uuid, chrono, tracing, thiserror, regex, base64, futures, futures-util, bytes
- Рантайм-данные: `main/config/roles.yaml` (сборка через `make init`), `main/data/`
- kubectl и docker CLI (в образе) — воркстейшны: поды (k8s) и контейнеры (dev)