import { Button } from 'animal-island-ui';
import { LOCALES, LOCALE_LABELS, LOCALE_SHORT, useI18n } from '../i18n';

/**
 * Picks the interface language.
 *
 * Three labels side by side rather than a dropdown: there are only three, and a
 * visitor who cannot read the current language still recognises their own name
 * on one of them.
 */
export function LanguageSwitcher({ className }: { className?: string }) {
  const { locale, setLocale, t } = useI18n();

  return (
    <div className={`langs${className ? ` ${className}` : ''}`} role="group" aria-label={t('language.label')}>
      {LOCALES.map((code) => (
        <Button
          key={code}
          type={code === locale ? 'primary' : 'text'}
          size="small"
          className="langs__item"
          aria-pressed={code === locale}
          title={LOCALE_LABELS[code]}
          aria-label={LOCALE_LABELS[code]}
          onClick={() => setLocale(code)}
        >
          {LOCALE_SHORT[code]}
        </Button>
      ))}
    </div>
  );
}
