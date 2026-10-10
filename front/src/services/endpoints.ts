// Адреса ядра и pub-sub выводятся из адреса страницы. Dev-стенд открывается по
// dev.<instance>.localhost:<port>, где instance — Linux-пользователь (несколько
// инстансов на одной машине различаются хостом и портом): ядро живёт на
// api.<instance>.localhost:<port>, Centrifugo — на
// pub-sub.<instance>.localhost:<port>. Поэтому хост и порт берём из location,
// а не хардкодим — иначе инстансы перепутали бы друг друга.

// API_ENDPOINT подставляется в index.html при старте контейнера
// (replace-env.sh, см. Dockerfile) — для прод-образа; в dev это заглушка.
const API_PLACEHOLDER = '<API_ENDPOINT>';

const { hostname, port, protocol } = location;

// Метка инстанса: "alice.localhost" для "dev.alice.localhost", "localhost"
// для "localhost" / "dev.localhost".
const suffix = hostname.slice(hostname.indexOf('.') + 1) || hostname;
// Стандартные порты в URL не пишем (k8s-стенд ходит по :80 без порта).
const portSuffix = port && port !== '80' && port !== '443' ? `:${port}` : '';

// <prefix>.<instance>.localhost для известных имён dev-стенда; иначе fallback
// (локальный make run-front: vite на :8081, ядро на localhost:8080).
function sibling(prefix: string, fallback: string): string {
  const head = hostname.split('.')[0];
  if (head === 'dev' || head === 'api' || head === 'pub-sub' || head === 'auth') {
    return `${prefix}.${suffix}${portSuffix}`;
  }
  return fallback;
}

const injected = (window as { API_ENDPOINT?: string }).API_ENDPOINT;
export const API_BASE =
  injected && injected !== API_PLACEHOLDER
    ? injected
    : `http://${sibling('api', 'localhost:8080')}`;

const wsProto = protocol === 'https:' ? 'wss' : 'ws';
export const WS_URL = `${wsProto}://${sibling('pub-sub', 'pub-sub.localhost')}/connection/websocket`;
