import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardTitle } from '@/components/ui/card';
import { toaster } from '@/utils/toaster';
import type { SshKeyInfo } from '@/services/settings';

export const SshKeyPanel = ({ info }: { info: SshKeyInfo }) => {
  const [copied, setCopied] = useState(false);

  if (!info.configured) {
    return (
      <Card>
        <CardTitle>SSH-ключ aga</CardTitle>
        <p className="text-sm text-slate-500">
          Ключ не настроен. Приватный ключ задаёт админ в env ядра{' '}
          <code className="rounded bg-slate-100 px-1 py-0.5">AGA_SSH_PRIVATE_KEY</code>{' '}
          (OpenSSH-формат, <code>ssh-keygen -t ed25519</code>); здесь появится публичный
          ключ для deploy-доступа воркстейшнов к репозиториям.
        </p>
      </Card>
    );
  }

  const copy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(info.public_key ?? '');
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      toaster.show({ message: 'Не удалось скопировать', intent: 'danger' });
    }
  };

  return (
    <Card>
      <CardTitle>SSH-ключ aga</CardTitle>
      <p className="mb-3 text-sm text-slate-500">
        Публичный ключ инстанса — добавьте его в deploy-ключи (GitHub/GitLab/Bitbucket)
        репозиториев, чтобы воркстейшны могли клонировать и пушить по{' '}
        <code className="rounded bg-slate-100 px-1 py-0.5">git+ssh</code>.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        {info.fingerprint && (
          <span className="text-xs text-slate-400">{info.fingerprint}</span>
        )}
        <pre className="overflow-x-auto rounded-md border border-slate-200 bg-slate-50 p-3 text-xs leading-relaxed">
          {info.public_key}
        </pre>
      </div>
      <Button variant="outline" size="sm" onClick={copy}>
        {copied ? 'Скопировано' : 'Скопировать публичный ключ'}
      </Button>
    </Card>
  );
};