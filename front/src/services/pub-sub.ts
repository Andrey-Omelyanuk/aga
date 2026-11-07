import { Centrifuge, UnauthorizedError } from 'centrifuge';
import { reaction, type IReactionDisposer } from 'mobx';
import http from './http';
import me from './me';

/** Адрес centrifugo: `window.PUBSUB_ENDPOINT` (подставляется при старте
 *  контейнера, как API_ENDPOINT) или fallback — `pub-sub.localhost` на порту
 *  страницы (dev-compose и k8s-стенд), `wss` на https-странице. */
export function pubSubUrl(
  loc: Pick<Location, 'protocol' | 'port'> = location,
  override: unknown = (window as any)?.PUBSUB_ENDPOINT,
): string {
  if (typeof override === 'string' && override && override !== '<PUBSUB_ENDPOINT>') {
    return override;
  }
  const scheme = loc.protocol === 'https:' ? 'wss' : 'ws';
  const port = loc.port ? `:${loc.port}` : '';
  return `${scheme}://pub-sub.localhost${port}/connection/websocket`;
}
// Канал обновлений чата: один общий для всех аутентифицированных (см. историю
// chat-websocket-centrifuge). Пока без пер-чатовых каналов.
const CHANNEL = 'common';

type MessageHandler = (data: any) => void;

/** Клиент Centrifuge для реального времени: общий канал `common`, токен на
 *  подключение ядро выдаёт на `/connection-jwt/`. Подключается только у
 *  вошедшего (или в локальном режиме без SSO) и отключается при выходе. */
class PubSub {
  centrifuge: Centrifuge | null = null;
  private handlers: MessageHandler[] = [];
  private watch: IReactionDisposer | null = null;

  private connect(): void {
    if (this.centrifuge) return;
    this.centrifuge = new Centrifuge(pubSubUrl(), {
      getToken: async () => {
        try {
          const response = await http.get('/connection-jwt/');
          return response.data.token;
        } catch (e) {
          // 401 (нет входа) или 404 (Centrifugo не настроен в ядре) — повтор
          // не поможет: UnauthorizedError останавливает переподключения.
          const status = (e as { response?: { status?: number } }).response?.status;
          if (status === 401 || status === 404) throw new UnauthorizedError('');
          throw e;
        }
      },
    });
    this.centrifuge.on('error', (ctx: any) => console.error(ctx));
    // Подписка серверная: connection-JWT несёт `channels: [common]`, и
    // Centrifugo сам подписывает соединение — в том числе заново после
    // переподключения. Клиентская подписка на тот же канал отвергалась
    // («already subscribed», 105), и после рестарта Centrifugo события в чат
    // переставали приходить (пойман e2e: 50-resilience).
    this.centrifuge.on('publication', (ctx: any) => {
      if (ctx.channel !== CHANNEL) return;
      for (const handler of this.handlers) {
        try {
          handler(ctx.data);
        } catch (e) {
          console.error('pub-sub handler failed', e);
        }
      }
    });
    this.centrifuge.connect();
  }

  private disconnect(): void {
    this.centrifuge?.disconnect();
    this.centrifuge = null;
  }

  /// Подписка на события общего канала. Возвращает функцию отписки.
  on_message(handler: MessageHandler): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter((h) => h !== handler);
    };
  }

  /// Ждёт результата входа (`me.init`) и держит подключение, пока
  /// пользователь вошёл: на экране входа не подключается (иначе
  /// `/connection-jwt/` → 401 и бесконечные переподключения), при выходе
  /// отключается.
  async init(): Promise<void> {
    if (this.watch) return;
    this.watch = reaction(
      () => me.is_ready && (me.user !== null || me.anonymous),
      (signedIn) => (signedIn ? this.connect() : this.disconnect()),
      { fireImmediately: true },
    );
    await me.ready();
  }
}

const pub_sub = new PubSub();
export default pub_sub;
