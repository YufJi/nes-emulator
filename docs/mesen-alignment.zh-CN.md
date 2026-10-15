# 与 Mesen 的对齐：现状、差距与推进方式

**简体中文** · [English](./mesen-alignment.md)

这是精度工作的**工作文档**：**把内核与 [Mesen2](https://github.com/SourMesen/Mesen2) 的 NES 内核
对齐，遇到歧义一律以 Mesen 为准。** `docs/architecture.md` 仍是"模拟器今天是什么样"的描述（其
§15.1 是已知限制的摘要）；本文档是计划、计划背后的实测数据，以及仍然不同的地方。

两条规则，按优先级：

1. **以 Mesen 的实现为准。** Mesen 与内核不一致之处即为缺陷，除非本文 §4 明确列为有意偏差。
   请直接读 Mesen 源码，不要猜：关键文件是 `Core/NES/NesCpu.cpp`/`.h`（执行、DMA、中断）、
   `Core/NES/NesPpu.cpp`/`.h`（取指流水线）、`Core/NES/BaseNesPpu.h`（PPU 状态）、
   `Core/NES/NesMemoryManager.cpp`、`Core/NES/Mappers/A12Watcher.h` 以及
   `Core/NES/Mappers/` 下的各 mapper。
2. **测试 ROM 基线是证据，不是约束。** `crates/nes-core/tests/rom_tests.rs` 里的失败清单是双向
   断言的，因此任何让套件结果变化的改动都必须更新清单——但清单跟随实测，凡因"更忠实于 Mesen"
   而产生的回退，都记录并写明原因，而不是回退代码。

## 1. 已对齐的部分

| 方面 | Mesen 的做法 | 内核现在的做法 | 依据 |
| --- | --- | --- | --- |
| 时钟 | 单一主时钟；NTSC 用 `cpuDivider` 12 / `ppuDivider` 4 推导两颗芯片 | `Region::cpu_divider()`/`ppu_divider()`（12/4，即真实晶振），单一主时钟，`NesBus::run_ppu_to` 推进到目标 | `test_clock_dividers_agree_with_the_frame_geometry`、`test_the_master_clock_path_places_the_same_dots` |
| CPU 执行 | `MemoryRead`/`MemoryWrite` **本身就是周期**（围绕访问做 `StartCpuCycle`/`EndCpuCycle`） | `Bus::read_cycle`/`write_cycle` 即周期；`NesBus::begin_cpu_cycle`/`end_cpu_cycle` 围绕访问推进 PPU/APU | `instr_timing` 2/2、`branch_timing` 3/3、`test_every_opcode_executes` |
| 操作数取指与丢弃读 | `FetchOperand` 把 dummy 逐条写出（零页索引、绝对/间接索引、隐含与累加器的那次读） | `Cpu::fetch_operand` 加上 `AddressingMode::AbsoluteXWrite`/`AbsoluteYWrite`/`IndirectYWrite`、`RTS` 两次、`RTI`/`PLA`/`PLP` 各一次 | 同上 |
| 中断时序 | 每周期采样；判决读上一周期的采样（`_prevRunIrq`/`_prevNeedNmi`） | `Cpu::end_cycle` 保持同样的滞后；`execute_instruction` 在指令结束后判决 | `cpu_interrupts_v2/2-nmi_and_brk` 转绿、`ppu_vbl_nmi` 9/10 |
| 向量选择 | 在压状态字节那一刻选定；此前到达的 NMI 接管（`IRQ()`、`BRK()`） | `Cpu::take_interrupt`/`Cpu::brk` 相同，含 BRK 的 `_prevNeedNmi = false` | `test_nmi_hijacks_an_irq_sequence`、`test_nmi_takes_brks_vector_over` |
| A12 | `SetBusAddress` 把**最后一次真实访问**的地址放上总线并保持；过滤是 `(GetMasterClock() - _a12LowClock) >= 3`，主时钟即 CPU 周期数 | `NesBus::bus_address`（保持）、`drive_ppu_bus` 重建 `LoadTileInfo` 的取指 dot、`Mapper::on_ppu_bus(addr, cpu_cycles)` 与 `mapper4` 以周期计长度且**每次上升都清零** | `2-details`（241 次）转绿、`4-scanline_timing` 推进到第 12 子项 |
| 预渲染行 | 流水线条件 `scanline < 240`，对预渲染行（-1）成立 | `drive_ppu_bus` 的 `in_vblank` 只排除 240..=260 | 同上 |
| 奇数帧跳点 | 在处理 dot 339 时判定（`_cycle = 340`），被跳掉的是 340 | 相同，由 `NesBus::skip_odd_frame_dot` 承载 | `test_a_register_write_is_in_time_for_the_odd_frame_decision` |

## 2. 尚未对齐的部分

### 2.1 PPU：sprite 仍是整行合成

背景已经是逐 dot 的了——`PixelPipeline` 每八个 dot 取一个瓦片（`LoadTileInfo` 的名称表/
属性/图案读，落在 `cycle % 8 == 1/3/5/7`），每 dot 移出一像素并在它自己的 dot 上画出来，
滚动取的就是芯片自己的 `v`/`t`/`x`；sprite 的**取指**也在各自的 dot 上跑，因此 A12 能看到它们。
Mesen 的 `ProcessScanlineImpl` 里还剩的是 sprite 侧本体：`ProcessSpriteEvaluation`
（65-256，含 OAM 地址行为与损坏标志）、`LoadSpriteTileInfo`（257-320）写入每 sprite 的移位
寄存器，以及逐 dot 的 sprite 像素。

仍未解决的后果：

- **sprite 0 命中不在像素处**：命中仍由整行渲染器记录，而不是在 PPU 的 dot 计数器走过那个 x 时。
- **sprite 的绘制比 A12 晚一行**：取指已逐 dot，画出来的 sprite 来自逐行的二级 OAM 快照。
- MMC5 竖向分屏基于同一理由仍然不做。
- **光栅分屏**（可见行内写 `$2005`/`$2006`）现在背景侧落在正确的边上了，因为取指跟随被改过的
  滚动寄存器。

随逐 dot 背景一起落地的还有：真实取指地址让 `4-scanline_timing` 转绿（先前的重建让首个
sprite 阶段取指之前的低电平段长了一个 dot），`RenderTiming` 与 `ppu.render_v` 已删除。

Mesen 有而此处缺的：`_lowBitShift`/`_highBitShift`、`TileInfo`、`LoadTileInfo`、
`LoadSprite`/`LoadExtraSprites`、`ShiftTileRegisters`、`GetPixelColor`、`DrawPixel`、
`_minimumDrawBgCycle`/`_minimumDrawSpriteCycle`，以及 `ProcessSpriteEvaluation`
（`ProcessSpriteEvaluationStart`/`End`、OAM 地址行为、`SetOamCorruptionFlags`/
`ProcessOamCorruption`）。

### 2.2 PPU：总线、OAM 与渲染周边的寄存器

- **open bus 没有衰减**：Mesen 每位一个时间戳（`_openBusDecayStamp`，3 帧后衰减）并掩蔽
  `$2002` 的低五位；内核只有一个从不衰减的锁存。`ppu_open_bus` 因此失败。
- **OAM 衰减完全没有建模**（`_oamDecayCycles`、`OamDecayCycleCount = 3000`）。
- **渲染期的 `$2007`**：dummy read、`_ignoreVramRead`、`UpdateVideoRamAddr` 的时序都未建模。
- 强调色/灰阶只有部分实现，未与 `UpdateGrayscaleAndIntensifyBits` 逐位比对。
- PAL 的 PPU 规则（`_nmiScanline`、`_vblankEnd`、PAL 的 OAM 刷新）缺失。

### 2.3 CPU：DMA 是事后补周期

Mesen 在**每次 `MemoryRead` 开头**跑 `ProcessPendingDma`：halt 与 dummy 周期、中止逻辑
（`StopDmcTransfer`、`_abortDmcDma`）、`$4016`/`$4017` 的 bit-loss 行为与 `ProcessDmaRead` 的取值
合并、DMC/OAM 的地址冲突。内核则是在指令之后一次性补 `dma_stall_cycles`
（`NesBus::step_instruction`），因此停顿与指令自身的访问不交错，停机期间也不采样中断线。这是
CPU 侧最大的差距，DMC 的工作依赖它。

较小的 CPU 差异：Mesen 复位后跑八个预热周期并把周期计数置为 `-1`（内核两者都没做）；Mesen 有
`_ppuOffset`（默认 1）与可选的 CPU/PPU 随机对齐（内核偏移为 0）。

### 2.4 APU：帧计数器相位与 DMC

- 帧计数器相位几乎是 `apu_test` 除 DMC 外全部剩余失败：`$4017` 重启被延迟（模式与 IRQ 禁止
  立即生效，计数器在三个周期后才过边界，落在 APU 时钟另一半时是四个），IRQ 标志在请求它的那一步
  之后三个周期才置位，且序列在最后两个 tick 会再请求一次。
- DMC 按 ROM 自己的报告 "isn't working well enough"，需要 §2.3 的 DMA 工作。
- 重采样是采样保持抽取；Mesen 用带限（blip）重采样。
- 扩展音源（VRC6、MMC5、Sunsoft 5B）有实现，但从未与参考录音比对过；N163 与 FDS 缺失。

### 2.5 Mapper

- **不解析 NES 2.0 submapper**。Mesen 用它决定板级行为，这正是 `6-MMC3_alt`（备用 IRQ 版本）、
  116、176 被挡住的原因。
- **总线冲突未建模**（没有 `HasBusConflicts`），即 2/3/7 这类离散板。
- 缺失的板：64（RAMBO-1，158 随之）、115、116、176、185（需要 pattern-data-from-VRAM 窗口）。
- MMC5：扩展属性模式（`$5104 = 3`）缺失；竖向分屏已决定不做。
- 钩子粒度：`on_ppu_bus` 已是逐 dot，但 `on_scanline` 与 `on_cpu_cycles` 仍是近似，Mesen 是每
  CPU 周期调 `ProcessCpuClock`。

## 3. 优先级，以及每一步应当拿回什么

1. **逐 dot PPU 渲染器**（§2.1）。拿回 `ppu_vbl_nmi/10-even_odd_timing`、`mmc3_test_2/
   4-scanline_timing` 的最后那 1 个 dot、AccuracyCoin 里 sprite 0 与渲染时序的一大批，并把 A12
   与 sprite 0 从"重建"变成"产生"。§2.1 的其余部分都随之而来。
2. **DMA 移进 `MemoryRead`**（§2.3）。拿回 `cpu_interrupts_v2/4-irq_and_dma`，并解锁 DMC。
3. **APU 帧计数器相位，然后 DMC**（§2.4）。拿回 `4-jitter`、`5-len_timing`、
   `6-irq_flag_timing`、`7-dmc_basics`、`8-dmc_rates`。
4. **open bus 衰减与 OAM 衰减**（§2.2）。拿回 `ppu_open_bus`。
5. **PAL**（16/5 加分数 dot 累加器，然后 PAL 专属的 PPU/APU 规则）。没有 PAL ROM 就无法验收。
6. **Mapper**（§2.5）：先做 submapper 表（一次解锁三个板），再总线冲突，再 64/158/185/115。
7. **音频重采样**对齐 Mesen 的带限参考。

## 4. 有意保留的偏差（未经决策不要"修正"）

- **KIL/JAM**。Mesen 把 KIL 当作不可恢复的崩溃并清掉中断采样，使挂死无法被打断。内核保留硬件
  行为——中断或复位可以把 CPU 重新拉起——因为宿主需要上报挂死（`Nes::is_cpu_halted`，Web 前端
  会提示）。PC 回退沿用 Mesen，清采样**不是**。
- **访问在周期内的落点**。`ACCESS_DOTS = 2`：芯片的访问落在该周期前两个 dot 之后。Mesen 的
  `_startClockCount`/`_endClockCount`（12 中的 6/6）把它放在约一个 dot 处。这个 dot 是拿
  blargg 的 `05-nmi_timing`/`06-suppression`/`02-vbl_set_time` 测出来的，且没有任何套件能区分
  写入时的 0/1/2；逐 dot 渲染落地后作为一次实验重新审视。
- **PAL 建模为 15/5**（每 CPU 周期 3 个 dot）而不是真机的 16/5，使 PAL 的帧率/音频/CPU 周期自洽。
  切换它只需两个数加上 `has_whole_dot_ratio`，有测试把三者钉在一起。

## 5. 观测手段

已有，且上述每次实测都用到：

- `cargo test -p nes-core --test rom_tests -- --nocapture` —— 逐个 ROM 的 pass/known 行。
- `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture` —— 帧耗时
  （nestest 连续 600 帧；多跑几次取中位数）。
- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture` —— 逐指令 trace
  （`NES_TRACE_ROM`）与逐行 PPU 总线地址（`NES_A12_ROM`、`NES_A12_SKIP`、`NES_A12_LINE`）。
- 测试 ROM 自带源码与 readme（`roms/blargg/*/source/`、`readme.txt`）：blargg 的套件写明精度是
  "一 PPU clock"，并打印期望轨迹。请与它们比对，而不是与我们的形状比对。
- `Nes::ppu_bus_trace()` / `Nes::clear_ppu_bus_trace()` / `Nes::set_ppu_bus_trace_capacity()`：
  PPU 的总线时间线，每 dot 一条采样，由机器在 dot 发生时填充（因此即使 CPU 以整条指令推进，
  顺序仍是机器顺序）。这与卡带看到的是同一个视图，`test_a12_clockings_per_frame` 就是通过它
  按帧统计合格的 A12 上升。`Nes::step_dot()` 是与之配套的推进器——调试器用的 dot 计数器，
  语义是把 `step_instruction()` 挪到另一颗芯片上。

缺失、需要时应当补上：

- PAL 测试 ROM、MMC5 套件、总线冲突 ROM，以及扩展音源的金标准音频比对；
- 以 Mesen 本身作为 oracle 的差分（它的调试器能输出逐 cycle 轨迹），可把"整片 ROM 通过/失败"
  变成逐指令差异。

## 6. 验收协议

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p nes-core --test rom_tests -- --nocapture   # 逐个 ROM 的 pass/known 行
```

- ROM 不入库：把 blargg 各套件与 AccuracyCoin 放到 `roms/` 下（布局与下载来源见
  `docs/architecture.md` §13.1）。
- 基线清单里"开始通过"的项必须从 `tests/rom_tests.rs` 删掉；清单之外出现失败会直接红。凡因更
  忠实于 Mesen 而产生的回退，**连同原因**加入清单。
- 确定性是不变量而非特性：`frame()` / `run_cycles()` / `step_instruction()` 在同一帧边界必须得到
  逐字节相同的状态，存档必须能往返。
- `docs/architecture.md` 与其 `zh-CN` 对照必须与代码同步；每一步的实测数字写进提交信息。

## 7. 历史与实测

这些工作分步落地，每一步都在前一步的基线上量过。数字来自当时的提交信息；`帧耗时` 为 release
模式、nestest 连续 600 帧、多次取中位数。

| 步骤 | 做了什么 | 实测 |
| --- | --- | --- |
| L1 | 机器改为逐 CPU cycle 推进，而非按指令批量补 | ROM 零变化；帧耗时 1.075 → 1.046 ms |
| L2 | 中断线在倒数第二个 cycle 采样；七周期向量过程逐 cycle 执行 | AccuracyCoin 78 → 79 |
| L3 第 1 步 | 对 `$2000-$401F` 的写入落在芯片驱动的那个 cycle | `4-irq_and_dma` 轨迹变成文档形状；帧耗时 1.033 → 1.034 ms |
| L3 第 2 步 | 写入还落在该 cycle 内的一个 dot 上，早于同 cycle 末尾的 APU tick | ROM 57/17 → **58/16**（`5-len_timing`）；AccuracyCoin 79/49 → 81/47 |
| L3 第 3 步 | 压状态字节前到达的 NMI 接管运行中的序列；BRK 的取向量保持临时 | 无套件翻转；`3-nmi_and_irq` 两行与 readme 对齐 |
| L3 第 4 步 | `$2000-$3FFF` 的读落在自己的 cycle **与 dot**；VBlank 抑制窗口宽 1 dot；NMI 线逐 cycle 采样 | ROM 58/16 → **62/12**；AccuracyCoin 81/47 → 82/46；帧耗时 1.11 → 1.16 ms |
| Phase 0 | 单一主时钟 + `cpuDivider`/`ppuDivider`；PPU 按目标推进；访问落点用 tick 偏移；`run_ppu_to` 与计数式快路径 | ROM 零变化；帧耗时 1.206 → 1.210 ms（首版每周期都做时钟算术，代价 1.266 ms） |
| Phase 1 | CPU 亲自驱动周期：每次访问即周期、丢弃读逐条写明、中断线逐周期采样且判决滞后一周期、向量过程归 CPU | ROM 构成变化：`2-nmi_and_brk` 转绿，`5-len_timing` 因 `$4015` 读落在自己周期而移入清单；`instr_timing`/`branch_timing` 保持全绿；帧耗时 1.204 → 1.204 ms |
| 奇数帧的 dot | 跳点在 dot 339 判定，与 Mesen 相同 | ROM 无移动（`10-even_odd_timing` 逐字节相同），因此排除这个边界 |
| A12 之一 | 交给卡带的是最后真实访问的地址并保持 | `4-scanline_timing` 从第 2 子项到第 4 子项 |
| A12 之二 | 预渲染行也跑取指流水线 | 从第 3 到第 4 子项，方向翻转 |
| A12 之三 | 过滤以 CPU 周期计长度，且**每次**上升都结束当前低电平段 | **`2-details` 转绿**（"每帧 241 次"）；`4-scanline_timing` 推进到第 12 子项 |
| 逐 dot 背景 | `PixelPipeline` 替换整行渲染器：取指落在 `cycle % 8 == 1/3/5/7`、移位寄存器、每 dot 一个像素，滚动取真实的 `v`/`t`；卡带拿到的是真实读过的地址；删除 `RenderTiming` 与 `ppu.render_v` | **`4-scanline_timing` 转绿**（先前的重建长了一个 dot）；其余基线不变（AccuracyCoin 46）；帧耗时 1.32 → 1.80 ms |

### 实现、实测、随后撤回的实验

它们的价值在于证据；不先读这一段就不要重试。

- **L2b——让 NMI 接管 IRQ 序列（Phase 3 之前）**。没有一个 blargg 用例转绿，AccuracyCoin 还少了
  一项。它在第 3 步、写入周期就位之后才回来。
- **L3a——单独把寄存器写入延后到真实 cycle**。`4-irq_and_dma` 变成文档形状，但打破了四条原本
  全绿的线，因为**寄存器写入不能独自移动**：立即 NMI 检测、A12 用的掩码、两条长度计数器线都
  必须随之移动。后来作为第 1 步**连同依赖**一起落地。
- **L4a——在 DMA 停顿期间轮询中断线**。`4-irq_and_dma` 的轨迹头部有无它都逐字节相同；那段滞后
  来自中断源的相位（帧计数器），而不是 CPU 对 DMA 的应答。
- **帧计数器相位，按三块实现**。让 `4-jitter` 与 `6-irq_flag_timing` 转绿，却丢掉三项 AccuracyCoin
  （两项帧计数器测试与两个错误码），两项换三项按本文的规则算回退。分歧在计数器的相位——两个套件
  读的不是同一个事件——先弄清是哪一个，是这项工作的第一件事。
- **按 dot 计数器重建 A12，却不带真实取指**。`4-scanline_timing` 前进，但 `2-details`
  （"每帧 241 次"）回退，因为地址序列与计数器过滤是同一套机制：用真实地址时 A12 在两次取指之间
  只低 4 个 dot，而真正应当构成每帧唯一合格上升的是跨扫描线边界的那段低电平。这是逐 dot 渲染器
  最具体的论证。
- **open bus 衰减（Mesen 的每位、三帧模型）**。按 Mesen 忠实实现；`ppu_open_bus` 没有移动（它的
  第 2 子项因衰减路径之外的原因失败——`$2000-$3FFF` 的读取路径返回的是 **CPU** 侧总线锁存而不是
  PPU 自己的那条，这是那里第一个该查的点）。

## 8. 仍需拍板的决策

1. **渲染器做到哪一层**：只做背景流水线（取指、移位寄存器、逐 dot 像素），还是连 sprite 流水线
   一起（`LoadSprite`、逐 dot sprite 像素、sprite 0 落在像素处）。前者是后者的前提，而 ROM 收益
   主要在后者。
2. **`6-MMC3_alt`**：加一张小的按 ROM 覆盖表来选备用 IRQ 版本，还是解析 NES 2.0 submapper
   （后者同时解锁 116/176）。
3. **PAL**：切换比例与 dot 累加器，还是在拿到 PAL ROM 之前把 PAL 作为已记录的近似保留。
4. **音频**：扩展芯片做金标准音频比对，还是维持单元测试。
5. **访问的 dot**（§4）：逐 dot 渲染落地后，对 `05-nmi_timing`/`06-suppression`/`02-vbl_set_time`
   做一次实验。
