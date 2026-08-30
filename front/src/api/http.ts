export const API_BASE =
  location.hostname === 'dev.localhost' ? 'http://api.localhost' : 'http://localhost:8080';

export const TOKEN_KEY = 'aga_token';

export class ApiError extends Error {
  constructor(
    public status: number,
    message = `HTTP ${status}`,
  ) {
    super(message);
  }
}

export interface HttpClientProps {
  onUnauthorized?: () => void;
}

export class HttpClient {
  readonly base = API_BASE;

  constructor(private props: HttpClientProps = {}) {}

  setOnUnauthorized(cb: () => void): void {
    this.props.onUnauthorized = cb;
  }

  private withToken(init?: RequestInit): RequestInit {
    const headers = new Headers(init?.headers);
    const token = localStorage.getItem(TOKEN_KEY);
    if (token) headers.set('Authorization', `Bearer ${token}`);
    return { ...init, headers };
  }

  async request(path: string, init: RequestInit = {}): Promise<Response> {
    const res = await fetch(this.base + path, this.withToken(init));
    if (res.status === 401) this.props.onUnauthorized?.();
    return res;
  }

  async json<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await this.request(path, init);
    if (!res.ok) {
      const body = await res.text().catch(() => undefined);
      throw new ApiError(res.status, body || undefined);
    }
    return res.json() as Promise<T>;
  }

  async send(path: string, method: string, body?: unknown): Promise<Response> {
    if (body === undefined) return this.request(path, { method });
    return this.request(path, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  }
}