import { observer } from 'mobx-react-lite';
import { cn } from '@/lib/utils';
import { toaster, type ToastIntent } from '@/utils/toaster';

const intentStyles: Record<ToastIntent, string> = {
  success: 'bg-green-600 text-white',
  danger: 'bg-red-600 text-white',
  primary: 'bg-slate-800 text-white',
};

export const Toaster = observer(() => {
  if (toaster.toasts.length === 0) return null;
  return (
    <div className="fixed right-4 top-4 z-50 flex flex-col gap-2">
      {toaster.toasts.map((toast) => (
        <div
          key={toast.id}
          className={cn('rounded-md px-4 py-2 text-sm shadow-lg', intentStyles[toast.intent])}
          onClick={() => toaster.dismiss(toast.id)}
        >
          {toast.message}
        </div>
      ))}
    </div>
  );
});