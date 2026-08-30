import { forwardRef, type SelectHTMLAttributes } from 'react';
import { cn } from '@/lib/utils';

export const Select = forwardRef<
  HTMLSelectElement,
  SelectHTMLAttributes<HTMLSelectElement>
>(function Select({ className, children, ...props }, ref) {
  return (
    <select
      ref={ref}
      className={cn(
        'h-9 rounded-md border border-slate-300 bg-white px-3 text-sm outline-none focus:border-blue-500',
        className,
      )}
      {...props}
    >
      {children}
    </select>
  );
});