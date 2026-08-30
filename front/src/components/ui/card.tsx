import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

export function Card({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn('mb-2.5 rounded-lg border border-slate-200 bg-white p-3.5', className)}
      {...props}
    >
      {children}
    </div>
  );
}

export function CardTitle({ className, children }: { className?: string; children: ReactNode }) {
  return <h4 className={cn('mb-1 text-sm text-slate-900', className)}>{children}</h4>;
}

export function CardMeta({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cn('text-xs text-slate-400', className)}>{children}</div>;
}