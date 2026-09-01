import { makeObservable, observable, runInAction } from 'mobx';
import { waitIsTrue } from 'mobx-model-ui';
import { API_BASE, TOKEN_KEY, setOnAuthError, setOnUnauthorized } from './http';
import http from './http';

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
      if (this.isAuthenticated) localStorage.removeItem(TOKEN_KEY);
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
    // Удаляем токен и уходим на /auth/logout ядра: оно сбрасывает HttpOnly-куку
    // aga_token и редиректит на end-session Keycloak (если настроен) → фронт.
    if (this.refreshTimer !== null) window.clearTimeout(this.refreshTimer);
    localStorage.removeItem(TOKEN_KEY);
    runInAction(() => {
      this.user = null;
      this.show_login = true;
    });
    window.location.href = this.logoutUrl;
  }

  // Обработчик 401 (из http.ts): молча обновляем токен в скрытом iframe через
  // Keycloak (prompt=none). True — новый токен сохранён, запрос повторится.
  private authError = async (): Promise<boolean> => {
    if (!this.isAuthenticated) return false;
    const token = await this.refresh();
    if (!token) return false;
    localStorage.setItem(TOKEN_KEY, token);
    return true;
  };

  // Silent-обновление: скрытый iframe на /auth/login?prompt=none&silent=1.
  // Ядро отвечает страницей, которая передаёт новый токен через postMessage
  // (aga_sso_token) или сообщает об отсутствии активной SSO-сессии
  // (aga_sso_error). Возвращает новый токен или null.
  refresh(): Promise<string | null> {
    return new Promise((resolve) => {
      const iframe = document.createElement('iframe');
      iframe.style.display = 'none';
      iframe.setAttribute('aria-hidden', 'true');
      iframe.src = `${API_BASE}/auth/login?prompt=none&silent=1`;
      let settled = false;
      const done = (token: string | null): void => {
        if (settled) return;
        settled = true;
        window.removeEventListener('message', onMessage);
        iframe.remove();
        resolve(token);
      };
      const onMessage = (event: MessageEvent): void => {
        if (event.source !== iframe.contentWindow) return;
        const data = event.data as { type?: string; token?: string } | null;
        if (data?.type === 'aga_sso_token' && typeof data.token === 'string') done(data.token);
        else if (data?.type === 'aga_sso_error') done(null);
      };
      window.addEventListener('message', onMessage);
      document.body.appendChild(iframe);
      window.setTimeout(() => done(null), 10000);
    });
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
    const match = location.hash.match(/^#token=(.+)$/);
    if (match) {
      localStorage.setItem(TOKEN_KEY, match[1]);
      if (window.history.replaceState) {
        window.history.replaceState(null, '', location.pathname + location.search);
      }
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