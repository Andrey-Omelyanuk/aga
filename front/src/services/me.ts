import { makeObservable, observable, runInAction } from 'mobx';
import { waitIsTrue } from 'mobx-model-ui';
import { API_BASE, TOKEN_KEY, setOnUnauthorized } from './http';

class Me {
  @observable is_ready = false;
  @observable show_login = false;

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

  ready = () => waitIsTrue(this, 'is_ready');

  async init(): Promise<void> {
    this.readTokenFromHash();
    runInAction(() => (this.is_ready = true));
  }

  login(): void {
    window.location.href = this.loginUrl;
  }

  private readTokenFromHash(): void {
    const match = location.hash.match(/^#token=(.+)$/);
    if (match) {
      localStorage.setItem(TOKEN_KEY, match[1]);
      if (window.history.replaceState) {
        window.history.replaceState(null, '', location.pathname + location.search);
      }
    }
    runInAction(() => (this.show_login = !localStorage.getItem(TOKEN_KEY)));
  }
}

const me = new Me();
export default me;