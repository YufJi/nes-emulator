# iOS App（NesApp）

**简体中文** · [English](./README.md)

一个可以直接跑的最小 iOS 应用：选一个 `.nes` 文件，画面与声音就出来了，屏幕下方是虚拟手柄。

```
ios/
  NesApp.xcodeproj/        直接 open 的 Xcode 工程（含共享 scheme）
  NesApp/
    NesApp.swift           App 入口 + 界面（Metal 画面 + 顶栏 + 虚拟手柄）
    Emulator.swift         宿主封装：模拟线程、FrameClock 驱动、输入、存档
    AudioRing.swift        模拟线程 ↔ 音频回调 之间的环形缓冲
    FrameSource.swift      零拷贝取景视图
    Platform/
      DisplayLinkDriver.swift   CADisplayLink 驱动
      MetalScreen.swift         MTKView + 上传 + 等比缩放
      AudioOutput.swift         AVAudioEngine + 路由变化
    Generated/             UniFFI 生成物（不入库，由 build-core.sh 产出）
  build-core.sh            构建 Rust 静态库 + 生成 Swift 绑定
  Artifacts/               生成物：libnes_uniffi.a（不入库）
```

应用里**没有一行模拟逻辑**：CPU/PPU/APU/Mapper 全在 `crates/nes-core`，经 `crates/nes-uniffi`
暴露成 Swift API。这一层只做宿主该做的事。

## 跑起来

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim   # 一次性
open ios/NesApp.xcodeproj                                   # 选模拟器/真机，Run
```

工程里有一个 **Build Rust core** 构建阶段，每次构建都会调用 `ios/build-core.sh`：
它按 `$PLATFORM_NAME` / `$ARCHS` 挑 Rust target、`cargo build`、生成 Swift 绑定、
把 `.a` 放进 `Artifacts/`。所以新克隆的仓库不需要任何手工准备步骤。

命令行构建（我验证过的两条）：

```bash
xcodebuild -project ios/NesApp.xcodeproj -scheme NesApp \
  -sdk iphonesimulator -configuration Debug -arch arm64 \
  -derivedDataPath /tmp/nes-dd build

xcodebuild -project ios/NesApp.xcodeproj -scheme NesApp \
  -sdk iphoneos -configuration Debug -arch arm64 \
  -derivedDataPath /tmp/nes-dd-dev build
```

`-arch arm64` 要显式给：多目的地匹配时 xcodebuild 会默认挑到 x86_64 模拟器，
而 Rust 侧只为 arm64 编译（要通用模拟器库就 `lipo` 合并 arm64 + x86_64，
真要分发则做成 `.xcframework`）。

## 为什么需要 iOS Rust target

UniFFI 是**绑定生成器**，它产出两半：Rust 侧 scaffolding（跟着 `nes-uniffi` 一起编译）
和 Swift 侧 API + C 头文件。Swift 调用的 `uniffi_nes_uniffi_*` 是 C 符号，必须由
**为 iOS 编译**的 Rust 静态库提供：

- 真机：`cargo build --target aarch64-apple-ios`
- Apple Silicon 模拟器：`--target aarch64-apple-ios-sim`（模拟器是独立 target，不是 macOS）

把 macOS 编出来的 `.a` 交给 iOS 工程会被链接器拒绝，最低系统版本与平台都对不上。
这和网页必须先 `cargo build --target wasm32-unknown-unknown` 是同一件事。

## 三个约定

**线程**：内核是单所有者的（`docs/architecture.md` §10.5）。`Emulator` 把所有内核调用
放在一条串行队列上，`onFrame` 也在那条队列回调——正好可以直接做 Metal 上传。
要碰 UI 就跳主线程。

**零拷贝的边界**：`FrameSource.pixels` 直接指向内核自己的帧缓冲区。没有中间数组、
没有逐像素计算，`MetalScreen` 用 `replace(region:)` 直接从它上传。两点注意：

- 每帧重取指针（`loadState` 可能搬动缓冲区），别把 `FrameSource` 或由它建的纹理留到下一帧；
- 剩下那一次"上传到纹理"是固有的：真正零上传要让内核渲染进 GPU 已映射的内存
  （`IOSurface`）并且页对齐——那是后续工作。

**音频**：`AudioRing` 是生产/消费之间唯一的桥（模拟线程写入、设备回调读出）。
临界区是一把 `os_unfair_lock` + 一次 `memcpy`；够用，但生产环境建议换成原子 head/tail，
因为 Apple 希望渲染回调完全不加锁。采样率会从 `AVAudioSession` 交给内核，
路由变化（蓝牙/耳机）时也会重新同步。

## 还没做的

- 通用模拟器库（`.xcframework`）与签名配置（当前 `CODE_SIGNING_ALLOWED = NO`，仅用于编译验证）。
- 画面滤镜（扫描线/调色板）与整数倍缩放设置。
- 电池存档落盘（`Emulator.saveRam()` 已经有了，接到 `Documents/` 即可）、即时存档 UI。
- 连发键、四人适配器、光枪。

## 免责声明

本应用属于纯个人学习研究项目：只为学习，不附带任何 ROM / 游戏文件，与任天堂没有任何
关联。顶栏的「关于」会在应用内展示同一份提示，完整声明（覆盖仓库、Web UI 与本应用）见
[`../DISCLAIMER.zh-CN.md`](../DISCLAIMER.zh-CN.md)。
