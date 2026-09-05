import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { HelpContent } from '@/components/settings/HelpContent';
import type { HelpLang } from '@/components/settings/AgentSetDiagram';
import { cn } from '@/lib/utils';

// Выбранный язык справки храним в localStorage: при первом открытии — русский,
// дальше страница возвращается на язык, выбранный в прошлый раз.
export const HELP_LANG_KEY = 'aga_help_lang';

const ConfigHelpPage = () => {
  const [lang, setLang] = useState<HelpLang>(() => {
    const saved = localStorage.getItem(HELP_LANG_KEY);
    return saved === 'en' ? 'en' : 'ru';
  });

  const choose = (next: HelpLang) => {
    localStorage.setItem(HELP_LANG_KEY, next);
    setLang(next);
  };

  return (
    <div className="max-w-3xl">
      <div className="mb-4 flex items-center gap-2">
        <h1 className="text-lg font-semibold text-slate-800">Help</h1>
        <span className="flex-1" />
        {(['ru', 'en'] as const).map((l) => (
          <Button
            key={l}
            variant={lang === l ? 'primary' : 'outline'}
            size="sm"
            onClick={() => choose(l)}
            className={cn('min-w-10')}
          >
            {l.toUpperCase()}
          </Button>
        ))}
      </div>
      <HelpContent lang={lang} />
    </div>
  );
};

export default ConfigHelpPage;