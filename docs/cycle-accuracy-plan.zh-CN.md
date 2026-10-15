# 中断与总线写入的 cycle 精度：方案

**简体中文** · [English](./cycle-accuracy-plan.md)

这是一份工作计划，不是"已实现行为"的说明：它记录精度欠账里还剩什么、cycle 粒度的工作走到
了哪一步、以及接下来怎么做。模拟器当前是什么样，仍以 `docs/architecture.md` 为准（该文档
§15.1 用摘要形式列了同一批限制）。

## 1. 为什么要做这件事

blargg 的 PPU 与 CPU 套件在 readme 里就把精度写明了："Timing is tested to an accuracy of
one PPU clock"（`ppu_vbl_nmi/readme.txt`）。`docs/architecture.md` §13.1 里仍未通过的项目，
需要的都是同一件事：模拟器要知道某个 CPU 动作落在**哪个 PPU dot** 上。目前它知道落在哪条
*指令*里，经由 L1/L2 又知道了落在哪个 *CPU cycle* 里。

## 2. 目前进度

这些工作已经分步落地。下表记录每一步做了什么、量到了什么；没有任何一步让基线回退：

| 阶段 | 提交 | 做了什么 | 实测效果 |
| --- | --- | --- | --- |
| L1 | `4018bba` | 机器改为逐 CPU cycle 推进：3 个 PPU dot、1 个 APU tick、卡带看到的地址，交错进行而非按指令批量补 | ROM 结果零变化（57 pass / 17 known 逐条一致）；帧耗时 1.075 → 1.046 ms（release，nestest 连续 600 帧） |
| L2 | `49d9d29` | 中断线在每条指令的倒数第二个 cycle 采样；7 cycle 的向量过程（2 次丢弃读、3 次压栈、2 次取向量）逐 cycle 执行 | 基线数量不变，但四个失败项的时序轨迹向期望值移动；AccuracyCoin 78 → 79 通过 |
| L3 第 1 步 | `a2b13e9` | 对 `$2000-$401F` 的写入落在芯片真正驱动的那个 cycle 上（store 是最后一个 cycle，读改写的丢弃写提前一个 cycle），`$2000` 写入抬起的 NMI 也在该 cycle 被接收 | ROM 数量零变化（57 pass / 17 known 与 AccuracyCoin 79/49 逐条一致）；`4-irq_and_dma` 的轨迹从错误形状变成 L3a 那份"与文档期望一致、只差常量偏移"的形状（`53 53 53 0…` → `0 0 0 0 1 1 2 2 4…`，CRC `5FA88FED` → `218E154C`）；`3-nmi_and_irq` 的 CRC 从 `90BC9498` 变为 `7E5D001A`；AccuracyCoin 两项错误码变化（`$0468`/`$0469` 由 0x06 变 0x0A，仍为失败）；帧耗时不变（1.033 → 1.034 ms，release，nestest 连续 600 帧） |
| L3 第 2 步 | `d4bf4ce` | 写入还落在该 cycle 内的一个 **dot** 上：PPU 的逐 dot 视图从该 cycle 的第一个 dot 起就能看到它，cycle 末尾的 APU tick 也看得到 | ROM 基线 57 pass / 17 known → **58 / 16**（`apu_test/5-len_timing`）；AccuracyCoin 79/49 → **81 / 47**（`Frame Counter 4-step`、`Frame Counter 5-step`）；`4-irq_and_dma` 的 CRC 从 `218E154C` 变为 `7DE0C30F`，`5-branch_delays_irq` 的 `BE443B12` → `29BB6E90`，`6-irq_flag_timing` 的子项前进（"Flag first set too late" → "Flag last set too soon"）；原本全绿的没有任何回退；帧耗时不变（1.034 → 1.032 ms，release，nestest 连续 600 帧）；dot 本身不被任何套件区分（0、1、2 结果逐字节相同） |
| L3 第 3a 步 | `32864f1` | 中断序列在压栈之前被 NMI 抬起时，NMI 接管取向量：压栈内容仍属原序列，处理程序换成 NMI 的 | 无一项翻绿、无任何回退（58 pass / 16 known、AccuracyCoin 81/47）；`3-nmi_and_irq` 在 "NMI occurs after CLC, interrupting IRQ" 五行上第二列与 readme 对齐（`23` → `00`），该处第一列由 `27` → `23`（文档值 `20`） |
| 修复：分支的采样 | （本次改动） | 撤掉 L1 之前的"未跨页的已跳转分支跳过那一次采样"锁存：采样本就属于分支自己的倒数第二个 cycle，把它推给下一条指令，等于让"单指令等待循环"永远不采样 | 没有任何套件量过这个锁存——ROM 基线仍是 **62 / 12**、AccuracyCoin 仍是 **82 / 46**，`5-branch_delays_irq` 的 CRC 保持 `29BB6E90` 不变；`cargo test --workspace` 826 → 827；热血格斗（mapper 4）恢复出画，它的 VBlank 等待是 `$F86F: D0 FE`（开着 NMI 的 `BNE self` 循环），此前 60 万条指令里 **0** 次 NMI |
| L3 第 4 步 | （本次改动） | `$2000-$3FFF` 的读落在它自己的 cycle **与 dot** 上——机器在芯片真正做这次访问的那个 cycle 的第二个 dot 之后运行 handler；VBlank 抑制窗口移到标志置位前一个 dot；NMI 线改为每个 CPU cycle 采样一次，而不再在标志抬起的那个 dot 直接置位 | ROM 基线 58 pass / 16 known → **62 / 12**（`05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、`08-nmi_off_timing`）；AccuracyCoin 81/47 → **82 / 46**；`2-nmi_and_brk` 前四行与 readme 一致（`27 26 26 36`），`3-nmi_and_irq` 第二列在前半段与 readme 一致；帧耗时 1.11 → 1.16 ms（release，nestest 连续 600 帧，两者同场测量），几乎全部来自逐 cycle 的采样 |
| Phase 1 | （本次改动） | CPU 不再是指令原子，而是像 Mesen 那样自己驱动自己的周期：每一次访问（`Bus::read_cycle`/`write_cycle`）**本身就是那个周期**，寻址的丢弃读与 handler 的访问都写在芯片真正做它的地方，中断线逐周期采样、判决滞后一个周期 | ROM 基线总数仍是 **62 pass / 12 known**，但构成发生变化：`2-nmi_and_brk` 转绿（向量在压状态字节时选定，因此 NMI 接管了它），`5-len_timing` 则移到失败清单——`$4015` 读现在落在它自己的周期上，它原先吸收的同步误差挪了位置（§4.3 正是这样预测的）；`instr_timing` 2/2 与 `branch_timing` 3/3 保持全绿，这说明周期计数没有走样；AccuracyCoin 82/46 不变；帧耗时 1.204 → 1.204 ms（release，nestest 连续 600 帧，各测 5 次）——寄存写入与事后补周期的机制整体删除，访问驱动的写法自己把开销赚了回来 |
| Phase 0 | （本次改动） | 机器获得 Mesen 的时钟形状：一条主时钟配上 `cpuDivider`/`ppuDivider`（`Region::cpu_divider()`/`ppu_divider()`），一个 cycle 的 dot 既可以走计数循环，也可以走"推进到某个主时钟目标" | ROM 结果零变化（所有套件逐条一致，AccuracyCoin 82/46，`4-irq_and_dma` 的 CRC `7DE0C30F` 不变）；帧耗时 1.206 → 1.210 ms（release，nestest 连续 600 帧，各测 5 次）——最初那版每周期都做时钟算术，代价 1.266 ms，已换成计数式快路径 |

当前状态：`cargo test --workspace` 829 全绿、ROM 基线 **62 pass / 12 known**、
AccuracyCoin **82 通过 / 46 失败**、`STATE_VERSION` 为 6。

### 2.1 Phase 0：后续工作所需的时钟模型

上面这些步骤让*机器*变成了逐 cycle 推进，但 CPU 仍是指令原子、渲染器仍是整行渲染。
两者无论往哪个方向再进一步，都先需要同一件东西：一个两颗芯片都由它推导的时钟——这正是
Mesen 的 NES 内核的骨架（`NesCpu::StartCpuCycle`/`EndCpuCycle` 推进同一条主时钟，并把 PPU
推进到 `_masterClock - _ppuOffset`；NTSC 的 `ppuDivider` 为 4、`cpuDivider` 为 12，PAL 为 5 与 16）。
Phase 0 就是这个结构，且不改变任何行为：

- `Region::cpu_divider()`/`ppu_divider()` 是该比例唯一的落点。NTSC 的 12/4 就是真实
  21.477 MHz 晶振的分频；PAL 暂时建模为 15/5，而不是真机的 16/5，等 PAL 有了可测的 ROM 再切。
- `NesBus::run_ppu_to` 把 PPU 推进到某个主时钟目标，而访问位置改用 *tick* 偏移表达
  （`ACCESS_DOTS * ppu_divider`），不再写成"三步里的第三步"，这样即使一个 cycle 不是恰好三个
  dot，位置依然成立。
- 整除的比例走计数式 dot 循环，而不是每周期都做一遍时钟算术：通用写法实测白吃 4% 帧时间；
  两者由 `test_the_master_clock_path_places_the_same_dots` 钉在一起。另一个测试
  `test_the_whole_ratio_flags_agree_with_the_dividers` 负责在"只改了 PAL 的数值、没同步改
  `has_whole_dot_ratio`"时直接失败。
- 主时钟由周期数推导而非存储，因此存档完全不受影响——这也是本次不需要升 `STATE_VERSION` 的原因。
- 新增 `Nes::cpu_cycles()` / `master_clock()` / `ppu_dots()` / `ppu_position()` / `cpu_pc()`，
  以及 `tests/cycle_trace.rs`：它用这些接口打印逐指令 trace，便于与 ROM readme 里记录的轨迹逐行比对。

### 2.2 Phase 1：CPU 自己驱动周期

已落地。`Cpu::execute_instruction` 不再返回一个"交给机器去补"的周期数：它通过
`Bus::read_cycle` 取操作数，handler 也通过同一个调用读写，芯片用内部周期做的那些丢弃读则
在 `Cpu::fetch_operand` 与需要它们的 handler 里逐条写出（`RTS` 两次、`RTI` 一次、`PLA`/`PLP`
各一次、NOP 的操作数读、分支的两次）。`NesBus::begin_cpu_cycle`/`end_cpu_cycle` 即 Mesen 的
`StartCpuCycle`/`EndCpuCycle`：PPU 先跑到该访问所属的那个时刻，访问发生，随后跑完这个周期剩下的部分。

随之删除的东西：`Access`/`plan_access`/`PendingAccess`、被寄存的寄存器写入及其回放、
`defer_writes`/`defer_register_reads`、机器侧的中断轮询与手工驱动的七周期向量过程，以及
`irq_inhibit`——中断判决滞后一个周期本身就带来了 CLI/SEI/PLP 的延迟，所以那个特例随之消失。
中断锁存改由 CPU 持有，这也正是 Mesen 的归置。

两点值得单独说明：

- **操作码表里的周期数现在只是交叉校验**。没有任何东西去 tick 它；约束它的是测试
  （`test_every_opcode_executes`、`instr_timing`、`branch_timing`）。而这次改造正是被它们驱动的：
  `instr_timing` 直接点名了那些"索引写入却用了读寻址"的操作码（`1E`、`3E`、`5E`、`7E`、`DE`、
  `FE`、`91`、`99`、`9D`、`13`、`33`、`1B`、`3B`、`1F`、`3F`），这正是
  `AddressingMode::AbsoluteXWrite` 这类写寻址存在的理由——store 与读改写总要发那次丢弃读，
  而读只在索引进位时才有。
- **KIL 保持本项目的行为，而不是 Mesen 的**。Mesen 把 KIL 当作不可恢复的崩溃，会清掉中断采样
  让挂死无法被打断；硬件——以及本内核（通过 `Nes::is_cpu_halted` 把挂死上报宿主）——允许中断
  把它重新拉起来。PC 回退是 Mesen 的做法，清采样不是。

### 2.3 奇数帧判定前移到 dot 339（逐 dot 工作的第一步）

Mesen 是在处理 pre-render 的 **dot 339** 时就决定奇数帧要短一拍的——它把计数器直接跳到 340，
于是真正被跳过的是 340——而那次判定读到的是该 dot 时刻的渲染位。内核原先在 340 上判定，
会把恰好落在两者之间的 `$2001` 写入算进去；现在判定前移一个 dot 到 339，用
`skip_odd_frame_dot` 锁存到被跳过的那个 dot（这是状态，因此 `STATE_VERSION` 由 5 升到 6）。

实测：没有任何 ROM 移动（`10-even_odd_timing` 仍报 "skipped too late"，输出逐字节相同），
帧耗时也不变。这个结果本身是有用的：写入在它周期内的 dot 与判定所在的 dot 现在都落在 Mesen
的位置上，因此那一项剩下的部分不是这个边界，而是可见行的取指级行为——也就是逐 dot 渲染器。

接下来渲染器要变成逐 dot 流水线（§6 的决策 2，A12、sprite 0 与奇数帧跳点都在等它），
而 APU 下一个要解决的是帧计数器的相位。

### 2.4 A12 无法单独重建（实验，已撤回）

通往 `mmc3_test_2/4-scanline_timing` 的"更便宜"路线，是保留整行渲染器、只按 dot 重建
*取指地址序列*（就像奇数帧那个 dot 的处理）：Mesen 的 `LoadTileInfo` 在 `cycle % 8 == 1`
读名称表字节、3 读属性字节、5 与 7 读图案字节，而 sprite 阶段在 `(cycle - 257) % 8 == 0`
与 `2` 的垃圾名称表/属性读，正是让 A12 能在两次 sprite 图案取指之间拉低的东西。

实现并实测：`4-scanline_timing` 从第 2 个子项推进到第 4 个子项（说明预渲染行与 sprite 阶段
的行为是对的），但 **`2-details` 回退了**——"Counter should be clocked 241 times in PPU frame"——
因为序列与计数器的过滤是同一套机制：用真实地址时 A12 在两次取指之间只低 4 个 dot，而真正应当
构成每帧唯一一次合格上升的，是跨扫描线边界的那段 9 dot 低电平（dot 337-340 加上下一行的
0-4），每行一次。只把更准的序列喂给过滤器、却不带产生它的取指，这个计数就会跑偏。

因此撤回。这也成了逐 dot 渲染器最具体的论证：地址**和**它落在哪个 dot，必须来自同一个地方，
也就是取指本身。

另有三个实验已经实现、量过、并**整体撤回**；它们的证据正是后文的起点（§4）。
L3a 的总线侧停放后来以"L3 第 1 步"的形式回来了，并且把当初逼它撤回的依赖者一起搬了过去；
第 4 步的读实验则变成了第 4 步本身（它做出来的二分数据正是确定读落在哪个 dot 的依据）。

## 3. 还剩什么

### 3.1 测试 ROM 已经点名的欠账

剩 11 项，且并非同一类问题：

| 套件 | 项目 | 需要什么 |
| --- | --- | --- |
| `cpu_interrupts_v2` | `2-nmi_and_brk`、`3-nmi_and_irq`、`4-irq_and_dma`、`5-branch_delays_irq` | 中断向量过程自身的 cycle——它前后的读/写现在已经各自落在自己的 cycle 上：`4-irq_and_dma` 剩下的偏移与帧计数器相位一致（§4.3），另外三项都只差一到两条指令 |
| `ppu_vbl_nmi` | `10-even_odd_timing` | 奇数帧时钟跳点判定所在的 dot——与 `4-scanline_timing` 一样都在等逐 dot 渲染 |
| `mmc3_test_2` | `4-scanline_timing` | 一帧中首个 A12 上升沿落在哪个 dot——§4，加上已有的逐 dot A12 模型 |
| `ppu_open_bus` | 整个 ROM | PPU 数据总线衰减，**独立特性**，与 cycle 无关 |
| `apu_test` | `4-jitter`、`6-irq_flag_timing`、`7-dmc_basics`、`8-dmc_rates` | 帧计数器与 DMC 时序；`5-len_timing` 其实**不是**长度计数器 bug，而是写入相位问题，第 2 步已修好 |

又有四项离开清单：§4.2 第 1-4 步把寄存器写入与 `$2002` 读放到了各自的 cycle 与 dot 上，
并给 NMI 线加上了采样——这正是 `05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、
`08-nmi_off_timing` 一直在等的。

`4-irq_and_dma` 离通过最近，也是最有用的尺子：§4.2 的第一步（已实现）已经把它的轨迹推到
"与文档期望形状一致、只差一个常量偏移"。

另有两项**根本不是 cycle 问题**，建议单独立项：

- `mmc3_test_2/6-MMC3_alt` 要的是 mapper 4 的 "alternate" IRQ 版本，由 NES 2.0 submapper
  选择，而本项目明确不解析 NES 2.0（见 `docs/architecture.md` §15.6）。要么加一张按 ROM 的
  覆盖表，要么接受它永久失败。
- `ppu_open_bus` 要的是 PPU 数据总线锁存随时间衰减。

### 3.2 MMC5

- **缺**：扩展属性模式（`$5104` = 3）需要单独解析它自己的字节布局；`$5105` 映射、ExRAM
  命名表与 fill 模式已经接通。
- **按决定不做**：硬件竖向分屏（§15.6）。请求分屏的游戏目前只会得到静态分屏。

### 3.3 Mapper

已实现 117 个编号（`supported_mappers()`）。缺的与原因见 §8：`115`（`$6000`/`$6001` 扩展
切换 + 自带 IRQ）、`116`/`176`（行为由 NES 2.0 submapper 决定）、`158`（需要先补 RAMBO-1，
即 mapper 64）、`185`（"CHR 窗口指向命名表 RAM"，需要渲染器向卡带询问图案数据）。

### 3.4 清单上的其它事

§15 里的：存档指纹、音频重采样质量、WASM 画面零拷贝、前端暴露 `RenderTiming`、IPS 补丁、
Zapper 与四人手柄、离散板卡的 bus conflict（2/3/7 等）、FDS 与 NSF。

## 4. 方案

### 4.1 已撤回实验的结论

**L2b——让 NMI 抢占进行中的序列。** 实现了 `poll_hijack` 与"被抢占后改取向量"：`3-nmi_and_irq`
的轨迹进一步向文档期望靠近（CRC `90BC9498` → `14B79C72`），也有单测钉住 B 位行为，但
**没有任何 blargg 项翻绿**，而 AccuracyCoin 掉了一项（79 → 78，三次运行结果一致，不是抖动）。
把窗口从"取向量之前"收窄到"压栈之前"没有区别。先搁置到 4.2 的第 3 步——那时写入 cycle 已经
到位，可以在一个"更多地方是对的"的模型上重新测量那个 AccuracyCoin 项。

**L3a——把寄存器写入推迟到它真实的 cycle。** 把 `$2000-$401F` 的写入停放、在指令最后一个
cycle 落地（读改写的"丢弃写"提前一个 cycle）。结果是把 `4-irq_and_dma` 从完全错误的轨迹
推到了**与文档期望形状一致、只差常量偏移**：

```
期望       0 +0   1 +1   1 +2   2 +3   2 +4   4 +5   4 +6   7 +7 ...
改后 L3a   0 +0   0 +1   0 +2   0 +3   1 +4   1 +5   2 +6   2 +7   4 +8 ...
改前       53 +0 53 +1  53 +2   0 +3   0 +4   0 +5   0 +6   0 +7   1 +8 ...
```

但它打断了四条本来全绿的线：`ppu_vbl_nmi/04-nmi_control`（立即 NMI 的检测仍在指令**开头**，
即写入落地之前）、`mmc3_test_2/3-A12_clocking`（逐 dot 的 A12 模型读 `$2001` 的 mask，而
mask 的生效时间点也跟着移动了），以及 `apu_test/2-len_table`、`oam_stress`；AccuracyCoin
79 → 76。

教训，也正是后续工作的形状：**寄存器写入不能单独搬**，它的依赖者必须一起搬。

下面的第 1 步就是把这条教训用上：保留 L3a 的停放，同时把它当初打断的两个依赖者一起搬
——立即 NMI 的检测，以及后面的 mask 逐 dot 相位。实测：只做停放（第 1 步）既能复现 L3a 的
`4-irq_and_dma` 轨迹，**又**保住了 L3a 当初打断的四条线——基线逐条一致：57 pass /
17 known，AccuracyCoin 79/49。

**L4a——DMA 停顿期间继续轮询中断线。** §4.3 对 `4-irq_and_dma` 的诊断是：剩下的偏移来自
"OAM/DMC DMA 停顿期间的中断轮询"以及"哪一 cycle 把 CPU 交还"。这半句话的两部分都在第 2 步
之上实现了——停顿的每个 cycle 都轮询中断线（于是 DMA 期间抬起的中断被记下，序列在 CPU 被
释放的那一刻开始），以及把 513/514 的奇偶判定挪到写入 cycle 的另一侧——并量了全部四种组合。
`4-irq_and_dma` 的**头部**（readme 记录的 `0 1 1 2 2 4 4 7 7 7 7 8 …` 那一部分）在四种组合下
**逐字节相同**：我们这条（`0 0 0 0 0 1 1 2 2 4 4 7 7 …`，CRC `7DE0C30F`）与 DMA 无关。唯一
会动的是轨迹最后一项：`8` → `7`，而 readme 记录的是 `9`——离正确值更远。其余套件没有任何
变化，也没有任何一项翻绿，因此该改动已撤回（从未提交）。

结论：头部那个滞后来自**中断源**的相位，而不是 CPU 对 DMA 的响应——也就是帧计数器，即下面的
`6-irq_flag_timing` 那一项；同一改动让它停在原地（"Flag last set too soon"）。§4.3 里
`4-irq_and_dma` 那条已按此更正。

**测试 ROM 自带文档。** `roms/blargg/*/readme.txt` 与 `cpu_interrupts_v2/source/` 随下载一起
提供：§3.1 里每个失败项的**期望**轨迹都能和我们的并排读。上文 L3a 那张表就是从某份 readme
来的，上面那组 `4-irq_and_dma` 期望值来自另一份。比较时要对照文档里的轨迹，而不是对照我们
这条的形状。

### 4.2 L3——把寄存器写入放到它的 cycle 上，并带上依赖者

**第 1 步（已完成）：写入落在自己的 cycle，立即 NMI 在该 cycle 检测。**

- 保留 L3a 的总线侧停放（`$2000-$401F` 的写入；store 落在最后一个 cycle，读改写的丢弃写
  落在前一个 cycle；只针对指令自己发出的写入，宿主外部 poke 仍然立即生效）。
  *已完成：`NesBus::write` 负责停放，`NesBus::replay_deferred_writes` 在机器推进指令各
  cycle 时从队尾往前逐个落地；两个槽位足够覆盖全部 opcode，因为只有读改写会写两次。*
- 把"这条指令是否在 VBlank 已置位时打开了 NMI"的检测，从指令的 cycle 之前搬到**写入落地的
  那个 cycle**，并用这个 cycle 重新表达 `nmi_raised_late`。判据就是
  `ppu_vbl_nmi/04-nmi_control`。
  *已完成：那个标记已经删掉。写入落地时接收 PPU 暂存的 NMI（`NesBus::latch_nmi_line`），
  而 store 的写入就是它的最后一个 cycle——比采样点晚一个 cycle——所以"延后一条指令"是 cycle
  顺序自然的结果，不再是人造约束。*
- 验证结果：`04-nmi_control` 保持全绿；`4-irq_and_dma` 的轨迹变成文档期望的形状
  （`0 0 0 0 1 1 2 2 4 …`，CRC `218E154C`，与 L3a 那份逐字一致）；`3-nmi_and_irq` 的轨迹
  移动（`90BC9498` → `7E5D001A`）；其余不动——L3a 当初打断的 `3-A12_clocking`、
  `2-len_table`、`oam_stress` 全部保持全绿。
- 本步留下的尾巴：读改写的丢弃写正落在采样点那个 cycle 上。它写的是**旧值**，所以目前无论
  如何都不会抬起 NMI；到第 2 步给出 dot 偏移，这一点才不再是巧合。

**第 2 步（已完成）：写入落在自己 cycle 内的哪个 dot。**

- `$2001` 目前按 cycle 边界生效，而 A12 模型是逐 dot 读 mask 的（`NesBus::ppu_bus_address`）。
  需要决定并实现"寄存器在写入 cycle 内的哪个 dot 生效"，即给停放中的写入一个 **dot 偏移**，
  而不只是 cycle。*已完成：写入落在该 cycle 的第一个 dot 上，即在该 cycle 的三个 dot 被推进之前落地
  （`NesBus::advance_one_cpu_cycle_with`）。*
- 上面那段预判只对了一半。可见效果确实落在 PPU 的逐 dot 视图上，但真正被 ROM 兑现的是随之
  而来的**顺序**：写入在自己的 cycle 内落地，也就落在该 cycle 末尾的 APU tick **之前**，
  而第 1 步的停放把它放在了 tick 之后。恰好有两类"帧计数器/长度计数器写入"因此从"差一个
  cycle"变成通过：
  - `apu_test/5-len_timing`——上文曾把它归为长度计数器 bug，其实不是。
  - AccuracyCoin 的 `Frame Counter 4-step` 与 `Frame Counter 5-step`（第 1 步已经动过
    错误码的那两项 `$0468`/`$0469`）。
- 验证结果：基线 57 pass / 17 known → **58 / 16**；AccuracyCoin 79/49 → **81 / 47**；
  `3-A12_clocking`、`2-len_table`、`oam_stress`、`04-nmi_control` 全部保持全绿，即 L3a
  当初打断的四条线依旧完好。`4-irq_and_dma` 的 CRC 从 `218E154C` 变为 `7DE0C30F`、
  `5-branch_delays_irq` 的 `BE443B12` → `29BB6E90`；`6-irq_flag_timing` 的子项前进，
  从 "Flag first set too late" 变成 "Flag last set too soon"。
- **哪个 dot 测不出来。** 把写入放在 dot 0、1、2 上，整套 ROM 结果逐字节相同——所有基线、
  轨迹与 AccuracyCoin 结果都一样。套件改变的是写入的 **cycle**，而翻转点如果在 cycle 之间，
  1–2 个 dot 的移动根本碰不到它，所以这里没有任何东西能钉住那个 dot。第 2 步因此只把
  **顺序**定下来，dot 取值留作约定：取 dot 0——"这次访存属于该 cycle，该 cycle 之后做的
  事都看得到它"，这也正是机器目前对"指令各 cycle 之前执行的读"所采用的顺序。等出现真正
  能读到该 dot 的测试再回来定它：`10-even_odd_timing` 与 `4-scanline_timing` 仍因本步没有
  触及的原因失败。
- **放到 cycle 内部的更晚 dot 会直接吃掉帧率。** 要把写入放进 cycle 内部，就得逐个推进该
  cycle 的三个 dot，也就需要在 `advance_one_cpu_cycle` 之外再留一条"逐 dot 走一遍"的路径；
  实测：仅仅是这条路径**存在**，帧耗时就从 1.034 ms 涨到 1.104 ms（release，nestest 连续
  600 帧，约 7%），而 ROM 结果一个都没动。把停放整体关掉仍是 1.11 ms，说明代价来自优化器
  放弃了对热 cycle 循环的优化，而不是写入处理本身。最终落地的形状让循环保持原样——只多一个
  内联小助手里的 `Option`——并把写入放在该 cycle 的第一个 dot 上，这样完全不需要逐 dot 推进。
  （第 4 步把它移到了 cycle 内第二个 dot 之后，即芯片做一次访问的那个点，见下文。）

**第 3 步：在 L3 之上重试 NMI 抢占（L2b）。** 写入与其依赖者都到位后，重新应用那份搁置的
抢占补丁并测量：预期 `3-nmi_and_irq` 从中获益，且 `$0452` 那个 AccuracyCoin 项不再掉。
如果仍然掉，那就要先弄懂它再落地补丁：它属 "Dummy Read Cycles" 一类（用 `Clockslide_29780`
停在一帧边界，然后围绕 `$2002` 读做文章），本身就是一个 dot 时序测试，很可能只是等第 1、2
步做对。

*已完成一半：IRQ 侧已落地。* 序列在走到压栈之前被 NMI 抬起时，NMI 接管取向量
（`Cpu::nmi_hijack`，在序列前四个 cycle 采样）；压栈内容仍属原来那个序列，所以栈上的
状态字 B 位为 0，而压栈之后才到的 NMI 依旧要等下一条指令的采样点。这个窗口的两侧各有一个
单测钉住。实测：**没有任何一项翻绿，也没有任何回退**；`3-nmi_and_irq` 的轨迹在
"NMI occurs after CLC, interrupting IRQ" 那五行上向 readme 靠拢——第二列由 `23` 变成 `00`
（文档值）：

```
readme   23 00  21 00  21 00  20 00  20 00  20 00  20 00  20 00  20 00  25 20  25 20
改前     22 00  22 00  23 00  23 00  27 23  27 23  27 23  27 23  27 23  27 23  27 23
改后     22 00  22 00  23 00  23 00  23 00  23 00  23 00  23 00  23 00  27 23  27 23
```

*仍未完成：BRK 侧。* `2-nmi_and_brk`（以及 AccuracyCoin 的 `$0462` "NMI Overlap BRK"）
需要 BRK 自己那次取向量也能被接管，而 BRK 是在指令内部、指令开头就完成"压栈 + 取向量"的，
所以它的取向量得像第 1 步的寄存器写入那样搬到指令最后几个 cycle——正是第 4 步要解决的
"指令不是原子"那件工作。readme 记录的期望行是 `27 36 00` / `26 36 00` / `26 36 00` /
`36 00 00` ×5 / `27 36 00` ×2。

*3b：已落地，但没有实测支撑。* BRK 现在把取到的向量视为"暂时有效"（`Cpu::brk_vector_pending`，
下一条指令开始时清掉），指令结束前抬起的 NMI 会接管它——只压一帧、B 位保留——而不是让机器再
叠一帧；单测钉住了帧数、B 位与处理程序地址。但在 ROM 上它**不可见**：`2-nmi_and_brk` 的轨迹
与 AccuracyCoin 的 `$0462` 在加与不加时逐字节相同，说明没有任何套件把 NMI 安排在 BRK 自己的
cycle 内。保留它的理由是这个角落符合硬件行为、且没有回退；但 `2-nmi_and_brk` 仍需单独调查：
我们的轨迹（`02 36 00 …`）与 readme 的（`27 36 00 …`）差的不是常量，说明该测试量的是这个窗口
之外的东西——最可能是它自己与 PPU 的同步。

*`2-nmi_and_brk` 调查结论（已读源码 `cpu_interrupts_v2/source/2-nmi_and_brk.s`）。* 它打印三列：
`nmi_flag | 0x02`、`irq_flag`、`X`（`X` 应为 0）。应由 NMI 接管 BRK 取向量那几行的期望是
`36 00 00`——NMI 处理程序运行时栈顶是 BRK 那一帧（`nmi_flag` = 0x34，B 位为 1），而 BRK 处理
程序从未运行（`irq_flag` = 0）。我们**十行全是** `02 36 00`：`nmi_flag` = 0（NMI 处理程序根本
没写过）而 `irq_flag` = 0x36（BRK 处理程序每次都跑了）。十行之间只差 NMI 到达的时刻（相隔一个
CPU clock），所以这与抢占窗口无关：**NMI 从未落进这个十 clock 窗口**。也不是 NMI 没开（`setb`
就是朴素的 `lda #imm` / `sta`，且 `ppu_vbl_nmi/04-nmi_control` 通过，NMI 通路本身没问题）。
对不上的是测试自己的同步：`sync_vbl` 用一个 `$2002` 读循环，跨多帧把读取时刻调到 **1 个 PPU
clock** 的精度（"the loop will effectively read $2002 one PPU clock later each frame"，一帧是
29780.67 个 CPU clock），随后 `delay 29709` 再据此安排 NMI。而我们的读仍发生在指令开头，落在
标志那一个 dot 上的读会在整个 CPU cycle 内赢得这场竞争（`cycle <= 1`，最宽三个 dot），于是这个
循环可能整体偏出一整帧，甚至把它正等的那一帧"吃掉"。

因此 `2-nmi_and_brk`（以及 `3-nmi_and_irq` 剩下的行）属于**第 4 步**的工作，而不是序列侧：在
`$2002` 的读能落在自己那个 dot 之前，中断序列这边做什么都推不动它。它的 readme 自己就提醒过
（"Occasionally fails on NES due to PPU-CPU synchronization"）。

**第 4 步（已完成）：带副作用的读取，以及 NMI 线。** `$2002` 的读会清 VBlank 标志，`$2007`
的读会推进 VRAM 地址；两者过去都发生在指令开头，早三个 cycle，而且完全不看 dot。
`02-vbl_set_time` 靠这种粒度侥幸通过，`06-suppression` 与 `05-nmi_timing` 则不行。这一
步挪了三样东西，每样都单独量过。

*读落在它自己的 cycle 与 dot 上。* 本来最直接的走法是把 `Cpu::execute_instruction` 拆成
"取操作数"与"执行访问"两段；落地的形状更小，而且不需要这个拆分：CPU 仍然一次性完成全部
寻址，只是把落在 PPU 窗口上的读像写入那样交给机器。`Cpu::plan_access` 算出 handler 的访问
属于指令的哪个 cycle——load 是最后一个 cycle（付跨页代价的再晚一个 cycle），读改写模式的
读是倒数第三个 cycle，这就是 `Access` 按 opcode 助记符编码出来的东西——然后由
`PendingAccess` 把 handler 交出去；机器在该 cycle 的第二个 dot 之后运行它
（`NesBus::advance_one_cpu_cycle_with_read`）。

*"第二个 dot"是量出来的，不是选的。* 第 4 步的第一次尝试——只在读自己的 cycle 做**清除**，
而值仍留在指令开头返回——让 `02-vbl_set_time` 量测的那一对读的清除晚了一个 cycle
（`00 - -`，而 readme 是 `00 - V`），因此撤回；它也证明了读的**值**必须和清除一起搬，
再怎么重排清除的位置都做不到。改为二分读落在哪个 dot，用 `05-nmi_timing`、
`06-suppression` 与 `02-vbl_set_time` 当尺子，得到的是一条干净的"每 dot 一行"梯度：
放在该 cycle 开头时 NMI 边界早两行，放在末尾时晚三行，中间那个 dot 正好。同一个窗口也
就是 VBlank 抑制竞争最终的样子：宽度一个 dot——读停在 scanline 241 的 dot 0 上，它的下一个
dot 正是标志置位那个 dot。

*NMI 线每个 cycle 采样一次。* `NesBus::nmi_pending` 过去在标志抬起的那个 dot 直接置位，
这使 `06-suppression` 的 `05`/`06` 两行点燃了硬件不会有的 NMI（在同一个 cycle 里清掉标志
的那次读收不回这条线）。现在它是由"每个 CPU cycle 末尾一次采样"留下的锁存：一个 cycle
内起落的电平永远不会被看到，而被看到的电平会一直锁存到 CPU 取走这次中断。这正是
`06-suppression` 里 `05 V -` 与 `07 V N` 的区别，也是 `07-nmi_on_timing` 第 02-04 行能
保住 NMI 的原因——`$2000` 写入在某个 cycle 里抬起了线，而那一个 cycle 末尾的采样看到了
它；之后同一条指令里那次清掉标志的 `$2002` 读收不回这个锁存。

*窗口只覆盖 PPU。* 被延后的是 `$2000-$3FFF`；APU 的 `$4015` 暂时不在内：blargg 的
`apu_test` 用它做同步，而它测的帧计数器相位本来还差一两个 cycle（§4.3），现在挪这次读
只会把误差挪个地方。这是本步唯一留待回头处理的部分。

实测：ROM 基线 58 pass / 16 known → **62 / 12**（`05-nmi_timing`、`06-suppression`、
`07-nmi_on_timing`、`08-nmi_off_timing`——blargg 的 `ppu_vbl_nmi` 现在 9/10，只剩奇数帧
跳点）；AccuracyCoin 81 通过 / 47 失败 → **82 / 46**；原本全绿的没有任何回退，
`02-vbl_set_time`、`04-nmi_control`、`3-A12_clocking`、`5-len_timing` 与 `oam_stress` 都
在内。`2-nmi_and_brk` 的前四行已经与 readme 一致（`27 26 26 36`，文档是
`27 26 26 36 00 …`），`3-nmi_and_irq` 第二列在前半段与 readme 的 `00` 一致；两者距离完整
表格还差的部分，是第 3 步的接管逻辑遇上测试自身的逐帧漂移，而不是读的摆放位置能够到的。
代价：逐 cycle 采样约占帧耗时的 4%（1.11 → 1.16 ms/帧，两者在同一场测量，release、
nestest 连续 600 帧）；摆放读本身是免费的，而把**写**从该 cycle 的第一个 dot 移到与读相同
的 dot 也没有可测量的代价——这只是让两次访问在同一个点上，`5-len_timing` 与 AccuracyCoin
的帧计数器测试依旧全绿。

### 4.3 L4——DMA 与渲染器（在 L3 之后）

- **`4-irq_and_dma`**：*这里原本的猜测——"停顿期间继续轮询"与"哪一 cycle 把 CPU 交还"——已经
  实现并量过，两部分都推不动文档记录的那条轨迹头部（见 §4.1 的 L4a）。* 这个滞后跟随的是中断
  源的相位，所以本项应归到下面的帧计数器一起做。
- **`7-dmc_basics`、`8-dmc_rates`、`4-jitter`、`6-irq_flag_timing`**：DMC 取字节时序，以及
  帧计数器相对 CPU cycle 的精确相位——`4-irq_and_dma` 剩下的偏移也在这个相位里。

  **帧计数器相位：已量过，未落地。** `$4017` 写入会重启计数器，而"重启落在什么时刻"决定
  了其它一切。为此一起实现并测量了三块：
  - 重启是延迟的：写入的模式与 IRQ inhibit 立即生效，但计数器要三个 CPU cycle 之后才真正
    经过它的边界——若写入落在 APU 时钟的另一半则是四个，这正是 `4-jitter` 两个方向都测的
    那个自由运行的二分频相位；
  - IRQ 标志在"要求它的那一步"之后三个 cycle 才置起，而序列在最后两个 tick 上会再要求
    一次——即 `6-irq_flag_timing` 所说的"连续置起三次"；
  - `$4015` 读像 PPU 的读一样摆放（落在自己 cycle 的第二个 dot 之后），而不是在指令开头；
    同一个测试的 `sync_apu` 必须这样才能看到相位。

  实测：`4-jitter` 与 `6-irq_flag_timing` 翻绿（"Frame irq is set too soon" → Passed、
  "Flag first set too soon" → Passed），`5-len_timing` 保持通过——但 AccuracyCoin 掉三项：
  `Frame Counter 4-step`/`Frame Counter 5-step`（`$0468`/`$0469`）重新失败，`$0460`/`$0461`
  的错误码也移动（0x06 → 0x0A、0x22 → 0x06）。按本方案的规矩，两项换三项算回退，因此撤回、
  没有落地。APU 侧的读摆放本身是无害的（加上它 AccuracyCoin 仍是 82/46），所以分歧在计数器
  相位上——两个套件里必有一个在测量另一个事件，先弄清是哪一个，是这条欠账下一步要做的事。
- **sprite 0 命中精确到 dot**：渲染器整行一次画完，命中在渲染点判定而非命中像素处。要修就得
  改逐 dot 渲染——与当初拒绝 MMC5 分屏是同一套机制，因此应当**一次性明确决定**。
- **PAL** 的 3.2:1 PPU:CPU 比例需要小数 dot 累加器。

### 4.4 顺序与每步的预期收益

1. L3 第 1 步——**已完成**：保住了 `04-nmi_control`，并把 `4-irq_and_dma` 推到文档期望的
   形状；ROM 数量如预期不变。L3a 留下的那个常量偏移本步没有改变；下一步的怀疑对象是 §4.3
   （L4）里 DMA 侧的采样。
2. L3 第 2 步——**已完成**，而且最初瞄错了目标：用现有 ROM 测不出"写入落在哪个 dot"，但把
   写入搬进 cycle 内部同时也把它搬到了该 cycle 的 APU tick 之前，于是修好了 `5-len_timing`
   与两项 AccuracyCoin 帧计数器测试。`10-even_odd_timing` 与 `4-scanline_timing` 完全没动：
   第 1 步已经把 `10-even_odd_timing` 从 "skipped too late" 变成 "skipped too soon"，而 dot
   并不能推动它；因此这两项现在更像是等 §6 决策 2 所说的逐 dot PPU 工作，而不是等写入位置的
   调整。
3. L3 第 3 步——**IRQ 侧与 BRK 取向量已完成，读侧原本卡住的部分由第 4 步解开**：抢占本身
   已在 3a/3b 落地，而 `2-nmi_and_brk` 与 `3-nmi_and_irq` 仍失败的那些行，其实是测试自己的
   `$2002` 同步问题，也就是第 4 步。
4. L3 第 4 步——**已完成**：四个 `ppu_vbl_nmi` 项全部翻绿。第 3 步等的两个
   `cpu_interrupts_v2` 项也随之前进（见第 4 步的实测），但还没到；它们剩下的是中断向量过程
   自身的 cycle，而不是读。
5. L4——DMA/帧计数器那一组，然后是 sprite 0 时序。

每一步一个提交，且只有"ROM 基线不回退"才保留：套件本身带有"已知不通过"清单，并**双向断言**
（`tests/rom_tests.rs`）；AccuracyCoin 带失败数 ratchet（当前 46）。

## 5. 验证流程（前面每个阶段都在用）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p nes-core --test rom_tests -- --nocapture   # 逐个 ROM 的 pass/known 行
```

- ROM 不入库：把 blargg 各套件与 AccuracyCoin 放到 `roms/` 下（布局与下载来源见 §13.1）。
- 基线清单里"开始通过"的项必须从 `tests/rom_tests.rs` 删掉；清单之外出现失败会直接红。
- 性能：用 release 模式跑帧耗时前后对比（nestest 连续 600 帧，由临时测试打印）——L1 的净效果
  是略微**变快**；L3 各步才是第一批预计会有开销的改动。现在直接用
  `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture`，跑几次取中位数。
  同一形式下 `tests/cycle_trace.rs` 打印逐指令 trace，用于与 ROM readme 记录的轨迹逐行比对。
- `docs/architecture.md` 与其 `zh-CN` 对照必须同步更新；每个阶段的实测数字要写进提交信息——
  本文档里的数字都来自那些提交信息。

## 6. 新项目需要拍板的决策

1. **L3 做到哪一步**：只做第 1 步（写入 cycle，成本低、预计不带来 ROM 收益），还是第 1–4 步
   （dot 精度那一组，多个提交）。第 1 步是所有后续的前提，无论如何都是安全的第一提交。
2. **sprite 0 命中 / PAL / MMC5 分屏**：三者都需要逐 dot 渲染。一次性决定它是否在范围内，
   而不是分三次讨论。
3. **`6-MMC3_alt`**：加一张按 ROM 的小覆盖表来给"alternate IRQ 版本"，还是把它作为有据可查
   的永久失败（NES 2.0 的决定挡住了正规修法）。
4. **Mapper**：是否补 mapper 64（RAMBO-1）以便派生 158；185 的"图案取自 VRAM"窗口是否值得
   那个渲染器钩子。
5. **非 cycle 的工作**（§3.4）与以上完全独立，可以穿插做：性价比最高的是 IPS 补丁、离散板卡
   的 bus conflict、以及存档指纹。
