# Static (Web Client)

Одностраничный веб-клиент к REST API aga: `static/index.html` (HTML + CSS + JS).

## Boundaries
- Делает: выбор ролей/агентов, сессии (корневые чаты), отправка сообщений,
  отображение дерева сообщений, кнопка шаринга, артефакты, опрос для ответов
  реактивных агентов.
- Не делает: никакой бизнес-логики — всё через REST API (`/users`, `/chats`,
  `/messages`, `/roles`, `/workstations`).

## Tech Stack
- Один файл `index.html`, без сборщика (vanilla JS), раздаётся сервером из `static/`.

## Architecture
- `index.html` — весь клиент: разметка, стили, логика на `fetch`.

## Patterns
- Все запросы — `fetch` к `API_BASE` (пустой для same-origin).
- Сессии = корневые чаты (`POST /chats`); ответы агентов приходят асинхронно —
  клиент опрашивает `GET /chats/:id` и перерисовывает при изменении.

## Verification
- `make run` и открыть `http://localhost:8080/` — страница грузится без ошибок консоли.
- Кнопка «Шарить» вызывает `POST /messages/:id/share` с корректным `target_chat_id`.
