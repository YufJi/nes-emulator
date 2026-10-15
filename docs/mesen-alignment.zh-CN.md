# 与 Mesen 的对齐：现状、差距与推进方式

**简体中文** · [English](./mesen-alignment.md)

这是精度工作的**工作文档**：**把内核与 [Mesen2](https://github.com/SourMesen/Mesen2) 的 NES 内核
对齐，遇到歧义一律以 Mesen 为准。** `docs/architecture.md` 仍是"模拟器今天是什么样"的描述（其
§15.1 是已知限制的摘要）；本文档是计划、计划背后的实测数据，以及仍然不同的地方。
**新会话请先读 §9 的任务台账**——那里是"哪些做完了、哪些卡在哪、下一步做什么"的一页汇总。

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
  $30 掩蔽调色板值；PAL 与 Dendy 的强调位含义互换。
- ~~**这九位颜色解出的 RGB**~~ 已对齐 Mesen 的默认视频滤镜（`NesDefaultVideoFilter.cpp`）：像素值
  是 `palette | Red<<6 | Green<<7 | Blue<<8`（Mesen 的 `_intensifyColorBits`），由一张 512 项表
  解出。置一个强调位会把**另外**两个通道压到原值的 84%（截断），而 `$xE`/`$xF` 两列不参与
  ——即 Mesen 的 `GenerateFullColorPalette`。本内核原有的 64 色基础表经核对就是 Mesen 的 2C02
  表（`_ppuPaletteArgb[0]`），逐项相同，所以真正要改的只有强调：0.75 → 0.84、黑列例外，以及把逐
  像素浮点运算换成一次查表。
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
- ~~**DMC**~~ 已完成：现在是 Mesen 的形状（`Core/NES/APU/DeltaModulationChannel.cpp`），
  `7-dmc_basics` 与 `8-dmc_rates` 都转绿（apu_test 6/8 → **8/8**）。原先"两项检查互相矛盾"
  的判断是错的，矛盾出在总线侧的一个缺陷上（每个取到的字节让 CPU 停两次四周期，见 §7），
  修掉之后三个要害都对上了：
  * 长度计数在 `SetDmcReadBuffer` 里移动，也就是字节被**从内存读出**时，状态位是
    `_bytesRemaining > 0`——所以状态在最后一个字节还在往外送的时候就翻，IRQ 标志也在那一次
    取数时置位（`7-dmc_basics` 第 19 项正是要这个）。
  * 取到的字节**留在缓冲里**，由输出单元在下一个字节边界取走（`Run` 里的搬移），而读取器就在
    那个边界被喂——所以任何时刻缓冲里只有一个字节，`$4015` 的第二次写不会排在正在播放的样本
    后面再压一个字节。
  * 定时器的周期就是表里的值，而不是它加一。
  * `8-dmc_rates` 的 135 个周期由上面三条加 ROM 自己的同步给出：`sync_dmc_fast` 结束时寄存器
    上还留着它那个一字节样本，新样本的第一次取数要等这个字节的边界，于是"7 个 bit"（换成新速率
    后是 7P）正好补上 128P 与 135P 之间的差。
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
3. ~~**APU 帧计数器相位，然后 DMC**（§2.4）~~ —— 都已完成：帧计数器让 `4-jitter`、
   `5-len_timing`、`6-irq_flag_timing` 转绿；DMC 改成 Mesen 的形状（计数在取数时移动、字节留在
   缓冲、静音保持电平、周期取表值）后 `7-dmc_basics` 与 `8-dmc_rates` 也转绿，apu_test 8/8。
4. ~~**§2.2 的寄存器细节**~~ —— 已完成：open bus 衰减（`ppu_open_bus` 转绿）、渲染期 `$2007` 的行为、
   调色板读的灰阶掩蔽、PAL 的强调位互换。OAM 衰减查明是 Mesen 默认关闭的设置，因此"不做"正是它的
   默认。那九位颜色解出的 RGB 也已对齐——Mesen 的 512 项表、84% 截断与黑列例外（§2.2/§7）。
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
| 九位颜色解出的 RGB（§2.2） | 读了 Mesen 的默认视频滤镜（`NesDefaultVideoFilter.cpp`）：像素值是 `palette \| Red<<6 \| Green<<7 \| Blue<<8`，由 512 项表解出；强调项把另外两个通道压到 **84%**、`$xE`/`$xF` 两列不动。本内核原有的基础表经核对就是 Mesen 的 2C02 表，逐项相同 | ROM 无移动（本地没有 ROM 看渲染出的颜色）；四个单元测试钉住表、84% 截断、黑列与 PAL 互换，一个端到端测试把某个像素钉到 `0xFF6493D6`；帧耗时 1.74 → 1.69 ms |
| Mesen 的 DMC 源码 | 读了 `Core/NES/APU/DeltaModulationChannel.cpp`（manifest 给出路径后取回）：状态位 = `_bytesRemaining > 0`；计数与 IRQ 在 `SetDmcReadBuffer` 里移动，即取数时；寄存器在 `_bitsRemaining` 归零时接收缓冲字节，静音标志保持 DAC 电平；`SetEnabled` 按奇偶把传输延迟 2/3 个周期；PAL 有独立的周期表 | 按 Mesen 形状的实现试过并**撤回**：它把 `8-dmc_rates` 放到约 128 个周期，而该 ROM 的窗口要的是约 135，`7-dmc_basics` 退回第 2 项。树里保留按播放计数的版本（135.3 个周期） |
| DMC 一次换两次取数 | 通道的请求锁存会一直保持到字节被送进去，而总线每个 APU 周期都从它那里重新武装待办——于是传输自己的四个周期又武装出第二次。每个字节两次四周期传输，CPU 对 `$4000` 的读永远看不到那个字节。现在总线**取走**请求（`Apu::take_dmc_dma_request`），也就是 Mesen 的一次性 halt 锁存 | **AccuracyCoin 重新能启动**：155 项测试、85 通过、43 失败（上表那些行记的是 46）；其余套件全无移动；帧耗时 1.72 → 1.70 ms；新增两个单元测试（锁存本身，以及一个按 AccuracyCoin 启动同步那样轮询 `$4000` 的手写 ROM） |
| DMC 改成 Mesen 的形状 | 长度计数在**取数**时移动（而不是播放结束时），取到的字节**留在缓冲里**等输出单元在字节边界取走（于是一个时刻只有一个字节，`$4015` 再写一次也压不进去第二个），寄存器安静时保持 DAC 电平而不是在零上衰减，定时器周期就是表里的值而不是加一 | **`7-dmc_basics`、`8-dmc_rates` 转绿**（apu_test 6/8 → **8/8**）；AccuracyCoin 43 → **34 项失败**（共跑 143、94 通过）；其余套件无移动；帧耗时 1.70 → 1.74 ms；存档版本 10 → 11 |

**一个过期数字，已更正。** 从 `8ec693d`（DMA 移进 `MemoryRead`）到上表最后一行之间，
AccuracyCoin 其实从未真正跑起来：ROM 在启动时卡进自己的 DMA 同步循环，`$37` 一直是 0，
而 harness 把它当成"这里没有 ROM"跳过了自己——于是中间几行原样重复了它在 `30b7267` 时的 46，
并没有实测（回头在本次会话之前的 `0934d3a` 上重测，菜单能正常显示，所以这个卡死来自 DMA 的
改动，不是逐 dot 渲染）。harness 现在在 ROM 什么都报不出来时会直接失败，本行的数字是实测的。

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
- **DMC 那"约 7 个周期"的分歧，已诊断（这不算实验）**。两项 DMC 判定其实同时在读总线侧与通道
  侧：通道每个取到的字节都让 CPU **停了两次**四周期（见 §7 最后一行），于是一个字节花掉 440 个
  CPU 周期而不是 432，所有样本都偏长。传输只收一次费之后，该比较的就是通道自己的数字了——所以
  §2.4 里 Mesen"按取数计数"的模型值得再试一次，而不是再猜一轮。
- **`cpu_interrupts_v2`：帧计数器标志晚一个周期（`$4017` 的 hold 4/3 → 5/4）**。实测：
  `3-nmi_and_irq` **通过**，`5-branch_delays_irq` 的 `test_jmp` 表与它 readme 里印的**逐行相同**。
  但 `4-jitter`（"Odd jitter not handled properly"）、`5-len_timing`（"First length of mode 0 is
  too late"）与 `6-irq_flag_timing`（"Flag first set too late"）全部变红，AccuracyCoin 从 34 掉到
  36。已撤回：这三项把标志的周期钉在 `$4017` 写入上，所以缺的那个周期不在标志里。
- **`cpu_interrupts_v2`：IRQ 判决晚一个周期（取两周期前的采样，而不是一个周期前）**。实测：
  `3-nmi_and_irq` 通过，`5-branch_delays_irq` 的 `test_jmp`、`test_branch_not_taken`、
  `test_branch_taken_pagecross` 三张表与 readme **逐行一致**——这是本内核第一次重现那张表——只有
  最后一个子项（源码里注释掉的"special case"）仍差一行（我们 `05 05 03`，readme 是 `05 01 00`）。
  但 `1-cli_latency` 的第 4 项（"exactly one instruction after CLI should execute before IRQ is
  taken"）与 `4-scanline_timing` 的第 3 项变红。已撤回：这两项把普通 IRQ 的判决钉在倒数第二个
  周期上，那也是 Mesen 的规则。
- **`cpu_interrupts_v2`：把 `$4017` 写入的奇偶对调**。实测：`5-branch_delays_irq` 逐字节不变
  （所以它的缺陷不是写入的 hold），`3-nmi_and_irq` 的输出有变化，AccuracyCoin 从 34 掉到 36。
  已撤回。

  **剩下的是什么**。三个失败项其实是同一个 ±1 周期的问题：帧计数器的标志、IRQ 的判决、以及指令流
  三者的相对位置。有三组测试在约束它，而我们试过的任何一个"挪一个周期"的旋钮都满足不了全部三组：
  `4-jitter`、`5-len_timing`、`6-irq_flag_timing` 把标志钉在 `$4017` 写入上；`1-cli_latency` 与
  `4-scanline_timing` 把判决钉在倒数第二个周期上；而三张 `cpu_interrupts_v2` 的表要的是这两者
  相对代码各挪一个周期（`4-irq_and_dma` 自己的表也是 readme 的整体错开一行：我们开头是
  `0 +0 / 0 +1 / 1 +2`，readme 是 `0 +0 / 1 +1 / 1 +2`）。下一次该做的是**量链条**——把
  `5-branch_delays_irq` 里 `begin` 的那些延时按指令逐条算出来，和 readme 的表反推出的周期对
  （它的 `test_jmp` 第 0 行要求标志落在 RTS 的最后一个周期，即写入后 29831）——而不是继续挪旋钮。

## 8. 仍需拍板的决策

**已拍板**：渲染器一次做到背景 + sprite（§2.1）；PAL 跑 16/5 加分数 dot 累加器（§4）；扩展音源
维持单元测试、不做金标准比对；访问的 dot（§4）等一次用 `Nes::ppu_bus_trace` 做的实验；
`6-MMC3_alt` 需要逐 ROM 覆盖表或按 CRC 的数据库而不是 submapper——它的头部是纯 iNES，§2.5 已记下。

1. ~~**访问的 dot**（§4）~~ —— 实验已做，结论就是保留 `ACCESS_DOTS = 2`：0 与 1 各自会让五个
   ROM 失败（§7）。
2. ~~**APU 帧计数器的相位**（§2.4）~~ —— 已完成：数值取自 blargg 源码里写下的实测 cycle
   （§7），`4-jitter`、`5-len_timing`、`6-irq_flag_timing` 转绿。
3. ~~**DMC**（§2.4）~~ —— 已拍板并落地：那"约 7 个周期"不是模型分歧，而是总线侧双重收费造成的
   假象（§7）；改成 Mesen 的形状后两项检查都过。

---

## 9. 任务台账（新会话从这里开始）

**怎么用**：每条给出「状态 → 证据 → 下一步」。开工前先按 §6 重跑一遍基线（数字会随环境浮动，但
套件构成应当一致），再挑一条往下做；没有证据的变化不要写进本文档。

**ROM 报不出结果时 harness 会失败，而不是跳过**：`accuracy_coin_matches_its_baseline` 现在把
"本机没有这个 ROM"与"ROM 根本没跑起来"区分开——启动期卡死就是后者（`$37` 一直是 0）。以后若有
改动让 ROM 在菜单之前卡住，这条会直接变红，而不是悄悄通过——DMC 的双重收费正是这样在这儿藏了
好几步（§7）。

**当前基线**（本轮最后一次实测，`cargo test -p nes-core --test rom_tests -- --nocapture`）：
`instr_test-v5` 16/16、`instr_timing` 2/2、`branch_timing_tests` 3/3、`blargg_ppu_tests` 5/5、
`oam_read` 1/1、`oam_stress` 1/1、`ppu_open_bus` **1/1**、`sprite_hit_tests` 11/11、
`sprite_overflow_tests` 5/5、`apu_test` **8/8**、`cpu_interrupts_v2` 2/5、`mmc3_test_2` **5/6**、
`ppu_vbl_nmi` 9/10、AccuracyCoin **34 项失败**（共跑 143 项、94 通过、0 跳过）。单元测试 592
全绿；帧耗时约 1.69 ms（release、nestest 600 帧、中位数）。工作分支 `codex/mesen-alignment`。

| # | 事项 | 状态 | 证据（提交 / 实测） | 下一步 |
| --- | --- | --- | --- | --- |
| S0 | 公开逐 dot 观测 API（`Nes::ppu_bus_trace` / `step_dot`，wasm+uniffi 导出） | 完成 | `c8957a7`；4 个单元测试 | 需要在差分/调试里用就继续用 |
| P1 | 逐 dot PPU 渲染器（背景 + sprite），A12 变成真实取指 | 完成 | `5dc9e32`、`30b7267`：`4-scanline_timing` 转绿；`RenderTiming`、`ppu.render_v`、整行渲染器删除 | sprite 的**求值**仍是逐行（§2.1），需要时再做 |
| P2 | DMA 移进 `MemoryRead`（`process_pending_dma`） | 完成 | `8ec693d`：`4-irq_and_dma` 轨迹逐字节未变 | 无（DMC 依赖它，已就位）——但 DMC 的双重取数也是从这里进来的，见 §7 最后一行 |
| P3a | APU 帧计数器相位（`$4017` 迟 3/4 周期、IRQ 标志连续三周期） | 完成 | `a1fbaa3`：`4-jitter`、`5-len_timing`、`6-irq_flag_timing` 转绿 | 无 |
| P3b | DMC | 完成 | `f9984d4`（两个语义 bug）、一次性取数锁存（§7）、以及 Mesen 的通道形状：计数在取数时移动、字节留在缓冲、静音标志、周期就是表里的值。**`7-dmc_basics`、`8-dmc_rates` 转绿**（apu_test 8/8）；AccuracyCoin 43 → 34 | ROM 侧没有欠账；相邻项是混音重采样（P7）与 DMC IRQ 对 CPU IRQ 线的影响 |
| P4 | §2.2 寄存器细节：open bus 衰减、渲染期 `$2007`、灰阶掩蔽、PAL 强调位，以及九位颜色解出的 RGB | 完成 | `af2f375`、`8247f10`，加上 512 项强调表（§7）：`ppu_open_bus` 转绿；8 个单元测试 | 无——相邻项是 sprite 的**求值**仍是逐行（P1） |
| P5 | PAL 16/5 + 分数 dot 累加器 + 按制式推导音频时钟 | 完成（比例部分） | `dbdf7cc`：PAL 一帧 33,247.5 个 CPU 周期 | PAL 专属 PPU/APU 规则（`_nmiScanline`、`_vblankEnd`、PAL 的 OAM 刷新）——需要 PAL ROM 才能验收 |
| P6 | Mapper：NES 2.0 头部、总线冲突 | 部分完成 | `4e1c7a6`：冲突覆盖 2/3/7/11/66/71；5 个头部 + 2 个冲突用例 | 按 submapper 分派板级行为（Mesen 靠按 CRC 的 game database，不是头部字段）；之后是 64/158/185/115 |
| P7 | 音频重采样 | 未开始 | — | 带限（blip）重采样；无参考录音，验收只到"不破坏现有测试" |
| P3c | `cpu_interrupts_v2` 剩下的三张表 | **进行中** | 两个"挪一个周期"的实验都已实测并撤回（标志晚一周期；IRQ 判决晚一周期）：后者把 `5-branch_delays_irq` 前三张表逐行重现，代价是 `1-cli_latency` 与 `4-scanline_timing` 变红。两个实验都在 §7。 | 先把 `5-branch_delays_irq` 里 `begin` 的链条按指令量出来，和它 readme 的表反推出的周期对（第 0 行要求标志落在 RTS 的最后一个周期＝写入后 29831），再去动旋钮 |
| — | 访问 dot 的实验 | 完成 | `f5943b3`：0/1 各让五个套件失败，2 全绿 → 保留 `ACCESS_DOTS = 2` | 无 |

**取 Mesen 源码的办法**（`/tmp` 里的副本可能不在了；下面这条本会话用过）：
仓库是 `SourMesen/Mesen2`，按需要抓单个文件即可，例如
`curl -sSf -O https://raw.githubusercontent.com/SourMesen/Mesen2/master/Core/NES/APU/DeltaModulationChannel.cpp`。
已读过并写进本文档的：`Core/NES/NesPpu.cpp`、`Core/NES/BaseNesPpu.{h,cpp}`、
`Core/NES/NesCpu.cpp`、`Core/NES/NesMemoryManager.cpp`、`Core/NES/APU/DeltaModulationChannel.{h,cpp}`、
`Core/NES/APU/ApuFrameCounter.h`、`Core/NES/Mappers/`（`MMC3.h`、`Mapper4.h`、`BaseMapper.*`、`VRC6.h`）。
仓库根的 `mesen_files.json` 是它全仓的清单，可用 `name` 字段查任意文件的路径。

**交接时的约定**：ROM 不入库（`roms/` 缺目录时测试打印跳过）；文档英中必须同时改（`*.zh-CN.md` 对照）；
每一步的实测数字写进提交信息与 §7；因"更忠实于 Mesen"而产生的回退要连同原因记进 §7 的撤回清单，
而不是把代码改回去。
