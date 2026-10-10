export function DropOverlay() {
  return (
    <div className="dropzone" aria-hidden="true">
      <div className="dropzone__card">
        <p className="dropzone__title">松手即可加载 ROM</p>
        <p className="dropzone__hint">支持 .nes（iNES 格式）文件</p>
      </div>
    </div>
  );
}
