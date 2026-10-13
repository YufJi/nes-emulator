import { useCallback, useState, type FormEvent } from 'react';
import { Button, Card, Input, Title } from 'animal-island-ui';
import { DownloadIcon, LinkIcon } from 'naive-icons';
import { useI18n } from '../i18n';

interface RomUrlFormProps {
  busy: boolean;
  onLoadUrl: (url: string) => void;
}

/**
 * Load a cartridge straight from a URL.
 *
 * Sharing a link is easier than shipping a file around, so the download is
 * handled here and validated exactly like a local one. The field keeps its
 * value after a successful load so the same game can be reloaded quickly.
 */
export function RomUrlForm({ busy, onLoadUrl }: RomUrlFormProps) {
  const [url, setUrl] = useState('');
  const { t } = useI18n();

  const submit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const trimmed = url.trim();
      if (!trimmed || busy) return;
      onLoadUrl(trimmed);
    },
    [busy, onLoadUrl, url],
  );

  return (
    <section className="panel">
      <Title size="small" color="app-blue">
        {t('romUrl.title')}
      </Title>
      <Card className="card--tight">
        <form className="rom-url" onSubmit={submit}>
          <Input
            className="rom-url__input"
            size="large"
            shadow
            allowClear
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            onClear={() => setUrl('')}
            placeholder="https://…/game.nes"
            aria-label={t('romUrl.label')}
            inputMode="url"
            spellCheck={false}
            autoComplete="off"
            disabled={busy}
            prefix={<LinkIcon size={18} color="#b09a7e" aria-hidden />}
          />
          <Button
            type="primary"
            htmlType="submit"
            icon={<DownloadIcon size={18} color="currentColor" aria-hidden />}
            disabled={busy || url.trim() === ''}
          >
            {t('romUrl.submit')}
          </Button>
        </form>
        <p className="panel__note">
          {t('romUrl.note')}
        </p>
      </Card>
    </section>
  );
}
