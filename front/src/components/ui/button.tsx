import {
  forwardRef,
  type ButtonHTMLAttributes,
} from 'react';
import { cn } from '@/lib/utils';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'outline';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: 'default' | 'sm' | 'icon';
}

const variants: Record<ButtonVariant, string> = {
  primary: 'bg-blue-600 text-white hover:bg-blue-700',
  secondary: 'bg-green-600 text-white hover:bg-green-700',
  ghost: 'bg-transparent text-slate-700 hover:bg-slate-200',
  outline: 'border border-slate-300 bg-white text-slate-700 hover:bg-slate-100',
};

const sizes = {
  default: 'h-9 px-4',
  sm: 'h-7 px-2 text-xs',
  icon: 'h-9 w-9',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { className, variant = 'primary', size = 'default', disabled, ...props },
    ref,
  ) {
    return (
      <button
        ref={ref}
        className={cn(
          'inline-flex items-center justify-center gap-2 rounded-md font-medium transition-colors disabled:pointer-events-none disabled:opacity-50',
          variants[variant],
          sizes[size],
          className,
        )}
        disabled={disabled}
        {...props}
      />
    );
  },
);