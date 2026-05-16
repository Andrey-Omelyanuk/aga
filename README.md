# Система Агентов для Кодинга на Rust

Система специализированных агентов для кодинга, где каждый агент работает в отдельном Docker контейнере и выполняет свою уникальную функцию.

## 🏗️ Архитектура

```
┌─────────────────────────────────────────────────────────────┐
│                      API Server (8080)                       │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  REST API для взаимодействия с агентами               │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│              Orchestrator (8081)                             │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Координация работы агентов, управление жизненным циклом │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│              NATS Message Broker (4222)                      │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Асинхронный обмен сообщениями между агентами         │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│              PostgreSQL (5432)                               │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Хранение state агентов и истории задач               │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              Shared Volume (/shared-files)                   │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Обмен файлами между агентами                         │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              Agent Containers                                │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Code Analysis Agent                                   │  │
│  │  Code Generation Agent                                 │  │
│  │  Code Review Agent                                     │  │
│  │  Test Generation Agent                                 │  │
│  │  Documentation Agent                                    │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## 🚀 Быстрый старт

### 1. Сборка и запуск всех сервисов

```bash
docker-compose up -d --build
```

### 2. Создание агентов

Создайте агента на основе шаблона:

```bash
# Копируем шаблон агента
cp -r agents/template agents/code-generation

# Конфигурируем агента
cat > agents/code-generation/agent.md << 'EOF'
# Code Generation Agent Configuration

## Role Definition
role: code-generation
description: Generates code based on requirements and specifications

## Capabilities
- code_generation
- refactoring
- optimization

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.7
max_tokens: 4096

## Communication
nats_subject: agent.code-generation.task
status_topic: agent.code-generation.status
result_topic: agent.code-generation.result
EOF

# Запускаем агента
docker-compose up -d agent-template
```

### 3. Взаимодействие с API

```bash
# Запустить задачу для агента
curl -X POST http://localhost:8080/agents/code-generation/execute \
  -H "Content-Type: application/json" \
  -d '{
    "task": "Generate a Rust function to parse JSON",
    "context": "Need to parse user input from API requests"
  }'

# Проверить статус агента
curl http://localhost:8080/agents/code-generation/status

# Получить список всех агентов
curl http://localhost:8080/agents
```

## 📁 Структура проекта

```
aga/
├── api-server/              # API сервер на Rust
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── src/
│       └── main.rs
│
├── orchestrator/            # Оркестратор агентов
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── src/
│       ├── main.rs
│       ├── agent_manager.rs
│       ├── nats_client.rs
│       └── db.rs
│
├── agents/                  # Шаблон агента
│   ├── template/           # Шаблон для генерации агентов
│   │   ├── Cargo.toml
│   │   ├── Dockerfile
│   │   └── src/
│   │       ├── main.rs
│   │       ├── agent.rs
│   │       ├── model.rs
│   │       └── tools.rs
│   └── configs/            # Примеры agent.md конфигураций
│       ├── code-generation.agent.md
│       ├── code-review.agent.md
│       ├── test-generation.agent.md
│       └── documentation.agent.md
│
├── nats-config/            # Конфигурация NATS
│   └── default.conf
│
├── init-db/                # SQL скрипты инициализации БД
│   └── 01_init.sql
│
├── docker-compose.yml      # Основной compose файл
│
├── ARCHITECTURE.md         # Подробная документация архитектуры
│
└── README.md               # Этот файл
```

## 🔧 Конфигурация агентов (agent.md)

Каждый агент имеет свой файл конфигурации `agent.md`:

```markdown
# Agent Configuration

## Role Definition
role: code-generation
description: Generates code based on requirements and specifications

## Capabilities
- code_generation
- refactoring
- optimization

## Tools
- file_read
- file_write
- search_replace
- code_execution

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.7
max_tokens: 4096

## Communication
nats_subject: agent.code-generation.task
status_topic: agent.code-generation.status
result_topic: agent.code-generation.result

## Database Schema
schema:
  - table_name: generated_code
    columns:
      - name: id
        type: uuid
        primary_key: true
      - name: task_id
        type: uuid
        foreign_key: tasks.id
      - name: code_content
        type: text
      - name: file_path
        type: varchar(1024)
```

## 📊 Поддерживаемые агенты

| Агент | Роль | Описание |
|-------|------|----------|
| `code-analysis` | Анализ кода | Понимание структуры и семантики кода |
| `code-generation` | Генерация кода | Создание нового кода на основе требований |
| `code-review` | Ревью кода | Проверка качества и лучших практик |
| `test-generation` | Генерация тестов | Создание unit и integration тестов |
| `documentation` | Документация | Генерация README и API документации |

## 🔄 Обмен сообщениями через NATS

### Topics

- `agent.*.task` - задачи для агентов
- `agent.*.result` - результаты выполнения задач
- `agent.*.status` - обновления статуса агентов

### Пример публикации задачи

```bash
curl -X POST http://localhost:8080/agents/code-generation/execute \
  -H "Content-Type: application/json" \
  -d '{
    "task": "Generate a Rust function to parse JSON",
    "context": "Need to parse user input from API requests",
    "priority": 1
  }'
```

## 📁 Shared Volume для обмена файлами

Все агенты имеют доступ к общему volume `/shared-files`:

- Загрузка файлов в shared volume через API
- Агенты могут читать/писать файлы из shared volume
- Поддерживается обмен исходным кодом, тестами, документацией

## 🛠️ Модель LLM

Модель настраивается через переменные окружения:

```bash
docker run --env MODEL_NAME=codellama-13b \
           --env MODEL_PATH=/models/codellama-13b \
           --env MODEL_TEMPERATURE=0.7 \
           --env MODEL_MAX_TOKENS=4096 \
           agent-template
```

Поддерживаемые модели:
- CodeLlama (13B, 7B)
- StarCoder
- DeepSeek-Coder
- Qwen-Coder

## 📝 Примеры использования

### Генерация кода

```bash
curl -X POST http://localhost:8080/agents/code-generation/execute \
  -H "Content-Type: application/json" \
  -d '{
    "task": "Create a Rust struct for API response",
    "context": "Need to handle JSON responses from external API"
  }'
```

### Ревью кода

```bash
curl -X POST http://localhost:8080/agents/code-review/execute \
  -H "Content-Type: application/json" \
  -d '{
    "task": "Review this Rust code for security issues",
    "context": "Code handles user authentication tokens"
  }'
```

### Генерация тестов

```bash
curl -X POST http://localhost:8080/agents/test-generation/execute \
  -H "Content-Type: application/json" \
  -d '{
    "task": "Generate unit tests for this function",
    "context": "Function calculates user statistics"
  }'
```

## 🔍 Мониторинг

```bash
# Просмотр логов всех сервисов
docker-compose logs -f

# Просмотр логов конкретного агента
docker-compose logs -f agent-template

# Проверка статуса контейнеров
docker-compose ps

# Просмотр состояния БД
docker exec agent-postgres psql -U agent_user -d agents_db -c "SELECT * FROM agents;"
```

## 📚 Документация

- [ARCHITECTURE.md](./ARCHITECTURE.md) - Подробная документация архитектуры
- [agents/template/configs/*.agent.md](./agents/template/configs/) - Примеры конфигураций агентов

## 🔐 Безопасность

- Все агенты работают в изолированных контейнерах
- NATS использует отдельную сеть для коммуникации
- PostgreSQL использует отдельные учетные данные
- Shared volume монтируется с ограничениями на запись

## 📦 Требования

- Docker 20.10+
- Docker Compose 2.0+
- Rust 1.75+ (для локальной разработки)

## 📄 Лицензия

MIT License



-------------------------------------------------
Ага - агентная система специально оптимизируемая для слабых LLM.
- No tools - это дорого, мы используем только коммандную строку в окрежинии linux.
- No agent config into a project - чистый код проекта, вся агентная система лежит отдельно, 
  люди могут использовать разные инструменты для разработки ну нужно все смешивать.
- 100 isolation - все крутиться в докер контейнерах, что позволяет максимально все изолировать.
- Minimum context - каждый агент сильно специализирован, что позовляет минимизировать концекст.
- Agent swarm - агенты могут общаться между собой, например frontend агент может запрашивать у backend агента текущий формат api для доступа к объекту.
  т.е. frontend-у не нужно сканировать и искать, это задача backend агента.


Notes:
- И идеале использовать одну и туже систему агентов для разных проектов, только переключая входную папку с проектом. 
  т.е. структура агентов димамическая и адаптируется не только на проект, но и на изменения в проектк,
  напрмер если добавился отдельный сервис, то под него создаеться отдельный агент который с ним работает.  