import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

type BadgeVariant = 'ok' | 'warn' | 'info';

const variants: Record<BadgeVariant, string> = {
  ok: 'bg-green-100 text-green-800',
  warn: 'bg-orange-100 text-orange-800',
  info: 'bg-blue-100 text-blue-800',
};

export function Badge({
  variant = 'ok',
  children,
  className,
}: {
  variant?: BadgeVariant;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'mr-1 inline-block rounded-full px-2 py-0.5 text-xs',
        variants[variant],
        className,
      )}
    >
      {children}
    </span>
  );
}