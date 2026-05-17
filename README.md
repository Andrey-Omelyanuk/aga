# 🎬 aga — Ага, и готово.

> *«— Сделаешь?  
> — Ага.»*  
> *(Двое из ларца, одинаковых с лица, 1974)*

Легковесный Rust-фреймворк для LLM-агентов. Один контейнер, прозрачный SSH, встроенная трассировка, human-in-the-loop. Без оркестраторов, очередей и облачного оверхеда.

---

## ✨ Ключевые возможности

| Фича | Описание |
|------|----------|
| 🐳 **Один контейнер** | Нет NATS, Redis, S3. Все роли работают в одном процессе, изолированы через конфиг и `/work/{role}/` |
| 🔒 **Прозрачный SSH** | LLM пишет `bash git log`, Rust незаметно подключается к целевому хосту и возвращает вывод. SSH полностью скрыт от модели |
| 📊 **Встроенная трассировка** | SQLite (WAL-режим) хранит каждый шаг, команду, ответ LLM и human-запросы. Доступно через `/trace/{id}` |
| 🤝 **Human-in-the-loop** | Агент может задать вопрос через `[ASK_HUMAN]...[/ASK_HUMAN]`. Ожидание ответа через API, конфигурируемые таймауты и fallback |
| 🛡️ **Изоляция по умолчанию** | Docker `read_only: true`, `cap_drop: ALL`, валидация команд, ограничение PID/CPU/RAM, строгий `workdir` |
| ⚡ **Гибкий LLM-бэкенд** | Любой OpenAI-compatible провайдер (Ollama, vLLM, OpenAI, LocalAI). Настраивается на каждую роль отдельно |

---

## 🏗️ Архитектура

```
[HTTP Client / CLI / CI]
│
▼ POST /tasks/{role}
┌─────────────────────────────────────────────────┐
│ aga (Rust container)                            │
│ ┌───────────────────────────────────────────┐   │
│ │ • roles.yaml (конфиг ролей + SSH-цели)    │   │
│ │ • trace.db (SQLite, WAL)                  │   │
│ │ • SSH Wrapper (async пул сессий)          │   │
│ │ • Agent Loop (LLM → parse → bash → trace) │   │
│ │ • Axum HTTP API + SSE                     │   │
│ └───────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
│ (transparent exec via SSH)
▼
[Target Host 1] [Target Host 2] [Target Host N]
(git, app, db...) (через read-only ключи)
```

---

## 🚀 Быстрый старт

### 1. Подготовка
```bash
mkdir -p config/keys data/work
# Сгенерируйте или скопируйте SSH-ключи
ssh-keygen -t ed25519 -f config/keys/git_ed25519 -N ""
chmod 600 config/keys/*
# Добавьте публичные ключи на целевые хосты в ~/.ssh/authorized_keys
```

### 2. Конфигурация ролей
Отредактируйте `config/roles.yaml` (пример ниже).

### 3. Запуск
```bash
docker compose up --build -d
docker compose logs -f
```

### 4. Тест
```bash
# Отправить задачу
curl -X POST http://localhost:8080/tasks/git-helper \
  -H "Content-Type: application/json" \
  -d '{"task": "Проверь статус репозитория и покажи последние 3 коммита"}'

# Посмотреть трассировку (замените UUID из ответа)
curl http://localhost:8080/trace/<task-uuid> | jq
```

## ⚙️ Конфигурация (config/roles.yaml)

```yaml
roles:
  git-helper:
    prompt: |
      Ты — ассистент по Git. Работай в репозитории.
      Разрешённые: git, ls, cat, head, wc, echo, diff.
      Перед push всегда делай `git status` и спрашивай подтверждение.
    target:
      host: "git.internal"
      port: 22
      user: "deploy"
      key_path: "/etc/aga/keys/git_ed25519"
      workdir: "/app/project"
    allowed_commands: ["git", "ls", "cat", "head", "wc", "echo", "diff"]
    max_iterations: 8
    llm:
      model: "qwen2.5-coder:7b"
      temperature: 0.2

  app-monitor:
    prompt: "Ты — монитор приложений. Ищи ошибки в логах, проверяй порты."
    target:
      host: "app.internal"
      port: 22
      user: "monitor"
      key_path: "/etc/aga/keys/app_ed25519"
      workdir: "/tmp"
    allowed_commands: ["journalctl", "ss", "grep", "head", "awk", "tail", "echo"]
    max_iterations: 12
    llm:
      model: "gpt-4o-mini"
      temperature: 0.3
```

## 🌐 API Reference

| Метод | Эндпоинт | Описание |
|-------|----------|----------|
| POST | `/tasks/{role}` | Запуск задачи. Тело: `{"task": "..."}`. Возвращает `{"status": "ok", "task_id": "uuid", "result": "..."}` |
| GET | `/trace/{task_id}` | Полная трассировка шагов (JSON-массив) |
| GET | `/human/pending` | SSE-стрим ожидающих human-запросов |
| POST | `/human/answer/{id}` | Ответ на вопрос. Тело: `{"answer": "..."}` |

## 🛡️ Безопасность и изоляция

- 🔒 `read_only: true` + `cap_drop: ALL` + `no-new-privileges` в Docker
- 📁 Все файловые операции строго ограничены `workdir` роли. Запрещён выход через `..`, `~`, `/`
- 🚫 Валидация `allowed_commands` перед отправкой в SSH. Блокировка пайпов, перенаправлений и опасных утилит
- ⏱️ Таймауты на каждую команду, лимит PID/CPU/RAM на контейнер
- 🔑 SSH-ключи монтируются `read_only`, никогда не попадают в логи, промпты или окружение процесса

## 📁 Структура проекта

```
aga/
├── docker-compose.yml
├── Cargo.toml
├── Dockerfile
├── config/
│   ├── roles.yaml
│   └── keys/
│       ├── git_ed25519
│       └── app_ed25519
├── data/                    # создаётся при первом запуске
│   ├── trace.db
│   └── work/
└── src/
    ├── main.rs
    ├── config.rs
    ├── ssh_wrapper.rs
    ├── trace.rs
    ├── llm.rs
    ├── agent.rs
    ├── human.rs
    └── server.rs
```

## 🔜 TODO / Roadmap

### 🧠 Ядро и агент
- Контекстное сжатие: автоматическая `summarize()` истории каждые N шагов
- Fallback-стратегии при `max_iterations`: graceful degradation vs abort
- Поддержка файлового ввода/вывода (upload/download через base64 или временные ссылки)
- Hot-reload `roles.yaml` без перезапуска контейнера
- Улучшенный парсер LLM-ответов (игнорирование markdown-обёрток, поддержка JSON-структур)
- Поддержка нескольких LLM-провайдеров на роль (fallback при rate-limit/ошибке)

### 📡 Наблюдаемость и API
- Полноценный SSE/WebSocket стрим: live-вывод шагов, статусы, human-запросы в реальном времени
- `/health` и `/metrics` (Prometheus-совместимые метрики: latency, errors, active tasks, SSH pool size)
- Пагинация и фильтрация для `/trace` (по статусу, роли, дате)
- Экспорт трассировки в JSON/Markdown/HTML для отчётов и аудита
- OpenAPI-спецификация + автогенерация клиентских SDK

### 🛡️ Безопасность и эксплуатация
- `sqlx offline`-режим (`sqlx prepare`) для воспроизводимых production-сборок
- Автоматическая очистка `/work/{role}/` и `VACUUM trace.db` (встроенный фоновый воркер)
- Rate-limiting на `/tasks` и `/human/answer` (защита от флуда и злоупотреблений)
- Аудит-лог всех SSH-соединений и изменений в `trace.db`
- Интеграция с seccomp-профилями на уровне контейнера (дополнительный слой к `cap_drop`)
- Поддержка `AllowGroups`/`ForceCommand` на стороне SSH-серверов для жёсткого контроля

### 🚀 DX и CI/CD
- GitHub Actions: тесты, `cargo audit`, `clippy`, `cargo fmt`, multi-arch build (amd64/arm64)
- Примеры интеграций: CLI-утилита `aga-cli`, Slack/Telegram бот для human-ответов
- Демо-режим: встроенная mock-роль без SSH для локальной отладки промптов
- Документация: Postman-коллекция, примеры промптов, чеклист безопасности

## 📜 Лицензия

MIT. Делайте что хотите. Ага. 🎬
