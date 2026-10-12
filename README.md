# NES Emulator（Rust + WebAssembly）

用 Rust 编写的 NES 模拟器内核，附带 WASM 绑定与 React 网页前端：把 `.nes`
文件拖进页面，或填入一个 ROM 链接，即可开始游戏。

![网页前端：从内置游戏列表选中《超级玛莉》后的运行画面，右侧依次是控制台、从链接加载、即时存档、虚拟手柄与键位面板](docs/img/web-ui.png)

## 特性

- **CPU**：6502（Ricoh 2A03）指令集，附 nestest 集成校验。
- **PPU**：背景与精灵渲染、sprite 0 命中、扫描线时序、OAM DMA、nametable 镜像。
- **APU**：2 个脉冲、三角、噪声、DMC 五个通道，支持帧序列器 IRQ。
- **Mapper**：33 个编号（NROM、MMC1/2/3/4/5、VRC 系列、Sunsoft、Irem 等）。
- **扩展音频**：VRC6、MMC5、Sunsoft 5B。
- **存档**：电池存档（按 ROM 名持久化）与即时存档（整机快照，4 个槽位）。
- **前端**：拖拽 / 链接 / 游戏列表三种载入方式、虚拟手柄、整数倍像素缩放、
  AudioWorklet 音频输出、4 槽即时存档；界面基于
  [animal-island-ui](https://github.com/guokaigdg/animal-island-ui) 的动森风格组件库。
  详见[网页前端](#网页前端)。

## 网页前端

React + Vite + TypeScript；Rust 内核经 `wasm-bindgen` 编译成 WASM，在前端按帧驱动。

### 载入 ROM

- **拖拽**：把 `.nes` 文件拖到页面任意位置松手即可，也可以用工具栏的「选择 ROM」。
- **链接**：侧栏「从链接加载」粘贴直链后回车，省略 `http(s)://` 时按 `https` 处理。
  GitHub 的 `blob`（文件页面）与 `raw` 链接会自动改写成 `raw.githubusercontent.com`
  的直链——`github.com/…/raw/…` 的 302 跳转不带 `Access-Control-Allow-Origin`，
  浏览器会直接丢弃响应；其他站点需要服务器允许跨域（CORS）。
- **游戏列表**：内置 [mengfei523060/nesrom](https://github.com/mengfei523060/nesrom)
  的 932 个 ROM 目录（只有名称与直链，不含 ROM 数据），可按名称搜索、分页翻看，
  正在运行的卡带标有「当前」。列表是静态的 `public/games.json`，弹窗第一次打开时
  才请求，可用 `cd web && npm run build:games` 重新生成。
- 三种来源走同一套校验（iNES 文件头 + Mapper 支持检查）；加载失败会保留当前正在
  运行的游戏，并用通知说明原因。

### 控制

- **键盘**：见下方[操作方式](#操作方式)。
- **虚拟手柄**：触屏可用，按下即持续按住，直到抬手。
- **暂停 / 继续**：工具栏按钮或 `P` 键。**全屏**：工具栏「全屏」，画面铺满屏幕。

### 画面与声音

- 画布后备缓冲固定 256×240，CSS 尺寸按**整数倍**缩放（配合
  `image-rendering: pixelated`），不会出现非整数倍带来的模糊。
- 内核每个模拟帧产出的交错立体声样本交给 **AudioWorklet** 按声卡节奏播放；不支持
  AudioWorklet 的浏览器自动回退到 `ScriptProcessorNode`。工具栏「静音」开关。
- 顶栏状态：运行状态（待机 / 运行中 / 已暂停 / 加载中）、卡带名（悬停显示来源）、实时 FPS。

### 存档

- **电池存档**：带电池的卡带按 ROM 名写入 `localStorage`，再次载入同一张时自动恢复；
  工具栏「清空存档」在二次确认后删除存档并重置卡带。
- **即时存档**：`save_state` / `load_state` 把 CPU、PPU、APU、总线与卡带的可变状态
  编码成一段字节流（PRG-ROM 不重复存储），4 个槽位存放在 IndexedDB，可随时保存、
  读取或清除。读取必须发生在同一张卡带上。

### 界面

- 基于 [animal-island-ui](https://github.com/guokaigdg/animal-island-ui) 的动森风格
  组件库；`src/styles.css` 只负责布局并读取库暴露的 `--animal-*` 变量，不覆盖组件的
  颜色、圆角与阴影。
- 窄屏（<940px）自动改为上下布局：画面在上，面板在下。
- 构建时会裁掉库中未使用的字体与场景插画，`dist/` 约 730 KB。详见
  [`web/README.md`](web/README.md)。

## 目录结构

```
crates/nes-core/   模拟器内核：CPU / PPU / APU / 总线 / mapper / 存档编码
crates/nes-wasm/   wasm-bindgen 绑定（JsNes），对内核的薄封装
web/               React + Vite + TypeScript 前端
docs/img/          README 里引用的界面截图
roms/              本地测试 ROM（可选；缺失时相关测试会跳过）
```

## 环境要求

- Rust 1.70+（edition 2021）
- Node.js 18+
- `wasm-pack`（仅在重新生成 WASM 绑定时需要）

## 构建与运行

内核：

```bash
cargo build -p nes-core     # 构建内核
cargo test --workspace      # 运行全部 Rust 测试
```

网页前端（首次需生成 WASM 绑定，`web/src/wasm/` 未纳入版本控制）：

```bash
cd web
npm install
npm run build:wasm   # 用 wasm-pack 生成 web/src/wasm（需要联网）
npm run dev          # 开发服务器 http://localhost:5173
npm run build        # 类型检查 + 生产构建
```

## 部署（GitHub Pages）

推送到 `main` 会触发 [`.github/workflows/deploy-pages.yml`](.github/workflows/deploy-pages.yml)：
先在 runner 上编译 WASM 内核（`web/src/wasm/` 未入库，必须重新生成），再构建前端，
最后通过 Pages 的 Actions 流程发布。

首次使用需要手动开启一次：仓库 **Settings → Pages → Source** 选择 **GitHub Actions**。
工作流不会替你开启——GITHUB_TOKEN 没有仓库管理权限。开启后站点地址是
`https://<owner>.github.io/<repo>/`。

Vite 的 `base` 由工作流按仓库名推导（普通仓库 `/<repo>/`，`<owner>.github.io`
这类用户站点 `/`），所以本地 `npm run build` 仍按站点根目录构建。要在本地复现
子路径构建（排查部署问题时很有用）：

```bash
cd web
PAGES_BASE=/<repo>/ npm run build:pages   # 产物在 web/dist
```

部署后的站点从 `raw.githubusercontent.com` 拉取游戏列表里的 ROM（对方允许跨域），
因此只能联网使用；断网环境请改用本地文件或自建 ROM 直链。

## 操作方式

| 动作 | 按键 |
| --- | --- |
| 方向 | `W` `A` `S` `D` |
| A | `J` / `I` |
| B | `K` / `O` |
| Start | `Enter` / `Space` |
| Select | `Shift` |
| 暂停 / 继续 | `P` |

触屏设备可直接使用右侧的虚拟手柄。更多前端细节见 [`web/README.md`](web/README.md)。

## 测试

- `cargo test --workspace`：单元测试 + 集成测试（`ppu`、`apu`、`mappers`、
  `nestest`）+ 端到端 Super Mario Bros 冒烟测试。
- 需要 ROM 的用例在 `roms/` 缺失时跳过或被标记为 `#[ignore]`。
- 前端改动请运行 `npm run typecheck` 与 `npm run build`。

## 说明

- 仅支持 iNES 格式；模拟器本身不附带游戏，请自备 ROM。
- 仓库里的 ROM 一律不入版本控制，内置游戏列表只保存名称与直链。

## 许可

MIT（见 [`Cargo.toml`](Cargo.toml) 中的 `license` 字段）。
