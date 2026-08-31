import { Centrifuge } from 'centrifuge';
import { autorun } from 'mobx';
import http from './http';
import me from './me';

// Клиент Centrifuge для реального времени. Аналогично эталону (infobiz):
// серверное pub-sub подпитывает кэш моделей. В aga pub-sub-сервис
// (centrifugo) пока не поднят — клиент готов, подключение деградирует
// молча; чат продолжает работать через polling в pages/app/chat.tsx.
const WS_URL = 'ws://pub-sub.localhost/connection/websocket';

class PubSub {
  centrifuge: Centrifuge | null = null;
  models: Record<string, any> = {};

  private async connect(): Promise<void> {
    if (this.centrifuge) return;
    this.centrifuge = new Centrifuge(WS_URL, {
      getToken: async () => {
        const response = await http.get('/connection-jwt/');
        return response.data.token;
      },
    });
    this.centrifuge.on('error', (ctx: any) => console.error(ctx));
    this.centrifuge.connect();
  }

  register_model(model_name: string, model_class: any): void {
    this.models[model_name] = model_class;
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