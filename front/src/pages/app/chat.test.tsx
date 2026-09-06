import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import ChatPage from './chat';
import pub_sub from '@/services/pub-sub';
import http from '@/services/http';
import { loadChatDetail } from '@/models/chat';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let messageHandler: ((data: any) => void) | null = null;

vi.mock('@/services/pub-sub', () => ({
  default: {
    on_message: vi.fn((h: (data: any) => void) => {
      messageHandler = h;
      return () => {
        messageHandler = null;
      };
    }),
  },
}));

vi.mock('@/services/http', () => ({
  default: { get: vi.fn(), post: vi.fn() },
}));

vi.mock('@/models/chat', () => ({
  Chat: class Chat {
    id = 0;
    title = '';
    state = 'OPEN';
    participants: any[] = [];
    messages: any[] = [];
    threads: any[] = [];
    action = vi.fn();
    create = vi.fn();
    participantName = (id: number) => this.participants.find((p: any) => p.id === id)?.name ?? `#${id}`;
  },
  loadChatDetail: vi.fn(),
}));

vi.mock('@/utils/mobx', () => ({
  useQuery: () => [{ items: [], load: vi.fn() }, Promise.resolve(true)],
}));

async function renderChat(id: string) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(
      <MemoryRouter initialEntries={[`/chat/${id}`]}>
        <Routes>
          <Route path="chat/:id" element={<ChatPage />} />
        </Routes>
      </MemoryRouter>,
    );
  });
  return { container, root };
}

function setInput(el: Element, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype,
    'value',
  )!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function threadChat(): any {
  return makeChat({
    id: 42,
    title: 'Тест',
    state: 'OPEN',
    participants: [{ id: 1, name: 'alice' }],
    messages: [{ id: 5, body: 'задача', author_id: 1, created_at: '2026-09-06T10:00:00Z', share_of_id: null }],
    threads: [
      {
        id: 9,
        title: null,
        state: 'OPEN',
        start_message_id: 5,
        participants: [{ id: 1, name: 'alice' }],
        messages: [
          {
            id: 20,
            body: 'Что падает?',
            title: 'Обсуждение',
            author_id: 1,
            created_at: '2026-09-06T10:00:30Z',
            share_of_id: null,
          },
        ],
        threads: [],
      },
    ],
  });
}

function makeChat(data: any): any {
  return {
    ...data,
    participantName: (id: number) =>
      data.participants?.find((p: any) => p.id === id)?.name ?? `#${id}`,
  };
}

beforeEach(() => {
  vi.mocked(loadChatDetail).mockReset();
  vi.mocked(pub_sub.on_message).mockClear();
  vi.mocked(http.get).mockReset();
  vi.mocked(http.get).mockResolvedValue({ data: [] });
  vi.mocked(http.post).mockReset();
  vi.mocked(http.post).mockResolvedValue({});
  messageHandler = null;
});

describe('ChatPage', () => {
  it('отправленное сообщение появляется у открывших чат: деталь перезагружается по событию канала', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue({
      id: 42,
      title: 'Тест',
      state: 'OPEN',
      participants: [],
      messages: [],
      threads: [],
    } as any);
    const { root } = await renderChat('42');
    await act(async () => {});
    expect(loadChatDetail).toHaveBeenCalledWith(42);
    expect(pub_sub.on_message).toHaveBeenCalled();

    vi.mocked(loadChatDetail).mockClear();
    // Новое сообщение в открытом чате приходит по общему каналу — деталь
    // перезагружается без перезагрузки страницы и без опроса.
    act(() => messageHandler!({ type: 'message', chat_id: 42, message_id: 1 }));
    await act(async () => {});
    expect(loadChatDetail).toHaveBeenCalledWith(42);

    act(() => root.unmount());
  });

  it('ответ реактивного агента в другом чате не трогает открытый', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue({
      id: 42,
      title: 'Тест',
      state: 'OPEN',
      participants: [],
      messages: [],
      threads: [],
    } as any);
    const { root } = await renderChat('42');
    await act(async () => {});

    vi.mocked(loadChatDetail).mockClear();
    act(() => messageHandler!({ type: 'message', chat_id: 43, message_id: 2 }));
    await act(async () => {});
    expect(loadChatDetail).not.toHaveBeenCalled();

    act(() => root.unmount());
  });

  it('сообщение в нити открытого чата перезагружает его деталь', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(threadChat());
    const { root } = await renderChat('42');
    await act(async () => {});

    vi.mocked(loadChatDetail).mockClear();
    // Новое сообщение в нити (id 9 — дочерний чат) приходит по общему каналу:
    // открытый родительский чат перезагружается.
    act(() => messageHandler!({ type: 'message', chat_id: 9, message_id: 21 }));
    await act(async () => {});
    expect(loadChatDetail).toHaveBeenCalledWith(42);

    act(() => root.unmount());
  });

  it('страница чата не опрашивает сервер по таймеру', async () => {
    const source = readFileSync('src/pages/app/chat.tsx', 'utf-8');
    expect(source).not.toMatch(/setInterval/);
    expect(source).not.toMatch(/POLL_INTERVAL_MS/);
  });

  it('на странице сессии есть ссылка на страницу Changes её воркстейшна', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue({
      id: 42,
      title: 'Тест',
      state: 'OPEN',
      workstation_id: 7,
      participants: [],
      messages: [],
      threads: [],
    } as any);
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const link = container.querySelector('a[href="/workstations/7/changes"]');
    expect(link).not.toBeNull();
    expect(link?.textContent).toContain('Изменения');
    act(() => root.unmount());
  });

  it('у чата без воркстейшна (не сессии) ссылки на Changes нет', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue({
      id: 43,
      title: 'Общий чат',
      state: 'OPEN',
      workstation_id: null,
      participants: [],
      messages: [],
      threads: [],
    } as any);
    const { container, root } = await renderChat('43');
    await act(async () => {});
    expect(container.querySelector('a[href*="/changes"]')).toBeNull();
    expect(container.textContent).not.toContain('Изменения');
    act(() => root.unmount());
  });

  it('начатая от сообщения нить видна у сообщения свёрнутой по своему заголовку', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(threadChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    // Заголовок нити виден у сообщения.
    expect(container.textContent).toContain('Обсуждение');
    // По умолчанию нить свёрнута: сообщений нити не видно.
    expect(container.textContent).not.toContain('Что падает?');
    act(() => root.unmount());
  });

  it('раскрытая нить показывает свои сообщения, и в неё можно писать', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(threadChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const toggle = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Обсуждение'),
    );
    act(() => toggle!.click());
    await act(async () => {});
    expect(container.textContent).toContain('Что падает?');
    expect(container.querySelector('input[placeholder*="Сообщение в нить"]')).not.toBeNull();
    expect(container.textContent).toContain('в родителя');
    act(() => root.unmount());
  });

  it('начало нити от сообщения создаёт её через API и с заголовком', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(
      makeChat({
        id: 42,
        title: 'Тест',
        state: 'OPEN',
        participants: [{ id: 1, name: 'alice' }],
        messages: [{ id: 5, body: 'задача', author_id: 1, created_at: '2026-09-06T10:00:00Z', share_of_id: null }],
        threads: [],
      }),
    );
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const start = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('начать нить'),
    );
    act(() => start!.click());
    await act(async () => {});
    const title = container.querySelector<HTMLInputElement>('input[placeholder="Заголовок нити"]');
    const body = container.querySelector<HTMLTextAreaElement>('textarea[placeholder="Первое сообщение нити"]');
    setInput(title!, 'Обсуждение');
    setInput(body!, 'Что падает?');
    act(() => {
      [...container.querySelectorAll('button')]
        .find((b) => b.textContent?.trim() === 'Начать')!
        .click();
    });
    await act(async () => {});
    expect(http.post).toHaveBeenCalledWith('/messages/5/thread', {
      title: 'Обсуждение',
      body: 'Что падает?',
    });
    act(() => root.unmount());
  });

  it('отправленное из нити в родителя сообщение уходит через API', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(threadChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const toggle = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Обсуждение'),
    );
    act(() => toggle!.click());
    await act(async () => {});
    const toParent = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('в родителя'),
    );
    act(() => toParent!.click());
    await act(async () => {});
    expect(http.post).toHaveBeenCalledWith('/messages/20/to-parent', {});
    act(() => root.unmount());
  });

  it('внутри раскрытой нити от сообщения можно начать вложенную нить', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(threadChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const toggle = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Обсуждение'),
    );
    act(() => toggle!.click());
    await act(async () => {});
    // У сообщения нити — своя кнопка «начать нить» (вложенная нить).
    const startButtons = [...container.querySelectorAll('button')].filter((b) =>
      b.textContent?.includes('начать нить'),
    );
    expect(startButtons.length).toBeGreaterThanOrEqual(2);
    act(() => startButtons[startButtons.length - 1]!.click());
    await act(async () => {});
    const title = container.querySelector<HTMLInputElement>('input[placeholder="Заголовок нити"]');
    const body = container.querySelector<HTMLTextAreaElement>('textarea[placeholder="Первое сообщение нити"]');
    setInput(title!, 'Вложенная');
    setInput(body!, 'глубже');
    act(() => {
      [...container.querySelectorAll('button')]
        .find((b) => b.textContent?.trim() === 'Начать')!
        .click();
    });
    await act(async () => {});
    expect(http.post).toHaveBeenCalledWith('/messages/20/thread', {
      title: 'Вложенная',
      body: 'глубже',
    });
    act(() => root.unmount());
  });

  it('клик по ссылке в сообщении родителя раскрывает нить у сообщения-источника', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(
      makeChat({
        id: 42,
        title: 'Тест',
        state: 'OPEN',
        participants: [{ id: 1, name: 'alice' }],
        messages: [
          { id: 5, body: 'задача', author_id: 1, created_at: '2026-09-06T10:00:00Z', share_of_id: null },
          {
            id: 30,
            body: 'Гоняю тесты.',
            author_id: 1,
            created_at: '2026-09-06T10:01:00Z',
            share_of_id: null,
            thread_of_id: 5,
          },
        ],
        threads: [
          {
            id: 9,
            title: null,
            state: 'OPEN',
            start_message_id: 5,
            participants: [{ id: 1, name: 'alice' }],
            messages: [
              {
                id: 20,
                body: 'Что падает?',
                title: 'Обсуждение',
                author_id: 1,
                created_at: '2026-09-06T10:00:30Z',
                share_of_id: null,
              },
            ],
            threads: [],
          },
        ],
      }),
    );
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const link = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('из нити от сообщения #5'),
    );
    expect(link).toBeTruthy();
    // Нить свёрнута, пока не кликнут по ссылке.
    expect(container.textContent).not.toContain('Что падает?');
    act(() => link!.click());
    await act(async () => {});
    expect(container.textContent).toContain('Что падает?');
    act(() => root.unmount());
  });
});