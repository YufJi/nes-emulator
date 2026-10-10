import { KEY_GUIDE } from '../emulator/constants';

export function KeyGuide() {
  return (
    <section className="panel">
      <h2 className="panel__title">键位</h2>
      <dl className="keyguide">
        {KEY_GUIDE.map(({ action, keys }) => (
          <div className="keyguide__row" key={action}>
            <dt>{action}</dt>
            <dd>{keys}</dd>
          </div>
        ))}
      </dl>
      <p className="panel__note">
        P 键暂停 / 继续，方向键不会滚动页面。
        <br />
        Select 未做任何连发处理，完全按原机时序：初代标题画面每 16 帧才采样一次 Select，
        单次轻点常被游戏丢弃，来回切换 P1 / P2 需要多点几次。
      </p>
    </section>
  );
}
