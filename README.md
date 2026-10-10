# NES Emulator（Rust + WebAssembly）

用 Rust 编写的 NES 模拟器内核，附带 WASM 绑定与 React 网页前端：把 `.nes`
文件拖进页面即可开始游戏。

## 特性

- **CPU**：6502（Ricoh 2A03）指令集，附 nestest 集成校验。
- **PPU**：背景与精灵渲染、sprite 0 命中、扫描线时序、OAM DMA、nametable 镜像。
- **APU**：2 个脉冲、三角、噪声、DMC 五个通道，支持帧序列器 IRQ。
- **Mapper**：33 个编号（NROM、MMC1/2/3/4/5、VRC 系列、Sunsoft、Irem 等）。
- **扩展音频**：VRC6、MMC5、Sunsoft 5B。
- **存档**：电池存档（按 ROM 名持久化）与即时存档（整机快照，4 个槽位）。
- **前端**：拖拽加载、虚拟手柄、整数倍像素缩放、AudioWorklet 音频输出。

## 目录结构

```
crates/nes-core/   模拟器内核：CPU / PPU / APU / 总线 / mapper / 存档编码
crates/nes-wasm/   wasm-bindgen 绑定（JsNes），对内核的薄封装
web/               React + Vite + TypeScript 前端
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

## 操作方式

| 动作 | 按键 |
| --- | --- |
| 方向 | `↑` `↓` `←` `→` |
| A | `X` / `K` |
| B | `Z` / `J` |
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
- 即时存档必须载入到同一张卡带（PRG-ROM 不写入存档），槽位按 ROM 名区分。

## 许可

MIT（见 [`Cargo.toml`](Cargo.toml) 中的 `license` 字段）。
