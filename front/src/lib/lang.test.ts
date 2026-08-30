import { describe, expect, it } from 'vitest';
import { langFor } from '@/models/file-browser';

describe('langFor', () => {
  it('maps common extensions to highlight languages', () => {
    expect(langFor('main.rs')).toBe('rust');
    expect(langFor('App.tsx')).toBe('typescript');
    expect(langFor('Cargo.toml')).toBe('ini');
  });

  it('falls back to plaintext for unknown extensions', () => {
    expect(langFor('file.unknownext')).toBe('plaintext');
    expect(langFor('README')).toBe('plaintext');
  });

  it('treats paths case-insensitively', () => {
    expect(langFor('Main.RS')).toBe('rust');
  });
});