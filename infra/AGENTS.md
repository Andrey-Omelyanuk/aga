# Infra

Docker Compose и окружение для запуска `aga`.

## Boundaries
- Делает: compose-определение сервиса, параметризация через `.env`, docker-образ (`Dockerfile`).
- Не делает: не содержит логики приложения (это `src/`), не управляет ролями агентов (`roles/`).

## Architecture
- `compose.yml` — единственный compose-файл, параметризуется `.env`.
- `Dockerfile` (в корне) — сборка Rust-бинарника мультистейджем.
- `.env.example` — шаблон, копируется в корневой `.env` через `make init`.

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс; compose напрямую не запускаем.
- Порт и LLM параметризуются из `.env` (по умолчанию `8080`, `http://ollama:11434/v1`).

## Verification
- `make init` — создаёт `.env` и `config/roles.yaml` из примеров.
- `make run-d` — поднимает сервис, `make ps` показывает состояние.
- Критерий: сервис отвечает на HTTP, агент выполняет задачу (`make`-targets).
