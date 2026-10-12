import { useCallback, useState, type FormEvent } from 'react';
import { Button, Card, Input, Title } from 'animal-island-ui';
import { DownloadIcon, LinkIcon } from 'naive-icons';

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
        从链接加载
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
            aria-label="ROM 链接"
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
            加载
          </Button>
        </form>
        <p className="panel__note">
          支持 iNES (.nes) 直链；GitHub 的 Raw / 文件页面链接会自动转换为直链，其他站点需允许跨域（CORS）。
        </p>
      </Card>
    </section>
  );
}
