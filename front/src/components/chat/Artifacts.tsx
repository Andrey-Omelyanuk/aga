import { ChatArtifact } from '@/models/chat';

/** Артефакты сообщения из детали чата (`message.artifacts`): раньше каждый
 *  ряд сам запрашивал `/messages/:id/artifacts` (N+1) и не обновлялся. */
export const Artifacts = ({ items }: { items?: ChatArtifact[] }) => {
  if (!items || items.length === 0) return null;
  return (
    <>
      {items.map((art, i) => (
        <div
          key={i}
          data-testid="artifact"
          className="mt-1 rounded-lg border border-yellow-200 bg-yellow-50 px-3 py-1.5 text-xs whitespace-pre-wrap text-slate-700"
        >
          📎 {art.title || art.kind}: {art.content}
        </div>
      ))}
    </>
  );
};
