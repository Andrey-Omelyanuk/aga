import { execFileSync } from 'node:child_process';
// Доступ к стенду мимо браузера — только для подготовки, которой нет в
// веб-клиенте (привязка набора агентов к проекту) или которая не является
// предметом проверки. Всё, что проверяется, делается через UI.

export const CORE = process.env.E2E_CORE ?? 'http://localhost:8080';
export const KEYCLOAK = process.env.E2E_KEYCLOAK ?? 'http://localhost:8082';

// Инстанс dev-стенда = Linux-пользователь (несколько на машине): контейнеры
// именуются с префиксом aga-<user> (см. makefile/run.sh).
export const CONTAINER_PREFIX = process.env.E2E_CONTAINER_PREFIX ?? 'aga';
/** Имя контейнера сервиса инстанса (agent, centrifugo, …). */
export function container(service: string): string {
  return `${CONTAINER_PREFIX}-${service}`;
}

/** Адрес mock-LLM инстанса (контейнер создаёт run.sh). */
export const MOCK_LLM_URL = process.env.E2E_MOCK_LLM_URL ?? 'http://aga-llm-mock:8000/v1';

/** Учётки realm Keycloak стенда, совпадают с сидом (`aga seed`). */
export const USERS = {
  alice: { login: 'alice', password: 'alice-pass' }, // participant
  bob: { login: 'bob', password: 'bob-pass' }, // admin
} as const;
export type UserName = keyof typeof USERS;

/** Проект сида с набором ui-kit: агент `ui` обслуживает alice. */
export const PROJECT_GIT_URL = 'git@github.com:Andrey-Omelyanuk/mobx-model-ui.git';

export const authFile = (user: UserName) => `.auth/${user}.json`;

async function token(user: UserName): Promise<string> {
  const { login, password } = USERS[user];
  const res = await fetch(`${KEYCLOAK}/realms/aga/protocol/openid-connect/token`, {
    method: 'POST',
    body: new URLSearchParams({
      grant_type: 'password',
      client_id: 'aga',
      client_secret: 'aga-secret',
      username: login,
      password,
    }),
  });
  if (!res.ok) throw new Error(`Keycloak token for ${user}: ${res.status}`);
  return (await res.json()).access_token;
}

export async function api<T = any>(user: UserName, method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${CORE}${path}`, {
    method,
    headers: {
      authorization: `Bearer ${await token(user)}`,
      ...(body === undefined ? {} : { 'content-type': 'application/json' }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${await res.text()}`);
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

/** docker на хосте (сокет и CLI проброшены в контейнер Playwright, см. run.sh):
 *  тесты устойчивости перезапускают контейнеры стенда. */
export function docker(...args: string[]): string {
  return execFileSync('docker', args, { encoding: 'utf8' });
}
