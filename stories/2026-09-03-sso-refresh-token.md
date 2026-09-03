# SSO: обновление токена через refresh-токен, а не silent-iframe

Вход в веб-клиент работал, но при перезагрузке страницы после истечения
access-токена (5 минут по умолчанию) снова показывался экран входа. Молчаливое
обновление делал скрытый iframe на `/auth/login?prompt=none&silent=1`: Keycloak
возвращал новый токен, только если его SSO-сессия жива, — а она определяется
кукой на `auth.localhost`. Страница SPA живёт на `dev.localhost`, поэтому чтение
куки внутри iframe — это third-party cookie context, который браузеры блокируют
по умолчанию. В итоге `prompt=none` возвращал `login_required` и фронт просил
войти заново.

Перевели обновление на refresh-токен: `/auth/callback` теперь возвращает пару
токенов фрагментом URL (`#token=...&refresh=...`), фронт хранит refresh-токен в
localStorage и при 401 обновляет access-токен через новую ручку ядра
`POST /auth/refresh` (обмен `grant_type=refresh_token` у Keycloak на стороне
сервера). Браузерные куки и их кросс-сайтовая блокировка не участвуют.

## Поведение
- после входа `/auth/callback` отдаёт веб-клиенту оба токена: `#token=...&refresh=...`
- фронт хранит refresh-токен в `localStorage[aga_refresh]`; при 401 любого
  запроса молча обновляет access-токен через `POST /auth/refresh` и повторяет запрос
- `/auth/refresh` меняет refresh-токен на свежую пару у Keycloak
  (`grant_type=refresh_token`, `client_id`/`client_secret` из sso-конфига),
  отдаёт `{access_token, refresh_token, expires_in}` и ставит cookie `aga_token`
  (прямые api.localhost-клиенты свежие)
- недействительный/истёкший refresh-токен — 401 → экран входа
- плановое обновление до истечения токена (`scheduleRefresh`, за 30 секунд)
  работает тем же путём
- Keycloak-реалм: `accessTokenLifespan` 300 с, `refreshTokenLifespan` и
  SSO-сессия — сутки (dev-стенд)
- silent-флоу (`prompt=none&silent=1`) в ядре остаётся как запасной путь, но
  фронт им больше не пользуется

## Проверка
- `make test` — новые тесты роутера: `refresh_exchanges_token_and_sets_cookie`,
  `refresh_rejects_invalid_token`, `refresh_requires_sso_config` (мок
  token-эндпоинта на случайном порту)
- `make dev-verify` — SSO работает, аноним получает 401
- ручная: вход через `dev.localhost`, перезагрузка страницы после истечения
  access-токена не требует повторного входа