import { afterEach, describe, expect, it, vi } from 'vitest';
import { throttle } from './throttle';

describe('throttle', () => {
  afterEach(() => vi.useRealTimers());

  it('runs the first call at once and collapses a burst into one trailing call', () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const t = throttle(fn, 500);
    t();
    expect(fn).toHaveBeenCalledTimes(1);
    for (let i = 0; i < 10; i++) t();
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(2);
    // Окно после повтора: тишина — больше вызовов нет.
    vi.advanceTimersByTime(1000);
    expect(fn).toHaveBeenCalledTimes(2);
    // После паузы — снова сразу.
    t();
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it('cancel drops the pending trailing call', () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const t = throttle(fn, 500);
    t();
    t();
    t.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
