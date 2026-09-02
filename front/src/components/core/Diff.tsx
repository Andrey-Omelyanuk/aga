import { diffLines, type Change } from 'diff';
import { cn } from '@/lib/utils';

function lines(change: Change): string[] {
  const parts = change.value.split('\n');
  if (parts.at(-1) === '') parts.pop();
  return parts;
}

/** Строчный дифф двух текстов: только добавленные (+) и удалённые (-) строки,
 * без неизменного контекста («только diff»). Пусто, когда тексты равны. */
export const Diff = ({ oldText, newText }: { oldText: string; newText: string }) => {
  const changes = diffLines(oldText, newText).filter((c) => c.added || c.removed);
  if (changes.length === 0) return null;

  return (
    <pre className="mt-2 overflow-x-auto rounded-md border border-slate-200 bg-slate-50 text-xs leading-relaxed">
      {changes.map((change, i) =>
        lines(change).map((line, j) => (
          <div
            key={`${i}-${j}`}
            className={cn(
              'whitespace-pre px-3',
              change.added
                ? 'bg-green-50 text-green-800'
                : 'bg-red-50 text-red-700',
            )}
          >
            <span className="mr-2 inline-block w-4 select-none text-slate-400">
              {change.added ? '+' : '-'}
            </span>
            {line || ' '}
          </div>
        )),
      )}
    </pre>
  );
};