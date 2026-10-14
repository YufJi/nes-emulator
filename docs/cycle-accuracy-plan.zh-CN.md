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

两个阶段已完成，各自一个提交，且都没有回退：

| 阶段 | 提交 | 做了什么 | 实测效果 |
| --- | --- | --- | --- |
| L1 | `4018bba` | 机器改为逐 CPU cycle 推进：3 个 PPU dot、1 个 APU tick、卡带看到的地址，交错进行而非按指令批量补 | ROM 结果零变化（57 pass / 17 known 逐条一致）；帧耗时 1.075 → 1.046 ms（release，nestest 连续 600 帧） |
| L2 | `49d9d29` | 中断线在每条指令的倒数第二个 cycle 采样；7 cycle 的向量过程（2 次丢弃读、3 次压栈、2 次取向量）逐 cycle 执行 | 基线数量不变，但四个失败项的时序轨迹向期望值移动；AccuracyCoin 78 → 79 通过 |

当前状态：`cargo test --workspace` 814 全绿、ROM 基线 **57 pass / 17 known**、
AccuracyCoin **79 通过 / 49 失败**、`STATE_VERSION` 为 3。

另有两个实验已经实现、量过、并**整体撤回**；它们的证据正是后文的起点（§4）。

## 3. 还剩什么

### 3.1 测试 ROM 已经点名的欠账

剩 16 项，且并非同一类问题：

| 套件 | 项目 | 需要什么 |
| --- | --- | --- |
| `cpu_interrupts_v2` | `2-nmi_and_brk`、`3-nmi_and_irq`、`4-irq_and_dma`、`5-branch_delays_irq` | NMI 能抢占进行中的中断序列，以及处理程序开始时的精确 CPU cycle——即 §4 的工作 |
| `ppu_vbl_nmi` | `05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、`08-nmi_off_timing`、`10-even_odd_timing` | `$2002` 读与 `$2000`/`$2001` 写到 **1 个 PPU dot** 的精度——§4 的工作 |
| `mmc3_test_2` | `4-scanline_timing` | 一帧中首个 A12 上升沿落在哪个 dot——§4，加上已有的逐 dot A12 模型 |
| `ppu_open_bus` | 整个 ROM | PPU 数据总线衰减，**独立特性**，与 cycle 无关 |
| `apu_test` | `4-jitter`、`5-len_timing`、`6-irq_flag_timing`、`7-dmc_basics`、`8-dmc_rates` | 帧计数器与 DMC 时序；`5-len_timing` 是长度计数器 bug，其余属 cycle/DMA 工作 |

`4-irq_and_dma` 离通过最近，也是最有用的尺子：§4.2 的第一步已经把它的轨迹推到"与文档期望
形状一致、只差一个常量偏移"。

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

### 4.1 两个已撤回实验的结论

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

### 4.2 L3——把寄存器写入放到它的 cycle 上，并带上依赖者

**第 1 步：写入落在自己的 cycle，立即 NMI 在该 cycle 检测。**

- 保留 L3a 的总线侧停放（`$2000-$401F` 的写入；store 落在最后一个 cycle，读改写的丢弃写
  落在前一个 cycle；只针对指令自己发出的写入，宿主外部 poke 仍然立即生效）。
- 把"这条指令是否在 VBlank 已置位时打开了 NMI"的检测，从指令的 cycle 之前搬到**写入落地的
  那个 cycle**，并用这个 cycle 重新表达 `nmi_raised_late`。判据就是
  `ppu_vbl_nmi/04-nmi_control`。
- 验证：`04-nmi_control` 保持全绿；`4-irq_and_dma` 的轨迹保持形状；其余不动。

**第 2 步：mask 的逐 dot 相位。**

- `$2001` 目前按 cycle 边界生效，而 A12 模型是逐 dot 读 mask 的（`NesBus::ppu_bus_address`）。
  需要决定并实现"寄存器在写入 cycle 内的哪个 dot 生效"，即给停放中的写入一个 **dot 偏移**，
  而不只是 cycle。
- 这决定 `mmc3_test_2/3-A12_clocking` 与 `10-even_odd_timing`。
- 验证用 `mmc3_test_2` 与 `ppu_vbl_nmi`，并复查 `apu_test/2-len_table` 与 `oam_stress`
  （它们在 L3a 实验里因同一原因回退）。

**第 3 步：在 L3 之上重试 NMI 抢占（L2b）。** 写入与其依赖者都到位后，重新应用那份搁置的
抢占补丁并测量：预期 `3-nmi_and_irq` 从中获益，且 `$0452` 那个 AccuracyCoin 项不再掉。
如果仍然掉，那就要先弄懂它再落地补丁：它属 "Dummy Read Cycles" 一类（用 `Clockslide_29780`
停在一帧边界，然后围绕 `$2002` 读做文章），本身就是一个 dot 时序测试，很可能只是等第 1、2
步做对。

**第 4 步：带副作用的读取。** `$2002` 的读会清 VBlank 标志，`$2007` 的读会推进 VRAM 地址，
两者目前都发生在指令开头。`02-vbl_set_time` 现在靠粒度侥幸通过，而 `06-suppression` 与
`05-nmi_timing` 不行。要把读取放到它真实的 cycle，需要把寻址阶段与数据访问拆到不同 cycle，
也就是把 `Cpu::execute_instruction` 拆成"取操作数"与"执行访问与操作"两段，中间由机器推进
寻址 cycle。可以先只覆盖能碰 `$2000-$3FFF` 与 `$4015-$4017` 的指令。

### 4.3 L4——DMA 与渲染器（在 L3 之后）

- **`4-irq_and_dma`**：OAM/DMC DMA 停顿期间的中断轮询。停顿本身已经是 cycle 计数，还差
  "停顿期间继续轮询"与"哪一 cycle 把 CPU 交还"。
- **`7-dmc_basics`、`8-dmc_rates`、`4-jitter`、`6-irq_flag_timing`**：DMC 取字节时序，以及
  帧计数器相对 CPU cycle 的精确相位。
- **sprite 0 命中精确到 dot**：渲染器整行一次画完，命中在渲染点判定而非命中像素处。要修就得
  改逐 dot 渲染——与当初拒绝 MMC5 分屏是同一套机制，因此应当**一次性明确决定**。
- **PAL** 的 3.2:1 PPU:CPU 比例需要小数 dot 累加器。

### 4.4 顺序与每步的预期收益

1. L3 第 1 步——保住 `04-nmi_control`、缩小 `4-irq_and_dma` 的偏移；预计 ROM 数量不变。
2. L3 第 2 步——`3-A12_clocking`（现已全绿）必须存活，应当移动的是 `10-even_odd_timing`
   与 `4-scanline_timing`。
3. L3 第 3 步——`2-nmi_and_brk`、`3-nmi_and_irq`。
4. L3 第 4 步——`05-nmi_timing`、`06-suppression`、`07-nmi_on_timing`、`08-nmi_off_timing`。
5. L4——DMA/帧计数器那一组，然后是 sprite 0 时序。

每一步一个提交，且只有"ROM 基线不回退"才保留：套件本身带有"已知不通过"清单，并**双向断言**
（`tests/rom_tests.rs`）；AccuracyCoin 带失败数 ratchet（当前 49）。

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
  是略微**变快**；L3 各步才是第一批预计会有开销的改动。
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
