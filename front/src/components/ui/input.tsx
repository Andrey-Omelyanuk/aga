import { forwardRef, type InputHTMLAttributes } from 'react';
import { cn } from '@/lib/utils';

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...props }, ref) {
    return (
      <input
        ref={ref}
        className={cn(
          'h-9 flex-1 rounded-md border border-slate-300 bg-white px-3 text-sm outline-none focus:border-blue-500',
          className,
        )}
        {...props}
      />
    );
  },
);