/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import en from './locales/en.json';
import ru from './locales/ru.json';

// В unit-тестах `t` замокан как `key => key`, поэтому отсутствующий ключ в
// интерфейсе показывался бы самим ключом незаметно. Проверяем статически:
// каждый литеральный `t('…')` в исходниках есть в обоих языках.
const sources = import.meta.glob(['../**/*.{ts,tsx}', '!../**/*.test.{ts,tsx}'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

function has(dict: unknown, key: string): boolean {
  let node: unknown = dict;
  for (const part of key.split('.')) {
    if (node === null || typeof node !== 'object' || !(part in node)) return false;
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === 'string';
}

function usedKeys(): Map<string, string> {
  const keys = new Map<string, string>();
  const re = /\bt\(\s*(['"])([A-Za-z0-9_.-]+)\1/g;
  for (const [file, code] of Object.entries(sources)) {
    for (const m of code.matchAll(re)) {
      if (m[2].includes('.')) keys.set(m[2], file);
    }
  }
  return keys;
}

describe('i18n keys', () => {
  it('finds translation keys in sources', () => {
    expect(usedKeys().size).toBeGreaterThan(100);
  });

  it('every t() key exists in en and ru', () => {
    const missing = [...usedKeys()]
      .flatMap(([key, file]) => [
        ...(has(en, key) ? [] : [`en: ${key} (${file})`]),
        ...(has(ru, key) ? [] : [`ru: ${key} (${file})`]),
      ]);
    expect(missing).toEqual([]);
  });
});
