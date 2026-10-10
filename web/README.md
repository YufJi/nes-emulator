# NES Emulator Web

React + Vite 前端，驱动 `crates/nes-wasm` 编译出的 Rust/WASM NES 内核。
把 `.nes` 文件拖到页面任意位置即可开始游戏。

## 环境要求

- Node.js 18+
- 重新编译 WASM 内核时需要 Rust 工具链与 [`wasm-pack`](https://drager.github.io/wasm-pack/)

## 命令

```bash
npm install         # 安装依赖
npm run dev         # 开发服务器 http://localhost:5173
npm run build       # 类型检查 + 生产构建到 dist/
npm run preview     # 本地预览生产构建
npm run typecheck   # 仅类型检查
npm run build:wasm  # 用 wasm-pack 重新生成 src/wasm/ 下的内核绑定
```

`src/wasm/` 由 `npm run build:wasm` 生成，未纳入版本控制。
首次执行该命令时 `wasm-pack` 会下载 `wasm-bindgen` 等工具，需要联网。

## 目录结构

```
src/
  emulator/
    NesEmulator.ts   # 帧时钟、画面渲染、输入与音频的总调度
    AudioOutput.ts   # AudioWorklet（回退 ScriptProcessorNode）音频输出
    rom.ts           # ROM 文件校验（iNES 文件头、Mapper 支持情况）
    constants.ts     # 分辨率、帧率、按键映射等常量
  hooks/
    useNesEmulator.ts  # 把模拟器接到 React 状态上
    useIntegerScale.ts # 画布整数倍缩放
  components/          # 画面、工具栏、虚拟手柄、键位表、拖拽覆盖层
public/
  nes-audio-processor.js  # 运行在音频线程上的 AudioWorklet
scripts/
  build-wasm.mjs      # wasm-pack 构建 + 生成物拷贝
```

## 操作方式

| 动作 | 按键 |
| --- | --- |
| 方向 | `↑` `↓` `←` `→` |
| A | `X` / `K` |
| B | `Z` / `J` |
| Start | `Enter` / `Space` |
| Select | `Shift` |
| 暂停 / 继续 | `P` |

触屏设备可直接使用右侧的虚拟手柄。

### 关于 Select

《超级马里奥兄弟》的标题画面只在**每 16 帧中的 1 帧**上采样 Select（游戏内部用一个
`$0780` 计数器做闸门），并且它的读手柄程序（`$8E5C`）会把 Select 变成一个**只有单帧
有效**的「刚按下」标志。因此一次约 100 ms 的普通点按只有约 1/16 的概率正好落在采样帧
上，而按住不放更是完全无效——标志在下一个采样窗口打开前就已经过期了。此外游戏还有一个
次级闸门（`$07A2`）会周期性屏蔽 Select，所以即使时机正确也可能被丢掉。

这是 ROM 自身的逻辑（在 `crates/nes-wasm` 里用 `smb_select_is_sampled_once_per_sixteen_frames`
测试固定住），不是模拟器或前端的缺陷。**本项目刻意不做任何连发 / 重试处理**，按键怎么按下
就怎么上报给手柄，保持与原机一致的输入时序；代价是切换 P1 / P2 需要像在原机上一样多点几次
（或在正确的那一帧上快速点按）。若以后想加输入辅助，只需在 `src/emulator/NesEmulator.ts`
的 `setButton` 外面包一层脉冲逻辑即可，核心与前端渲染都不必改动。

## 实现要点

- **帧时钟**：按 NES 的 60.0988 Hz 而非屏幕刷新率推进帧，用 `requestAnimationFrame`
  累积时间片并限制单次追帧数量；标签页切回时丢弃积压，避免快进。
- **整数倍缩放**：画布后备缓冲固定为 256×240，用 `ResizeObserver` 计算整数缩放比后
  只改 CSS 尺寸，配合 `image-rendering: pixelated` 得到锐利像素。
- **音频**：内核每个模拟帧产出一批交错立体声样本，交给 AudioWorklet 队列按声卡节奏
  播放；不支持 AudioWorklet 的浏览器自动回退到 `ScriptProcessorNode`。
- **ROM 校验**：读取文件前先检查 iNES 文件头与 Mapper 是否已被内核实现，给出可读的
  错误提示；加载失败时保留当前正在运行的游戏。
- **电池存档**：带电池的卡带其存档按 ROM 文件名写入 `localStorage`，再次拖入同一
  ROM 时自动恢复；工具栏的「清空存档」在二次确认后删除该存档并重置卡带，方便重开。
- **即时存档**：`save_state` / `load_state` 把 CPU、PPU、APU、总线以及卡带的可变状态
  （bank 选择、IRQ 计数器、工作 RAM、扩展音频…）编码成一段字节流，PRG-ROM 不重复存储；
  侧栏提供 4 个槽位，保存在 IndexedDB，可随时保存、读取或清除。

> 注意：`crates/nes-wasm` 的 `generate_audio` 目前仍是占位实现（输出静音），
> 因此前端音频链路已就绪但暂时没有声音，待内核 APU 输出补齐后会自动生效。
