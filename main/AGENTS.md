# main — Ядро (Rust-сервис)

## Overview
Реализация ядра фреймворка: HTTP REST API, цикл агента (LLM → parse → exec → trace),
LLM-клиент, SQLite-трассировка, управление проектами и ролями, воркстейшны как
поды Kubernetes (стенд) или контейнеры Docker (dev). Чистый бэкенд — UI в этом
уровне нет.

## Boundaries
- **Делает:** обрабатывает HTTP-запросы (Axum); запускает цикл агента (LLM,
  парсинг, валидация, выполнение команд); хранит/отдаёт трассировку; управляет
  проектами и наборами агентов (AgentSet) в SQLite; поднимает воркстейшны-поды
  через kubectl (стенд) или контейнеры через docker CLI (dev,
  `AGA_WS_BACKEND=docker`); раздаёт API и REST-клиентам; включает/выключает SSO.
- **Не делает:** не раздаёт веб-клиент (это `front/`); не управляет Docker
  напрямую в k8s-стенде (в dev-режиме воркстейшны — контейнеры, которыми ядро
  управляет через docker CLI); не предоставляет CLI.

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
├── prompts/             # общая инструкция для агентов
├── config/              # sso-конфиг (runtime, gitignored) + config.example.yml
└── data/                # runtime: trace.db, work/ (gitignored)
```
```
src/
├── main.rs       # Точка входа: env, init Config, TraceStore, ChatStore, Cluster, JwtVerifier→AppState
├── config.rs     # Config, RoleConfig, LlmConfig, SsoConfig (JWKS/authorize/token/клиент) — загрузка из YAML
├── llm.rs        # LlmClient — OpenAI-compatible HTTP-клиент
├── agent.rs      # Agent — цикл LLM → extract → exec → trace; Executor (Sh / KubectlExec / DockerExec)
├── trace.rs      # TraceStore — SQLite: tasks, trace_entries, human_requests, projects, project_roles
├── chat.rs       # ChatStore — модель чата: chat_users, chats, participants, messages, artifacts, workstations
├── cluster.rs    # Cluster — воркстейшн: рендер манифеста пода (k8s) / docker-run (dev), wait_ready, delete
├── workstation.rs# executor_for_workstation: воркстейшн → kubectl exec / docker exec
├── reactive.rs   # ReactiveRunner: реактивные агенты по @Agent.<role>, очередь на воркстейшн
├── centrifuge.rs # CentrifugeClient: connection-JWT (HS256) + publish в общий канал
├── seed.rs       # Тестовый набор (aga seed): очистка БД + детерминированная фикстура
├── auth.rs       # JwtVerifier (JWKS, RS256) + resolve_user: участник/админ из Keycloak или аноним-супер
└── server.rs     # Axum Router + хендлеры API + SSE для human-запросов + /auth/login, /auth/callback, /auth/refresh, /auth/logout, /users/me, /connection-jwt/
```

## Patterns
- **Изоляция модулей:** `agent` зависит от `config`, `llm`, `trace`; `chat`/`workstation`/`reactive`/`auth` зависят друг от друга и от `trace`; `server` зависит от всех; `main` инициализирует и связывает.
- **Состояние:** `AppState { config, trace_store, chat_store, llm_client, reactive }` передаётся через Axum State.
- **Ошибки:** `thiserror` для типизированных ошибок; `Box<dyn Error + Send + Sync>` в agent-цикле (потенциально разные типы ошибок).
- **LLM-клиент:** один `LlmClient` на все роли; url, ключ и модель — из
  подключения агента или дефолтной LLM, температура 0.7.
- **SQLite:** CREATE TABLE IF NOT EXISTS при старте; WAL-режим; raw-запросы. Обязательно `SqliteConnectOptions::create_if_missing(true)` — `SqlitePool::connect` файл НЕ создаёт.
- **SSE:** `/human/pending` — SSE-стрим для получения pending-запросов в реальном времени.
- **Centrifugo (реальное время чата):** `CentrifugeClient` (`centrifuge.rs`) — два
  канала взаимодействия: `/connection-jwt/` выдаёт connection-JWT (HS256) с
  правом подписки на общий канал в claims (`channels`); публикация новых
  сообщений — HTTP API (`POST /api`, method `publish`). Клиент бывает
  `disabled()`-заглушкой (центрифуго не настроен).
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
  Переключить ws на другой проект (`POST /workstations/:id/switch`) можно только на
  свободной станции (нет открытой сессии); сам ws не пересоздаётся — `/work/project`
  переписывается кодом нового проекта через exec (см. `ws_ops::replace_project`;
  clone идёт во временный каталог `/work/project.new.<pid>`, подмена — только
  при успехе). В dev (docker) перед клоном ядро инжектит SSH-ключ aga в контейнер
  (`inject_ssh_key`) — станция переиспользуется, и ключ из `create_workstation`
  в неё не попадал. `/work` в образе воркстейшна принадлежит uid 1000
  (entrypoint `chown 1000:1000 /work`) — временный каталог клона рядом с
  проектом должен создавать именно агент. Отпустить станцию
  (`POST /workstations/:id/release`) — тоже только без открытой сессии: `project_id`
  сбрасывается в 0 (сантинел «свободен», id проектов с 1), `/work/project`
  очищается до пустого git-репо (`ws_ops::release_workspace`). Смена/отпускание уже
  зафиксированы в БД до exec — сбой перезаписи `/work/project` (например, плейсхолдер
  git_url в dev-стенде) не откатывает назначение, а только логируется.
  Упавший ws админ отмечает `POST /workstations/:id/down` (state=`down`; сессии на
  `down`-станции открыть нельзя). Восстановление после падения — ручное: открытие
  сессии на свободном ws определяет незакрытую сессию на `down`-станции того же
  проекта (`interrupted_session_for_project`), помечает новую сессию как
  продолжение (`continues_session_id`) и восстанавливает файлы из её ветки
  (`ws_ops::restore_workspace`, checkout `ws-<id>`).
- **Воркстейшнами из интерфейса не управляют:** создание/удаление — только суперпользователь (админ внешний, поднимает станции через k8s); участники видят список и состояние (`GET /workstations`), но `POST`/`DELETE` им запрещены (403). Сессию открывают на готовом воркстейшне (`POST /workstations/:id/session`), один воркстейшн — одна активная сессия.
- **Сессия = корневой чат воркстейшна:** открывается только на воркстейшне в состоянии `ready` и когда активной сессии на нём нет (`open_workstation_session`). Закрыть сессию может только её владелец (или суперпользователь локального режима) — `close_workstation_session`; закрытие освобождает воркстейшн.
- **Видимость открытая:** «все участники видят все проекты и сессии» — `list_chats_for_user` не фильтрует по участнику, `can_read` всегда true. Персональной видимости нет.
- **SSO:** `JwtVerifier` проверяет подпись RS256 по JWKS; роли из realm-ролей Keycloak (`participant`/`admin`). Вход веб-клиента — `/auth/login` (редирект в Keycloak), `/auth/callback` (обмен code→token, веб-клиенту токены возвращаются фрагментом URL `#token=...&refresh=...`), молчаливое обновление токена — `/auth/refresh` (ядро меняет refresh-токен на свежую пару у Keycloak, без браузерных кук — см. `me.refresh()` на фронте), выход — `/auth/logout` (сброс cookie `aga_token`, при заданном `end_session_url` — редирект на end-session Keycloak). Имя учётки — `preferred_username` из JWT (логин), не UUID `sub`. Текущий пользователь — `GET /users/me`. API принимает токен из `Authorization: Bearer` или cookie `aga_token` (см. `auth.rs`). Без SSO — аноним-суперпользователь.
- **Реальное время чата — один общий канал для аутентифицированных:** `GET /connection-jwt/` подписывает connection-JWT с `sub` = `chat_users.id` (не SSO-`sub`) и правом подписки на канал `common` в claims (`channels`) — отдельного channel-токена нет, канал общий, гейт — сам факт входа. Публикация — после `send_message` (server.rs), ответов реактивных агентов (reactive.rs) и share — best-effort: сбой центрифуго не ломает отправку, только логируется. Конфиг — блок `centrifuge:` в roles.yaml (api_url, api_key, secret, channel); не задан — клиент-заглушка (`disabled()`), `/connection-jwt/` отдаёт 404, чат без автообновления.
- **Безопасность команд:** `is_command_allowed()` банит пайпы (`|`), редиректы (`>`, `<`) и конкатенацию (`;`, `&`) на уровне строки — это поверх проверки `allowed_commands`.
- **Agent per task / per reactive-сообщение:** Agent создаётся на каждую задачу и каждый реактивный запуск; state не хранится между вызовами.
- **Команды из LLM:** извлекаются из markdown-блоков <code>```bash</code>. Код-блок может содержать несколько команд (построчно). Пустые строки и комментарии (`#`) игнорируются.
- **Команды чата** (`#invite`/`#kick`/`#start`/`#end`/`#share`) — это обычные сообщения с дополнительной реакцией; разбираются в `chat::parse_command`, исполняются в server.rs.
- **AgentSet заменяет роли:** агентов проекта определяет прикреплённый набор
  (`agent_sets`/`agents`/`project_agent_set`), а не глобальные роли. Набор
  настраивается через API (`/agent-sets`, прикрепляется к проекту
  (`/projects/:id/agent-set`); у проекта один набор (повторная привязка меняет
  его), один набор — на многие проекты (каскад ON DELETE). Агенты набора —
  дерево по иерархии папок проекта: у агента есть `parent_id` на родителя.
- **Способности агента — каталог с единственным содержимым и историей,
  инструменты — список без версий:** скиллы и команды живут в общем каталоге
  (`capabilities` с `kind='skill'|'command'` и полем `content`), агент получает
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
  исполняемого в консоли воркстейшна, версий у них нет. Промпт агента =
  его `description` + раскрытые данные скиллы/команды (см.
  `TraceStore::agent_prompt`).
- **Территория агента = его узел в дереве набора:** папка узла — имя агента как
  путь проекта, кроме корня дерева: у корня папки-родителя нет, его территория —
  корень проекта (папка `""`), а имя корня — метка слоя (например, `ui`
  библиотеки), папкой в репозитории оно не является (`scope::territory_for_list`).
  Территория заканчивается перед папками ближайших наследников.
  Чтение не ограничено; граница только на
  изменения. Enforcement — `Agent.scope` (только при воркстейшне): команда
  отвергается, если пишущий инструмент ссылается на путь вне территории;
  инструменты чтения (`cat`/`ls`/`find`/`grep`/...) пропускаются всегда
  (`scope::Territory::write_allowed`). Команды исполняются с cwd = папка агента
  (`cd /work/project/<folder> &&`), у корня — `/work/project`, чтобы относительные записи ложились в
  территорию. «Команды чата» (`#start`/`#invite`/...) — сообщения, в каталог
  не входят.
- **Правка набора — PATCH `/agent-sets/:id`:** состав (имя, агенты с их
  инструментами, данными скиллами/командами и деревом) заменяется целиком;
  старые агенты удаляются каскадно.
- **LLM — выбранное подключение и дефолтная LLM, env-дефолта нет:** подключения
  к LLM (`llm_connections`: имя, url API, ключ доступа, модель) живут в БД и
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
  Ключ наружу отдаётся как есть, маскировки нет. Дефолтной LLM из env
  (LLM_API_URL/LLM_API_KEY/LLM_MODEL) нет — в dev-стенде ядро при первом старте
  создаёт подключение к контейнеру ollama из `AGA_LLM_BOOTSTRAP_*` и ставит его
  дефолтным (`bootstrap_default_llm` в main.rs), пока в БД нет ни одного
  подключения.
- **Реактивные агенты:** `@Agent.<имя>` в сообщении триггерит ReactiveRunner;
  конфиг запускаемого агента берётся из набора проекта чата (`agent_role_config`);
  очередь сериализуется per-workstation (ключ 0 = локальный хост).
- **Тестовый набор (`aga seed`):** восстанавливает в БД детерминированную
  фикстуру (юзеры, каталог способностей, наборы агентов `dev-team` и `ui-kit`,
  проекты — в т.ч. UI-библиотеку `git@github.com:Andrey-Omelyanuk/mobx-model-ui.git`
  с набором `ui-kit` (скиллы `ui-review`, команда `run-ui-tests`, тулы
  `cat`/`ls`/`grep`/`find`), воркстейшны ws-1/ws-2, сессия с тредом и артефактом,
  общий чат с share, выполненная задача с trace-записями + pending human-запрос).
  Семантика —
  полный сброс: все таблицы очищаются (`clear_all` в TraceStore/ChatStore) и
  `sqlite_sequence` сбрасывается — ID стабильны между запусками. Seed создаёт
  и дефолтное подключение к LLM (`ollama-local`) — адрес и модель из
  `AGA_LLM_BOOTSTRAP_*` (в dev-стенде — контейнер ollama), иначе
  `http://localhost:11434/v1`; seed стирает БД, поэтому bootstrap ядра после
  него уже не отработает. Запуск:
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

## Verification
- `make test` — `cargo test` в `main/`: unit-тесты для: `extract_commands`, `is_command_allowed`, `Config::load`, `parse_command`, `mentioned_roles`, `project_registered_by_git_url`, `migrates_compose_path_to_git_url`, `deleted_workstation_disappears_from_list`, `workstation_renders_pod_with_git_url_and_branch`, `each_workstation_gets_its_own_pod`, `workstation_pod_has_no_k8s_api_access`, `agent_commands_run_in_workstation_pod`, `agent_commands_run_in_workstation_container`, `docker_run_clones_git_url_like_k8s`, `docker_run_mounts_local_project_path`, `workstation_executor_targets_its_pod`, `docker_backend_executor_targets_its_container`, `chatstore_opens_db`, JWKS-верификации (`verifies_valid_token_and_extracts_sub_and_roles`, `rejects_invalid_token`, `rejects_tampered_payload`), `resolve_user` (`participant_resolves_from_valid_token`, `invalid_token_rejected`, `anonymous_superuser_without_sso`, `admin_role_maps_to_super_user`), сессий воркстейшна (`session_binds_to_ready_workstation`, `workstation_not_ready_rejects_session`, `workstation_holds_single_open_session`, `session_closed_only_by_owner`, `participant_cannot_close_foreign_session`, `closed_session_frees_workstation`), видимости (`participant_sees_all_sessions`, `participant_sees_all_projects`, `created_project_visible_to_all_participants`), Centrifugo (`connection_jwt_has_sub_and_common_channel`, `disabled_client_has_no_jwt`, `config_uses_default_channel`, `authenticated_user_gets_connection_jwt`, `connection_jwt_requires_authentication`, `connection_jwt_missing_when_centrifuge_not_configured`), AgentSet (`one_agent_set_attaches_to_many_projects`, `each_agent_keeps_own_rules_commands_and_llm`, `agent_set_agents_form_tree_by_parent`, `replacing_agent_set_changes_project_agents`, `deleted_agent_set_disappears_from_projects`, `mentioned_agent_resolves_own_rules_commands_and_llm`, `agent_without_connection_and_default_has_no_llm`, `agent_without_connection_uses_default_llm`, `updated_connection_changes_url_and_key_for_agents`, `deleted_default_connection_clears_default_and_agent_has_no_llm`, `own_connection_wins_over_default`, `setting_default_moves_it_and_clearing_removes_it`, `connection_keeps_model_and_default_flag`, `each_agent_owns_territory_by_its_tree_node`, `tools_are_plain_list_capabilities_have_single_content`, `agent_uses_only_assigned_skills_and_commands`, `agent_always_uses_latest_capability_content`, `capability_actions_written_to_history_with_author`, `capability_renamed_and_deleted_with_history`, `mentioned_agent_applies_territory_skills_commands_and_tools`, `agent_tool_cannot_modify_file_outside_territory`, `agent_tool_can_read_file_outside_territory`, `agent_executes_only_tools_from_its_list`, `agent_set_detail_includes_territory_skills_commands_and_tools`, `agent_set_update_persists_changes_via_api`), роутера (`participant_sees_all_projects_via_api`, `participant_creates_project_visible_to_others`, `participant_cannot_create_workstation`, `workstations_listed_with_state`, `personnel_listed_from_sso_but_not_editable`, `invalid_token_rejected_by_api`, `refresh_exchanges_token_and_sets_cookie`, `refresh_rejects_invalid_token`, `refresh_requires_sso_config`, `anonymous_superuser_works_without_sso`, `created_agent_set_listed_via_api`, `attached_agent_set_appears_on_project`, `replacing_agent_set_changes_project_agents_via_api`, подключений к LLM (`created_llm_connection_listed_via_api`, `updated_llm_connection_changes_url_and_key_via_api`, `deleted_llm_connection_disappears_from_list_via_api`, `llm_connections_require_authentication`, `default_llm_set_and_cleared_via_api`, `default_llm_requires_authentication`, `chat_uses_config_url_key_and_model`, `chat_without_configured_llm_fails`), каталога способностей (`agent_set_detail_includes_territory_skills_commands_and_tools`, `capability_renamed_and_deleted_via_api`, `capability_history_shows_who_when_and_what_via_api`), `role_endpoints_removed`, просмотра содержимого (`project_tree_lists_files_and_folders`, `text_file_read_returns_content`, `image_file_read_returns_bytes`, `video_and_audio_file_read_returns_bytes`, `missing_file_returns_error`, `sanitize_rel_rejects_path_traversal_and_shell_breaks`, `mime_detected_by_extension`, `workstation_tree_requires_authentication`, `workstation_file_requires_authentication`, `participant_browses_any_workstation_content`, `missing_workstation_content_returns_not_found`, `no_write_route_for_project_files`), жизненного цикла воркстейшна (`workstation_pod_mounts_named_secret`, `workstation_keeps_named_secret`, `switching_workstation_rejected_while_session_open`, `switched_workstation_points_to_new_project`, `switch_command_replaces_project_contents_without_recreate`, `crashed_workstation_marked_down`, `workstation_down_rejects_new_session`, `session_opened_on_free_ws_recovers_interrupted_session`, `fresh_session_without_crash_has_no_continuation`, `restore_command_checks_out_interrupted_session_branch`, `switching_workstation_forbidden_for_participant`, `marking_workstation_down_forbidden_for_participant`).
- `make lint` (cargo clippy --all-targets) — статический анализ без предупреждений.
- **Составные тесты:** `chatstore_opens_db` проверяет открытие TraceStore+ChatStore на временной БД; роутер-тесты гоняют запросы через `tower::ServiceExt::oneshot`.
- **Тесты веб-клиента** (`web_client_*` из старого `src/server.rs`) вынесены в уровень `front/` — ядро UI не раздаёт.
- **Интеграция (кластер):** `make k8s-verify` (см. `infra/k8s/AGENTS.md`) — воркстейшн поднимает под в локальном кластере.
- **Критерий готовности:** все unit-тесты проходят; публичные API компилируются без ошибок; сервер отвечает на `/users`, `/chats`, `/chats/:id/messages`.

## Dependencies
- Стандартная библиотека Rust
- Внешние крейты: axum, tokio, tower-http, reqwest (rustls-tls), serde, serde_yaml, serde_json, sqlx (sqlite + uuid + chrono), uuid, chrono, tracing, thiserror, regex, base64, futures, futures-util, bytes
- Рантайм-данные: `main/roles/`, `main/prompts/`, `main/config/roles.yaml` (сборка через `make init`), `main/data/`
- kubectl и docker CLI (в образе) — воркстейшны: поды (k8s) и контейнеры (dev)