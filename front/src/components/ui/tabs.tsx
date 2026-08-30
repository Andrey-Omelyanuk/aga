import { cn } from '@/lib/utils';

export interface Tab<T extends string> {
  value: T;
  label: string;
}

export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: Array<Tab<T>>;
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <nav className="flex gap-1">
      {tabs.map((tab) => (
        <button
          key={tab.value}
          onClick={() => onChange(tab.value)}
          className={cn(
            'rounded-md px-4 py-2 text-sm cursor-pointer',
            value === tab.value
              ? 'bg-blue-100 font-semibold text-blue-700'
              : 'text-slate-500 hover:bg-slate-100',
          )}
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}

export function EmptyState({ children }: { children: React.ReactNode }) {
  return <div className="py-16 text-center text-slate-400">{children}</div>;
}