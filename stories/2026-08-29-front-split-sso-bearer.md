# Фронт отдельным сервисом, SSO через Bearer

Веб-клиент вынесен из ядра в отдельный сервис `front/` (nginx, свой Dockerfile),
ядро переехало в `main/` — теперь это монорепо из двух деплоев. Раньше ядро
раздавало `static/index.html` само (`ServeDir`) и веб-клиент ходил в API
same-origin, получая токен через httpOnly-cookie `aga_token`. После разделения
SPA живёт на `dev.localhost`, а API — на `api.localhost`, поэтому same-origin
сломался: cookie на чужой хост не уходит. Фронт теперь хранит токен в
`localStorage` и шлёт его заголовком `Authorization: Bearer`, а ядро разрешает
CORS-запросы только со своего веб-клиента.

## Поведение
- ядро (`main/`) — чистый API-сервис: статику не раздаёт, `front/index.html`
  отдаёт отдельный под nginx (`infra/k8s/front/`)
- `dev.localhost` ведёт на фронт, `api.localhost` — на ядро (ingress)
- вход: SPA переходит на `/auth/login` ядра → Keycloak → `/auth/callback`
  редиректит на `AGA_FRONT_URL/#token=...` (токен фрагментом URL, cookie
  ставится для прямых api.localhost-клиентов)
- SPA кладёт токен в `localStorage`, все запросы (`api()`) шлют
  `Authorization: Bearer <token>`; при 401 показывается кнопка «Войти через SSO»
- ядро отвечает CORS-заголовками только на origin `AGA_FRONT_URL` (в стенде
  `http://dev.localhost`, локально `http://localhost:8081`)
- локальная разработка: `make run` (ядро на :8080) + `make run-front`
  (фронт на :8081) — SPA сама выбирает API: `api.localhost` в стенде,
  `localhost:8080` локально

## Проверка
- `make test` — тесты ядра (Bearer-токен уже принимался и раньше, cookie
  остаётся как запасной путь)
- `make k8s-verify` — добавлена проверка CORS: `GET /users` с
  `Origin: http://dev.localhost` возвращает `Access-Control-Allow-Origin`