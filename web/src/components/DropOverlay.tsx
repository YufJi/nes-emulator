import { Card, Title } from 'animal-island-ui';

export function DropOverlay() {
  return (
    <div className="dropzone" aria-hidden="true">
      <Card type="dashed" className="dropzone__card">
        <Title size="middle" color="app-green">
          松手即可加载 ROM
        </Title>
        <p className="dropzone__hint">支持 .nes（iNES 格式）文件</p>
      </Card>
    </div>
  );
}
