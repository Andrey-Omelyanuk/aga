import axios from 'axios';
import { makeObservable, observable, runInAction } from 'mobx';
import { waitIsTrue } from 'mobx-model-ui';
import { API_BASE, TOKEN_KEY, setOnAuthError, setOnUnauthorized } from './http';
import http from './http';

// Refresh-токен Keycloak: живёт дольше access-токена и позволяет обновлять его
// молча через /auth/refresh ядра, не завися от SSO-куки в кросс-сайтовом iframe.
export const REFRESH_KEY = 'aga_refresh';

// Отдельный клиент для /auth/refresh: без response-интерцептора http.ts, чтобы
// 401 на недействительном refresh-токене не уходил в рекурсивный refresh.
const authHttp = axios.create({ baseURL: API_BASE });

export interface MeUser {
  id: number;
  name: string;
  kind: string;
  is_super_user: boolean;
}

class Me {
  @observable is_ready = false;
  // Показывать ли экран входа (SSO включён, валидного токена нет).
  @observable show_login = false;
  // Бэкенд пускает анонимов (SSO выключен — локальный режим, аноним-супер).
  @observable anonymous = false;
  // Текущий пользователь из GET /users/me (null в локальном режиме без SSO).
  @observable user: MeUser | null = null;
  private refreshTimer: number | null = null;

  constructor() {
    makeObservable(this);
    setOnUnauthorized(() => runInAction(() => (this.show_login = true)));
    setOnAuthError(this.authError);
  }

  get isAuthenticated(): boolean {
    return Boolean(localStorage.getItem(TOKEN_KEY));
  }

  get loginUrl(): string {
    return `${API_BASE}/auth/login`;
  }

  get logoutUrl(): string {
    return `${API_BASE}/auth/logout`;
  }

  ready = () => waitIsTrue(this, 'is_ready');

  async init(): Promise<void> {
    this.readTokenFromHash();
    // /users/me — и проба доступа, и данные текущего пользователя: 401 без
    // токена → нужен вход; 200 без токена → локальный режим (аноним-супер).
    // Просроченный токен interceptor в http.ts сначала попробует молча обновить.
    let anonymous = false;
    let user: MeUser | null = null;
    try {
      const { data } = await http.get('/users/me');
      user = data;
      anonymous = !this.isAuthenticated;
    } catch {
      if (this.isAuthenticated) {
        localStorage.removeItem(TOKEN_KEY);
        localStorage.removeItem(REFRESH_KEY);
      }
    }
    runInAction(() => {
      this.user = user;
      this.anonymous = anonymous;
      this.show_login = !this.isAuthenticated && !anonymous;
      this.is_ready = true;
    });
    if (user && this.isAuthenticated) this.scheduleRefresh();
  }

  login(): void {
    window.location.href = this.loginUrl;
  }

  logout(): void {
    // Удаляем токены и уходим на /auth/logout ядра: оно сбрасывает HttpOnly-куку
    // aga_token и редиректит на end-session Keycloak (если настроен) → фронт.
    if (this.refreshTimer !== null) window.clearTimeout(this.refreshTimer);
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(REFRESH_KEY);
    runInAction(() => {
      this.user = null;
      this.show_login = true;
    });
    window.location.href = this.logoutUrl;
  }

  // Обработчик 401 (из http.ts): молча обновляем access-токен по refresh-токену
  // через /auth/refresh ядра. True — новый токен сохранён, запрос повторится.
  private authError = async (): Promise<boolean> => {
    if (!this.isAuthenticated) return false;
    const token = await this.refresh();
    if (!token) return false;
    localStorage.setItem(TOKEN_KEY, token);
    return true;
  };

  // Обновление токена: ядро меняет refresh-токен на свежую пару у Keycloak
  // (grant_type=refresh_token). Возвращает новый access-токен или null.
  // Кросс-сайтовые куки и silent-iframe не участвуют — браузерная блокировка
  // third-party cookies на обновление не влияет.
  async refresh(): Promise<string | null> {
    const refreshToken = localStorage.getItem(REFRESH_KEY);
    if (!refreshToken) return null;
    try {
      const { data } = await authHttp.post('/auth/refresh', { refresh_token: refreshToken });
      const token = data?.access_token;
      if (typeof token === 'string') {
        localStorage.setItem(TOKEN_KEY, token);
        if (typeof data.refresh_token === 'string') {
          localStorage.setItem(REFRESH_KEY, data.refresh_token);
        }
        return token;
      }
    } catch {
      // Недействительный/истёкший refresh-токен — нужен повторный вход.
    }
    return null;
  }

  // Плановое обновление до истечения токена (за 30 секунд), чтобы 401 не
  // прерывал работу. После успешного обновления — перепланирование по exp.
  private scheduleRefresh(): void {
    if (this.refreshTimer !== null) window.clearTimeout(this.refreshTimer);
    const token = localStorage.getItem(TOKEN_KEY);
    if (!token) return;
    const expMs = tokenExpiryMs(token);
    const delay = Math.max(0, expMs - Date.now() - 30_000);
    if (delay <= 0) return;
    this.refreshTimer = window.setTimeout(() => {
      void this.refresh().then((fresh) => {
        if (fresh) {
          localStorage.setItem(TOKEN_KEY, fresh);
          this.scheduleRefresh();
        }
      });
    }, delay);
  }

  private readTokenFromHash(): void {
    const params = new URLSearchParams(location.hash.replace(/^#/, ''));
    const token = params.get('token');
    if (!token) return;
    localStorage.setItem(TOKEN_KEY, token);
    // Refresh-токен (второй параметр фрагмента) нужен для молчаливого
    // обновления access-токена после входа.
    const refresh = params.get('refresh');
    if (refresh) localStorage.setItem(REFRESH_KEY, refresh);
    else localStorage.removeItem(REFRESH_KEY);
    if (window.history.replaceState) {
      window.history.replaceState(null, '', location.pathname + location.search);
    }
  }
}

const me = new Me();
export default me;

/// Момент истечения JWT (exp в миллисекундах). Разбираем payload без
/// криптографии — нужен только для планирования обновления.
function tokenExpiryMs(token: string): number {
  try {
    const payload = token.split('.')[1] ?? '';
    const json = JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/')));
    return typeof json.exp === 'number' ? json.exp * 1000 : 0;
  } catch {
    return 0;
  }
}