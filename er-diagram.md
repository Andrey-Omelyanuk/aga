# ER-диаграмма базы данных aga

SQLite-БД ядра (`main/data/trace.db`, WAL). Схема создаётся в двух модулях:
`TraceStore` (`main/src/trace.rs`) и `ChatStore` (`main/src/chat.rs`) — одна БД.

## Основная диаграмма

Модель чата (`chat.rs`) и набор агентов (`trace.rs`) вынесены во внешние
сущности `CHAT` и `AGENT_SET` — на основной диаграмме видны только их связи с
остальными таблицами. Детали — в отдельных диаграммах ниже.

```mermaid
erDiagram
    PROJECTS ||--o{ WORKSTATIONS : "станции"
    PROJECTS ||--o{ PROJECT_AGENT_SET : "прикреплённый набор"
    AGENT_SET ||--o{ PROJECT_AGENT_SET : "на многих проектах"
    TASKS ||--o{ TRACE_ENTRIES : "шаги"
    TASKS ||--o{ HUMAN_REQUESTS : "human-in-the-loop"
    PROJECTS ||--o{ SESSION : "сессия проекта"
    WORKSTATIONS ||--o{ SESSION : "текущая сессия (current_session_id)"
    SESSION ||--|| CHAT : "1:1 корневой чат (chat_id)"

    WORKSTATIONS {
        integer id PK
        integer project_id FK
        integer current_session_id FK "активная сессия, nullable"
        text name
        text state "creating|ready|down"
        text secret
        datetime created_at
    }

    SESSION {
        integer id PK
        integer chat_id FK "корневой чат, 1:1"
        integer project_id FK
        integer workstation_id FK
        integer owner_id FK
        integer result_id "сообщение-итог"
        integer continues_session_id "продолжение после сбоя"
        datetime created_at
        datetime closed_at
    }

    TASKS {
        text id PK
        text role
        text status "pending|running|done|error"
        datetime created_at
        datetime completed_at
    }

    TRACE_ENTRIES {
        text id PK
        text task_id FK
        integer step
        text entry_type
        text content
        text metadata
        datetime created_at
    }

    HUMAN_REQUESTS {
        text id PK
        text task_id FK
        text question
        text answer
        text status "pending|answered"
        datetime created_at
        datetime answered_at
    }

    PROJECTS {
        integer id PK
        text git_url "git-URL репозитория, UNIQUE"
        datetime created_at
        datetime updated_at
    }

    CHAT {
        integer id PK "внешняя сущность — см. диаграмму Chat"
    }

    AGENT_SET {
        integer id PK
        text name "внешняя сущность — см. диаграмму AgentSet"
    }

    PROJECT_AGENT_SET {
        integer project_id PK, FK
        integer agent_set_id FK
    }
```

### Замечания

- **Session** — связующая сущность между проектом, чатом и воркстейшном:
  `project_id` связывает сессию с проектом напрямую, `chat_id` — 1:1 с корневым
  чатом (сессией), `workstation_id` — со станцией. Воркстейшн ссылается на
  активную сессию через `current_session_id` (NULL, когда свободен).
- Сессионные поля (`result_id`, `continues_session_id`) переехали из `chats` в
  `sessions`. `chats.state` остаётся в `chats` — он нужен и нитям.
- К проекту прикрепляется один набор агентов (`PROJECT_AGENT_SET`); один набор
  можно прикрепить к нескольким проектам. Детали набора — в диаграмме AgentSet.
- Задача (`tasks`) порождает шаги трассировки (`trace_entries`) и
  human-in-the-loop запросы (`human_requests`).

## Chat (внешняя сущность)

Модель чата (`main/src/chat.rs`): учётки, чаты (сессии/нити), участники,
сообщения и артефакты. Корневой чат воркстейшна — сессия.

```mermaid
erDiagram
    CHAT_USERS ||--o{ CHATS : "создаёт (created_by_id)"
    CHAT_USERS ||--o{ CHAT_PARTICIPANTS : "участвует"
    CHATS ||--o{ CHAT_PARTICIPANTS : "имеет"
    CHAT_USERS ||--o{ MESSAGES : "автор (author_id)"
    CHATS ||--o{ MESSAGES : "содержит"
    MESSAGES ||--o{ ARTIFACTS : "артефакты"
    CHATS ||--o{ SESSION : "корневой чат-сессия 1:1 (chat_id)"

    CHAT_USERS {
        integer id PK
        text name
        text kind "human|agent|anonymous"
        integer is_super_user
        text sso_subject
        text role
        datetime created_at
    }

    SESSION {
        integer id PK
        integer chat_id FK "корневой чат, 1:1"
        integer project_id FK
        integer workstation_id FK
        integer owner_id FK
        integer result_id "сообщение-итог"
        integer continues_session_id "продолжение после сбоя"
        datetime created_at
        datetime closed_at
    }

    CHATS {
        integer id PK
        integer root_id "корень сессии/ветки"
        integer parent_id "родительский чат"
        integer level "макс 30 уровней нитей"
        text title
        integer start_message_id "сообщение, от которого нить"
        integer created_by_id FK
        datetime created_at
        datetime updated_at
        text state "OPEN|CLOSED"
    }

    CHAT_PARTICIPANTS {
        integer chat_id PK, FK
        integer chat_user_id PK, FK
    }

    MESSAGES {
        integer id PK
        integer chat_id FK
        integer parent_id "ответ на сообщение"
        integer author_id FK
        integer shared_by_id
        integer share_of_id "копия-шар оригинала"
        datetime created_at
        integer last_message_id
        text title "заголовок нити"
        integer thread_of_id "из нити в родительский чат"
        text body
    }

    ARTIFACTS {
        integer id PK
        integer message_id FK
        text kind
        text title
        text content
        datetime created_at
    }
```

### Замечания

- `chat_users` отделён от SSO: `sso_subject` связывает учётку с Keycloak, агенты
  и аноним-суперпользователь `kind` не имеют `sso_subject`.
- `chats.root_id` — корневой чат (сессия воркстейшна или общий чат); нити —
  дочерние чаты с `parent_id` и `start_message_id`.
- Корневой чат-сессия связан с `SESSION` 1:1 (см. основную диаграмму): через
  `sessions.project_id` проект чата достаётся напрямую, без транзита через
  воркстейшн. Общие чаты (`workstation_id` нет) сессии не имеют.
- `messages` самоссылается через `parent_id` (ответы) и `share_of_id` (шаринг).

## AgentSet (внешняя сущность)

Детали набора агентов: `agent_sets` — вершина, её агенты (дерево через
`parent_id`), данные способности из общего каталога (`capabilities`) и
подключения к LLM (`llm_connections`).

```mermaid
erDiagram
    AGENT_SETS ||--o{ AGENTS : "агенты набора"
    AGENTS ||--o{ AGENTS : "дерево (parent_id)"
    AGENTS ||--o{ AGENT_CAPABILITIES : "данные способности"
    CAPABILITIES ||--o{ AGENT_CAPABILITIES : "данные агентам"
    CAPABILITIES ||--o{ CAPABILITY_HISTORY : "история правок"
    LLM_CONNECTIONS ||--o{ AGENTS : "модель (llm_id)"

    AGENT_SETS {
        integer id PK
        text name "UNIQUE"
        datetime created_at
        datetime updated_at
    }

    AGENTS {
        integer id PK
        integer set_id FK
        text name "UNIQUE(set_id, name)"
        text description
        text tools "JSON-список инструментов"
        integer max_iterations
        integer llm_id FK "подключение к LLM, ON DELETE SET NULL"
        integer parent_id FK "дерево набора"
    }

    AGENT_CAPABILITIES {
        integer agent_id PK, FK
        integer capability_id PK, FK
    }

    CAPABILITIES {
        integer id PK
        text kind "skill|command"
        text name "UNIQUE(kind, name)"
        text content "единственное содержимое"
        integer deleted "мягкое удаление"
    }

    CAPABILITY_HISTORY {
        integer id PK
        integer capability_id FK
        text action "create|update|rename|delete"
        integer actor_id
        text actor_name
        datetime created_at
        text detail
        text content "содержимое после действия"
    }

    LLM_CONNECTIONS {
        integer id PK
        text name "UNIQUE"
        text api_url
        text api_key
        text model
        integer is_default "дефолт один (частичный UNIQUE)"
    }
```

### Замечания

- Агенты набора образуют дерево через `parent_id`; территория агента — его узел.
- `capabilities` и `agent_capabilities` — связь «многие-ко-многим» по имени/записи;
  `capability_history` — журнал правок каталога.
- `llm_connections.is_default` — единственная дефолтная LLM (частичный уникальный
  индекс `WHERE is_default = 1`); удаление подключения сбрасывает `agents.llm_id` в NULL.