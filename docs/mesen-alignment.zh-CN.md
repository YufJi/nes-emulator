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
| A12 | `SetBusAddress` 把**最后一次真实访问**的地址放上总线并保持；过滤是 `(GetMasterClock() - _a12LowClock) >= 3`，主时钟即 CPU 周期数 | `NesBus::bus_address`（保持），由 `PixelPipeline` **真实读过**的地址、在它读的那个 dot 上喂入；`Mapper::on_ppu_bus(addr, cpu_cycles)` 与 `mapper4` 以周期计长度且**每次上升都清零** | `2-details`（241 次）与 `4-scanline_timing` 均转绿 |
| 逐 dot 渲染 | `ProcessScanlineImpl`：`cycle % 8 == 1/3/5/7` 取指，每 dot `ShiftTileRegisters` 与 `DrawPixel`，用同一个 `_videoRamAddr` | `PixelPipeline`：同样的取指/装载/移位/绘制节奏，滚动取 `v`/`t`/`x`；sprite 阶段的取指、逐 dot sprite 像素与 sprite 0 命中都在这里 | `4-scanline_timing`、`sprite_hit` 11/11，且 `nestest` 的帧与整行渲染器逐字节相同 |
| open bus | 每位一个时间戳（`_openBusDecayStamp`）、三帧衰减、`$2002` 低五位从中取，且用 PPU 自己的锁存而非 CPU 的 | 相同：`Ppu::drive_open_bus`/`open_bus_decay_stamp`，`$2002` 只驱动高三位，`$2007` 的调色板读只驱动六位 | `ppu_open_bus` 1/1 |
| 预渲染行 | 流水线条件 `scanline < 240`，对预渲染行（-1）成立 | `drive_ppu_bus` 的 `in_vblank` 只排除 240..=260 | 同上 |
| 奇数帧跳点 | 在处理 dot 339 时判定（`_cycle = 340`），被跳掉的是 340 | 相同，由 `NesBus::skip_odd_frame_dot` 承载 | `test_a_register_write_is_in_time_for_the_odd_frame_decision` |

## 2. 尚未对齐的部分

### 2.1 PPU：sprite 的**求值**仍是逐行

渲染已经逐 dot（`PixelPipeline`）：背景每八个 dot 取一个瓦片、每 dot 移出一像素并在它自己的
dot 上画出来，滚动取芯片自己的 `v`/`t`/`x`；sprite 阶段的图案取指落在各自的 dot 上
（257-320），sprite 像素与 sprite 0 命中都由取到的字节逐 dot 画出。Mesen 的
`ProcessScanlineImpl` 里剩的是求值本身：`ProcessSpriteEvaluation`（65-256）仍是"一次调用填满
下一行的二级 OAM"，而不是芯片那套奇偶 OAM 读模式加
`SetOamCorruptionFlags`/`ProcessOamCorruption`。

仍未解决的后果：

- **sprite 只能被搬进二级 OAM 一次**：求值过程中写 `$2003` 造成的 OAM 地址损坏没有建模。
- MMC5 竖向分屏仍然不做。
- sprite 像素与 sprite 0 命中都跟随取指，因此行内切 CHR 银行现在会在它落到的那个 dot 上
  影响 sprite。

随逐 dot 背景与 sprite 一起落地的还有：真实取指地址让 `4-scanline_timing` 转绿（先前的重建
让首个 sprite 阶段取指之前的低电平段长了一个 dot），`RenderTiming`、`ppu.render_v` 与整行渲染器
都已删除。

Mesen 有而此处缺的：`_lowBitShift`/`_highBitShift`、`TileInfo`、`LoadTileInfo`、
`LoadSprite`/`LoadExtraSprites`、`ShiftTileRegisters`、`GetPixelColor`、`DrawPixel`、
`_minimumDrawBgCycle`/`_minimumDrawSpriteCycle`，以及 `ProcessSpriteEvaluation`
（`ProcessSpriteEvaluationStart`/`End`、OAM 地址行为、`SetOamCorruptionFlags`/
`ProcessOamCorruption`）。

### 2.2 PPU：总线、OAM 与渲染周边的寄存器

- ~~**OAM 衰减**~~ 不算差距：Mesen 把它放在 `EnableOamDecay` 这个设置后面，而该设置**默认关闭**
  （`BaseNesPpu.h` 里 `_enableOamDecay = false`）；规则本身是"每八个字节一行一个 3000 周期时间戳，
  衰减后的值就是该行自己的地址"（`NesPpu::ReadSpriteRam`）。不做它正合 Mesen 的默认。
- ~~**渲染期的 `$2007`**~~ 已按 `UpdateVideoRamAddr` 与 `ReadRegister` 建模：在绘制行上且渲染开启时，
  `$2007` 的访问推进的是**滚动**——粗 X 自增与 Y 自增同时发生——而不是地址；同一 `$2007` 的第二次
  访问若落在六个 PPU dot 内会被忽略并读回 open bus（`_ignoreVramRead = 6`）。不在绘制行上（或渲染
  关闭时）仍是文档里的 1/32 自增。
- ~~**强调色/灰阶**~~ 已对齐 `UpdateColorBitMasks`：灰阶位不仅在送往屏幕时、在 `$2007` 读出时也用
  $30 掩蔽调色板值；PAL 与 Dendy 的强调位含义互换。**没有**对齐的是这九位最终解出的 RGB：Mesen 把
  Red/Green/Blue 或进像素值、由视频滤镜查一张 512 项调色板，而本内核是逐像素转换、把未被强调的通道
  乘 0.75。
- PAL 的 PPU 规则（`_nmiScanline`、`_vblankEnd`、PAL 的 OAM 刷新）缺失。

### 2.3 CPU：DMA 是事后补周期

Mesen 在**每次 `MemoryRead` 开头**跑 `ProcessPendingDma`：halt 与 dummy 周期、中止逻辑
（`StopDmcTransfer`、`_abortDmcDma`）、`$4016`/`$4017` 的 bit-loss 行为与 `ProcessDmaRead` 的取值
合并、DMC/OAM 的地址冲突。内核则是在指令之后一次性补 `dma_stall_cycles`
（`NesBus::step_instruction`），因此停顿与指令自身的访问不交错，停机期间也不采样中断线。这是
CPU 侧最大的差距，DMC 的工作依赖它。

较小的 CPU 差异：Mesen 复位后跑八个预热周期并把周期计数置为 `-1`（内核两者都没做）；Mesen 有
`_ppuOffset`（默认 1）与可选的 CPU/PPU 随机对齐（内核偏移为 0）。

### 2.4 APU：DMC、重采样与扩展音源

- ~~帧计数器相位~~ 已完成：`$4017` 重启让计数器迟 3 个周期起步（APU 奇数相位时 4 个），帧 IRQ
  标志从写入后第 29831 个周期起**连续三个周期**置位——两个数字都取自 blargg 自己在
  `roms/blargg/apu_test/source/` 里写下的实测 cycle，而不是猜的。`4-jitter`、`5-len_timing`、
  `6-irq_flag_timing` 转绿。
- **DMC**：ROM 找出的读写语义 bug 已修——`$4015` 的 bit 4 此前**从未置位**（`write_4015` 没设
  enable 标志），且读 `$4015` 会错误地清掉 DMC 的 IRQ 标志；`7-dmc_basics` 只剩最后一项检查。
  Mesen 自己的模型现已抄录在 §7（`Core/NES/APU/DeltaModulationChannel.cpp`），而它与剩下两项检查
  想要的东西**互相矛盾**，这才是必须先解决的事：
  * Mesen 的状态位是 `_bytesRemaining > 0`，长度计数在 `SetDmcReadBuffer` 里移动——也就是字节被
    **从内存读出**时，而不是它播完时。按这个模型，17 字节样本的状态翻转落在约 128 个周期。
  * `8-dmc_rates` 的窗口要的是约 135 个周期（rate 0 下 57715–57805 周期），那是**最后一个字节播完**
    的位置；本内核按播放计数的版本落在 135.3 个周期，晚了 110 个周期。
  动手实现 Mesen 版本之前，先弄清硬件到底是哪一种——ROM 与源码在这里不一致，而
  `7-dmc_basics` 第 19 项读的正是同两位、且两种模型给出的答案相反。
- ~~**DMC 的 PAL 周期表**~~ 已完成：`DmcChannel::set_region` 在 PAL 上选用 Mesen 的
  `_dmcPeriodLookupTablePal`（`398, 354, ... 50`），卡带载入时由主机把制式传下去。
- 重采样是采样保持抽取；Mesen 用带限（blip）重采样。
- 扩展音源（VRC6、MMC5、Sunsoft 5B）有实现，但从未与参考录音比对过；N163 与 FDS 缺失。

### 2.5 Mapper

- ~~**不解析 NES 2.0 submapper**~~ 现在会解析：byte 7 的 bit 2-3 判定版本，byte 8 携带 submapper
  与 mapper 号的高四位，byte 9 的两个半字节是尺寸的高位，byte 12 直接给出制式；指数-乘数形式的
  尺寸编码会被明确拒绝而不是误读（`Nes::rom_submapper` 把它暴露出来）。**没有**做的是按它分派：
  需要这件事的作品用的都是纯 iNES 头部，选择信息在数据库里而不在文件里——`6-MMC3_alt` 就是例子，
  116/176 同理。
- ~~**总线冲突未建模**~~ 已建模：`Mapper::has_bus_conflicts`，在每次写到卡带的路径上
  （`NesBus::write_now`）按"CPU 的值 AND 板子在该地址返回的字节"计算——即 Mesen 的
  `HasBusConflicts`。确实带冲突的离散板会声明：2、3、7、11、66、71。
- 缺失的板：64（RAMBO-1，158 随之）、115、116、176、185（需要 pattern-data-from-VRAM 窗口）。
- MMC5：扩展属性模式（`$5104 = 3`）缺失；竖向分屏已决定不做。
- 钩子粒度：`on_ppu_bus` 已是逐 dot，但 `on_scanline` 与 `on_cpu_cycles` 仍是近似，Mesen 是每
  CPU 周期调 `ProcessCpuClock`。

## 3. 优先级，以及每一步应当拿回什么

1. ~~**逐 dot PPU 渲染器**（§2.1）~~ —— 已完成：`4-scanline_timing` 转绿，A12 与 sprite 0 从
   "重建"变成"产生"，整行渲染器删除。`ppu_vbl_nmi/10-even_odd_timing` 没有移动；§2.1 里还剩
   sprite 的**求值**仍是逐行。
2. ~~**DMA 移进 `MemoryRead`**（§2.3）~~ —— 已完成，但没有 ROM 移动：`4-irq_and_dma` 的轨迹
   逐字节未变，与 §7 的 L4a 结论一致。它买到的是 DMC 需要的那套形状。
3. **APU 帧计数器相位，然后 DMC**（§2.4）。帧计数器已完成——`4-jitter`、`5-len_timing`、
   `6-irq_flag_timing` 转绿。DMC 只剩两项检查（`7-dmc_basics` 的一字节缓冲、
   `8-dmc_rates` 的 rate 0 窗口）。
4. ~~**§2.2 的寄存器细节**~~ —— 已完成：open bus 衰减（`ppu_open_bus` 转绿）、渲染期 `$2007` 的行为、
   调色板读的灰阶掩蔽、PAL 的强调位互换。OAM 衰减查明是 Mesen 默认关闭的设置，因此"不做"正是它的
   默认。唯一没对齐的是那九位颜色最终解出的 RGB（Mesen 的 512 项表，归它的视频滤镜所有）。
5. **PAL**。比例已完成——PAL 跑 16/5，启用分数 dot 累加器，一帧是 33247.5 个 CPU 周期，音频时钟
   也改成按真实的 3.2 推导而不是照 NTSC 形状的常量。剩下的是 PAL 专属的 PPU/APU 规则
   （`_nmiScanline`、`_vblankEnd`、PAL 的 OAM 刷新）；没有 PAL 测试 ROM 就无法验收。
6. **Mapper**（§2.5）：NES 2.0 头部与总线冲突已完成。剩下的是按 submapper 分派（需要 Mesen 放在
   数据库里的逐 ROM 数据，而不是头部字段），以及 64/158/185/115 四个板。
7. **音频重采样**对齐 Mesen 的带限参考。

## 4. 有意保留的偏差（未经决策不要"修正"）

- **KIL/JAM**。Mesen 把 KIL 当作不可恢复的崩溃并清掉中断采样，使挂死无法被打断。内核保留硬件
  行为——中断或复位可以把 CPU 重新拉起——因为宿主需要上报挂死（`Nes::is_cpu_halted`，Web 前端
  会提示）。PC 回退沿用 Mesen，清采样**不是**。
- **访问在周期内的落点**。`ACCESS_DOTS = 2`：芯片的访问落在该周期前两个 dot 之后，Mesen 的
  `_startClockCount`/`_endClockCount`（12 中的 6/6）把它放在约一个 dot 处。这是**实测**的，且
  逐 dot 渲染让这个测量变得尖锐：流水线按 dot 取指之后，`ACCESS_DOTS = 1`（以及 0）各自会让
  五个 ROM 失败——`ppu_vbl_nmi` 的 `05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`，
  `cpu_interrupts_v2/2-nmi_and_brk`，以及 `mmc3_test_2/4-scanline_timing`——而 2 让所有套件通过。
  上文原先写的"没有任何套件能区分 0/1/2"在整行渲染器时代成立，现在不成立了。

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
| 逐 dot sprite | sprite 阶段的图案取指、逐 dot 像素、以及落在自己那个 dot 上的 sprite 0 命中；整行渲染器（`BackgroundRenderer`、`SpriteRenderer`、`ScanlineProcessor`）删除 | ROM 无移动（基线不变，AccuracyCoin 46）；帧耗时 1.80 → 1.68 ms |
| DMA 移进 `MemoryRead` | `NesBus::process_pending_dma`（Mesen 的 `ProcessPendingDma`）：`$4014` 写与 DMC 请求只留下待办，在 CPU 下一次访问时以真实机器周期完成 | ROM 无移动——`4-irq_and_dma` 的轨迹逐字节相同，与 L4a 的结论一致：它的偏移来自帧计数器相位 |
| open bus 衰减 | 换成 PPU 自己的总线锁存；每位三帧时间戳；`$2007` 用它返回的字节驱动总线；调色板读只驱动六位、另两位取自总线；`$2002` 只驱动三位 | **`ppu_open_bus` 转绿**（0/1 → 1/1） |
| 帧计数器相位 | `$4017` 让计数器迟 3 个周期起步（奇数 APU 相位时 4 个）；帧 IRQ 标志从写入后第 29831 个周期起连续三个周期置位 | **`4-jitter`、`5-len_timing`、`6-irq_flag_timing` 转绿**（apu_test 3/8 → 6/8）；其余基线不变 |
| DMC 修复 | `write_4015` 现在会置 enable 标志（此前 `$4015` 的 bit 4 从未置位）；读 `$4015` 不再清 DMC 的 IRQ 标志；字节计数按**已播放**的字节走，并改成两级缓冲 | `7-dmc_basics` 从第 2 项推进到第 19 项；`8-dmc_rates` 仍在基线（rate 0 的窗口） |
| PAL 16/5 | `cpu_divider` 15 → 16，`has_whole_dot_ratio` 的 PAL 分支改 false 以启用主时钟路径，APU 的速率改由 `dots_per_cpu_cycle()` 推导而不再用 3.0 常量 | ROM 无移动（没有 PAL ROM）；PAL 一帧是 33,247.5 个 CPU 周期，音频时钟与公布值相差不到 10 Hz |
| 访问的 dot | 逐 dot 渲染落地后，把 `ACCESS_DOTS` 取 0/1/2 各跑一遍完整 ROM 基线 | **实测值是 2**：0 与 1 各自让五个 ROM 失败（`05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、`2-nmi_and_brk`、`4-scanline_timing`）；2 让所有套件通过 |
| NES 2.0 头部 + 总线冲突 | 头部解析版本、submapper、byte 9 的尺寸高位与 byte 12 的制式（指数-乘数编码明确拒绝）；`Mapper::has_bus_conflicts` 让卡带写入等于 CPU 值与该地址板子字节的按位与，覆盖 2/3/7/11/66/71 | ROM 无移动（本地套件用的是 mapper 0/1/4，都不带那个冲突陷阱）；`Nes::rom_submapper` 把解析结果暴露出来 |
| PPU 寄存器细节（§2.2） | 直接读 Mesen 源码（`NesPpu.cpp`、`BaseNesPpu.cpp`）：绘制行上 `$2007` 同时推进粗 X 与 Y、六个 dot 内的第二次 `$2007` 被忽略、调色板读带灰阶掩蔽、PAL/Dendy 互换强调位；OAM 衰减查明默认关闭 | ROM 无移动（`blargg_ppu_tests` 覆盖 `$2007` 路径）；三个单元测试钉住行为 |
| Mesen 的 DMC 源码 | 读了 `Core/NES/APU/DeltaModulationChannel.cpp`（manifest 给出路径后取回）：状态位 = `_bytesRemaining > 0`；计数与 IRQ 在 `SetDmcReadBuffer` 里移动，即取数时；寄存器在 `_bitsRemaining` 归零时接收缓冲字节，静音标志保持 DAC 电平；`SetEnabled` 按奇偶把传输延迟 2/3 个周期；PAL 有独立的周期表 | 按 Mesen 形状的实现试过并**撤回**：它把 `8-dmc_rates` 放到约 128 个周期，而该 ROM 的窗口要的是约 135，`7-dmc_basics` 退回第 2 项。树里保留按播放计数的版本（135.3 个周期） |

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
- **open bus 衰减（Mesen 的每位、三帧模型）**。让 `ppu_open_bus` 转绿一共要三处，顺序如下：
  `$2000-$3FFF` 的读必须返回 **PPU 自己**的锁存而不是 CPU 侧总线锁存（正如先前的记录所料，这是
  第一个该查的点）；`$2007` 的读必须用它返回的那个字节去驱动这条锁存；调色板的读要从 open bus
  取高两位、却只驱动低六位，而 `$2002` 只驱动高三位，好让低五位继续衰减。

- **DMC：按 Mesen 模型实现（§2.4）**。读了源码之后模型是明确的，于是照做：长度计数与 IRQ 跟随
  取数、寄存器只在位计数归零时才接收缓冲字节、静音标志在样本之间保持 DAC 电平、enable 的传输
  延迟两个周期（奇数周期三个）。结果是 `8-dmc_rates` 从"周期太短"变成**超时**，`7-dmc_basics`
  退回第 2 项——两项都比现有的播放计数版更差。因此撤回：在弄清上面那约 7 个周期的分歧之前，
  计数/IRQ 的落点无法对齐。树里保留按播放计数的版本与它对应的 ROM 判定。
- **DMC：把内存读取器提前一个输出时钟（§2.4）**。当长度计数在字节边界移动时，每个字节的传输
  延迟都会被加进样本长度，这正是让 `8-dmc_rates` 的 rate 0 样本刚好越过窗口的那一百多个周期。
  把读取器移到 `bits_remaining == 1`（在最后一位尚未移出时就开始取，好让寄存器见底前缓冲已满）
  形状是对的，孤立测下来一份常驻的 17 字节样本会落到 134.3 个周期；但 ROM 的判定完全没动（仍
  越过窗口），说明多出来的并不是逐字节的传输延迟，而且有一个单元测试
  （`test_reading_4015_does_not_clear_the_dmc_irq`）不再看到它的 IRQ。已撤回：这说明下一次该从
  总线侧诊断（`NesBus::process_pending_dma` 里一次 DMC 取数到底占多少周期），而不是通道侧。
- **帧计数器的相位，两次撤回（§2.4）**。两轮对 Mesen 顺序的猜测都是"一个套件前进、另一个后退"。
  真正解决它的是读 blargg 的**源码**，它就在 `roms/blargg/apu_test/source/` 里：
  `6-irq_flag_timing.s` 的头几行写着标志"is set three times in a row 29831 clocks after
  writing $00 to $4017"，其检查分别在 29830/29831/29832/29833 读 `$4015`——所以标志不是置一次，
  而是保持三个周期，从写入后 29831 开始。`4-jitter.s` 则给出重启的抖动：它在 29831 的读必须在
  偶相位与奇相位下得到不同的值，这正是把计数器 hold 住 3（奇相位 4）个周期所产生的结果。下一次
  在这里动手的教训是：先读 ROM 的源码与注释，并把事件落在哪个 cycle 打出来。

## 8. 仍需拍板的决策

**已拍板**：渲染器一次做到背景 + sprite（§2.1）；PAL 跑 16/5 加分数 dot 累加器（§4）；扩展音源
维持单元测试、不做金标准比对；访问的 dot（§4）等一次用 `Nes::ppu_bus_trace` 做的实验；
`6-MMC3_alt` 需要逐 ROM 覆盖表或按 CRC 的数据库而不是 submapper——它的头部是纯 iNES，§2.5 已记下。

1. ~~**访问的 dot**（§4）~~ —— 实验已做，结论就是保留 `ACCESS_DOTS = 2`：0 与 1 各自会让五个
   ROM 失败（§7）。
2. **APU 帧计数器的相位**（§2.4）：先弄清 `4-jitter` 与 `6-irq_flag_timing` 各自读的是哪个事件，
   再动计数器（§7）。
