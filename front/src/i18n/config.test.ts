import { describe, it, expect, beforeEach } from 'vitest';
import { getCurrentLanguage, changeLanguage } from './config';

describe('i18n config', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('defaults to Russian', () => {
    expect(getCurrentLanguage()).toBe('ru');
  });

  it('switches language and persists to localStorage', () => {
    changeLanguage('en');
    expect(getCurrentLanguage()).toBe('en');
    expect(localStorage.getItem('aga_lang')).toBe('en');
  });

  it('loads saved language from localStorage', () => {
    localStorage.setItem('aga_lang', 'en');
    window.location.reload();
    expect(getCurrentLanguage()).toBe('en');
  });
});
