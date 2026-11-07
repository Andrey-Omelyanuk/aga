import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { post, get } = vi.hoisted(() => ({ post: vi.fn(), get: vi.fn() }));
vi.mock('axios', async (orig) => {
  const actual: any = await orig();
  return { ...actual, default: { ...actual.default, create: () => ({ post }) } };
});
vi.mock('./http', () => ({
  default: { get },
  API_BASE: 'http://api',
  TOKEN_KEY: 'aga_token',
  setOnAuthError: vi.fn(),
  setOnUnauthorized: vi.fn(),
}));

import { AxiosError, AxiosHeaders } from 'axios';
import me, { REFRESH_KEY } from './me';

function httpError(status: number): AxiosError {
  return new AxiosError('fail', String(status), undefined, undefined, {
    status,
    statusText: '',
    headers: {},
    config: { headers: new AxiosHeaders() },
    data: null,
  });
}

describe('me', () => {
  beforeEach(() => {
    localStorage.clear();
    post.mockReset();
    get.mockReset();
  });
  afterEach(() => vi.useRealTimers());

  it('parallel refreshes share one /auth/refresh request', async () => {
    localStorage.setItem(REFRESH_KEY, 'r1');
    let resolve!: (v: unknown) => void;
    post.mockReturnValue(new Promise((r) => (resolve = r)));
    const a = me.refresh();
    const b = me.refresh();
    const c = me.refresh();
    resolve({ data: { access_token: 'fresh', refresh_token: 'r2' } });
    expect(await Promise.all([a, b, c])).toEqual(['fresh', 'fresh', 'fresh']);
    expect(post).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(REFRESH_KEY)).toBe('r2');
  });

  it('keeps tokens when /users/me fails for a non-auth reason', async () => {
    localStorage.setItem('aga_token', 't');
    localStorage.setItem(REFRESH_KEY, 'r');
    get.mockRejectedValue(httpError(503));
    await me.init();
    expect(localStorage.getItem('aga_token')).toBe('t');
    expect(me.show_login).toBe(false);
  });

  it('drops tokens and shows login on 401', async () => {
    localStorage.setItem('aga_token', 't');
    localStorage.setItem(REFRESH_KEY, 'r');
    get.mockRejectedValue(httpError(401));
    await me.init();
    expect(localStorage.getItem('aga_token')).toBeNull();
    expect(me.show_login).toBe(true);
  });
});
