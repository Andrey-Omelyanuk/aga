import { describe, expect, it, vi } from 'vitest';

vi.mock('./http', () => ({ default: { get: vi.fn() } }));
vi.mock('./me', () => ({ default: { is_ready: false, user: null, anonymous: false, ready: vi.fn() } }));

import { pubSubUrl } from './pub-sub';

describe('pubSubUrl', () => {
  it('uses runtime PUBSUB_ENDPOINT when substituted', () => {
    expect(pubSubUrl({ protocol: 'https:', port: '' }, 'wss://rt.example.com/connection/websocket')).toBe(
      'wss://rt.example.com/connection/websocket',
    );
  });

  it('falls back to pub-sub.localhost with page port and scheme', () => {
    expect(pubSubUrl({ protocol: 'http:', port: '8088' }, '<PUBSUB_ENDPOINT>')).toBe(
      'ws://pub-sub.localhost:8088/connection/websocket',
    );
    expect(pubSubUrl({ protocol: 'http:', port: '' }, '')).toBe(
      'ws://pub-sub.localhost/connection/websocket',
    );
    // На https-странице — wss: браузер не пустит ws с защищённой страницы.
    expect(pubSubUrl({ protocol: 'https:', port: '' }, undefined)).toBe(
      'wss://pub-sub.localhost/connection/websocket',
    );
  });
});
