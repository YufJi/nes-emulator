import { Card, Collapse, Divider, Tag, Title } from 'animal-island-ui';
import { KEY_GUIDE } from '../emulator/constants';

export function KeyGuide() {
  return (
    <section className="panel">
      <Title size="small" color="app-green">
        键位
      </Title>
      <Card className="card--tight">
        <dl className="keyguide">
          {KEY_GUIDE.map(({ action, keys }) => (
            <div className="keyguide__row" key={action}>
              <dt>{action}</dt>
              <dd className="keyguide__keys">
                {keys.map((key) => (
                  <Tag key={key} size="small" variant="outlined" color="default">
                    {key}
                  </Tag>
                ))}
              </dd>
            </div>
          ))}
        </dl>

        <Divider type="squiggle" />

        <p className="panel__note">P 键暂停 / 继续；方向键不再绑定，移动请用 WASD。</p>

        <Collapse
          question="Select 为什么要按好几次？"
          answer="Select 未做任何连发处理，完全按原机时序：初代标题画面每 16 帧才采样一次 Select，单次轻点常被游戏丢弃，来回切换 P1 / P2 需要多点几次。"
        />
      </Card>
    </section>
  );
}
