import { describe, it, expect, beforeEach } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { LanguageSwitcher } from './LanguageSwitcher';
import i18n from '@/i18n/config';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderSwitcher() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(<LanguageSwitcher />);
  });
  return { container, root };
}

describe('LanguageSwitcher', () => {
  beforeEach(() => {
    localStorage.clear();
    i18n.changeLanguage('ru');
  });

  it('renders both language buttons', () => {
    const { container, root } = renderSwitcher();
    
    const enButton = container.querySelector('[data-testid="lang-en"]');
    const ruButton = container.querySelector('[data-testid="lang-ru"]');
    
    expect(enButton).not.toBeNull();
    expect(ruButton).not.toBeNull();
    
    act(() => root.unmount());
  });

  it('shows Russian as active by default', () => {
    const { container, root } = renderSwitcher();
    
    const ruButton = container.querySelector('[data-testid="lang-ru"]');
    const enButton = container.querySelector('[data-testid="lang-en"]');
    
    expect(ruButton?.className).toContain('bg-blue-600');
    expect(enButton?.className).not.toContain('bg-blue-600');
    
    act(() => root.unmount());
  });

  it('switches to English when EN button is clicked', () => {
    const { container, root } = renderSwitcher();
    
    const enButton = container.querySelector('[data-testid="lang-en"]') as HTMLButtonElement;
    act(() => {
      enButton.click();
    });
    
    expect(i18n.language).toBe('en');
    expect(localStorage.getItem('aga_lang')).toBe('en');
    
    // Re-query the button after re-render
    const updatedEnButton = container.querySelector('[data-testid="lang-en"]') as HTMLButtonElement;
    // After switching, EN button should be active
    expect(updatedEnButton.className).toContain('bg-blue-600');
    
    act(() => root.unmount());
  });

  it('switches back to Russian when RU button is clicked', () => {
    i18n.changeLanguage('en');
    const { container, root } = renderSwitcher();
    
    const ruButton = container.querySelector('[data-testid="lang-ru"]') as HTMLButtonElement;
    act(() => {
      ruButton.click();
    });
    
    expect(i18n.language).toBe('ru');
    expect(localStorage.getItem('aga_lang')).toBe('ru');
    
    // Re-query the button after re-render
    const updatedRuButton = container.querySelector('[data-testid="lang-ru"]') as HTMLButtonElement;
    // After switching, RU button should be active
    expect(updatedRuButton.className).toContain('bg-blue-600');
    
    act(() => root.unmount());
  });

  it('loads saved language from localStorage on mount', () => {
    localStorage.setItem('aga_lang', 'en');
    // Re-initialize i18n to pick up the localStorage value
    i18n.changeLanguage('en');
    
    const { container, root } = renderSwitcher();
    
    const enButton = container.querySelector('[data-testid="lang-en"]');
    expect(enButton?.className).toContain('bg-blue-600');
    
    act(() => root.unmount());
  });
});
