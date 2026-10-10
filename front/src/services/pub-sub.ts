import { Centrifuge } from 'centrifuge';
import { autorun } from 'mobx';
import http from './http';
import me from './me';
import { WS_URL } from './endpoints';

// Клиент Centrifuge для реального времени. Аналогично эталону (infobiz):
// серверное pub-sub подпитывает кэш моделей. В aga pub-sub-сервис — это
// centrifugo (поднят в dev-compose и k8s), общий канал `common` для всех
// аутентифицированных. Токен на подключение ядро выдаёт на `/connection-jwt/`.
// Адрес — из endpoints (pub-sub.<instance>.localhost:<port>).
// Канал обновлений чата: один общий для всех аутентифицированных (см. историю
// chat-websocket-centrifuge). Пока без пер-чатовых каналов.
const CHANNEL = 'common';

type MessageHandler = (data: any) => void;

class PubSub {
  centrifuge: Centrifuge | null = null;
  models: Record<string, any> = {};
  private handlers: MessageHandler[] = [];

  private async connect(): Promise<void> {
    if (this.centrifuge) return;
    this.centrifuge = new Centrifuge(WS_URL, {
      getToken: async () => {
        const response = await http.get('/connection-jwt/');
        return response.data.token;
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

  register_model(model_name: string, model_class: any): void {
    this.models[model_name] = model_class;
  }

  /// Подписка на события общего канала. Возвращает функцию отписки.
  on_message(handler: MessageHandler): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter((h) => h !== handler);
    };
  }

  async init(): Promise<void> {
    if (this.centrifuge) return;
    await me.ready;
    await this.connect();

    // disconnect on logout
    autorun(() => {
      if (!me.isAuthenticated && this.centrifuge) {
        this.centrifuge.disconnect();
        this.centrifuge = null;
      }
    });
  }
}

const pub_sub = new PubSub();
export default pub_sub;