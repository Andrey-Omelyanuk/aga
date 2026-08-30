# front — Веб-клиент (SPA)

## Overview
React-приложение: UI к REST API ядра. Собирается Vite, раздаётся отдельным
сервисом (nginx). Отдельный деплой и образ — ядро его не знает.

## Boundaries
- **Делает:** вкладки Проекты / Воркстейшны / Сессии / Персонал / Файлы / Чат;
  создание проектов, открытие/закрытие сессий на воркстейшнах, просмотр
  воркстейшнов и персонала; активный проект в шапке (селект), на странице
  воркстейшнов — два списка (на текущем проекте / остальные) с действиями
  «Отпустить» (ws становится свободным) и «Занять» (свободный ws привязывается
  к текущему проекту); просмотр содержимого проекта воркстейшна (дерево
  файлов, текст с подсветкой синтаксиса, картинки/видео/аудио через blob-URL);
  отправка сообщений, отображение дерева сообщений, артефакты, опрос для ответов
  реактивных агентов; вход через SSO.
- **Не делает:** никакой бизнес-логики — всё через REST API ядра (`/users`,
  `/chats`, `/messages`, `/agent-sets`, `/workstations`, `/projects`). Не хранит
  состояние на сервере, не авторизует (только шлёт токен ядру). Управление
  воркстейшнами (создание/удаление) из интерфейса недоступно — станции
  поднимает админ внешне. Внутренние папки `src/*` уровнями не являются — их
  границы и проверка раскрыты здесь, в `Architecture`/`Patterns`.

## Tech Stack
- React, TypeScript, Vite (сборка + дев-сервер).
- mobx-model-ui — состояние и UI-модели (сто-модели домена).
- Tailwind CSS — дизайн; shadcn/ui — компоненты дизайн-системы.
- Storybook — витрина компонентов.
- nginx — раздача собранного `dist/` (`front/Dockerfile`).

## Architecture
```
front/
├── AGENTS.md            # этот уровень
├── package.json
├── vite.config.ts       # сборка; дев-прокси на ядро (api.localhost)
├── tsconfig.json
├── tailwind.config.ts
├── components.json      # конфиг shadcn/ui
├── index.html           # entry Vite (собирается в dist/)
├── .storybook/          # конфиг Storybook
├── src/
│   ├── main.tsx         # точка входа, Provider
│   ├── models/          # mobx-model-ui сто: Auth, Project, Workstation,
│   │                    #   Session/Chat, Message, AgentSet, File
│   ├── api/             # fetch-клиент к REST ядра: API_BASE + Bearer, 401-хук
│   ├── components/      # UI: shadcn/ui + переиспользуемые (FileView, MessageTree)
│   ├── pages/           # вкладки: Projects / Workstations / Sessions /
│   │                    #   People / Files / Chat
│   └── styles/          # глобальные стили + tailwind
├── stories/             # *.stories.tsx рядом с компонентами
└── Dockerfile           # build → nginx (раздаёт dist/)
```

## Patterns
- Все запросы — один клиент `src/api/` (`API_BASE` — адрес ядра,
  `http://api.localhost` для стенда; `Authorization: Bearer <token>`; на 401 —
  «Войти через SSO»).
- Состояние — модели в `models/` (mobx-model-ui). Страницы — тонкие: читают
  модели через observer/hooks, сами данных не носят.
- Компоненты дизайн-системы — shadcn/ui; экраны собираются из них.
- Сессии = корневые чаты (`POST /chats`); ответы агентов приходят асинхронно —
  клиент опрашивает `GET /chats/:id` и перерисовывает при изменении модели.
- Файлы: вкладка «Файлы» читает `GET /workstations/:id/tree` (дерево лениво по
  папкам) и `GET /workstations/:id/file` (только чтение). Текст в `<pre><code>`
  с подсветкой, медиа — blob-URL в `<img>`/`<video>`/`<audio>`. Редактирования
  в просмотрщике нет — правки через чат с LLM.
- Вход: переход на `/auth/login` ядра, после входа токен возвращается клиенту и
  хранится в `localStorage` (модель Auth).

## Verification
- Локально: `make run-front` (vite dev) — страница грузится без ошибок консоли;
  API-запросы уходят на ядро.
- Unit: (vitest) — модели (actions/views) и api-клиент (заголовки, обработка 401).
- Компоненты: в `stories/` у каждого экранного/переиспользуемого компонента есть
  `*.stories.tsx`; Storybook строится без ошибок.
- Стенд: `curl http://dev.localhost/` возвращает собранный `dist/index.html`;
  ядро (`api.localhost`) отвечает с CORS для `Origin: http://dev.localhost`.
- Тесты контента интерфейса (воркстейшны показываются и отпускаются/занимаются,
  персонал только показывается, есть логин-ссылка) живут здесь, а не в ядре.

## Dependencies
- Ядро aga (`main/`) — REST API + `/auth/*`.
- npm-пакеты: react, Vite, mobx-model-ui, tailwind, shadcn/ui, storybook.
- nginx (образ), стенд — `infra/k8s/`.