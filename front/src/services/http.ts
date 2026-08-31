import axios from 'axios';

// API_ENDPOINT подставляется в index.html при старте контейнера
// (replace-env.sh, см. Dockerfile). В dev-режиме — fallback по hostname.
export const API_BASE =
  (window as any)?.API_ENDPOINT && (window as any).API_ENDPOINT !== '<API_ENDPOINT>'
    ? (window as any).API_ENDPOINT
    : location.hostname === 'dev.localhost'
      ? 'http://api.localhost'
      : 'http://localhost:8080';

export const TOKEN_KEY = 'aga_token';

axios.defaults.baseURL = API_BASE;

let onUnauthorized: (() => void) | null = null;

export function setOnUnauthorized(cb: () => void): void {
  onUnauthorized = cb;
}

axios.interceptors.request.use((config) => {
  const token = localStorage.getItem(TOKEN_KEY);
  if (token) config.headers.Authorization = `Bearer ${token}`;
  return config;
});

axios.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error?.response?.status === 401) onUnauthorized?.();
    return Promise.reject(error);
  },
);

const http = axios;
export default http;