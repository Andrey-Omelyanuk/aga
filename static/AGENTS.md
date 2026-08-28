# Static (Web Client)

Одностраничный веб-клиент к REST API aga: `static/index.html` (HTML + CSS + JS).

## Boundaries
- Делает: вкладки Проекты / Воркстейшны / Сессии / Персонал / Чат; создание
  проектов, открытие/закрытие сессий на воркстейшнах, просмотр воркстейшнов
  и персонала; отправка сообщений, отображение дерева сообщений, артефакты,
  опрос для ответов реактивных агентов; вход через SSO (`/auth/login`).
- Не делает: никакой бизнес-логики — всё через REST API (`/users`, `/chats`,
  `/messages`, `/roles`, `/workstations`, `/projects`). Управление
  воркстейшнами (создание/удаление) из интерфейса недоступно — станции
  поднимает админ внешне.

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
- Тесты интерфейса — в `src/server.rs` (`web_client_views_workstations_and_personnel_without_management`,
  `web_client_has_sso_login_link`): проверяют, что клиент только показывает
  воркстейшны/персонал и умеет начинать вход через SSO.
- Кнопка «Открыть сессию» вызывает `POST /workstations/:id/session`; закрытие — `POST /chats/:id/close`.
