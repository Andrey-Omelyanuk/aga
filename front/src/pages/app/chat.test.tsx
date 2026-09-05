import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import ChatPage from './chat';
import pub_sub from '@/services/pub-sub';
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

vi.mock('@/models/chat', () => ({
  Chat: class Chat {
    id = 0;
    title = '';
    state = 'OPEN';
    participants: any[] = [];
    messages: any[] = [];
    action = vi.fn();
    create = vi.fn();
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

beforeEach(() => {
  vi.mocked(loadChatDetail).mockReset();
  vi.mocked(pub_sub.on_message).mockClear();
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
    } as any);
    const { root } = await renderChat('42');
    await act(async () => {});

    vi.mocked(loadChatDetail).mockClear();
    act(() => messageHandler!({ type: 'message', chat_id: 43, message_id: 2 }));
    await act(async () => {});
    expect(loadChatDetail).not.toHaveBeenCalled();

    act(() => root.unmount());
  });

  it('страница чата не опрашивает сервер по таймеру', async () => {
    const source = readFileSync('src/pages/app/chat.tsx', 'utf-8');
    expect(source).not.toMatch(/setInterval/);
    expect(source).not.toMatch(/POLL_INTERVAL_MS/);
  });
});