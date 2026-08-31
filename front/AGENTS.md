# front — Веб-клиент (SPA)

## Overview
React-приложение: UI к REST API ядра. Собирается Vite, раздаётся отдельным
сервисом (nginx). Отдельный деплой и образ — ядро его не знает. Эталон
структуры и паттернов — `infobiz/front/web` (BluePrint-приложение на
mobx-model-ui); здесь те же приёмы, адаптированные под Rust-API ядра и
shadcn/ui.

## Boundaries
- **Делает:** вкладки Проекты / Наборы / Воркстейшны / Сессии / Персонал /
  Файлы / Чат; создание проектов, открытие/закрытие сессий на воркстейшнах,
  просмотр воркстейшнов и персонала; активный проект в шапке (ObjectInput c
  URL-sync `?project=`), на странице воркстейшнов — два списка (на текущем
  проекте / остальные) с действиями «Отпустить» (ws становится свободным) и
  «Занять» (свободный ws привязывается к текущему проекту); просмотр
  содержимого проекта воркстейшна (дерево файлов, текст, картинки/видео/аудио
  через blob-URL); страница «Наборы»: список наборов агентов, создание/удаление
  и редактор состава — агенты, их территория, данные скиллы и команды с
  версиями, инструменты; отправка сообщений, дерево сообщений, артефакты,
  опрос для ответов реактивных агентов; вход через SSO.
- **Не делает:** никакой бизнес-логики — всё через REST API ядра (`/users`,
  `/chats`, `/messages`, `/agent-sets`, `/workstations`, `/projects`). Не хранит
  состояние на сервере, не авторизует (только шлёт Bearer-токен ядру).
  Управление воркстейшнами (создание/удаление) из интерфейса недоступно —
  станции поднимает админ внешне.

## Tech Stack
- React, TypeScript, Vite (сборка + дев-сервер), react-router.
- **axios** — HTTP-клиент (один инстанс, `services/http.ts`).
- **mobx-model-ui** — состояние и UI-модели: `@api`-модели, Repository/Adapter,
  Query, Variable/ObjectInput, формы.
- **centrifuge** — клиент реального времени (`services/pub-sub.ts`).
- Tailwind CSS + shadcn/ui — дизайн-система.
- Storybook — витрина компонентов; vitest — unit-тесты.
- nginx — раздача собранного `dist/` (`front/Dockerfile`).

## Architecture
```
front/
├── AGENTS.md            # этот уровень
├── index.html           # entry Vite; window.API_ENDPOINT (подставляется при старте)
├── Dockerfile           # build → nginx; replace-env.sh вписывает API_ENDPOINT
├── replace-env.sh       # sed <API_ENDPOINT> → $API_ENDPOINT в index.html
├── src/
│   ├── main.tsx         # маршруты (lazy-страницы), BrowserRouter
│   ├── index.css        # tailwind
│   ├── services/        # http (axios), http-adapter (@api + HttpAdapter),
│   │                    #   me (SSO-токен), pub-sub (centrifuge)
│   ├── utils/           # mobx.ts (хуки), useMobX_ORM (URL-sync), toaster,
│   │                    #   dates/html; barrel index.ts
│   ├── models/          # @api-модели по доменам (barrel в index.ts):
│   │                    #   core/ (User), project/ (Project, AgentSet),
│   │                    #   workstation/, chat/ (Chat), files/ (FileBrowser)
│   ├── components/
│   │   ├── core/        # Page (гейт готовности Query), AppHeader, Toaster,
│   │   │                #   inputs/ (StringInput, SelectInput, DeleteObjectButton)
│   │   ├── project/ workstation/ chat/ files/   # доменные компоненты
│   │   │                # project/: AgentSetList + AgentSetEditor (страница «Наборы»)
│   │   └── ui/          # shadcn/ui-примитивы (Button, Input, Select, ...)
│   ├── pages/
│   │   ├── app/         # layout.tsx + projects/agentSets/workstations/sessions/
│   │   │                #   personnel/files/chat
│   │   └── 404.tsx
│   ├── store-hooks.ts   # AppContext: { activeProject } (ObjectInput)
│   └── lib/utils.ts     # cn() (tailwind-merge)
└── stories/             # *.stories.tsx (Storybook)
```

## Patterns
- **HTTP:** один axios-клиент (`services/http.ts`): `API_BASE` из
  `window.API_ENDPOINT` (старт контейнера) или fallback по hostname
  (`api.localhost` / `localhost:8080`); request-interceptor добавляет
  `Authorization: Bearer <token>` из `localStorage[aga_token]`; на 401 —
  `me.show_login = true`.
- **Вход (гейт):** `me.init()` читает токен из `#token=` в hash, сохраняет в
  localStorage и пробует `GET /users/me` — это и проба доступа, и данные
  текущего пользователя: 401 без токена → ядро требует SSO — layout показывает
  полноэкранную страницу входа (`pages/app/login.tsx`), приложение скрыто;
  200 без токена — локальный режим без SSO (аноним-супер), UI открыт.
  С токеном та же проба валидирует его (просроченный удаляется → экран входа).
  Текущий пользователь хранится в `me.user` и выводится в шапке (`AppHeader`)
  с кнопкой «Выйти» (`me.logout()` → `/auth/logout` ядра: сброс HttpOnly-куки
  и end-session Keycloak, если настроен).
- **Модели:** класс + `@api('endpoint')` + `@model`; поля через `@id`/`@field`.
  `@api` назначает `defaultRepository.adapter = new HttpAdapter(endpoint)`
  (декоратор снизу-вверх: сначала `@model`, потом `@api`). Ручных registry нет.
- **HttpAdapter** (`services/http-adapter.ts`): JSON (не multipart — ядро Rust),
  методы `create/update/delete/get/action/modelAction/find/load/getTotalCount/
  getDistinct` + `getURLSearchParams`. URL строится как `endpoint/{id}/{action}/`.
- **Запросы:** страницы собирают Query через хуки `useQuery/useQueryPage/
  useQueryCacheSync` (`utils/mobx.ts`); фильтры — `EQ/IN/AND` над Variable.
  `Page` ждёт готовности переданных Query.
- **Ввод:** `Variable`/`ObjectInput` с `syncURL`; URL-синхронизация —
  `useMobX_ORM()` (однажды в `pages/app/layout.tsx`): конфигурирует
  `config.UPDATE_SEARCH_PARAMS`/`config.WATCH_URL_CHANGES`.
- **Формы:** встроенные mobx-model-ui (`SaveObjectForm`, `DeleteObjectForm`,
  `ActionObjectForm`); `useForm` держит жизненный цикл формы.
- **Активный проект:** `ObjectInput` с `options=projects`, `syncURL='project'`,
  `autoReset=autoResetId` — создаётся в layout, раздаётся через `AppContext`;
  на него реактивно смотрят страницы (фильтры воркстейшнов/сессий).
- **Уведомления:** `utils/toaster.ts` + `<Toaster/>` в layout (вместо `alert`).
- **Чат:** `useQuery(Chat, {autoupdate:true})` для списка; текущий чат —
  `loadChatDetail(id)` (`GET /chats/:id` отдаёт обёртку `{chat, messages,
  participants}` — разворачивается в плоский объект модели). Реактивных агентов
  ждём опросом (`setInterval`, pub-sub-сервер пока не поднят).
- **Вход:** `me.init()` читает токен из `#token=` в hash; после входа —
  в localStorage; без токена и с включённым SSO ядра — полноэкранная страница
  входа (`pages/app/login.tsx`), приложение не рендерится (см. «Вход (гейт)» выше).

## Non-Obvious Rules
- **`@api` работает через статику:** декоратор присваивает
  `cls.defaultRepository.adapter` (не через `getModelDescriptor().defaultRepository` —
  в mobx-model-ui 0.4.1 дескриптор его не имеет).
- **`Input` не экспортируется** из mobx-model-ui 0.4.1 — используем `Variable`.
  `required` — свойство TypeDescriptor (`STRING({required:true})`), не аргумент
  Variable.
- **`updateFromRaw` не затирает отсутствующие поля** — поэтому повторные
  загрузки списка `/chats` (без `messages`/`participants`) не стирают сообщения,
  подтянутые деталью.
- **API детали чата — обёртка:** `GET /chats/:id` → `{chat, messages,
  participants}`, список → плоские строки. Модель совпадает со списком;
  деталь разворачивает `loadChatDetail` (`models/chat/Chat.ts`).
- **API_ENDPOINT в runtime:** index.html содержит `window.API_ENDPOINT =
  '<API_ENDPOINT>'`; Dockerfile/`replace-env.sh` подставляют значение при
  старте контейнера (k8s: `env: API_ENDPOINT` в deployment). В dev — fallback.
- **Действия воркстейшнов/сессий** — `model.action(name, kwargs)` →
  `POST endpoint/{id}/{name}/`. После действия страница перезагружает Query
  (`shadowLoad()`), т.к. ядро не всегда возвращает обновлённый объект.
- **`WS` pub-sub:** клиент centrifuge готов (`services/pub-sub.ts`), бэкенд
  (`centrifugo` + JWT-эндпоинты) не поднят — подключение деградирует молча,
  чат живёт на polling. Токен для pub-sub берётся из `http` (тот же axios).

## Verification
- Локально: `make run-front` (vite dev) — страница грузится без ошибок консоли;
  API-запросы уходят на ядро.
- Unit (vitest): модели/фильтры (`workstation.test.ts`), утилиты
  (`html.test.ts`, `file-browser.test.ts`), состав набора на странице
  (`project/agent-set.test.tsx` — агент, его территория, данные скиллы/команды
  с версиями и инструменты видны в редакторе). Запуск: `npm test`.
- Сборка: `npm run build` (tsc --noEmit + vite build) без ошибок.
- Компоненты: `stories/` — у каждого экранного/переиспользуемого компонента
  есть `*.stories.tsx`; Storybook строится без ошибок (`npm run build-storybook`).
- Стенд: `curl http://dev.localhost/` возвращает собранный `dist/index.html`;
  ядро (`api.localhost`) отвечает с CORS для `Origin: http://dev.localhost`.
- Критерий готовности: фреймворк компилируется, тесты проходят, страницы
  воркстейшнов показываются и отпускаются/занимаются, персонал показывается,
  есть логин-ссылка.

## Dependencies
- Ядро aga (`main/`) — REST API + `/auth/*`.
- npm-пакеты: react, axios, centrifuge, mobx, mobx-model-ui, react-router-dom,
  tailwind, shadcn/ui (clsx, tailwind-merge), storybook, vitest.
- nginx (образ), стенд — `infra/k8s/`.