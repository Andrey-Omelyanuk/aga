import axios, { type AxiosError, type InternalAxiosRequestConfig } from 'axios';

// Адрес ядра — из общего вывода endpoints (location/API_ENDPOINT); см. там.
import { API_BASE } from './endpoints';
export { API_BASE };

export const TOKEN_KEY = 'aga_token';

axios.defaults.baseURL = API_BASE;

// Флаг повторного запроса (после успешного обновления токена), чтобы не
// зациклиться: ещё один 401 на повторе — показываем вход.
interface RetriedConfig extends InternalAxiosRequestConfig {
  _agaRetried?: boolean;
}

let onUnauthorized: (() => void) | null = null;
// Попытка молча обновить токен (SSO silent refresh); true — токен обновлён,
// запрос можно повторить; false — обновить не удалось, нужен вход.
let onAuthError: ((error: unknown) => Promise<boolean>) | null = null;

export function setOnUnauthorized(cb: () => void): void {
  onUnauthorized = cb;
}

export function setOnAuthError(cb: (error: unknown) => Promise<boolean>): void {
  onAuthError = cb;
}

axios.interceptors.request.use((config) => {
  const token = localStorage.getItem(TOKEN_KEY);
  if (token) config.headers.Authorization = `Bearer ${token}`;
  return config;
});

axios.interceptors.response.use(
  (response) => response,
  async (error: AxiosError) => {
    if (error?.response?.status === 401) {
      // Повторный 401 (уже после обновления токена) — обновление не помогло,
      // показываем вход. Иначе пробуем молча обновить токен и повторить запрос.
      const config = error.config as RetriedConfig | undefined;
      if (!config?._agaRetried && onAuthError) {
        const recovered = await onAuthError(error);
        if (recovered) {
          const retry = { ...config, _agaRetried: true } as RetriedConfig;
          // Заголовок Authorization подставит request-interceptor из localStorage.
          return axios.request(retry);
        }
      }
      onUnauthorized?.();
    }
    return Promise.reject(error);
  },
);

const http = axios;
export default http;