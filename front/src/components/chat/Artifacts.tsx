import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { ChatArtifact } from '@/models/chat';
import http from '@/services/http';

export const Artifacts = observer(({ messageId }: { messageId: number }) => {
  const [items, setItems] = useState<ChatArtifact[]>([]);

  useEffect(() => {
    let cancelled = false;
    void http
      .get<ChatArtifact[]>(`/messages/${messageId}/artifacts`)
      .then((r) => {
        if (!cancelled) setItems(r.data ?? []);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [messageId]);

  if (items.length === 0) return null;
  return (
    <>
      {items.map((art, i) => (
        <div
          key={i}
          className="mt-1 rounded-lg border border-yellow-200 bg-yellow-50 px-3 py-1.5 text-xs whitespace-pre-wrap text-slate-700"
        >
          📎 {art.title || art.kind}: {art.content}
        </div>
      ))}
    </>
  );
});