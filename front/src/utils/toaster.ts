import { makeAutoObservable } from 'mobx';

export type ToastIntent = 'success' | 'danger' | 'primary';

export interface Toast {
  id: number;
  message: string;
  intent: ToastIntent;
}

class ToasterStore {
  toasts: Toast[] = [];
  private nextId = 1;

  constructor() {
    makeAutoObservable(this);
  }

  show({ message, intent }: { message: string; intent?: ToastIntent }): void {
    const id = this.nextId++;
    this.toasts.push({ id, message, intent: intent ?? 'primary' });
    setTimeout(() => this.dismiss(id), 3500);
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }
}

export const toaster = new ToasterStore();