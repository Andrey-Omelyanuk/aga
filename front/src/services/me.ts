import { makeObservable, observable, runInAction } from 'mobx';
import { waitIsTrue } from 'mobx-model-ui';
import { API_BASE, TOKEN_KEY, setOnUnauthorized } from './http';
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

  constructor() {
    makeObservable(this);
    setOnUnauthorized(() => runInAction(() => (this.show_login = true)));
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
  }

  login(): void {
    window.location.href = this.loginUrl;
  }

  logout(): void {
    // Удаляем токен и уходим на /auth/logout ядра: оно сбрасывает HttpOnly-куку
    // aga_token и редиректит на end-session Keycloak (если настроен) → фронт.
    localStorage.removeItem(TOKEN_KEY);
    runInAction(() => {
      this.user = null;
      this.show_login = true;
    });
    window.location.href = this.logoutUrl;
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