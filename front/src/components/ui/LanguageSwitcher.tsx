import { useTranslation } from 'react-i18next';
import { useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * Language switcher component for toggling between English and Russian.
 * Persists language choice in localStorage and provides visual feedback
 * for the currently active language.
 */
export function LanguageSwitcher() {
  const { i18n } = useTranslation();
  const [currentLang, setCurrentLang] = useState(i18n.language);

  useEffect(() => {
    const handleLanguageChanged = (lng: string) => {
      setCurrentLang(lng);
    };
    
    i18n.on('languageChanged', handleLanguageChanged);
    
    return () => {
      i18n.off('languageChanged', handleLanguageChanged);
    };
  }, [i18n]);

  const handleLanguageChange = (lang: string) => {
    i18n.changeLanguage(lang);
    localStorage.setItem('aga_lang', lang);
  };

  return (
    <div className="flex gap-1" data-testid="language-switcher">
      <Button
        variant={currentLang === 'en' ? 'primary' : 'ghost'}
        size="sm"
        onClick={() => handleLanguageChange('en')}
        className={cn(
          'px-3 py-1 text-xs font-medium',
        )}
        data-testid="lang-en"
      >
        EN
      </Button>
      <Button
        variant={currentLang === 'ru' ? 'primary' : 'ghost'}
        size="sm"
        onClick={() => handleLanguageChange('ru')}
        className={cn(
          'px-3 py-1 text-xs font-medium',
        )}
        data-testid="lang-ru"
      >
        RU
      </Button>
    </div>
  );
}
