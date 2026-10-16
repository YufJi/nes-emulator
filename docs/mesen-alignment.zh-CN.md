# 与 Mesen 的对齐：现状、差距与推进方式

**简体中文** · [English](./mesen-alignment.md)

这是精度工作的**工作文档**：**把内核与 [Mesen2](https://github.com/SourMesen/Mesen2) 的 NES 内核
对齐，遇到歧义一律以 Mesen 为准。** `docs/architecture.md` 仍是"模拟器今天是什么样"的描述（其
§15.1 是已知限制的摘要）；本文档是计划、计划背后的实测数据，以及仍然不同的地方。
**新会话请先读 §9 的任务台账**——那里是"哪些做完了、哪些卡在哪、下一步做什么"的一页汇总——以及
**§10：与 MesenCE 的 NES 内核逐文件对比**（2026-10-16），它列出仍然不同的地方、**已经一致**的地方
（免得重做），以及每一项该由哪张 ROM 来证。

两条规则，按优先级：

1. **以 Mesen 的实现为准。** Mesen 与内核不一致之处即为缺陷，除非本文 §4 明确列为有意偏差。
   请直接读 Mesen 源码，不要猜：关键文件是 `Core/NES/NesCpu.cpp`/`.h`（执行、DMA、中断）、
   `Core/NES/NesPpu.cpp`/`.h`（取指流水线）、`Core/NES/BaseNesPpu.h`（PPU 状态）、
   `Core/NES/NesMemoryManager.cpp`、`Core/NES/Mappers/A12Watcher.h` 以及
   `Core/NES/Mappers/` 下的各 mapper。
2. **测试 ROM 基线是证据，不是约束。** 各套件的失败清单（在 `crates/nes-core/tests/rom_tests.rs`
  里，本轮对齐期间已移出树——见 §6）当初是双向断言的，因此任何让套件结果变化的改动**都曾**必须
  更新清单——但清单跟随实测，凡因"更忠实于 Mesen"而产生的回退，都记录并写明原因，而不是回退代码。

## 1. 已对齐的部分

| 方面 | Mesen 的做法 | 内核现在的做法 | 依据 |
| --- | --- | --- | --- |
| 时钟 | 单一主时钟；NTSC 用 `cpuDivider` 12 / `ppuDivider` 4 推导两颗芯片 | `Region::cpu_divider()`/`ppu_divider()`（12/4，即真实晶振），单一主时钟，`NesBus::run_ppu_to` 推进到目标 | `test_clock_dividers_agree_with_the_frame_geometry`、`test_the_master_clock_path_places_the_same_dots` |
| CPU 执行 | `MemoryRead`/`MemoryWrite` **本身就是周期**（围绕访问做 `StartCpuCycle`/`EndCpuCycle`） | `Bus::read_cycle`/`write_cycle` 即周期；`NesBus::begin_cpu_cycle`/`end_cpu_cycle` 围绕访问推进 PPU/APU | `instr_timing` 2/2、`branch_timing` 3/3、`test_every_opcode_executes` |
| 操作数取指与丢弃读 | `FetchOperand` 把 dummy 逐条写出（零页索引、绝对/间接索引、隐含与累加器的那次读） | `Cpu::fetch_operand` 加上 `AddressingMode::AbsoluteXWrite`/`AbsoluteYWrite`/`IndirectYWrite`、`RTS` 两次、`RTI`/`PLA`/`PLP` 各一次 | 同上 |
| 中断时序 | 每周期采样；判决读上一周期的采样（`_prevRunIrq`/`_prevNeedNmi`） | `Cpu::end_cycle` 保持同样的滞后；`execute_instruction` 在指令结束后判决 | `cpu_interrupts_v2` 5/5、`ppu_vbl_nmi` 10/10 |
| 芯片落在周期里的位置 | `StartCpuCycle` 先跑到访问点的 PPU dot，再跑 `ProcessCpuClock`（APU 与卡带）；`EndCpuCycle` 跑剩下的 dot，由 CPU 采样中断线 | `NesBus::begin_cpu_cycle` 跑完 dot 后、在访问之前给 APU 打点；`end_cpu_cycle` 跑剩下的 dot | `test_the_apu_clocks_before_the_cycles_access`，以及 `cpu_interrupts_v2` 5/5——它最后三张表读的就是这一个周期 |
| 向量选择 | 在压状态字节那一刻选定；此前到达的 NMI 接管（`IRQ()`、`BRK()`） | `Cpu::take_interrupt`/`Cpu::brk` 相同，含 BRK 的 `_prevNeedNmi = false` | `test_nmi_hijacks_an_irq_sequence`、`test_nmi_takes_brks_vector_over` |
| A12 | `SetBusAddress` 把**最后一次真实访问**的地址放上总线并保持；过滤是 `(GetMasterClock() - _a12LowClock) >= 3`，主时钟即 CPU 周期数 | `NesBus::bus_address`（保持），由 `PixelPipeline` **真实读过**的地址、在它读的那个 dot 上喂入；`Mapper::on_ppu_bus(addr, cpu_cycles)` 与 `mapper4` 以周期计长度且**每次上升都清零** | `2-details`（241 次）与 `4-scanline_timing` 均转绿 |
| 逐 dot 渲染 | `ProcessScanlineImpl`：`cycle % 8 == 1/3/5/7` 取指，每 dot `ShiftTileRegisters` 与 `DrawPixel`，粗 X 在每组最后一个 dot 推进（`(_cycle & 0x07) == 0`），全程用同一个 `_videoRamAddr` | `PixelPipeline`：同样的取指/装载/移位/绘制节奏，粗 X 在装载那个 dot 推进，滚动取 `v`/`t`/`x`；sprite 阶段的取指、逐 dot sprite 像素与 sprite 0 命中都在这里 | `4-scanline_timing`、`sprite_hit` 11/11，且 `nestest` 的帧与整行渲染器逐字节相同（那一份检查看不见属性取指，见 §7 末行） |
| open bus | 每位一个时间戳（`_openBusDecayStamp`）、三帧衰减、`$2002` 低五位从中取，且用 PPU 自己的锁存而非 CPU 的 | 相同：`Ppu::drive_open_bus`/`open_bus_decay_stamp`，`$2002` 只驱动高三位，`$2007` 的调色板读只驱动六位 | `ppu_open_bus` 1/1 |
| 预渲染行 | 流水线条件 `scanline < 240`，对预渲染行（-1）成立 | 流水线自己的 `visible` 判断，`scanline < 240 \|\| scanline == pre_render` | 同上 |
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
让首个 sprite 阶段取指之前的低电平段长了一个 dot），`RenderTiming` 与整行渲染器都已删除。
（`ppu.render_v` 其实**没有**删：字段还在，没人用它却仍进存档——见 §10.3。）

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
- **背景取指的开关**。Mesen 的 `LoadTileInfo` 挂在 `IsRenderingEnabled()`（即
  `BackgroundEnabled | SpritesEnabled`）上：关闭背景、只开 sprite 时它照样取背景瓦片、照样装载
  移位寄存器、照样推进 `v`——而本内核把这整套挂在 `SHOW_BG` 上。可见后果是 A12（关闭背景时 MMC3
  少一批时钟）与关闭背景期间的滚动推进；本地没有 ROM 覆盖这一条。

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
- ~~**APU 落在 CPU 周期的哪一端**~~ 已完成：芯片在 CPU 周期的**开头**给 APU 打点，早于该周期
  自身的访问（Mesen 的 `ProcessCpuClock`，由 `StartCpuCycle` 调用），而本内核原先在周期的**末
  尾**打点。就是这一个周期卡着 `cpu_interrupts_v2`：`$4017` 重启要被**下一个**周期的 tick 看
  到，于是帧 IRQ 标志落在写入**周期**之后第 29831 个周期（而不是 29830），中断线的每一次采样
  都因此晚一个周期。该套件剩下的三张表全部是从各自的中断落点上读出这个周期的，把 APU 挪到周期
  开头之后三张一起转绿（§7）。
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
- ~~重采样~~ 已完成：音频链路换成带限重采样，即 blargg 的 `blip_buf` 的形状，也是 Mesen 的 NES
  混音器跑的东西（`NesSoundMixer` 喂两个 `blip_t`）：混音电平的每一次变化算一个阶跃，阶跃按它在输出
  采样点里的分数位置加一段 32 相位、相位间线性插值、16 抽头的脉冲响应；输出是这些贡献的累加，再由
  一个一阶高通保持交流耦合（blip 的 `bass_shift = 9`）。有一处有意的偏差：Mesen 先让 blip 跑在
  96 kHz、再用 Hermite 重采样到设备率，本内核直接重采样到设备率——少一级，而且带宽限制正好落在
  宿主请求的那个速率上，而不是一个中间速率。
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
   这一项的第三块是 APU 落在周期里的**哪一端**（§2.4）：挪到开头而不是末尾，才是
   `cpu_interrupts_v2` 最后三项转绿的那个周期。
4. ~~**§2.2 的寄存器细节**~~ —— 已完成：open bus 衰减（`ppu_open_bus` 转绿）、渲染期 `$2007` 的行为、
   调色板读的灰阶掩蔽、PAL 的强调位互换。OAM 衰减查明是 Mesen 默认关闭的设置，因此"不做"正是它的
   默认。那九位颜色解出的 RGB 也已对齐——Mesen 的 512 项表、84% 截断与黑列例外（§2.2/§7）。
5. **PAL**。比例已完成——PAL 跑 16/5，启用分数 dot 累加器，一帧是 33247.5 个 CPU 周期，音频时钟
   也改成按真实的 3.2 推导而不是照 NTSC 形状的常量。剩下的是 PAL 专属的 PPU/APU 规则
   （`_nmiScanline`、`_vblankEnd`、PAL 的 OAM 刷新）；没有 PAL 测试 ROM 就无法验收。
6. **Mapper**（§2.5）：NES 2.0 头部与总线冲突已完成。剩下的是按 submapper 分派（需要 Mesen 放在
   数据库里的逐 ROM 数据，而不是头部字段），以及 64/158/185/115 四个板。
7. ~~**音频重采样**~~ —— 已完成：上面的带限重采样器（§2.4/§7）。

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
- **DMC 使能时立即取数**。Mesen 会先等两三周期（`_transferStartDelay`，再由 `ProcessClock` 调
  `StartDmcTransfer`）；这里在 `$4015` 写入自己那个周期就发出请求——这正是 blargg 在
  `7-dmc_basics` 里说的"一字节缓冲应立即填充"，也让 AccuracyCoin 的 `Frame Counter 4-step`/
  `5-step` 保持绿：带上延迟那两项会失败，因为 DMC 的 active 位会多滞留到使能之后，被它们读成
  长度计数器提前停止（两个方向都量过，§7：不带延迟 18 项失败，带延迟 20 项）。**禁用**仍走
  Mesen 的延迟，`StopDmcTransfer` 的窗口就是它给的。
- **上电内容**。这里的 RAM 与 PPU 调色板 RAM 都是全零上电，而 Mesen 的默认是
  `RamPowerOnState = Random`（非随机那条路还会把调色板填成固定的 32 字节上电图案，好让 blargg
  那张老调色板 ROM 通过）。随机上电内容只是 Mesen 提供的三种设置之一
  （`AllZeros`/`AllOnes`/`Random`），所以全零也是一种 Mesen 配置，而且是唯一能保住本内核
  确定性不变量（`frame()`/`run_cycles()`/`step_instruction()` 在同一帧边界逐字节相同）的那个。
  因此"先读后写" RAM 的 ROM 与 Mesen 的默认跑法可能不同。

## 5. 观测手段

已有，且上述每次实测都用到。**本轮对齐期间 ROM 套件不在树里**（§6）；ROM 仍在 `roms/` 下，
下面这些诊断就是"逐个 ROM 那几行"的替代：

- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture`，配 `NES_STATUS_ROM`
  （跟踪 `$6000`/`$6004` 与 PC）或 `NES_DUMP_ROM`（把某一帧导成 PPM，看"画面即判决"的 ROM）——
  这就是"改动想推动的那张 ROM"的点测方式。
- `cargo test -p nes-core --test rom_tests -- --nocapture` —— 逐个 ROM 的 pass/known 行，套件回去
  之后才有（§10.1 与 `architecture.md` §13.1 仍然描述着它们）。
- `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture` —— 帧耗时
  （nestest 连续 600 帧；多跑几次取中位数）。
- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture` —— 逐指令 trace
  （`NES_TRACE_ROM`）与逐行 PPU 总线地址（`NES_A12_ROM`、`NES_A12_SKIP`、`NES_A12_LINE`）。
- 测试 ROM 自带源码与 readme（`roms/blargg/*/source/`、`readme.txt`）：blargg 的套件写明精度是
  "一 PPU clock"，并打印期望轨迹。请与它们比对，而不是与我们的形状比对。
- 把帧缓冲的 raw 存成 PNG 之前看清字节序：`PixelFormat::Argb8888` 的 `0xAARRGGBB` 在小端机上
  按 **B, G, R, A** 落盘，脚本若按 `A, R, G, B` 读就会红蓝对调——颜色本来是对的，看起来却像调色板
  坏了（本节这一条就是为此记的）。
- `Nes::ppu_bus_trace()` / `Nes::clear_ppu_bus_trace()` / `Nes::set_ppu_bus_trace_capacity()`：
  PPU 的总线时间线，每 dot 一条采样，由机器在 dot 发生时填充（因此即使 CPU 以整条指令推进，
  顺序仍是机器顺序）。这与卡带看到的是同一个视图，`test_a12_clockings_per_frame` 就是通过它
  按帧统计合格的 A12 上升。`Nes::step_dot()` 是与之配套的推进器——调试器用的 dot 计数器，
  语义是把 `step_instruction()` 挪到另一颗芯片上。

缺失、需要时应当补上：

- PAL 测试 ROM、MMC5 套件、总线冲突 ROM，以及扩展音源的金标准音频比对；
- 以 Mesen 本身作为 oracle 的差分（它的调试器能输出逐 cycle 轨迹），可把"整片 ROM 通过/失败"
  变成逐指令差异。

### 5.1 Mesen 自己的测试套件**不是**这一套（2026-10-16 核实）

动手追基线之前先看这一段——两套回答的是不同问题，认错了就会把修复做成死循环：

- **维护中的 Mesen 在哪。** `SourMesen/Mesen2` 的 README 指向 `nesdev-org/MesenCE`：带 CI、
  带发布、带测试的是那个分叉。源码布局相同，所以本文档用的那些路径照样能取，但**要读的是
  MesenCE**。
- **MesenCE 的精度 CI 是 `.github/workflows/tests.yml`**：它把 `nesdev-org/MesenTests` checkout
  到 `Tests/`，再跑 `PGOHelper <Tests 目录> citests`——把找到的每个 `.mtp` 都跑一遍（多线程，
  失败时上传截图）。`SourMesen/Mesen2` 自己的 CI **只构建**，不跑任何 ROM。
- **`.mtp` 是"录制型 ROM 测试"**（`Core/Shared/RecordedRomTest`）：回放输入记录，并逐帧比对
  **截图哈希**（`_screenshotHashes`、`_badFrameCount`）。套件按机种分目录（`NES/`、`GB/`、
  `SNES/`、`GBA/` …），而且**不含 ROM**：运行时按**文件名 + CRC32** 到使用者自己的游戏目录里找
  （`Core/Shared/RomFinder.h`）。
- **我们用的是"自报结果的精度 ROM"**：blargg 的套件把结论打印出来并写进 `$6000`，
  AccuracyCoin 为每个测试写一个结果字节到 `$0400`；而我们的 CI 是 `cargo test --workspace`，
  这些套件在没有 `roms/` 时**跳过**——所以这份清单是本地工具，不是 CI 在守的门。

由此得出：**AccuracyCoin 的计数是我们的基线，不是 Mesen 的**——Mesen 的 CI 从不跑它。所以每个
失败项的 oracle 必须是 **Mesen 的源码**（规则 1），ROM 的裁决只是证据、不是目标。AccuracyCoin 的
README 自己也写着这些测试是给 RP2A03G + RP2C02G 写的、有些与芯片版本有关（它甚至删过一个测试，
因为 G 版 PPU 会挂），因此那 34 项里可能有几项是"ROM 想要的行为 Mesen 有意不建模"——那种应当按
§4 记成**有意偏差**并附上依据，而不是为了让数字好看去弯模拟器。它的 README 还逐条解释了错误码，
这是判断"某一项到底在测什么"最快的办法。若哪天真的想要更强的 oracle，可以拿 MesenCE 跑
`.mtp` 式的录制、比对金标准帧；但那需要构建 Mesen 并备好 ROM，不是免费的。

### 5.2 基线现在也看像素了——顺手查出 26 个"空通过"

套用 Mesen 的**判定机制**顺手暴露了我们这边的一个洞。到 2026-10-16 为止的现状——**套件与金标准
当天就一起移出了树**（§6/§9），所以这一节记的是那套机制与它查出的东西，而不是"现在在跑什么"；
harness 本体是 git 历史里的 `tests/common/frames.rs`（`b6c06a1`）：

- **每个套件的逐帧哈希**（`tests/common/frames.rs`）：对原始帧缓冲做
  `SHA1::GetHash(frame.FrameBuffer, frame.FrameBufferSize)`（与 Mesen 的 `.mtp` 同一个函数、同一
  份字节），RLE 存到 `tests/golden/<套件>/<rom>.hashes`。为省下昂贵的哈希，先用一个廉价指纹判断
  "画面是否变了"，只在变化时算 SHA1——"变了一帧又变回去"会漏掉，这是让套件跑得完的代价。要重新
  录制就用 `NES_GOLDEN_RECORD=1`，并**读一遍 diff**。
- **它查出的洞：`$6000` 协议**。ROM 若从不写这个字节，它就一直是 0，而"状态不是 `$80`"被当成
  通过——于是**有 26 个 ROM 在测试从未运行的情况下被计为通过**：
  `sprite_hit_tests_2005.10.05`（11）、`sprite_overflow_tests`（5）、
  `blargg_ppu_tests_2005.09.15b`（5）、`branch_timing_tests`（3）、`oam_stress`（1），以及
  `instr_timing` 两个中的一个（逐个 ROM 观察 `$6000` 实测得到）。这些是 blargg **较早**的 ROM，
  结论只打印在屏幕上。
- **修法**：不自报结果的 ROM 由**它的帧**来判——金标准本身就是全部裁决（Mesen 对每个 ROM 都是这么
  判的）；会自报的 ROM 必须**真的被看到在运行**过，它的结果才算数。这些套件的帧预算按 ROM 的实际
  需要收紧（画面约三十帧内稳定；`oam_stress` 是文档注明的例外）。
- **它有效**：把 B1 那个 bug（sprite 图案 high 平面读成 low 平面的地址）改回去，
  `blargg_sprite_hit` 立刻以 "frames moved from the goldens" 失败，其余一切不变——正是当初让本目录
  所有套件绿了一整天的那一类 bug。
- **它不是**"正确性裁决"：金标准只说"这个 ROM 的画面没有移动"，不说它是对的；而且这些是**我们的**
  哈希，不是 Mesen 的。要换成 Mesen 的值，需要写 `.mtp` 读取器（zip 容器里是 `TestMovie.mmo` 加
  哈希表），哈希来自 MesenCE；格式见 §5.1。

## 6. 验收协议

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- **对齐期间 ROM 套件不在树里**（2026-10-16 移除，见 §10.6）：跑满一轮要几分钟，而这轮工作需要
  以秒计的循环。ROM 本体留在 `roms/` 下，套件在收尾时从 git 取回；在此之前，验收就是
  `cargo test --workspace`（604 个单元测试）加上 `tests/cycle_trace.rs` 里那些 `#[ignore]` 的
  诊断工具。改动想推动的那张 ROM，应当用 `NES_STATUS_ROM` 与 `NES_DUMP_ROM` 做点测。
- 套件放回去之后，它们原先的规则照旧：ROM 不入库（布局与下载来源见 `docs/architecture.md` §13.1）；
  清单里"开始通过"的项必须从里面删掉；清单之外出现失败会直接红；凡因更忠实于 Mesen 而产生的回退，
  **连同原因**加入清单而不是把代码改回去。
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
| 带限重采样（§2.4） | 音频链路换成 blip 的形状：电平变化算阶跃，32 相位 × 16 抽头并在相位间线性插值，输出用积分器累加、再用一阶高通保持交流耦合（Mesen 的 `NesSoundMixer` + `blip_buf`）；直接重采样到宿主速率，没有 Mesen 那个 96 kHz 中间级 | ROM 基线零移动（没有 ROM 听声音）；单元测试 592 → 597，含这一项的要害——30 kHz 输入折成的 18 kHz 在采样保持下是满强度，带限之后至少低 20 dB；帧耗时 1.69 → 1.82 ms；存档版本 11 → 12 |
| APU 挪到 CPU 周期的开头 | 芯片在该周期自身的访问**之前**给 APU 打点（Mesen 的 `ProcessCpuClock`，由 `StartCpuCycle` 调用），而不是在它之后。先按 ROM 量：tick 落在访问之后时，`5-branch_delays_irq` 第 0 行中断的是 `begin` 末尾那条 `RTS`，比标志置位晚两个周期，而 readme 要的是它后面那条 `NOP`、即晚四个周期——那张表里对不上的四行差的是同一个周期 | **`cpu_interrupts_v2` 2/5 → 5/5**：`3-nmi_and_irq`、`4-irq_and_dma`、`5-branch_delays_irq` 全部通过。其余零移动——`apu_test` 8/8、`ppu_vbl_nmi` 9/10、`mmc3_test_2` 5/6、AccuracyCoin 的 34 项失败都不变，而把标志钉在 `$4017` 写入上的三个套件保持全绿，因为标志是**跟着 APU 一起**挪的。单元测试 597 → 598（新增一条钉住 APU 在周期里的位置）；帧耗时 1.82 → 1.86 ms（五次中位，抖动 ±0.06） |
| sprite 图案的第二个平面 | sprite 阶段读 high 平面时也用了 low 平面的地址（`addr + row`），等于把 low 平面读了两遍。两个平面是 (0, 1) 的像素本该是颜色 2，读成 (0, 0) 就成了**透明**——于是 sprite 四种颜色里有两色是窟窿，游戏人物看起来一块块透明。high 平面是 tile 的后八个字节（`addr + row + 8`），背景取指一直就是这么做的 | **没有一个套件看见它**：本地没有 ROM 会去看自己画出来的像素，所以带着这个 bug，`sprite_hit` 11/11、`sprite_overflow` 5/5、所有 blargg 套件与 AccuracyCoin 的 34 项与修好之后一字不差。回归测试渲染一个两平面不同的 sprite，在旧代码上断言失败（那个像素是 `0xFF000000`，即背景色——sprite 在那里是隐形的）。帧耗时 1.83 ms（五次中位；与改动前 1.86 的差在抖动范围内） |
| SHx 的地址损坏 | `SHA`/`SHS`/`SHX`/`SHY` 在索引把进位带出低字节时，**保留**进位（只有取指那次丢弃读落在未进位的地址上），再把这个高位与"要写的寄存器对"按位与——于是写可能落到算式根本提不到的页；写的值 = 寄存器对 &（**base** 的高位 + 1），而若 DMA 打断了写之前那个周期，这个掩蔽会被取消（Mesen 的 `SyaSxaAxa` 与它的 `hadDma`）。旧代码把写留在 base 那一页、完全不碰地址，那既不是四种行为里的任何一种，也正是 AccuracyCoin 的行为探测在找的东西 | AccuracyCoin 的五项 `UnOp_`（`SHA $93`/`$9F`、`SHS $9B`、`SHY $9C`、`SHX $9E`）同时从第 3 项推进到第 15 项，而失败总数仍是 34——因为那最后一项落在这些测试的 DMA 段里（源码自己的注释写着"错误码 7、8、9、A、B、C"，从那往后必须 DMA 正确）。所有 blargg 套件零移动；单元测试 602 → 603 |
| 背景属性的粗 X | 粗 X 原先紧跟 nametable 取指自增，而 Mesen 在**每组的最后一个 dot**（`(_cycle & 0x07) == 0`，本内核的装载 dot）才推进它：属性取指在 nametable 之后两个 dot，读的却是 `v` 的粗 X 位 1（象限）与粗 X 位 2-4（属性字节地址）。于是每个背景瓦片都拿到**右邻**的调色板，跨四瓦片边界时还多读一个属性字节；图案取指不受影响（行号取自 fine Y）。症状只落在调色板边界：SMB 1-1 的 `?` 砖右半、砖行末尾与标题画面 "1 PLAYER GAME" 末尾的 "E" 都变成天空那一档的绿 | **没有一个 ROM 看见它**——本地 ROM 不看自己画出来的像素，blargg 全套与 AccuracyCoin 的 143 跑 94 过 34 失败在改前改后一字不差。与整行渲染器逐像素比：`894cc07`（逐 dot 之前）与修好后**完全一致**，而 `5dc9e32`（逐 dot 背景）已经差 304 像素——这个缺陷是逐 dot 背景流水线带进来的，`nestest` 的逐字节比对看不见它。三条回归测试（自增的 dot、象限、属性字节地址）在旧代码上全部失败。SMB 的标题画面与 1-1 都恢复原色（同一帧 248 / 283 个像素变化，全部落在调色板边界上）。帧耗时：同机各三次中位 3.68 ms，未修 3.84 ms（抖动 ±0.3 ms；本机当前绝对值约为上表的 2 倍） |

| §10.3 的前五项，一次做完 | NES 2.0 的 byte 8 按 Mesen 读（mapper 取 `(Byte8 & 0x0F) << 8`、submapper 取 `(Byte8 & 0xF0) >> 4`，并补上 mapper 只写在 byte 6 的古老 iNES）；`$2002` 读**只**清 VBlank（`UpdateStatusFlag`），两个 sprite 标志留给预渲染行；混音改用 Mesen 的常数与声道权重；上电复位与软复位分成两件事（上电 `SP = $FD`、`A/X/Y = 0`，按键 `SP -= 3` 且置 `I`），之后都跑八个空周期；奇数帧跳点读的是 dot 338 **末尾**的渲染状态（Mesen 的 `_renderingEnabled` 比 `$2001` 慢一个 dot） | **`ppu_vbl_nmi` 9/10 → 10/10**（`10-even_odd_timing` 通过，输出正是它自己注释里写的 `08 08 09 07`），**AccuracyCoin 34 → 32**：`Suddenly Resize Sprite` 与 `Misaligned OAM DMA` 不再失败。`cpu_reset/registers` 越过第 2 项，打印出 `A X Y P S = 00 00 00 34 FD`——正是 ROM 对上电的期望值，此前是 `S=$F7` 而失败。把所有会自报结果的 ROM 复测一遍：`instr_test-v5` 16/16、`instr_timing` 2/2、`cpu_interrupts_v2` 5/5、`apu_test` 8/8、`mmc3_test_2` 5/6（已知那一项）、`oam_read`、`ppu_open_bus`、`cpu_dummy_writes` 2/2、`cpu_exec_space/ppuio`，以及被 `ACCESS_DOTS = 2` 钉住的五张（`05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、`2-nmi_and_brk`、`4-scanline_timing`）全部仍然绿。单元测试 604 → 605 |

| 手柄口（§10.3 第 4/5 条剩下的部分） | 照 `NesController`/`NesControlManager` 的形状：移位寄存器在 strobe 的下降沿装载，strobe 保持高时每次读都重新装载；读出的位是**补 1**的（原厂手柄在八个按键之后一律回 1）；手柄口读的高三位是 CPU 的 open bus，而不是硬写成 `$40`；同一个 CPU 周期内对同一端口的第二次读不再走位，而是重复第一次的位；`$4016` 的写要到下一个 PUT 周期开头才落到引脚上，而不是写入自己那个周期 | **AccuracyCoin 32 → 31**：`Controller Strobing` 不再失败，`Controller Clocking` 从第 2 项（"读满八次补 1"）推进到第 7 项——要用 DMA 的 put/halt 周期去走手柄口，属于第 7 条。`APU Register Activation` 的第 2 项查下来是 `DMA + $2007 Read` 的前置条件（ROM 自己的注释就是这么写的），所以它等的也不是手柄这条线。单元测试 605 全绿；`read_joy3` 的几张 ROM 仍需要脚本化输入（它们在等按键被按下） |

| DMA 逐字移植（§10.3 第 7 条），Mesen 的 `ProcessPendingDma` | 传输从总线里搬出来，改成由 CPU **逐周期泵**的状态机：`Bus::start_pending_dma`/`dma_running`/`run_dma_cycle`，由 `Cpu::run_pending_dma` 调用——于是被打断的每个周期都以 CPU 自己的收尾工作结束，中断终于能在 DMA **内部**到达。`NesBus` 保存 Mesen 的状态原样：`need_halt`/`need_dummy_read`/`dmc_dma_running`/`abort_dmc_dma`/`sprite_dma_transfer`，halt 周期读的是 CPU 本来要用的那个地址，读落在偶数周期、写落在奇数周期，中止路径（`StopDmcTransfer`、`_abortDmcDma`）齐备，`ProcessDmaRead` 的内部寄存器合并也在（CPU 在 `$4000-$401F` 内被停机时，DMA 会读 CPU 自己的寄存器：可能背着程序清掉帧 IRQ 标志、吃掉输入位、或把 DMC 要的字节换成总线冲突的结果）。DMA 永不落在写周期上，两次传输可以首尾相接。DMC 同步跟进：`$4015` 的 bit 4 就是 `_bytesRemaining > 0`，禁用先倒数两三周期再请求取消传输，取数请求是一次性的 | **AccuracyCoin 31 → 18 项失败，而且 146 项全部跑到结果**（§10.3 首批修完时是 34）。一次转绿 13 项——凡是带 DMA 的，加上五项 `UnOp_` 存储与 `Instruction Timing`：`DMA + $2007 Read`/`Write`、`DMA + $4015 Read`、`DMA + $4016 Read`、`DMA + $2002 Read`、`DMA + Open Bus`、`DMALandingOnWrite`、`DMC DMA Bus Conflicts`、`DMC DMA + OAM DMA`、`Misaligned OAM DMA`、`Controller Clocking`、`UnOp_SHA`/`SHS`/`SHY`/`SHX`、`Instruction Timing`——而 `Implied Dummy Reads` 与 `Explicit`/`Implicit DMA Abort` 则推进到了更靠后的检查项。`dmc_tests` 的四张 ROM（此前卡在空白画面）全部上报 `$6000 = 0`。新增单元测试把 sprite 传输钉在按奇偶 513/514 周期、DMC 取数钉在 3 或 4 周期——正是 ROM 数的那些数。单元测试 605 → 609；存档版本 12 → 13。**回退，也正是这一条还没收尾的原因**：`7-dmc_basics` 退回第 19 项（"一字节缓冲应立即填充"）、`8-dmc_rates` 退回第 2 项（"rate N 的周期太短"），这是 P3b 之后这两项第一次失败；同时 `Controller Strobing` 也从绿退回第 4 项（"从 put 周期转到 get 周期时不应触发手柄 strobe"）——它是从一次 sprite 传输留下的 CPU 相位读出来的，所以和上面两项是同一个问题。两个方向都量过：把 Mesen 的 `_transferStartDelay` 从使能路径里去掉**并不**改变它们（不过 AccuracyCoin 从 20 降到 18 是去掉换来的），所以原因在移植的别处，下一步要拿 ROM 自己打印的期望值逐周期对。`4-irq_and_dma` 需要把 APU 的"周期之间"相位翻转（`odd_apu_cycle` 由 `cpu_cycles & 1 == 1` 改成 `== 0`）：旧映射下该 ROM 失败在第 1 项，新映射下通过，AccuracyCoin 还多两项，而 `4-jitter`、`5-len_timing`、`6-irq_flag_timing` 两种映射都绿。代价钉在 `test_the_apu_clocks_before_the_cycles_access` 里：这种合成相位下 `$4017` 写入让标志落在第 29832 个周期，而 blargg 的注释写的是 29831——记录在案而不是藏起来，因为钉住这个绝对数字的 ROM 两种映射都过 |

| DMA 移植带出的三处回退（同日） | 两处修正，都是回去读 Mesen 源码而不是看 ROM 得来的。**取数请求有 `_bufferEmpty` 守卫**（`StartDmcTransfer` 自己的条件）：缓冲里已经有一个字节时不再去取，于是重新使能会等上一个样本最后一个字节的边界。**周期奇偶变成一个有名有姓的约定**：我们的计数器是"已完成周期数"，Mesen 的 `CycleCount` 在周期**开始**时自增，所以凡是在周期**内**读到的奇偶都要按当前周期算（`NesBus::in_an_odd_cpu_cycle`）——`$4017` 重启延迟、`$4016` 的 `_writePending`、DMC 的使能/禁用延迟读的都是它；DMA 循环的奇偶是在周期**之间**读的，仍用普通计数器 | **三处回退全部消失**：`7-dmc_basics`、`8-dmc_rates` 回到 `$6000 = 0`，`Controller Strobing` 转绿，AccuracyCoin 18 → **17**。所有会自报结果的 ROM 复测通过，只剩两项已知的（`6-MMC3_alt`、`cpu_exec_space/apu`）。此前卡在空白画面的 `dmc_dma_during_read4` 五张现在都能跑并打印：`dma_2007_write` 与 `read_write_2007` 报 "Passed"，`double_2007_read` 打印出它四个可接受校验和之一，`dma_2007_read`（0FED8C78 对 159A7A8F/5E3DF9C4）与 `dma_4016_read`（08 08 09 07 08 对 08 08 07 08 08）各差一个周期的相位——和 `sprdma_and_dmc_dma` 同一个形状，后者现在能收尾并报失败码 1，而不再是永远空转 |

| PPU 寄存器状态机（§10.3 第 6 条）——**试过、量过、撤回** | 先读 `NesPpu.cpp`：`$2007` 读交还的是缓冲，缓冲要五 dot 后才重新装入（`_ppuMemoryDataReadStateMachine`），`v` 再晚一 dot 动（`_needVideoRamIncrement`）；`$2007` 写会先锁存、五 dot 后落地，而且**渲染期间写入的值被丢弃、存进去的是地址低字节**；`$2006` 的第二次写要三 dot 后才移动 `v`（`_updateVramAddrDelay`）；渲染关闭且 `v` 落在 `$3F00-$3FFF` 时，画出来的是 `palette[v & 0x1F]` 而不是 backdrop。四件都实现了，延迟由逐 dot 的循环驱动（Mesen 的 `UpdateState`） | **撤回，各piece 分别量过。** 只做"读缓冲延迟"就会让 AccuracyCoin **17 → 20**（`DMA + $2007 Write` 与 `$0476`/`$047E` 两项回来），并让 `cpu_dummy_writes_ppumem` **0x00 → 0x07**；而它真正针对的两张 ROM 根本不通过 `$6000` 上报（`ppu_read_buffer`、`full_palette`：没有 `$80`，判决在屏幕上）——也就是说这项改动拿不出能支撑它的实测。写的"渲染期写地址低字节"正是 `cpu_dummy_writes_ppumem` 第 7 项反对的（它要 RMW 的两次 `$2007` 写按序到达），而 `$2006` 的延迟正是 `mmc3_test_2/3-A12_clocking` 第 6 项（"通过 PPUDATA 写让 A12 变 1 时应被计数"）在量的东西——延迟的 `$2006` 改变了地址到达卡带总线的**时机**。这一版留在 `/tmp/ppu-statemachine-attempt.rs` 与本会话记录里；工作树已回到它之前的状态 | 下次按"ROM 能量到的顺序"做：先 `$2007` 读缓冲，用**帧导出**对照 ROM 自己打印的数字清单；再是写的地址低字节；再是 `$2006`，每一步都复测 `3-A12_clocking` 与 `cpu_dummy_writes_ppumem`。最需要小心的是总线地址的管线：Mesen 是在**延迟后的访问**真正发生时把被访问地址放上总线，而我们现在是在 CPU 那个周期就放上去了 |

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
  （这也是三张表最终要的答案：它们要挪的是 **APU**，不是判决。）
- **`cpu_interrupts_v2`：把 `$4017` 写入的奇偶对调**。实测：`5-branch_delays_irq` 逐字节不变
  （所以它的缺陷不是写入的 hold），`3-nmi_and_irq` 的输出有变化，AccuracyCoin 从 34 掉到 36。
  已撤回。

  **最后是怎么结的**。三个失败项就是同一个 ±1 周期的问题，而答案既不在标志、也不在判决、更不在
  写入的奇偶，而在 **APU** 落在 CPU 周期的哪一端：中断线的每一次采样都早了一个周期，因为 APU 的
  tick 排在周期自身的访问之后，而不是之前。先量链条正是找到它的原因——逐周期 trace 显示，
  `5-branch_delays_irq` 里 `begin` 的那个标志落在 `RTS` 的**第 5 个**周期（写入后 29830），而
  readme 第 0 行要的是第 6 个（写入后 29831，也就是 RTS 的最后一个周期）。把 APU 挪到周期开头
  （Mesen 的 `ProcessCpuClock`）是一处改动，没有任何旋钮：三张表一起转绿，而把标志钉在
  `$4017` 写入上的那三个套件（`4-jitter`、`5-len_timing`、`6-irq_flag_timing`）保持全绿，因为
  标志是**跟着** APU 一起挪的。下一次再追 ±1 周期的教训：当三组测试互不相容时，去找那颗不在
  Mesen 位置的芯片，而不是找一个可以加的数。
- **DMA 的 dummy 读，第一次尝试（AccuracyCoin 那 13 项 DMA）**。先读 ROM 源码这一步很值：
  `DMA + $2007 Read` 的第 3 项是 `CMP #$03 / BMI fail`，量的是"DMA 落在 `LDA $2007` 最后一个
  周期里之后，这一读返回的字节"——只有在 **DMA 的 dummy 周期是对 CPU 即将访问那个地址的真实读**
  时才对得上（Mesen 的 `ProcessPendingDma` 在 halt 周期、DMC 那个额外 dummy 周期、以及 OAM 传输
  的对齐周期里都读 `readAddress`；我们的这些周期是空转，于是窗口里的 `$2007` 从不推进 `v`，
  返回值老是 0/1/2）。
  照这个写的第一版**实测后撤回**：Mesen 的循环给 DMC 与 sprite 传输定了固定优先级，并且每个分支
  恰好消耗 `_needHalt` / `_needDummyRead` 中的一个；我那一版近似了这个握手，结果是
  `4-irq_and_dma` 的表动了（`+526` 那一行变成指令 9，readme 要的是 8），并且 **AccuracyCoin
  卡在启动**——0 项测试，被 harness 那条"失败而不是跳过"当场抓住（单元测试仍全绿）。
  下一次应当**逐字**移植 `ProcessPendingDma`：`(_state.CycleCount & 0x01) == 0` 的奇偶、
  两个标志的握手、`_abortDmcDma`、以及 `$4016`/`$4017` 的 `skipDummyReads`/`skipFirstInputClock`，
  同时保持 OAM 传输仍是 513/514 个周期。

  第二版把上面这些（奇偶、两个标志的握手、`skipDummyReads`、OAM 字节经 `$2004` 落地）都做了，
  实测同样撤回：`4-irq_and_dma` 同一行仍然动，AccuracyCoin 这次是启动期走进了它自己的陷阱——
  `$37` 保持 0（0 项测试），ROM 停在 `$80DD`（`JMP $80DD`），它是在 `JSR $F109`
  （`LDA $F0 / ORA #$80 / STA $F0 / STA $2000`）与 `INC $EC` 之后落到那里的，也就是菜单还没出现
  就已经走错。这把范围缩小到了：dummy 读进来之后，DMC 的一次取数变成"看奇偶的 3 或 4 个周期"，
  而我们的模型一直是固定 4；AccuracyCoin 的启动**是拿 DMC 的字节做同步的**（上面那条同类症状的
  卡死记录也是这个），所以字节送达的那个周期必须跟着一起挪。`dmc_sync_rom`（那份同步的简化模型）
  依然通过，因此下一次应当把**真的**那段同步循环写成测试，用它对通道的字节边界送达，而不只是对
  DMA 的长度。同一版里还有两件小事：我们的机器在 DMA 周期里不采样中断线（Mesen 的
  `StartCpuCycle`/`EndCpuCycle` 会采），以及 `$4016`/`$4017` 还需要 `skipFirstInputClock` 与
  `ProcessDmaRead` 的内部寄存器合并。

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

**怎么用**：每条给出「状态 → 证据 → 下一步」。开工前先按 §6 跑一遍验收，再挑一条往下做；没有
证据的变化不要写进本文档。

**套件目前（2026-10-16）被有意移出树了。** 跑满一轮要几分钟，而 §10 的对齐工作需要以秒计的循环：
`cargo test --workspace` 现在约 11 s（604 个单元测试全绿）。ROM 留在 `roms/` 下，套件都能从 git
取回——连同它们原来的规则：ROM 报不出结果时**失败**而不是跳过（ROM 根本没跑起来正是启动期卡死的
样子，DMC 的双重收费就是这样在这儿藏了好几步，§7）。收尾时把它们放回去，连同 §10.6 列出的那些。

**上一次实测的基线**（2026-10-16，移除之前）：
`instr_test-v5` 16/16、`instr_timing` 2/2、`branch_timing_tests` 3/3、`blargg_ppu_tests` 5/5、
`oam_read` 1/1、`oam_stress` 1/1、`ppu_open_bus` **1/1**、`sprite_hit_tests` 11/11、
`sprite_overflow_tests` 5/5、`apu_test` **8/8**、`cpu_interrupts_v2` **5/5**、`mmc3_test_2` **5/6**、
`ppu_vbl_nmi` **10/10**、AccuracyCoin **17 项失败**（§10.3 首批修完 34 → 32，手柄口降到 31，
DMA 那一刀带使能延迟是 20、去掉是 18；逐项清单见 §10.5）。单元测试 609 全绿；帧耗时约 1.86 ms（release、nestest 600 帧、五次中位，
抖动 ±0.06）。**帧耗时只做同轮对比**：同一台机器上这项测过 1.82 ms（1.815/1.825/1.826，空闲）
也测过 3.68 ms（10-15 那轮，机器上有别的负载），差别来自负载而不是代码——`0520b59` 里那个
5–8% 是 Android 前端自己关掉 PPU 总线轨迹换来的（`set_ppu_bus_trace_capacity(0)`），内核默认
没动。工作分支 `main`（10-16 起对齐工作与前端工作合流；`codex/mesen-alignment` 那支停在
`226bd01`）。

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
| P7 | 音频重采样 | 完成 | `e33ebeb`，见 §7 的行：带限重采样器；四个单元测试盯滤波器本身（阶跃幅度、交流耦合、带内幅度、带外折返）加一个整机静音测试 | 本地 ROM 侧没有欠账；与 Mesen 两级链路剩下的差异只能靠参考录音判断 |
| P3c | `cpu_interrupts_v2` 剩下的三张表 | **完成** | 链条按周期量了一遍（§7），缺的那一个周期是 APU 在周期里的位置：芯片在 CPU 周期的**开头**给 APU 打点（Mesen 的 `ProcessCpuClock`），而不是末尾。**`cpu_interrupts_v2` 2/5 → 5/5**——`3-nmi_and_irq`、`4-irq_and_dma`、`5-branch_delays_irq` 全部通过，而把标志钉在 `$4017` 写入上的那些一条没动。见 §2.4；两个撤回的旋钮实验在 §7 里都得到了解释 | 无；相邻项是 sprite 的**求值**仍是逐行（P1） |
| B1 | 游戏里 sprite 一块块透明（"人物像透明的"） | **完成** | sprite 取指的 high 平面用了 low 平面的地址（§7），颜色 2 的像素读成颜色 0 而消失。改成 `addr + row + 8`；单元测试 `test_a_sprites_high_plane_is_the_tiles_second_half` 在旧代码上失败 | 无。值得记住：ROM 基线看不见"画出来的像素"，这类问题靠游戏或单元测试来抓 |
| B3 | 未文档 `SHx` 的地址损坏（AccuracyCoin 5 项） | 部分完成 | 按 Mesen 的 `SyaSxaAxa` 建模：保留进位、高位与寄存器对按位与、值用 base 的高位 +1 掩蔽、DMA 打断则不掩蔽（§7）。五项 `UnOp_SHA/SHS/SHY/SHX` 从第 3 项推进到第 15 项 | 剩下那一项在这些测试的 DMA 段里——并入下一刀（DMA 那一组 13 项） |
| B4 | AccuracyCoin 的 DMA 组（13 项） | **进行中** | 机制已定位：DMA 的 halt/dummy 周期必须是"对 CPU 即将访问地址的真实读"（`DMA + $2007 Read` 第 3 项就在数它）。两版实现都实测后撤回（都让 `4-irq_and_dma` 的表动了、AccuracyCoin 启动卡死；第二版停在 ROM 自己的陷阱 `$80DD`），两轮证据与收窄后的下一步写进 §7 | **逐字**移植 Mesen 的 `ProcessPendingDma`（奇偶、两个标志、`_abortDmcDma`、`$4016/$4017` 的跳过规则），保持 OAM 513/514 |
| B2 | 背景瓦片拿到右邻的调色板（`?` 砖、砖行末尾、标题文字末尾变绿） | **完成** | 粗 X 在属性取指之前自增（§7 那一行）；回归测试三条在旧代码上失败 | 无；相邻项是 §2.2 里背景取指的开关（关背景时 Mesen 仍取指） |
| — | 访问 dot 的实验 | 完成 | `f5943b3`：0/1 各让五个套件失败，2 全绿 → 保留 `ACCESS_DOTS = 2` | 无 |
| C1 | **与 MesenCE 的逐文件源码对比（§10）** | 完成 | 把整棵 `Core/NES` 取下来与本内核对照读了一遍（551 个文件、约 275 个板级类）。查出 12 处缺陷，含 NES 2.0 半字节对调、`$2002` 清掉 sprite 两个标志、CPU 复位的 `SP`、手柄口的 open bus；核实 12 项已一致；AccuracyCoin 的 34 项按"第几项失败"重读（`(value >> 2) + 1`，§7/§9 原先的编号整体多一） | 按 §10.7 的顺序做：DMA 逐字移植 → `$4016`/`$4017` 配 `read_joy3` → PPU 寄存器状态机 → `$2002` 标志与逐 dot sprite 求值 |
| C2 | **为对齐让路而移除 ROM 套件**（§6/§9） | 完成 | 10-16 删除 `tests/rom_tests.rs`、`tests/common/`、`tests/golden/`（可从 git 取回：`b6c06a1` 及更早；另有一份在 `/tmp/nes-baseline-snapshot-20261016`）。`cargo test --workspace` 130 s → 约 11 s，604 个单元测试全绿。§10.6 列出的套件已放到 `roms/nes-test-roms/`（37 张 ROM，已 gitignore）并各自量过一次 | 对齐收尾时全部放回，连同 §10.6 的那些：那次实测里 `cpu_dummy_writes`（2/2）与 `cpu_exec_space` 的 `ppuio` 已经报 "Passed"，`cpu_exec_space_apu` 失败在 open bus 第 2 项，`cpu_reset/registers` 失败在第 2 项并打印 `SP=$F7`（ROM 要的是 `$FD`）——正是 §10.3 第 3 条要修的缺陷 |
| C3 | §10.3 的前五项：NES 2.0 半字节、`$2002` 标志寿命、Mesen 混音、复位分成两种加八个预热周期、奇数帧跳点读慢一个 dot 的渲染状态 | **完成** | 实测整行见 §7。`ppu_vbl_nmi` 10/10、AccuracyCoin 32 项失败、`cpu_reset/registers` 越过第 2 项并打出期望的 `S=$FD`；所有会自报结果的 ROM 复测无回退；单元测试 605 | §10.3 剩下的，按 §10.7 的顺序：手柄口与 `$4016` 写入延迟（第 4/5 条），再是 PPU 寄存器状态机（第 6 条），再是 DMA（第 7 条）——APU 边沿时序（第 8 条）与死代码 `addressing.rs`（第 10 条）作为侧刀 |
| C6 | PPU 寄存器状态机（§10.3 第 6 条） | **试过并撤回** | §7 那一行：四件都做了并分别量过；只做读缓冲延迟就会让 AccuracyCoin 17 → 20、`cpu_dummy_writes_ppumem` 0x00 → 0x07，而它针对的两张 ROM（`ppu_read_buffer`、`full_palette`）根本不通过 `$6000` 上报 | 按可测量的步骤重做，那两张 ROM 用帧导出对照；总线地址的管线要挪到「延迟后的访问」（Mesen 的 `SetBusAddress`）而不是 CPU 的那个周期 |
| C5 | **DMA 逐字移植（§10.3 第 7 条）** | **完成** | §7 的两行：AccuracyCoin 31 → 18 → **17** 项失败且 146 项全部跑到结果、`dmc_tests` 4/4 转绿、DMA 状态机与内部寄存器合并、DMC 的状态规则与 `_bufferEmpty` 守卫、以及把周期奇偶写成有名约定而不是各处翻转。它带出的四处回退全部关闭；除两项已知项外，所有会自报结果的 ROM 复测通过 | `dmc_dma_during_read4` 那五张只差一个周期（五张里三张已打印可接受输出）；`sprdma_and_dmc_dma` 报失败码 1——sprite/DMC 重叠相位，现在是可测的而不是卡死 |
| C4 | 手柄口（§10.3 第 4/5 条） | 完成（端口自己这一侧） | 见 §7 那一行：Mesen 的 strobe 沿、补 1 的移位、open bus 掩码、同周期重复读、写入的两周期延迟。AccuracyCoin 32 → 31（`Controller Strobing` 转绿，`Controller Clocking` 由第 2 项推进到第 7 项） | 这一组剩下的是"DMA 的 put/halt 周期走手柄口"，属于 DMA 那一刀（第 7 条） |

**先记住一件容易搞错的事**：AccuracyCoin 的计数是**我们的**基线，不是 Mesen 的——它不在 Mesen
的测试套件里（§5.1）。每一项的 oracle 都应当是 Mesen 的源码，ROM 的裁决是证据而不是目标；少数项
可能是"ROM 想要、Mesen 有意不建模"的行为，那些按 §4 记成有意偏差而不是弯模拟器。

**AccuracyCoin 那 34 项已经按名字归好类**（13 项 DMA 边界情形、7 项 CPU 指令细节、5 项未文档
`SHx`、3 项 sprite、3 项 APU、2 项手柄、1 项 PPU），清单在
`docs/architecture.md` §13.1；13 项 DMA 失败在同一两项检查上，说明它们是**同一个机制**——
这是当前最值得开的一刀。

**取 Mesen 源码的办法**：整棵树取下来，别按单个文件抓——只有 3.8 MB 一条命令（配方在 §10.1：
blob 过滤的 clone 加稀疏 checkout `Core/NES`、`Core/Shared`、`UI/Dependencies`、`UI/Config`）。
逐文件抓（`curl -sSf -O https://raw.githubusercontent.com/SourMesen/Mesen2/master/Core/NES/NesPpu.cpp`）
是之前会话的做法，也正是两棵树被混在一起来源的由来：`SourMesen/Mesen2` 与 `nesdev-org/MesenCE`
同一文件能差 5–100 行。**要读的是 `nesdev-org/MesenCE`**（§5.1），`UI/Dependencies/MesenNesDB.txt`
也在那棵树上。已读过并写进本文档的文件清单在 §10.1。

**交接时的约定**：ROM 不入库（`roms/` 缺目录时测试打印跳过）；文档英中必须同时改（`*.zh-CN.md` 对照）；
每一步的实测数字写进提交信息与 §7；因"更忠实于 Mesen"而产生的回退要连同原因记进 §7 的撤回清单，
而不是把代码改回去。

---

## 10. 与 MesenCE 的 NES 内核逐文件对比（2026-10-16）

§2 记的是"已知的差距"；这一节是把两边的源码逐子系统并排读完之后的结果，因此也包含此前没人找过的
那些。它是接活时的第一参考：每一行都写明两边各是什么，§10.4 列出**已经一致**的部分以免重复劳动。

### 10.1 对比的是哪一份，怎么拿到同一棵树

Mesen 维护中的分叉是 `nesdev-org/MesenCE`（§5.1）。本次读的是真正的 checkout，不是逐文件拷贝：

```bash
git clone --depth 1 --filter=blob:none https://github.com/nesdev-org/MesenCE.git /tmp/MesenCE
cd /tmp/MesenCE
git sparse-checkout set --cone Core/NES Core/Shared UI/Dependencies UI/Config
git checkout                      # 取回稀疏集需要的 blob
```

当时的 `HEAD` 是 `4b8a196`。`Core/NES` 共 551 个文件：顶层十个翻译单元 10,644 行（`NesPpu.cpp`
1,769、`BaseMapper.cpp` 1,383、`NesCpu.h` 914、`NesConsole.cpp` 775、`MapperFactory.cpp` 670、
`NesCpu.cpp` 612、`NesTypes.h` 527、`GameDatabase.cpp` 346、`NesControlManager.cpp` 303、
`NesHeader.cpp` 250），`Core/NES/Mappers/` 另有 296 个文件（1.3 MB），`MapperFactory::GetMapperFromID`
里 include 了 275 个板级头、共 326 个 `case` 标签。与内核有交集的文件都读完了：`NesCpu.{h,cpp}`、
`NesPpu.{h,cpp}`、`BaseNesPpu.{h,cpp}`、
`DefaultNesPpu.h`、`NesConsole.{h,cpp}`、`NesMemoryManager.{h,cpp}`、`OpenBusHandler.h`、
`InternalRamHandler.h`、`NesControlManager.{h,cpp}`、`Input/NesController.h`、`NesConstants.h`、
`NesHeader.cpp`、`Loaders/iNesLoader.cpp`、`APU/` 全部十一个文件、`NesSoundMixer.{h,cpp}`、
`Shared/Audio/SoundResampler.cpp`、`NesDefaultVideoFilter.cpp`、`BaseMapper.{h,cpp}`、
`Mappers/Nintendo/MMC3.h`、`Mappers/A12Watcher.h`、`Shared/SettingTypes.h`（`NesConfig` 的默认值）
以及 `UI/Dependencies/MesenNesDB.txt`。

这样做带来两个必须记住的结论：

- 之前的会话是按单个文件从 `SourMesen/Mesen2` 抓的。两棵树接近但**并不相同**（同一文件相差 5–100
  行：`ApuFrameCounter.h` 236 对 256、`NesPpu.cpp` 1,614 对 1,769），所以任何结论都要写明出自哪
  个分叉。下面的行都是 MesenCE 的。
- `NesConfig` 的默认值就是"Mesen 的行为"的定义（默认关闭的开关不是要模仿的对象）。与本内核相关的：
  `EnableOamDecay = false`、`EnablePpuOamRowCorruption = false`、`EnablePpu2000ScrollGlitch = false`、
  `EnablePpu2006ScrollGlitch = false`、`DisablePpu2004Reads = false`、`DisablePaletteRead = false`、
  `RestrictPpuAccessOnFirstFrame = false`、`RemoveSpriteLimit = false`、
  `ForceBackgroundFirstColumn = false`、`ForceSpritesFirstColumn = false`、
  `EnableDmcSampleDuplicationGlitch = false`、`SwapDutyCycles = false`、`ReverseDpcmBitOrder = false`、
  `SilenceTriangleHighFreq = false`、`ReduceDmcPopping = false`、`IsFullColorPalette = false`、
  `RandomizeCpuPpuAlignment = false`、`RandomizeMapperPowerOnState = false`、
  **`RamPowerOnState = Random`**、`InputScanline = 241`、`ConsoleType = Nes001`、`Region = Auto`，
  以及超频用的额外扫描线数全为 0。

Mesen 自己的数字，供后续步骤算账用：时钟 1,789,773 / 1,662,607 Hz（`NesConstants.h`）；
`cpuDivider`/`ppuDivider` NTSC 12/4、PAL 16/5、Dendy 15/5（`NesCpu::Reset`、`NesPpu::UpdateTimings`）；
CPU 周期里访问的切分 `_startClockCount`/`_endClockCount` NTSC 6/6、PAL 8/8、Dendy 7/8
（`NesCpu::SetMasterClockDivider`）；`_ppuOffset = 1`；`_nmiScanline` 241/241/291、`_vblankEnd`
260/310/310；一行 341 dot；APU 的 `_stepCycles` 与 DMC 的周期表。

### 10.2 仍然不同的地方

#### 机器与 CPU

| | MesenCE | 本内核 | 代价 |
| --- | --- | --- | --- |
| 复位 | `NesCpu::Reset`：`CycleCount = -1`、主时钟先偏 `cpuDivider`、`_ppuOffset = 1`，然后**跑八个空周期**才执行第一条指令；上电时 `SP = $FD`、`A/X/Y = 0`、`PS = I`，且**不**从 SP 减 3（只有软复位才减） | `Cpu::reset`（`cpu.rs`）在 SP 从 `$FD` 开始时无条件减 3——所以这里上电后是 **`SP = $FA`**——周期计数从 0 起，也没有预热周期 | `cpu_reset` 一类 ROM；第一条指令执行时 PPU/APU 的相位 |
| 访问落在周期的哪里 | 按制式取 `_startClockCount`/`_endClockCount`：NTSC 一个周期 12 个主时钟、访问落在第 5 个，PAL 16 中第 7 个，Dendy 15 中第 6 个 | 三个制式都是 `ACCESS_DOTS = 2` 个 dot——PAL 上等于 16 个 tick 里的第 10 个，不是第 7 个 | NTSC 是实测过的、也是有意的（§4）；PAL 的落点从未量过，而且没有 PAL ROM |
| DMA | `NesCpu::ProcessPendingDma` 在 halt 周期、DMC 的 dummy 周期与 OAM 的对齐周期里都读 `readAddress`；DMC 取数是**按奇偶 3 或 4 个周期**；DMC 与 sprite 传输共用一个两标志握手；`StopDmcTransfer`/`_abortDmcDma` 在确定点取消；`$4016`/`$4017` 有跳过规则；一次 DMC DMA 若撞上位计数复位可以再次请求（开关控制） | `NesBus::process_pending_dma`（`nes.rs`）：DMC 取数固定四个周期，任何 halt/dummy 周期都不做总线访问，没有中止路径，也不看奇偶 | AccuracyCoin 的 13 项 DMA、`4-irq_and_dma`、`Misaligned OAM DMA`、`DMC DMA + OAM DMA`、`Explicit`/`Implicit DMA Abort` |
| DMA 期间的中断线 | DMA 周期就是普通的 `StartCpuCycle`/`EndCpuCycle`，`_prevRunIrq`/`_needNmi` 照常采样 | `advance_one_cpu_cycle` 只是总线的 begin/end，而唯一采样中断线的 `Cpu::end_cycle` 不会被调用——停机期间到达的中断永远不会被锁存 | `cpu_interrupts_v2`、`DMA + Open Bus` |
| Dendy | 第三个制式：`cpuDivider` 15、`ppuDivider` 5、`_nmiScanline` 291、`_vblankEnd` 310、`startClockCount` 7 | 没有这个制式；Dendy 卡带按 NTSC 跑 | 本地无法验收（没有 Dendy ROM）；它是头部能声明而内核无法表示的制式 |
| 未文档 `KIL`/`JAM` | `NesCpu::HLT` 让 CPU 停住**并且**清掉中断采样，挂死不可被打断 | CPU 停住但仍可被中断拉起（`Cpu::jam`），宿主因此能上报并恢复 | 有意偏差（§4） |
| `cpu/addressing.rs` | — | 760 行寻址加一整个测试模块**已经不再运行**：真正的实现是 `instructions.rs::fetch_operand`，模块外没有任何地方引用 `addressing::` | 不是精度差距而是陷阱：那些测试钉的是一个机器并不使用的模型，其中一个还曾钉住错误的 SHx 行为 |

#### PPU

| | MesenCE | 本内核 | 代价 |
| --- | --- | --- | --- |
| `$2002` 读 | **只**清 VBlank 标志与 CPU 的 NMI 锁存（`UpdateStatusFlag`）；sprite overflow 与 sprite 0 命中在预渲染行 cycle 0 清 | `ppu.rs` 每次读都清 VBlank **加** sprite overflow **加** sprite 0 命中 | 任何在还没用完另两个标志就提前读 `$2002` 的 ROM；AccuracyCoin 读第 5 位之后还指望这个标志的状态有意义 |
| `$2002` 低五位 | open bus，`openBusMask = 0x1F`，另有 2C05 的几种变体（`ProcessStatusRegOpenBus`） | 同样取 PPU 自己锁存的五位 | 已对齐；2C05 变体只对 VS System 游戏有意义 |
| `$2004` | 渲染期间读返回 `_oamCopybuffer`，并在 257-340 与 dot 0 从二级 OAM 重新装载；渲染期间写**被丢弃**、只让 OAMADDR 的高六位跳（`+4 & 0xFC`）；属性字节读回时掩蔽 `& 0xE3`；自增在 PAL 上有奇偶规则 | 普通的 OAM 读写加无条件自增 | 行内的 OAM 流量；`Misaligned OAM DMA`（Mesen 的传输经 `$2004` 落地，因此继承这些规则） |
| `$2007` 读 | 返回的是缓冲，且缓冲要**再过五个 PPU 周期**才重新装载（`_ppuMemoryDataReadStateMachine = 5`）；`v` 再晚一个周期自增（`_needVideoRamIncrement`）；六个 dot 内的第二次读返回 open bus | 立刻换缓冲、立刻自增 `v`；六个 dot 的忽略窗口已建模（`_ignoreVramRead`） | `ppu_read_buffer`、`full_palette`；读 `$2007` 的那些 DMA 项 |
| `$2007` 写 | 值先锁存、五个 PPU 周期后才写；**渲染期间这个值被丢掉，写进去的是地址的低字节** | 立刻写，而且永远写真实的值 | 同上两套 ROM；任何在渲染期间写 VRAM 的游戏 |
| `$2006` | `v` 要**三个 PPU 周期后**才更新（`_updateVramAddrDelay = 3`），`t` 随之跟进 | 在写入的那个周期就 `v = t` | 在自增之前紧接着改 `v` 的行内分屏 |
| 交给卡带的总线地址 | `SetBusAddress` 除了每次 `ReadVram`/`WriteVram`，还在若干非取指时刻被调用：不渲染时的 `UpdateVideoRamAddr`、vblank 起点、扫描线 0 的第一个周期、行内开关渲染、延迟的 `$2006` 更新 | 只喂流水线真正读过的地址（`take_bus_access`），每 dot 一次 | A12 滤波器看到的边沿比 Mesen 少——这正是 A12 那一刀必须逐 dot 量的原因（§7） |
| sprite 求值 | 逐 dot，65-256：1-64 清二级 OAM、65 调 `ProcessSpriteEvaluationStart` 且 dot 66 定 `_sprite0Added`、OAM 地址损坏（`spriteAddrH`/`spriteAddrL` 同时走）、`_oamCopyDone` 重对齐与伪溢出计数 | dot 65 一次调用填满下一行的二级 OAM | `oam_flicker_test*`；AccuracyCoin 的 `$2002 flag timing` 第 2 项（要 `$55 == $60`，即同一读里 sprite 0 与 overflow 同时置位）、`Arbitrary Sprite zero`、`Suddenly Resize Sprite` |
| sprite 像素 | 八个硬件移位器加 `_dotSkipped`，含 counting/active/expired 三种模式，`RemoveSpriteLimit` 时还有额外 sprite；奇数帧跳掉的那个 dot 会把所有 sprite 推迟一个像素（`_spriteShifterList[i] += 1 << 4`） | 每 dot 从逐行槽位重算 | 奇数帧的一像素 sprite 延迟缺失；sprite 上限开关不存在 |
| 关背景时的背景取指 | `LoadTileInfo` 挂在 `IsRenderingEnabled()`（背景**或** sprite）上，所以只开 sprite 时仍然取指、仍然移位、`v` 仍然推进 | 整条背景路径挂在 `SHOW_BG` 上 | 只开 sprite 时的 A12（MMC3 时钟不同）；本地没有 ROM 覆盖 |
| 左侧列掩蔽 | 三个独立阈值 `_minimumDrawBgCycle`/`_minimumDrawSpriteCycle`/`_minimumDrawSpriteStandardCycle`，各按掩码与"强制第一列"设置取 8 或 300 | 用两个掩码位做 `x < 8` 判断 | 默认设置下等价；三个阈值并非在所有情形都是同一个数 |
| 强制消隐 | 渲染关闭且 `v` 落在 `$3F00-$3FFF` 时，`DrawPixel` 显示 `paletteRam[v & 0x1F]` | 显示 backdrop | `full_palette` 一类 ROM |
| 复位后的状态 | 调色板 RAM 取固定的 32 字节上电图案（默认 `RamPowerOnState = Random` 时为随机）；`v` **不**被复位清掉，只有上电才清；`_scanline = -1`、`_cycle = 340`、`_frameCount = 1` | 调色板、VRAM、OAM 全部清零，`v = 0`、`scanline = 0`、`cycle = 0`、`frame = 0` | `blargg_ppu_tests` 里的 `power_up_palette` 与 `palette_ram`（今天绿是因为它们先写自己测的东西） |
| 帧边界 | `SendFrame` 跑在扫描线 240 的 cycle 0，所以前端的"一帧"从 vblank 开始 | 预渲染行绕回时才换帧 | 只影响宿主：我们的边界早一行 |
| OAM 衰减 | `ReadSpriteRam` 带逐行时间戳，**默认关闭**（`EnableOamDecay = false`）；时间戳窗口是 `OamDecayCycleCount = 4500`（约 40 扫描线） | 未建模 | 没有：不做正是 Mesen 的默认（§2.2）。§2.2 里写的"3000"是 `SourMesen/Mesen2` 的值——正好是两棵树不一致的例子（MesenCE 是 4500） |

#### APU 与音频

| | MesenCE | 本内核 | 代价 |
| --- | --- | --- | --- |
| 长度计数器装载 | `LoadLengthCounter` 只记下 `_reloadValue`/`_previousValue`；`ReloadCounter` 在**帧计数器为该周期跑完之后**才应用，且只在 `_counter == _previousValue` 时应用；halt 标志也在那里落地 | `LengthCounter::load` 在寄存器写入时立刻改计数器 | blargg 的 `len_reload_timing` 第 4、5 项——Mesen 自己的注释点名的那张 ROM |
| 帧计数器 | 六项表（`...29828, 29829, 29830` 与 `...37281, 37282`）、带 3/4 周期延迟的 `_newValue` 待写机制、`_blockFrameCounterTick = 2`（一次 tick 会挡住随后两个周期里写入的时钟），以及 `GetIrqFlag` **在被读之后的下一个 APU 周期开头自动清标志** | 五项表加 `hold`，标志用三周期的 `frame_counter_irq_window` | `4-jitter`/`5-len_timing`/`6-irq_flag_timing` 通过；AccuracyCoin 的 `Frame Counter IRQ` 与 `Controller Strobing`（第 3 项读 put/get 转换）不通过 |
| DMC | `SetEnabled` 的效果延迟 2/3 个周期（`_disableDelay`）、传输起点延迟 2/3 个周期（`_transferStartDelay`）、未开始的传输会被中止，另有采样重复与单字节采样的特例（开关控制）；定时器周期存的是表值减一 | `set_enabled` 立即生效；周期就是表值 | `dmc_dma_start_test`、`dmc_dma_start_test_v2`、AccuracyCoin 的 `DMC Reload Timing`，以及 DMA 组里的 DMC 那一半 |
| 混音 | 非线性，但用的是自己的常数：`95.88 * 5000 / (8128 / square + 100)` 与 `159.79 * 5000 / (22638 / tnd + 100)`，其中 `tnd = dmc + 2.7516713261 * triangle + 1.8493587125 * noise`，全部按声道原始 0-15 单位，送进 blip 前再乘 4 | `95.52 / (8128 / pulse + 100)` 与 `163.67 / (24329 / tnd + 100)`，`tnd = dmc + 3 * triangle + 2 * noise`，浮点 | 几个百分点的声道平衡差异：满音量下三角波 Mesen 是 0.2464、这里是 0.2555（三角波响 3.7%、噪声响 3.0%），脉冲轻 0.4%。能裁决它的是 `apu_mixer` 与 `apu_mixer_recordings` |
| 扩展音源混音 | 线性叠加，权重固定：FDS 20、MMC5 14、N163 20、5B 15、VRC6 5、VRC7 1 | 每个扩展音源各走各的路径 | 只有那些板子上听得出来；本地没有金标准 |
| 重采样 | blip 跑 96 kHz（`bass_shift = 9`），再用 Hermite 重采样到声卡的真实速率 | 一级带限直接到设备率 | 有意偏差，已记录（§2.4/§7） |

#### 内存、open bus 与手柄

| | MesenCE | 本内核 | 代价 |
| --- | --- | --- | --- |
| 数据总线 | 两个锁存，`NesCpuBusType::Internal`/`External`（`OpenBusHandler.h`）：读会同时更新两者，读 `$4015` **只**更新内部那个，DMA 读可以只更新其中一个 | 总线上一个 `open_bus` 锁存，每次读、每次写都覆盖 | AccuracyCoin 的 `Internal Data Bus`、`Open Bus`，以及 `APU Register Activation` 里 `$4015` 那一半 |
| 内部 RAM | 默认 `RamPowerOnState = Random`，另有 Famicom Box 的 8 KB 变体（`InternalRamHandler<0x1FFF>`） | 清零的 2 KB | 先读后写的 `cpu_reset` 一类 ROM |
| `$4016`/`$4017` 读 | `openBus(GetOpenBusMask(port)) \| device`，掩码按机种：NES-001 是 $E0，NES-101 的一号口是 $E4，HVC-001 是 $F8；同一个 CPU 周期内重复读同一地址返回上一次的值（`ReadDevice`）；读满八次之后手柄移位寄存器补 1 | 一律 `bit \| 0x40`，一律移位，八次之后寄存器空成 0 | `read_joy3`；AccuracyCoin 的 `Open Bus`（第 2 项）、`APU Register Activation`（第 2 项）、`Controller Clocking`（第 2 项） |
| `$4016` 写 | strobe 与 OUT 引脚在下一个 PUT 周期更新（`_writePending = (masterClock & 1) ? 1 : 2`，由 `ProcessCpuClock` 落地） | 在写入自己的周期就生效 | AccuracyCoin 的 `Controller Strobing` |
| 手柄采样 | 控制设备每帧在 PPU 扫描线 `InputScanline = 241` 刷新一次 | 宿主按钮状态随设随到 | 帧中途改按钮的宿主会立刻看到，而不是等到 vblank |

#### 卡带、载入与视频

| | MesenCE | 本内核 | 代价 |
| --- | --- | --- | --- |
| MMC3 的 IRQ 修订版 | `_forceMmc3RevAIrqs = DatabaseInfo.Chip.substr(0,5) == "MMC3A"`——逐游戏，来自数据库 | 两个修订版都没有：只有 rev-B 条件 | `6-MMC3_alt`（见 §10.6，**Mesen 同样失败**）以及板子标注 `MMC3A` 的那些游戏 |
| `$C001` | `_irqCounter = 0; _irqReload = true` | `irq_counter \|= 0x80`——blargg 描述的"病态"行为，Mesen 并未实现 | 观察不到：两种写法在每次被计数的上升都会从锁存重装 |
| MMC6 | `SubMapperID == 1` 时工作 RAM 切成四个 $400 字节的 bank，各自带读写使能 | 未建模 | mapper 4 submapper 1 的游戏——而且要先修下面 NES 2.0 那条才看得见 |
| 非 MMC3 板的 A12 | `A12Watcher::UpdateVramAddress(addr, frameCycle)`，`minDelay = 10` 个**帧周期**（PPU dot），MMC4/VRC/FME7 一类板子用它 | `Mapper::on_ppu_bus(addr, cpu_cycles)`，三个 CPU 周期的规则 | 这里只有 MMC3 有滤波器；其它板的 watcher 不存在 |
| 总线冲突 | `BaseMapper::WriteRam` **只在寄存器地址上**做按位与（`_isWriteRegisterAddr[addr]`），数据库还能逐游戏强制这个标志（`BusConflictType`） | `NesBus::write_now` 只要板子声明冲突就对所有卡带写入做与 | 冲突板上的 PRG RAM 写入会在这里被破坏（本地没有这种板子） |
| Mapper 钩子 | `EnableCpuClockHook()` → **每个 CPU 周期**调 `ProcessCpuClock()`，声明它的有 Sunsoft FME7/3/4、IremH3001、Kaiser 202/7017、Mapper 40/42/43/50/83/106/253、Tf1201、Fk23C、Smb2j、CityFighter、T230、Yoko、DripGame、Rainbow 等 | `on_cpu_cycles(cycles)` 每条指令一次，另有 `on_scanline` | 本应落在指令中途的 mapper IRQ 最多晚六个周期（这里用到它的有 mapper 21/24/16/18/5/69） |
| 板级覆盖 | 296 个 mapper 文件、`MapperFactory::GetMapperFromID` 里约 275 个 `case`，外加数据库覆盖 | 44 个 mapper 文件 | 64（含 158）、115、116、176、185、MMC5 扩展属性模式、VRC7、Namco 163、FDS、EPSM、VS System、NSF、HD pack——长尾，且有意排在 CPU/PPU 之后 |
| NES 2.0 头部 | `GetMapperID()` 里 mapper 的高四位取自 byte 8 的**低**半字节，`GetSubMapper()` 取 byte 8 的**高**半字节；指数-乘数尺寸已实现；byte 13 的机种/VS/PPU 型号与 byte 15 的输入类型都解析 | **两个半字节被对调**（`nes.rs`）：mapper 取了 byte 8 的高半字节，"submapper"取了低半字节——于是 NES 2.0 文件里 submapper 1 的会当成 mapper 260 载入，submapper 读出来是 0 | 本地没有 ROM 带 NES 2.0 头，所以什么都没动；任何 submapper 非 0 的 NES 2.0 文件都会中招 |
| 游戏数据库 | `UI/Dependencies/MesenNesDB.txt`：10,655 条，按 PRG+CHR 的 CRC32 索引，18 个字段（板、PCB、芯片、mapper、尺寸、镜像、输入、总线冲突、submapper、VS 类型……）。`MMC3A` 与镜像覆盖都出自这里 | 没有 | 商业游戏的怪癖；**`roms/` 下 82 个 ROM 没有一个在库里**（逐个算 CRC 核实过），所以本基线不受任何覆盖影响（§10.6） |
| 视频 | 2C02/2C03/2C04-A-D/2C05-1-5 调色板、用户调色板、强调色生成，以及可选的 NTSC 复合视频滤波（`NesNtscFilter`、`BisqwitNtscFilter`） | 2C02 表、强调色生成、`nes_color` | 调色板与强调色已经**完全一致**（§10.4）；RGB PPU 的调色板与复合滤波不存在 |

### 10.3 本轮查出的缺陷（每条附修法）

**进度：**第 1、2、3、9 条已修（C3 那一步，见 §7 与 §9），奇数帧跳点也随之改成读慢一个 dot 的
渲染状态（§10.2 的 PPU 行）。**同日更新：**第 4/5 条在"端口自己这一侧"已经做完——strobe 的下降沿、
读满八次后补 1、open bus 掩码、同周期重复读、写入的两周期延迟（§7）。手柄这一组剩下的是
"DMA 的 put/halt 周期去走手柄口"，属于第 7 条。其余未做的是第 6 条（PPU 寄存器状态机）、
第 7 条（DMA）与第 8 条（APU 边沿时序），外加第 10、11 两条清理。

1. **NES 2.0 的 mapper 与 submapper 半字节对调**（`crates/nes-core/src/nes.rs` 的 `RomHeader` 解析）。
   Mesen 是 `MapperID = (Byte8 & 0x0F) << 8 | (Byte7 & 0xF0) | (Byte6 >> 4)`、`SubMapper = (Byte8 & 0xF0) >> 4`；
   这里 mapper 取 `(flags8 & 0xF0) << 4`、submapper 取 `flags8 & 0x0F`。`roms/` 里没有一个带 NES 2.0
   头，六套 ROM 因此从未发现它。
2. **`$2002` 读会清掉 sprite overflow 与 sprite 0 命中**（`ppu.rs` 的 `0x2002` 分支）。Mesen 在那里
   只清 VBlank，另两个在预渲染 cycle 0 清。帧中途的一次读，在这里会把那两个标志拿走，在那里会留下。
3. **CPU 复位**：上电时也执行了 `SP -= 3`（新机器是 `SP = $FA` 而不是 `$FD`），周期计数从 0 而不是 -1
   起算，八个空周期缺失。
4. **手柄口一律回答 `bit | 0x40`**（`nes.rs` 的 `ControllerState::read`）：没有按机种的 open bus 掩码、
   没有"同一周期内重复读"规则，移位寄存器读满八次后是 0 而不是补 1。
5. **`$4016` 的写立刻传到手柄**，而不是落在下一个 PUT 周期。
6. **PPU 的寄存器状态机不存在**：`$2007` 的延迟读缓冲与延迟写（含渲染期"写地址低字节"）、`$2006`
   的三周期延迟、`$2004` 的渲染期行为，以及强制消隐时的 `palette[v & 0x1F]` 路径。
7. **DMA 的形状**（§2.3/B4）：halt 与 dummy 周期不做总线访问，DMC 取数永远四个周期，没有中止路径，
   这些周期也不采样中断线。
8. **APU 的边沿时序**：长度计数器的延迟装载、帧 IRQ 标志真实的生命周期、DMC 使能/禁用的 2/3 周期延迟。
9. **混音常数**用的是 wiki 的而不是 Mesen 的（数字见 §10.2）。
10. **`cpu/addressing.rs` 是死代码**——760 行外加它整个测试模块。要么删掉，要么让它成为唯一实现；
    留着它，下一个会话就会"修"一个并不在跑的模型。
11. **`ppu.render_v` 仍然存在**（`ppu.rs`，而且在 `state_fields!` 里，会被存档序列化），尽管 §2.1
    写它已删除。
12. **§1 的预渲染行仍在归功于 `NesBus::drive_ppu_bus`**，而那个函数早已不在。

### 10.4 已核实一致——不要重开这些

两边都读过，答案相同：

- 2C02 调色板表与强调色生成：把 MesenCE 的 `_ppuPaletteArgb[0]` 与这里的 64 项表取出来机械比对，
  结果**完全相同**；`GenerateFullColorPalette` 把另外两个通道乘 **0.84** 后截断、并跳过 `$xE`/`$xF`
  两列——与 §7 九位色那一行记的完全一样。
- 调色板读：低六位来自调色板 RAM、高两位来自 open bus、读出时套灰阶掩蔽（`_paletteRamMask`），
  `$3F00/04/08/0C` 与 `$3F10/14/18/1C` 互为别名。
- PPU 侧的 open bus：`_openBusDecayStamp` 按位、**3 帧**后失效，`$2002` 只驱动三位，`$2007` 驱动它
  返回的那个字节。
- 第二次 `$2007` 访问的 `_ignoreVramRead = 6`，以及 `>= $3F00` 的调色板路径。
- 奇数帧跳点：在预渲染行的 dot 339 决定，被跳掉的是 340，且仅 NTSC、仅 2C02。
- OAM 衰减在 Mesen 里默认关闭（开启时每行 4500 周期）——不做正是对齐它的默认值。
- MMC3 的 A12 滤波：`_a12LowClock > 0 && GetMasterClock() - _a12LowClock >= 3`，`GetMasterClock()`
  就是 CPU 周期数，且每次上升都结束当前低电平段（`NotifyVramAddressChange`）——`mapper4.rs` 正是这样。
  `5-MMC3` 在两边都是绿的。
- 总线冲突的语义（与本身；只有上面那条"仅在寄存器地址"的差别）。
- blip 的 `bass_shift = 9`；DMC"取数时移计数、字节留在缓冲、静音标志保持电平"的形状；DMC 的 NTSC
  与 PAL 周期表逐值相同。
- 每次 `ReadVram` 都会调 `SetBusAddress`（上面总线地址那一条的"逐取指"那一半）。
- `$2002` 读的 VBlank 抑制窗口：AccuracyCoin 的 `$2002 flag timing` 第 1 项（答案 `$E0 $E0 $80 $00`）
  在 `ACCESS_DOTS = 2` 下通过，这是 §4 那条有意偏差的独立证据。
- `UpdateStatusFlag` 的特例（NMI 扫描线 cycle 0 置 `_preventVblFlag`）与抑制窗口建模的是同一次竞争。
- 渲染状态慢一个 dot（`_renderingEnabled` 由 `UpdateState` 在每个 dot 的处理之后刷新）：奇数帧跳点
  现在读的是 dot 338 末尾的状态，`10-even_odd_timing` 那四次跨边界写入量的正是它。

### 10.5 AccuracyCoin 那 34 项，各自到底要什么

harness 打印的是 ROM 存下的那个字节，它的构造是 `(ErrorCode << 2) | 2`（`AccuracyCoin.asm` 的 `TEST_Fail`）。
**因此失败的是第 `(value >> 2) + 1` 项**，比 §7、§9 里 `SHx` 组与 DMA 组用的 `value / 2` 少一。本 checkout
实测（`cargo test -p nes-core --test rom_tests accuracy -- --nocapture`：跑 140、过 94、失败 34）：

| 地址 | 测试 | 失败第几项 | 那一项要什么 |
| --- | --- | --- | --- |
| `$0408` | Open Bus | 2 | 用 `LDA absolute` 读 open bus 时返回操作数的高字节 |
| `$0446`-`$044A` | `UnOp_SHA_93`、`_SHA_9F`、`_SHS_9B`、`_SHY_9C`、`_SHX_9E` | 8 | `SHx` 在各自测试的 DMA 段里的行为（地址损坏本身已落地，§7） |
| `$044C` | DMA + `$2007` Read | 3 | halt/对齐周期必须让 `v` 前进足够多次 |
| `$044F` | DMA + `$2007` Write | 2 | DMA 必须被写周期推迟 |
| `$0458` | Arbitrary Sprite zero | 3 | OAM 地址非 0 时的 sprite 0 |
| `$0459` | Sprite overflow behavior | 2 | 一行九个 sprite 时，`CLV` 之后读 `$2002` 第 5 位仍置位 |
| `$045C` | APU Register Activation | 2 | 读手柄口时不能有不该置的位 |
| `$045D` | DMA + `$4015` Read | 3 | — |
| `$045E` | DMA + `$4016` Read | 2 | — |
| `$045F` | Controller Strobing | 2 | 任何 bit 0 为 1 的值写入 `$4016` 都要 strobe |
| `$0460` | Instruction Timing | 2 | DMA 时序精确到可以拿它同步 |
| `$0461` | Interrupt flag latency | 11 | — |
| `$0467` | Frame Counter IRQ | 8 | 标志置位后切到 5-step 模式不得把它清掉 |
| `$046A` | Delta Modulation Channel | 20 | — |
| `$046B` | DMC DMA Bus Conflicts | 2 | DMC DMA 与 APU 寄存器的总线冲突 |
| `$046C` | DMA + Open Bus | 3 | — |
| `$046D` | Implied Dummy Reads | 4 | open bus（或者 `JSR` 的周期顺序不对） |
| `$0477` | DMC DMA + OAM DMA | 2 | 两次重叠 DMA 的周期数 |
| `$0478`/`$0479` | Implicit/Explicit DMA Abort | 2 | 被中止的 DMA 的周期数 |
| `$047A` | Controller Clocking | 2 | strobe 期间读手柄不得扰动移位寄存器 |
| `$047C` | JSR Edge Cases | 3 | open bus |
| `$0488` | DMA + `$2002` Read | 3 | — |
| `$0489` | Suddenly Resize Sprite | 2 | HBlank 开始时写 `$2000` 能把已经判为在范围内的 sprite 改尺寸 |
| `$048B` | Branch Dummy Reads | 6 | — |
| `$048D` | `$2002` flag timing | 2 | sprite 0 与 overflow 必须在同一次读里置位（`$55 == $60`） |
| `$0490` | Internal Data Bus | 2 | 读 `$4015` 不得动外部总线 |
| `$0494` | Misaligned OAM DMA | 2 | 传输从已设置的 OAM 地址开始 |
| `$0496` | DMA Landing on Write | 3 | DMC DMA 不得落在写周期上——它要等一个读 |
| `$0497` | DMC Reload Timing | 2 | — |

每一行 "DMA + …" 都是同一个缺失机制（§10.2 的 DMA 行）；两组手柄项与 `Internal Data Bus`、`Open Bus`
都是 `$4016`/`$4015` 的 open bus 工作。这里没有哪一项需要 AccuracyCoin 是对的——它只是一份顺手就有的
检查表。

**在 §10.3 前四项修完之后重测（同一天）：32 项失败。** 停下来的两项是 `Suddenly Resize Sprite`
（`$0489`）与 `Misaligned OAM DMA`（`$0494`）；上表其余每一项都还在同一项检查上失败，这个形状正是
预期的——剩下四组是手柄口、PPU 寄存器状态机、DMA 与 APU 边沿时序。上面那套失败项换算正是让这件事
读得出来的原因：表里记的是 ROM 打印出来的原始字节。

**手柄口自己这一侧做完之后：31 项。** `Controller Strobing`（`$045F`）不再失败，
`Controller Clocking`（`$047A`）从第 2 项推进到第 7 项——从"读满八次补 1"进到"DMA 的 put/halt
周期走手柄口"，那是 §10.3 第 7 条。`APU Register Activation` 的第 2 项是 `DMA + $2007 Read`
的前置条件（ROM 自己的注释如此），所以它等的同样是 DMA，而不是手柄规则。

### 10.6 基线套件：Mesen 自己开发时用的那些

Mesen 在注释里点名了自己的 oracle，所以缺哪些套件很好选。下面这些都在 `christopherpow/nes-test-roms`
合集里（本机 checkout 在 `/tmp/nes-test-roms`），而且今天一个都不在 `crates/nes-core/tests/rom_tests.rs` 里：

| 套件 | 出处（Mesen 注释） | 钉住什么 |
| --- | --- | --- |
| `blargg_apu_2005.07.30/11.len_reload_timing.nes`（`pal_apu_tests/` 里也有） | `APU/NesApu.cpp:166`："This fixes the test len_reload_timing (tests 4 & 5)" | 长度计数器的延迟装载 |
| `full_palette/` | `NesPpu.cpp:1589`："This allows the full_palette tests to properly display single-pixel glitches" | `$2007` 之后 `v` 的自增晚一个 dot |
| `ppu_read_buffer/` | — | `$2007` 的读缓冲及其延迟 |
| `read_joy3/` | — | `$4016`：strobe、重复读、位丢失、读满八次后的 1 |
| `sprdma_and_dmc_dma/`（含 `_512`） | `NesCpu.cpp:397`："Fixes dmc_dma_start_test_v2 case C" | sprite 与 DMC DMA 重叠 |
| `dmc_tests/`（`latency`、`status`、`status_irq`、`buffer_retained`） | `DeltaModulationChannel.cpp:268`："Allows behavior to match dmc_dma_start_test" | DMC 延迟与状态 |
| `dmc_dma_during_read4/` | — | DMC 取数落在某次读的最后一个周期 |
| `cpu_dummy_reads/`、`cpu_dummy_writes/`、`cpu_exec_space/`、`cpu_reset/` | — | CPU 的周期模型：丢弃读、复位后的 `SP` |
| `vbl_nmi_timing/`、`nmi_sync/`、`scanline/` | — | `ppu_vbl_nmi` 之外的 NMI 时序面 |
| `mmc3_irq_tests/`、`exram/`、`mmc5test_v2/` | — | 卡带的 IRQ 与扩展 RAM |
| `apu_mixer/` + `apu_mixer_recordings/` | — | 混音常数——唯一可得的音频金标准 |
| `pal_apu_tests/`、`tvpassfail/` | — | PAL——今天一张 ROM 都没有（P5） |

Mesen 点名的两张 ROM **不在**那个合集里，要另行获取：`oam_flicker_test_reenable`（`NesPpu.cpp:1177`）
与 `dmc_dma_start_test`/`_v2`（`NesCpu.cpp:397`）。

**已就位，并各自量过一次。** 上面这些都已拷进 `roms/nes-test-roms/`（37 张 ROM，不入库），并在
harness 被移除之前（§6）各跑了一次——下面就是那次的结果，也是套件放回去之后第一批要盯的门：

| 套件 | 那次量到的结果 |
| --- | --- |
| `cpu_dummy_writes` | 两张都报 `$6000 = 0`（通过） |
| `cpu_exec_space` | `ppuio`："Passed"；`apu`：失败在第 2 项，即 open bus 那一项（§10.3 第 4 条） |
| `cpu_reset` | `registers`：**失败在第 2 项**，打印 `S=$F7` 而 ROM 要的是 `$FD`——§10.3 第 3 条。`ram_after_reset` 要求**人**按复位（"Press reset AFTER this message disappears"），在从不复位的 harness 里不可能通过 |
| `read_joy3`、`full_palette`、`ppu_read_buffer`、`blargg_apu_2005`、`dmc_tests`、`dmc_dma_during_read4`、`cpu_dummy_reads` | 帧预算内没有 `$6000` 上报——它们的判决是画面或卡死，harness 无从判定（帧导出诊断与金标准帧就是为这类 ROM 准备的）。DMC/DMA 那几张**什么都不画**：`sprdma_and_dmc_dma` 在它自己的 `$E200`/`$E220`/`$E240` 测量序列里空转（`$E22D` 的延时加 `$4015` 的等待），自带的时钟计数器永不前进——它的 DMA 相位永远对不上，正是 §10.3 第 7 条现场 |

**还有一项基线根本不是缺陷。** `mmc3_test_2` 的 `6-MMC3_alt` 要的是 MMC3 **修订版 A** 的 IRQ 条件，
而 Mesen 只在数据库说这块板的芯片是 `MMC3A` 时才用它（`MMC3.h:209`）。数据库
（`UI/Dependencies/MesenNesDB.txt`，10,655 条，按 PRG+CHR 的 CRC32 索引）**对 blargg 的 mmc3 ROM
一条都没有**——逐个算 CRC32 核实过——所以 Mesen 会用 rev-B 条件跑 `6-MMC3_alt`，并和本内核一样失败。
诚实的处理是保留这一条并注明原因，而不是加一张 oracle 都没有的逐 ROM 表。

### 10.7 接下来按顺序做什么

1. **DMA 的逐字移植**（§10.2 的 DMA 行）。本轮给原先的诊断补上两点：DMA 周期必须采样中断线，DMC
   取数是按奇偶 3 或 4 个周期。这一刀换回 AccuracyCoin 的 13 项加 `4-irq_and_dma`。
2. **`$4016`/`$4017`**——open bus 掩码、PUT 周期 strobe、同周期重复读——先把 `read_joy3` 加进基线。
   小、独立，且一次换回四项 AccuracyCoin（Open Bus 2、APU Register Activation 2、Controller
   Clocking 2、Controller Strobing 2）。
3. **PPU 的寄存器状态机**（`$2004`/`$2006`/`$2007`、渲染期写入），配 `ppu_read_buffer` 与 `full_palette`。
4. **`$2002` 各标志的生命周期**与逐 dot 的 sprite 求值（含伪溢出计数）——三项 sprite 与 `$2002 flag
   timing` 第 2 项。
5. **APU 的边沿时序**，先把 `len_reload_timing` 放进基线。
6. **混音常数**，用 `apu_mixer` 裁决。
7. **CPU 复位与预热**、NES 2.0 的两个半字节、总线冲突的范围、Dendy。
8. **Mapper**（64/158/185/115/116/176、MMC5 扩展属性、逐 CPU 周期钩子）——长尾，也是唯一以"板数"
   而不是以"行为"计量的部分。
