import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import ChatPage from './chat';
import pub_sub from '@/services/pub-sub';
import http from '@/services/http';
import { loadChatDetail } from '@/models/chat';
import { Shortcut } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let messageHandler: ((data: any) => void) | null = null;

// Сокращения для подсказки: тесты подставляют фикстуры до рендера.
let shortcutItems: any[] = [];

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
  useQuery: (model: any) => {
    if (model === Shortcut) {
      return [{ items: shortcutItems, load: vi.fn() }, Promise.resolve(true)];
    }
    return [{ items: [], load: vi.fn() }, Promise.resolve(true)];
  },
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

function keyDown(el: Element, key: string) {
  act(() => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  });
}

function openChat(): any {
  return {
    id: 42,
    title: 'Тест',
    state: 'OPEN',
    participants: [],
    messages: [],
    threads: [],
    action: vi.fn(),
  };
}

function shortcutFixtures(): any[] {
  return [
    { id: 1, name: 'review', content: 'Проверять диф', deleted: false },
    { id: 2, name: 'deploy-check', content: 'Проверка деплоя', deleted: false },
  ];
}

function chatInput(container: HTMLElement): HTMLTextAreaElement {
  return container.querySelector<HTMLTextAreaElement>('textarea[placeholder="Введите сообщение..."]')!;
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
  shortcutItems = [];
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

  it('у сообщения со скрытой частью есть ссылка «скрытое», по клику текст разворачивается', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(
      makeChat({
        id: 42,
        title: 'Тест',
        state: 'OPEN',
        participants: [{ id: 1, name: 'alice' }],
        messages: [
          {
            id: 5,
            body: 'задача /review',
            author_id: 1,
            created_at: '2026-09-06T10:00:00Z',
            share_of_id: null,
            hidden: 'Проверять диф',
          },
        ],
        threads: [],
      }),
    );
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const toggle = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('скрытое'),
    );
    expect(toggle).toBeTruthy();
    // По умолчанию скрытая часть свёрнута.
    expect(container.textContent).not.toContain('Проверять диф');
    act(() => toggle!.click());
    await act(async () => {});
    expect(container.textContent).toContain('Проверять диф');
    act(() => root.unmount());
  });

  it('у сообщения без скрытой части ссылки «скрытое» нет', async () => {
    vi.mocked(loadChatDetail).mockResolvedValue(
      makeChat({
        id: 42,
        title: 'Тест',
        state: 'OPEN',
        participants: [{ id: 1, name: 'alice' }],
        messages: [
          { id: 5, body: 'задача', author_id: 1, created_at: '2026-09-06T10:00:00Z', share_of_id: null },
        ],
        threads: [],
      }),
    );
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const toggle = [...container.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('скрытое'),
    );
    expect(toggle).toBeUndefined();
    act(() => root.unmount());
  });

  it('при вводе «/» в начале сообщения или после пробела появляется список сокращений', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    expect(container.querySelector('ul[role="listbox"]')).toBeNull();

    // В начале сообщения.
    setInput(input, '/');
    expect(container.querySelector('ul[role="listbox"]')).not.toBeNull();
    expect(container.textContent).toContain('/review');
    expect(container.textContent).toContain('/deploy-check');

    // Без слэша в начале слова списка нет.
    setInput(input, 'сделай ');
    expect(container.querySelector('ul[role="listbox"]')).toBeNull();

    // После пробела — текущее слово начинается со слэша, список снова виден.
    setInput(input, 'сделай /');
    expect(container.querySelector('ul[role="listbox"]')).not.toBeNull();
    act(() => root.unmount());
  });

  it('по мере набора после «/» список фильтруется по началу имени', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/de');
    const names = [...container.querySelectorAll('li')].map((li) => li.textContent).join(' ');
    expect(names).toContain('/deploy-check');
    expect(names).not.toContain('/review');
    act(() => root.unmount());
  });

  it('пункт списка показывает имя сокращения и его текст-подсказку', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    setInput(chatInput(container), '/rev');
    const item = [...container.querySelectorAll('li')][0]!;
    expect(item.textContent).toContain('/review');
    expect(item.textContent).toContain('Проверять диф');
    act(() => root.unmount());
  });

  it('если сокращений нет или ни одно не начинается с набранного текста, список не показывается', async () => {
    // Перечня нет вовсе.
    shortcutItems = [];
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container: emptyContainer, root: emptyRoot } = await renderChat('42');
    await act(async () => {});
    setInput(chatInput(emptyContainer), '/');
    expect(emptyContainer.querySelector('ul[role="listbox"]')).toBeNull();
    act(() => emptyRoot.unmount());

    // Ни одно имя не начинается с набранного.
    shortcutItems = shortcutFixtures();
    const { container, root } = await renderChat('42');
    await act(async () => {});
    setInput(chatInput(container), '/zzz');
    expect(container.querySelector('ul[role="listbox"]')).toBeNull();
    act(() => root.unmount());
  });

  it('стрелки вниз и вверх перемещают выбор по списку', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/');
    // Список отсортирован: deploy-check, review. Вниз — на review, вверх — обратно.
    keyDown(input, 'ArrowDown');
    keyDown(input, 'ArrowUp');
    keyDown(input, 'Enter');
    expect(input.value).toBe('/deploy-check ');
    act(() => root.unmount());
  });

  it('Tab дополняет выбранное сокращение до «/имя» и вставляет после него пробел, список закрывается', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/rev');
    keyDown(input, 'Tab');
    expect(input.value).toBe('/review ');
    expect(container.querySelector('ul[role="listbox"]')).toBeNull();
    act(() => root.unmount());
  });

  it('Enter при открытом списке дополняет, как Tab, без списка отправляет сообщение', async () => {
    shortcutItems = shortcutFixtures();
    const chat = openChat();
    vi.mocked(loadChatDetail).mockResolvedValue(chat);
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);

    setInput(input, '/rev');
    keyDown(input, 'Enter');
    expect(input.value).toBe('/review ');
    expect(chat.action).not.toHaveBeenCalled();

    setInput(input, 'привет');
    keyDown(input, 'Enter');
    expect(chat.action).toHaveBeenCalledWith('messages', { body: 'привет' });
    act(() => root.unmount());
  });

  it('Escape закрывает список, оставляя набранный текст как есть', async () => {
    shortcutItems = shortcutFixtures();
    const chat = openChat();
    vi.mocked(loadChatDetail).mockResolvedValue(chat);
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/rev');
    expect(container.querySelector('ul[role="listbox"]')).not.toBeNull();
    keyDown(input, 'Escape');
    expect(container.querySelector('ul[role="listbox"]')).toBeNull();
    expect(input.value).toBe('/rev');
    expect(chat.action).not.toHaveBeenCalled();
    act(() => root.unmount());
  });

  it('клик по пункту списка дополняет сокращение, как Tab', async () => {
    shortcutItems = shortcutFixtures();
    vi.mocked(loadChatDetail).mockResolvedValue(openChat());
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/de');
    const item = [...container.querySelectorAll('li')].find((li) =>
      li.textContent?.includes('deploy-check'),
    )!;
    act(() => {
      item.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    });
    expect(input.value).toBe('/deploy-check ');
    act(() => root.unmount());
  });

  it('дополненное «/имя» остаётся в видимом тексте и уходит при отправке', async () => {
    shortcutItems = shortcutFixtures();
    const chat = openChat();
    vi.mocked(loadChatDetail).mockResolvedValue(chat);
    const { container, root } = await renderChat('42');
    await act(async () => {});
    const input = chatInput(container);
    setInput(input, '/rev');
    keyDown(input, 'Tab');
    expect(input.value).toBe('/review ');
    keyDown(input, 'Enter');
    // В отправленном теле «/имя» сохраняется (пробел после него send срезает) —
    // ядро по нему заполнит скрытую часть, как и для введённого вручную.
    expect(chat.action).toHaveBeenCalledWith('messages', { body: '/review' });
    act(() => root.unmount());
  });
});