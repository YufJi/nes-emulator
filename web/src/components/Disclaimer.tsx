import { useI18n } from '../i18n';
import { DISCLAIMER_URL } from '../repo';

/**
 * The non-commercial notice, repeated on every page a visitor can land on — the
 * wide-screen footer and the phone pages.
 *
 * The line stays short (and the full statement lives in `DISCLAIMER.md`, which
 * the trailing link opens) because a phone footer has room for one sentence,
 * not for the whole text.
 */
export function Disclaimer({ className }: { className?: string }) {
  const { t } = useI18n();

  return (
    <p className={className ? `disclaimer ${className}` : 'disclaimer'}>
      {t('disclaimer.line')}
      <a href={DISCLAIMER_URL} target="_blank" rel="noreferrer">
        {t('disclaimer.link')}
      </a>
    </p>
  );
}
