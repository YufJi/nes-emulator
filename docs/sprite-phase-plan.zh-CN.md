# sprite 阶段与 `$2002` 的 sprite 标志：怎么落地

**简体中文** · [English](./sprite-phase-plan.md)

这是对齐文档里 **C8**（§2.1、§9）与 **§10.7 第 4 条**那两个事项的执行计划：PPU 的 sprite
**求值**仍是逐行的，而 `$2002` 的两个 sprite 标志还没有 Mesen 的生命周期。请先读对齐文档的
§2.1、§7 与 §9——那里有两次尝试、两次撤回，以及把问题收窄到今天的实测。本文件只补三件事：做事的
顺序、每一步可以被判定的门、以及用来量它的工具。

对齐文档的两条规则不变（以 Mesen 源码为准；ROM 结果是证据，不是约束）。这一轮再补三条，它们都是
从上两次尝试的收场方式里来的：

3. **不许只用 AccuracyCoin 的失败数来判定一步。** 那个数是滞后指标，在这种规模的移植落地期间它
   会两头跑（两次尝试：111/17 → 119/22 → 116/27）。领先指标是**每个失败项停在第几项检查**，以及
   那些从不写 `$6000` 的套件在屏幕上的判决。
4. **任何一步都不整块回撤。** 出现回退就用下面的工具找第一处分歧，然后修掉、收窄，或连同证据记成
   有意偏差。走不到自己那道门的一步就**拆开**：站得住的部分提交，剩下的另立一步。
5. **这一轮里别的东西不许动。** `ACCESS_DOTS`、寄存器状态机的落点、DMA 状态机、APU 在 CPU 周期里
   的位置都已经被实测并用测试钉住；需要改其中任何一个的，是另一条工作线。

## 1. 这里的"对齐"指的是什么

### 1.1 树里今天的形状

- `NesBus::ppu_step`（`crates/nes-core/src/nes.rs`）在**每一行**都驱动 sprite 那一半，既没有渲染
  门，也没有"预渲染行除外"的门；`SpriteEvaluator::evaluate`
  （`crates/nes-core/src/ppu/sprite.rs`）**在 dot 65 一次调用**填满下一行的二级 OAM，并在那里当场
  升起溢出标志。
- 像素流水线（`crates/nes-core/src/ppu/pipeline.rs`）在 dot 257 锁存八个槽位、在 dot 261 与 263
  读两个图案平面（`(cycle - 257) % 8 == 4 / 6`），每个 sprite 像素都按槽位的 X 区间与取到的字节
  **重算**出来。
- Mesen 有三次总线访问在这里完全没有：dot **337** 与 dot **339** 的 nametable 读，以及 dot **320**
  的 `LoadExtraSprites`。
- 渲染期间的 `$2004` 就是普通的 OAM 读写加无条件自增。

### 1.2 要实现的逐 dot 时刻表（MesenCE `NesPpu::ProcessScanlineImpl`）

下面行号即 dot。除表里另行说明，"渲染"指的是 `_prevRenderingEnabled`——上一个 dot 末尾的掩码状态。

| Dots | MesenCE | 树里今天 |
| --- | --- | --- |
| 0 | `ProcessScanlineFirstCycle`：走行；落在 240 行以下（`_scanline < 240`）时对 `_spriteShifterList[0..8]` 排序、把扫描位置重置到队首；落到预渲染行时清掉 `SpriteOverflow` 与 `Sprite0Hit`，并在渲染开启时做 `CorruptOamRow(_spriteRamAddr >> 3, _secondaryOamAddr & 0x1F)`（`DisableOamAddrBug` 默认为 false）；落到 240 行时 `SendFrame()` | 无（没有排序、没有步进状态、没有行拷贝） |
| 1-64 | 清二级 OAM：`_oamCopybuffer = $FF`、`_secondarySpriteRam[_secondaryOamAddr & 0x1F] = $FF`、`_secondaryOamAddr += !(_cycle & 1)` | 无 |
| 65 | `ProcessSpriteEvaluationStart`：`_sprite0Added`/`_spriteInRange` 置假、`_secondaryOamAddr = 0`、`_overflowBugCounter = 0`、`_oamCopyDone` 置假、`_firstVisibleSpriteAddr = _spriteRamAddr & $FC` | 一次性 `evaluate()` 从这里开始 |
| 奇数 65-255 | `_oamCopybuffer = ReadSpriteRam(_spriteRamAddr)` | —（都在 `evaluate` 里） |
| 偶数 66-256 | 判决：先做范围判断（`_scanline >= buf && _scanline < buf + height`），再进"有空位"分支（把字节拷进二级 OAM，含错位重对齐规则）或溢出分支（对角步进、`_overflowBugCounter`）；把 `_spriteRamAddr` 写回 | —（都在 `evaluate` 里；标志是在**扫描找到**第九个时升起，而不是在判决它的那个 dot 上） |
| 1-256 | `DrawPixel()`（背景来自移位寄存器、sprite 来自移位器表、sprite 0 命中就在其中）、`ShiftTileRegisters()`、`ProcessSpriteShifters()` | 画像素；没有移位器表 |
| `(cycle & 7) == 0` | `IncHorizontalScrolling()`；256 时再 `IncVerticalScrolling()` | 有 |
| 257 | `_spriteIndex = 0`、`_spriteCount = 0`；从 `t` 拷横向滚动；`_secondaryOamAddr = 0` | 只做了一部分（滚动） |
| 257-320 每个 dot | `_sprite0Visible = _sprite0Added`；`_spriteRamAddr = 0`；`_secondaryOamAddr += !((_cycle - 1) & 4)` | 三条都没有 |
| `(cycle - 257) % 8 == 0` 与 `== 2` | 两次被丢掉的 nametable 读，但它们照样到卡带（MMC3 看的就是这些） | 有 |
| `(cycle - 257) % 8 == 4`（261、269、… 317） | `LoadSpriteTileInfo`：`_spriteIndex = (_cycle - 257) >> 3`，从二级 OAM 读那四个字节，`LoadSprite` 在**这一个 dot 里**读**两个**图案平面；范围判断是 `scanline8Bit - spriteY`，预渲染行上 `scanline8Bit = 261`，再由属性 bit 7 取反；超出范围 → 置零并标 `SpriteShifterDone`，在范围内 → `_spriteShifterList[i] = ((spriteX + 1) << 4) \| i`、`_spriteCount++`、`_processSprites = true` | 261+8k 读 low、263+8k 读 high；范围判断早已被 `evaluate()` 定死 |
| 280-304（仅预渲染） | 从 `t` 拷纵向滚动 | 有 |
| 320 | `LoadExtraSprites()` | 缺（`RemoveSpriteLimit = false` 下是空操作，但属于这张时刻表） |
| 321-336 | 321 时 `_secondaryOamAddr++`；328 与 336 时 `IncHorizontalScrolling()`；`ShiftTileRegisters()`；`LoadTileInfo()` | 后两条有 |
| 337 | `if (IsRenderingEnabled()) _tile.TileAddr = ReadVram(GetNameTableAddr())`——读的是**实时**掩码，不是慢一个 dot 的状态 | 缺 |
| 339 | `_activeSpriteShifters = 0`；渲染时同上做那次 nametable 读并写进 `_tile.TileAddr`，同时把每个既未完成、也未过期的移位器转入 **counting** 模式；奇数 NTSC 2C02 预渲染帧上 `_cycle = 340`、`_dotSkipped = 3`、每个移位器 `+= 1 << 4`、`_nextSpriteShifterCycle++`；随后所有不在 counting 模式的移位器转 **active**；`UpdateProcessSpritesFlag()` | 只有奇数帧跳点本身 |

移位器自己的生命周期（上表一直在引用它）：

- 被装载的移位器是 `((spriteX + 1) << 4) | index`——也就是它被**激活**的那个 dot，正好是 sprite
  最左像素的后一个；
- `ProcessSpriteShifters` 按 X 顺序走这张表，在各自的那个 dot 上把移位器从 counting 变成 active，
  并标记为 expired；
- 新行 dot 0 上表会重新排序、扫描位置回到队首；dot 339 上没轮到的转入 counting 模式，其余被激活，
  于是它们的像素从 X 0 开始；
- 像素取自移位后的字节，sprite 0 命中在像素输出的地方升起（`_cycle != 256`、两个使能都开、两个平面
  都没被裁掉）。

### 1.3 `$2002`

| | MesenCE | 树里今天 |
| --- | --- | --- |
| 一次读 | bit 7/6/5 来自标志；`UpdateStatusFlag()` **只**清 VBlank 与 CPU 的 NMI 锁存；bit 4-0 来自 PPU 自己的 open bus；在 `_scanline == _nmiScanline && _cycle == 0` 上读会置 `_preventVblFlag` | 已对齐（C3） |
| 两个 sprite 标志何时**被清** | `ProcessScanlineFirstCycle`，即预渲染行的 dot 0（Mesen）；AccuracyCoin 自己的注释写的是 dot 1，而本内核在 dot 1 清并**通过了**量它的那项检查 | dot 1，与 VBlank 一起 |
| 何时**被置** | `SpriteOverflow`：在步进判决第九个范围内的 sprite 的那个偶数 dot 上；`Sprite0Hit`：在 `GetPixelColor` 里，像素输出的那个 dot 上 | 溢出固定在第 65 个 dot，无论渲染是否开启、也无论第九个 sprite 离那个 dot 多远 |
| 读的延迟 | AccuracyCoin 的 `$048D` 注释：G 版 CPU 上 VBlank 位在 M2 升起时锁存，两个 sprite 位要晚约 1.875 个 PPU dot，在 M2 落下时 | 整个寄存器在一个瞬间采样（`ACCESS_DOTS = 2`，对齐文档 §4） |

### 1.4 决定"什么不要做"的默认值

来自 `NesConfig` 的默认值（对齐文档 §10.1）：`RemoveSpriteLimit = false`（没有
`_spriteTiles[8..64]`、不画额外的 sprite）、`EnablePpuSpriteEvalBug = false`（所以 `_oamCopyDone`
那个重读分支**是**要走的）、`EnablePpuOamRowCorruption = false`（`$2003` 与状态变化不引发行损坏）、
`DisableOamAddrBug = false`（所以预渲染的 `CorruptOamRow` **是要做**的）、
`ForceBackgroundFirstColumn`/`ForceSpritesFirstColumn = false`、
`RestrictPpuAccessOnFirstFrame = false`。这些都不是可以新加的旋钮。

## 2. 基线：2026-10-16 实测（HEAD `8ed8713`）

**AccuracyCoin。** `NES_ACCURACYCOIN=1 cargo test --release -p nes-core --test cycle_trace --
--ignored --nocapture` → 跑了 146 项、通过 111、失败 17，用时 **7.4 s**。同一次跑在 debug 下是
63 s，所以这条工作线的循环用 `--release`。

**ROM 套件。** 它当时不在树里（b4caaf6；快照在 `/tmp/nes-baseline-snapshot-20261016`）。在 HEAD 上
放回去，它几乎全线红，而**不是**内核回退了：

- 金标准早于它们被录制之后落地的每一次渲染改动（B1 的 sprite 高平面、B2 的属性粗 X、逐 dot
  流水线、寄存器状态机、强制消隐的调色板路径），所以一个 `$6000` 写着 "Passed" 的 ROM 照样会被
  报成 "frames moved from the goldens"；
- §10.6 新列的那 37 张 ROM 根本没有金标准，因此天然算失败。

所以凡以画面判决的 ROM 都先按它自己的屏幕读了一遍（导一帧、人眼读），之后才重录金标准——具体做了
什么见下面的第 0 步。这些判决就是本计划据以书写的数字：

| 套件 | HEAD 上的判决，以及它是怎么读出来的 |
| --- | --- |
| `blargg/instr_test-v5`（16） | 16/16，走 `$6000` |
| `blargg/instr_timing`（2） | 2/2，走 `$6000` |
| `blargg/branch_timing_tests`（3） | 3/3，按画面：`PASSED` |
| `blargg/cpu_interrupts_v2`（5） | 5/5，走 `$6000` |
| `blargg/ppu_vbl_nmi`（10） | 10/10，走 `$6000` |
| `blargg_ppu_tests`（5） | 4/5，按画面：`palette_ram`、`sprite_ram`、`vbl_clear_time`、`vram_access` 打 `$01`；`power_up_palette` 打 `$02`——上电调色板，§4 记作有意偏差 |
| `blargg/oam_read`（1） | 1/1，走 `$6000` |
| `blargg/oam_stress`（1） | 1/1，走 `$6000`（它要约 1700 帧） |
| `blargg/ppu_open_bus`（1） | 1/1，走 `$6000` |
| `blargg/sprite_hit_tests`（11） | **11/11**，按画面：`PASSED` |
| `blargg/sprite_overflow_tests`（5） | 第 0 步时按画面 2/5（`1.Basics` `FAILED: #6`、`3.Timing` `FAILED: #4`、`5.Emulator` `FAILED: #3`）；**S1 之后是 4/5**，见下面那一步 |
| `blargg/mmc3_test_2`（6） | 5/6，走 `$6000`；`6-MMC3_alt` 要 MMC3 修订版 A，Mesen 是从数据库选的，因此它同样失败（§10.6） |
| `blargg/apu_test`（8） | 8/8，走 `$6000` |
| `nes-test-roms/blargg_apu_2005.07.30`（11） | 9/11，按画面：九张打 `$01`；`10.len_halt_timing` 打 `$03`、`11.len_reload_timing` 打 `$04`——长度计数器的 halt/reload 时序（§10.3 第 8 条） |
| `nes-test-roms/cpu_dummy_reads`（1） | 1/1，按画面：`Passed`（要 >600 帧才到那里） |
| `nes-test-roms/cpu_dummy_writes`（2） | 2/2，走 `$6000`（同样要 >600 帧） |
| `nes-test-roms/cpu_exec_space`（2） | `ppuio` 1/1，走 `$6000`；`apu` 停在 `$6000 = 2`，即 APU 页的 open bus（§10.3 第 4 条） |
| `nes-test-roms/cpu_reset`（2） | 0/2：两张都打印出它们要的值，然后等**人**按复位 |
| `nes-test-roms/dmc_dma_during_read4`（5） | 1/5，按画面：`dma_2007_write` `Passed`；`dma_4016_read` 与 `read_write_2007` `Failed`；`dma_2007_read` 与 `double_2007_read` 打出的校验和差一个周期 |
| `nes-test-roms/dmc_tests`（4） | 0/4：四张都在等 DMC，永远等下去（第 9000 帧时 PC 卡在一个小循环里） |
| `nes-test-roms/full_palette`（3） | 只判画面，而且没有静止画面——它们是动画（`jmp begin_frame`），所以逐帧金标准就是全部 |
| `nes-test-roms/ppu_read_buffer`（1） | 1/1，按画面：`Passed` |
| `nes-test-roms/read_joy3`（4） | 0/4：四张都在等脚本化输入 |
| `nes-test-roms/sprdma_and_dmc_dma`（2） | 0/2：两张都打 `Failed`（sprite/DMC 重叠相位） |
| AccuracyCoin | 跑 146、过 111、失败 17 |

这三项 `sprite_overflow` 的失败要说清楚，因为对齐文档 §9 把这套记为 5/5：那个数字来自一份在录制
时画面里就已经带着这些失败的构建，也就是说这套从那轮对齐之前起就一直在失败。三项都在本工作线里，
而且是手上最锋利的工具：`1.Basics #6` 是"全部渲染关闭时不得置位"（它写 `$2001 = 0`）、
`3.Timing #4` 是"第一行上置得太早"（ROM 用连续两次 `$2002` 读把那个 dot 夹住，容差一两个 CPU
时钟）、`5.Emulator #3` 是"关闭渲染没有重算标志的时间"。三条失败同一个原因：`ppu_step` 在每一行都
驱动步进、没有渲染门，而 `evaluate()` 把标志固定在 dot 65 升起。

### 目标与守门项

| 测试 | HEAD 上 | 在这里的角色 |
| --- | --- | --- |
| `sprite_overflow_tests/1.Basics` | FAILED #6 | 目标：S1 让 #6 过去，#7/#8 归 S2 |
| `sprite_overflow_tests/3.Timing` | FAILED #4 | 目标：S2——标志必须出现在步进判决第九个 sprite 的那个 dot 上 |
| `sprite_overflow_tests/5.Emulator` | FAILED #3 | 目标：S2 |
| `sprite_overflow_tests/2.Details`、`4.Obscure` | PASSED | 守门（标志寿命与溢出 bug 的对角线） |
| `sprite_hit_tests`（11） | PASSED | 守门：S3 是唯一可能弄坏它们的一步 |
| AccuracyCoin `$0458` | 第 3 项 | 目标：S2/S3（`_sprite0Added` 与 `$2003` 不为 0） |
| AccuracyCoin `$0459` | 第 2 项 | 目标：S2。上一次尝试**确实**修好了它——代价是弄坏 `$0489` |
| AccuracyCoin `$0489` | **通过** | 守门：上一次尝试把它从绿变红，所以取指 dot 与整个 sprite 阶段都得保住它 |
| AccuracyCoin `$048D` | 第 2 项 | 目标：S3/S5（同一次读里 sprite 0 与溢出同时置位） |
| AccuracyCoin 整张表 | 跑 146、过 111、失败 17 | 除某一步预先声明过的回退外，不得差于 111/17；而且任何时候都要跑到全部 146 项结果 |

## 3. 第 0 步——已完成，2026-10-16

套件回到了树里、金标准是**先读判决再重录**的、清单条目开始带着原因、四个诊断都在。
`cargo test --workspace --release` 全绿（其中 ROM 套件 release 下 90 s；613 个单元测试约 1 s）。

**做了什么。**

1. 从快照把 `tests/rom_tests.rs`、`tests/common/`、`tests/golden/` 放回
   （`git checkout b4caaf6^ -- crates/nes-core/tests/…` 得到同样的文件）。
2. 逐张读以画面判决的 ROM 自己的屏幕——`NES_DUMP_ROM` 导一个靠后的帧，把一套 ROM 的顶部若干行叠成
   一张图，十六个判决一眼读完——结果写进 §2 那张表。三项失败的 `sprite_overflow` 检查就是这样找出来的。
3. 给每个套件重录金标准（`NES_GOLDEN_RECORD=1 … --test rom_tests`），再不录音跑一遍确认吻合：
   26 个测试、0 失败、90 s。
4. 让基线能带原因。`expected_failures` 里的条目现在写成 `"<file>.nes — <为什么>"`，而且**清单上的
   ROM 不再需要金标准**：卡在一个没实现的功能上、或在等按键的 ROM 没有可钉的判决，钉一张卡死的画面
   等于宣称一个没人量过的"通过"。清单上的 ROM 若**有**金标准且画面动了，仍然会被报出来——这就是
   "去读那张画面"的信号。清单上通过 `$6000` 自报通过的，照旧会大声退场（`FIXED`）。
5. 修了帧预算：`nes-test-roms` 里那五个"拿 DMC 取数对 CPU 读"的套件用 `DMC_FRAMES = 1800`
   （`dmc_dma_during_read4`、`sprdma_and_dmc_dma`、`cpu_dummy_reads`、`cpu_dummy_writes`、
   `cpu_exec_space`），因为 600 帧会在它们的判决出现之前就把它们掐掉。AccuracyCoin 测试里的
   `BASELINE_FAILURES` 是 17。
6. 补上工具需要的观测 API——全部只读，而且都不进存档：
   - `PpuBusSample` 现在随每个 dot 带上寄存器与滚动（`ctrl`、`mask`、`status`、`oam_addr`、`v`、
     `t`、`x`），于是可以读一个 dot 而不必用 `peek`（`peek` 会**真的执行**那次读：`peek` 一次 `$2002`
     会清掉 VBlank，把要量的东西改掉）。
   - `Nes::ppu_write_log` / `set_ppu_write_log_capacity`（默认关闭）、`clear_ppu_write_log`、
     `ppu_write_log_capacity`：每一次 PPU 寄存器写，连同它落在的 scanline、dot、值与该 dot 上的
     `$2002`。
7. 四个诊断，都是 `#[ignore]`，都在 `crates/nes-core/tests/cycle_trace.rs`：
   - `NES_PPU_DOT_LOG=<rom>` + `NES_DOT_WINDOW=<line>:<dot>-<line>:<dot>`（另加 `NES_DOT_SKIP`）：
     每个 dot 一行——`$2000`、`$2001`、`$2002`、`$2003`、`v`、`t`、`x`、总线地址，以及落在该 dot 上的
     每一次寄存器写。它直接回答"那次 `$2000`/`$2001` 写落在取指 dot 之前还是之后"；S3 会把 sprite
     移位器也加进它打印的东西里。
   - `NES_REGLOG_ROM=<rom>`（配 `NES_REGLOG_FRAMES`、`NES_REGLOG_OUT`）：每一次寄存器写写成
     `frame / line / dot / $addr = $value / $2002`，落到文件里，供两个构建互相对 diff。
   - `NES_RESULTLOG=1`（配 `NES_ACCURACYCOIN_FRAMES`）：AccuracyCoin 结果字节的每一次变化，连同它发生
     在第几帧、意味着第几项检查——第一个挪动的测试就有名字了。
   - `NES_PROGRESS_ROM=<rom>`（配 `NES_PROGRESS_FRAMES`、`NES_PROGRESS_ADDR`、`NES_PROGRESS_STALL`、
     `NES_PROGRESS_MENU`、`NES_PROGRESS_START`）：进度计数变化时打一行，连续 `NES_PROGRESS_STALL` 帧
     没变再打一行"停住了"——"ROM 不再跑到表尾"这个失败模式，在它一开始的那一帧就被抓住。

**下面每一步的循环。** 四条命令都用 `--release`；没有 `roms/` 的机器上 ROM 套件会自己跳过，所以这些
对全新克隆一分钱不花。

```bash
# ~7 s：AccuracyCoin 整张表，今天跑 146、过 111、失败 17
NES_ACCURACYCOIN=1 cargo test --release -p nes-core --test cycle_trace -- --ignored --nocapture
# ~15 s：sprite 套件，这条工作线最可能弄坏的就是它们
cargo test --release -p nes-core --test rom_tests sprite -- --nocapture
# ~5 s：sprite 阶段改动能够到的邻近套件
cargo test --release -p nes-core --test rom_tests mmc3 -- --nocapture
cargo test --release -p nes-core --test rom_tests vbl_nmi -- --nocapture
# 90 s：全部，在每一步的边界上跑
cargo test --release -p nes-core --test rom_tests -- --nocapture
```

## 4. 各步

一步一个提交，提交信息带上这一步被判定用的数字（对齐文档 §6 要求这样，§7 是它们汇总的地方）。每一步
的门就是它下面列的领先指标；AccuracyCoin 的计数随后才看，而"领先指标全绿、计数却往坏处走"是一处要
记下来的发现，不是回撤的理由。

### S1——把步进按 Mesen 的门限起来 —— **已完成，2026-10-16**

**改了什么。** `NesBus::ppu_step` 只在画面正在绘制时驱动步进：它挂在 `rendering_enabled_delayed`
上（Mesen 的 `_prevRenderingEnabled`，上一个 dot 末尾的掩码状态），并且**不再**在预渲染行上运行。
预渲染行保留**取指**，dot 257 的锁存现在把"最后一条可见行的步进留在二级 OAM 里的槽位"原样交给流水线
——`latch_sprite_slots` 因此单独成函数，并同样受门限，因为 Mesen 的整个 257-320 窗口就在同一个
`if(_prevRenderingEnabled)` 里面。

**有一件事必须一起做。** 给步进加门限，正是让第 0 行改成吃"陈旧的二级 OAM"的那一步，而旧的槽位模型
把它们画了出来：`sprite_hit_tests/07.screen_bottom` 从 `PASSED` 掉到 `FAILED: #4`（"Y = 255 时必须
不命中"），AccuracyCoin 的 `Sprite0Hit_Behavior`（`$0457` 第 13 项）也进了失败清单。两处缺的是同一件
东西——Mesen 的 `LoadSprite` 在**取指那个 dot** 上就判定这个 sprite 到底在不在这一行上：

```cpp
const uint8_t scanline8Bit = (uint8_t)(_scanline >= 0 ? _scanline : (_region == ConsoleRegion::Ntsc ? 261 : 311));
uint16_t rangeResult = scanline8Bit - spriteY;
```

行号被截成八位（261 与 -1 都变成 5），差值按有符号加宽，于是 Y 落在这一行之下的 sprite 会算出一个很大的
数并被拒绝。`PixelPipeline::sprite_fetch` 现在做的就是这件事，行号与 8x16 的第二块瓦片位也都取自同一个
`rangeResult`——这才让 `07.screen_bottom` 回到 `PASSED`、`$0457` 也回到清单之外。把它放进 S1 而不是
S3，是计划自己的规则（§5 第 4 条）：收窄回退，而不是回撤这一步。S3 会挪动这个判定**住在哪里**（移位器
表），这套算术不变。

**先写的测试**（三条在上一个提交上都是红的）：
`test_the_sprite_walk_is_skipped_while_rendering_is_off`、
`test_the_pre_render_line_does_not_walk_oam`、
`test_line_zero_is_fetched_from_the_last_walks_secondary_oam`。

**实测。**

| 门 | S1 之前 | S1 之后 |
| --- | --- | --- |
| `sprite_overflow_tests/1.Basics` | `FAILED: #6` | **`PASSED`** |
| `sprite_overflow_tests/5.Emulator` | `FAILED: #3` | **`PASSED`** |
| `sprite_overflow_tests/3.Timing` | `FAILED: #4` | `FAILED: #4`——那是 S2 的门 |
| `sprite_overflow_tests` 合计 | 2/5 | **4/5** |
| `sprite_hit_tests` | 11/11 | 11/11——补上取指判定之后；没补时 `07.screen_bottom` 回退了 |
| AccuracyCoin | 跑 146 / 过 111 / 失败 17 | 146 / 111 / 17，失败地址逐个相同 |
| `mmc3_test_2`、`ppu_vbl_nmi`、`cpu_interrupts_v2`、`apu_test` | 全绿 | 全绿（`5/6`、`10/10`、`5/5`、`8/8`） |
| 单元测试 | 613 | **616** |

`cargo test --workspace --release` 全绿。`1.Basics` 与 `5.Emulator` 的新屏幕读过（都是 `PASSED`）之后
重录了金标准，两条也一并从清单里删掉。


### S2——步进变成逐 dot

> **状态，2026-10-16：已移植、已实测，卡在 S4 上。** 移植在
> `/tmp/s2-walk-dot-exact.diff`（另有两个整文件版本 `…-sprite.rs`、`…-test-ppu.rs`）；重新套用
> 之前先读下面那张对照表。

**移植了什么。** 用 `/tmp/newsrc4` 里那份忠实的 `ProcessSpriteEvaluation` 替换 `evaluate()`——
1-64 清二级 OAM、65 起步、奇数 dot 按 `$2003` 读主 OAM、偶数 dot 判决（范围判断在空位判断之外、
错位重对齐、溢出 bug 的对角步进、`_oamCopyDone` 的重读）——并由 `ppu_step` 逐 dot 驱动；加上 Mesen
在 257-320 上的 `OAMADDR = 0`（限**绘制行**：`Exec` 只在 `_scanline < 240` 时跑
`ProcessScanlineImpl`，所以预渲染行算、vblank 不算，程序自己在 vblank 设的 `$2003` 因此得以保留）；
以及 Mesen 渲染期的 `$2004`——读回 `_oamCopybuffer`（257-340 与 dot 0 先从二级 OAM 重读）、写只做
抖动自增。单元测试 617 + 集成 55 全绿。

**四组实测**（AccuracyCoin 3600 帧 + sprite 套件）：

| 配置 | AccuracyCoin | `sprite_overflow_tests/3.Timing` | `sprite_hit_tests` |
| --- | --- | --- | --- |
| 只有步进 + `$2004` | 跑 155 / 失败 18 | **第 4 项 → 第 10 项** | 坏：8 张画面变 `FAILED` |
| 再加 257-320 的 `$2003 = 0` | 跑 144 / 失败 24 | 第 4 项，没动 | 11/11 |
| 再加 `$2003 = 0`、去掉写回 | 跑 155 / 失败 19 | **第 4 项 → 第 8 项** | 坏：sprite 0 整体不命中 |
| 三者齐全（Mesen 的形状） | 跑 144 / 失败 24 | 第 4 项 | 11/11 |

**这说明什么。** 步进对 `$2003` 的写回在本内核里是**承重**的——关掉它，`sprite_hit_tests` 全线报
"根本不工作"；而开着它时，Mesen 那条 257-320 的 `$2003 = 0` 会一次代价十一项 AccuracyCoin
（`Address2004_Behavior`、`OAM_Corruption`、`2004_Stress`、`FrozenOAM2Inc`、`HybridAddresses`、
`ALERead`、`StaleBGShiftRegs`、`StaleBGShiftRegisters`、`RenderingFlagBehavior`、`BGSerialIn`、
`2007_Stress`）。这十一项正是盯着**渲染期间** `$2003`/`$2004` 行为的测试，而写回只有在 OAM 地址簿记的
其余部分到位之后才会以 Mesen 想要的方式可观测：`$2004` 的写要晚一两个 dot 才落地、OAM 行损坏的几行、
以及 DMA 自己的地址时序。**那是 S4 的活，所以 S2 卡在 S4 上**，而不是卡在 S2 自身；正确的顺序是先做
S4 的地址簿记，再把这个写回压上去。

有绿门的是步进的**时序**，而且它前进在关键处：关掉写回时 `3.Timing` 从第 4 项走到第 8 项——第 4、5 项
（"第一条扫描线上置得太早/太晚"）通过——而 AccuracyCoin 的 `$0459 Sprite overflow behavior` 在四组
配置里都通过。

**记在账上的代价**：`$0457 Sprite0Hit_Behavior`、`$0489 Suddenly Resize Sprite` 与
`$0494 Misaligned OAM DMA` 在有写回时通过、无写回时失败；"去掉写回"那一列里变成 `FAILED` 的 sprite
画面，就是上面那个 sprite 0 失效本身。

**S4 的另一件也试过并排除了**（2026-10-16）：`UpdateState` 里还有一条无条件的抖动——在绘制行的
65-256 中途把渲染**关掉**会让 OAM 地址 +1（"disabling rendering during OAM evaluation will trigger a
glitch causing the current address to be incremented by 1"）。实现后按 Mesen 形状那一组量：结果完全
相同，144 跑 / 24 失败、失败地址逐项一致。所以那十一项要的是 S4 里的别的东西——剩下两个候选是
`$2004` 写的落地时机与 DMA 的地址时序，不是这条抖动。

**改动。** 用移植版 `SpriteEvaluator`（`/tmp/newsrc4/sprite.rs`）替换 `evaluate()`：1-64 清二级 OAM、
65 起步、奇数 dot 读 `$2003` 指的那个字节、偶数 dot 判决（范围判断放在空位判断**之外**、错位重对齐
规则、溢出 bug 的对角步进、`_oamCopyDone` 的重读分支），`$2003` 由步进写回，溢出标志**在它被升起的
那个 dot** 交还调用方。渲染期的 `$2004` 回 `_oamCopybuffer`（257-340 与 dot 0 上先从二级 OAM 重读），
而 `$2004` 的写只移动地址（`+4 & 0xFC`）。

**依据。** `ProcessSpriteEvaluation`、`ProcessSpriteEvaluationStart`，以及 `ReadRam`/`WriteRam` 的
`SpriteData` 分支。

**先写测试。** 第一次尝试当场揪出的三个移植 bug，写成在当前树上会失败的测试：范围判断必须在空位判断
之外（二级 OAM 已满时第九个范围内的 sprite 仍要被发现）、预渲染行上步进不得运行、渲染期的 `$2004`
写必须**取代**而不是叠加普通存储。再加
`test_overflow_is_raised_on_the_dot_the_ninth_sprite_is_judged`、
`test_a_misaligned_oam_addr_copies_a_window_shifted_sprite`、`test_the_walk_writes_oam_addr_back`。

**门。** `sprite_overflow_tests/3.Timing` 过 #4/#5、`5.Emulator` 过 #3、`1.Basics` 越过 #6/#7、
`2.Details` 与 `4.Obscure` 仍过，AccuracyCoin `$0459` 第 2 项通过。

### S3——sprite 阶段：取指 dot 与移位器表

> **已完成，2026-10-16。** 这个阶段现在是 Mesen 的形状：取指落在它自己的 dot 上、从二级 OAM 的字节
> 出发，像素来自一串"在各自 sprite 的 X 所指 dot 上上电"的移位器，而不是绘制时按 X 挑槽位。

**实测。** 除了两张必须动的画面，什么都没有移动：

| 门 | S3 之前 | S3 之后 |
| --- | --- | --- |
| AccuracyCoin | 跑 146 / 过 111 / 失败 17 | **146 / 111 / 17，失败地址与检查项逐一相同** |
| `sprite_hit_tests` | 11/11 | 11/11 |
| `sprite_overflow_tests` | 4/5（`3.Timing` 第 4 项） | 4/5，没动——那是 S2 的门 |
| `mmc3_test_2`（A12 那一半） | 5/6 | 5/6——`3-A12_clocking` 与 `4-scanline_timing` 仍过 |
| `ppu_vbl_nmi` | 10/10 | 10/10 |
| `cpu_interrupts_v2`、`apu_test`、`instr_test-v5`、`instr_timing`、`oam_read`、`oam_stress`、`ppu_open_bus`、`blargg_ppu_tests`、`branch_timing` | 全绿 | 全绿 |
| 单元测试 | 616 | **620** |

两张金标准在读过画面之后重录：`ppu_read_buffer` 仍打印 `Passed`、`Hit`/`No-Hit` 表逐格相同（动的是它
动画的帧相位），AccuracyCoin 的结果表逐字节相同，因此只是屏幕的帧挪了。

**落地了什么。** `PixelPipeline` 现在持有 Mesen `NesPpu` 的那套状态：`_spriteTiles`（八个 `SpriteInfo`）、
带 `SpriteShifterDone` 哨兵的 `_spriteShifterList`、扫描位置（`_nextSpriteShifter`/
`_nextSpriteShifterCycle`）与它的 `active`/`counting`/`expired` 三个掩码、
`_spriteCount`/`_processSprites`、`_sprite0Visible`，以及三个 `_minimumDraw*` 裁剪 dot。总线在 dot 257
把这一行的二级 OAM 字节交过来（`begin_sprite_fetch`）；261、269、… 317 上的取指是
`LoadSpriteTileInfo`——槽位来自 dot、两个图案平面在同一个 dot 里读完、范围判断与地址算术取自
`LoadSprite`；像素来自 `draw_sprites`，即 `GetPixelColor` 的 sprite 那一半：每 dot 移位一次、最前面的
不透明 sprite 胜出、sprite 0 命中在重叠的那个像素上升起。dot 0 排序并把扫描位置重置到队首，dot 339 把
没轮到的以 counting 模式交接。左列裁剪 dot 在帧顶**以及**每次 `$2001` 写时重算，正如 `SetMaskRegister`
所为——`/tmp/newsrc4` 里那句"不在 `$2001` 写时更新"的注释是错的。

除模型本身之外还买到两件事：取指窗口挂在渲染状态而不是 `$2001` 的 sprite 位（Mesen 把 257-320 整段挂在
`_prevRenderingEnabled` 上），以及两个图案平面在同一个 dot 里读完，于是两个 sprite 取指之间总线只低一个
dot 而不是两个——上面那张表里的 MMC3 那一半盯着这件事，它没动。

**测试**（五条，改动前全部失败）：
`test_the_fetch_takes_the_control_register_as_it_stands_on_the_fetch_dot`（`Suddenly Resize Sprite` 的
判别条件：比取指行高十行的 sprite 在 8x8 下出界、16x16 下在界内，于是在两个槽位的取指 dot 之间翻转
`SPRITE_SIZE` 就决定了谁进得了移位器）、`test_the_fetch_runs_with_the_sprite_enable_bit_clear`、
`test_a_shifter_activates_at_sprite_x_plus_one`、`test_dot_339_hands_the_unreached_shifters_over`，
以及改写过的 `test_line_zero_is_fetched_from_the_last_walks_secondary_oam`（预渲染取指把每一条遗留
都拒掉，因为 261 截成八位是 5）。

**改动。** dot 257 的槽位锁存与逐像素重算让位给 Mesen 的那套机器：带 `SpriteShifterDone` 哨兵的
移位器表、`(cycle - 257) % 8 == 4` 上的 `LoadSpriteTileInfo`（从二级 OAM 取四个字节，并在**同一个
dot 里**读**两个**图案平面）、257 时重置 `_spriteIndex`/`_spriteCount`、257-320 每个 dot 把
`_sprite0Added` 拷进 `_sprite0Visible`、取指窗口只以渲染为门（不看 sprite 使能位）、
`ProcessSpriteShifters` 让每个移位器在 `spriteX + 1` 激活、dot 0 的排序、dot 339 的交接、
`_processSprites`/`UpdateProcessSpritesFlag`、三个 `_minimumDraw*` 阈值（在帧初**以及**每次 `$2001`
写时计算，Mesen 的 `SetMaskRegister` 就是这么做的），以及 `DrawPixel`/`GetPixelColor` 读移位后的字节
而不是重算像素。

**依据。** `LoadSprite`、`LoadSpriteTileInfo`、`ProcessSpriteShifters`、`GetPixelColor`、
`ProcessScanlineFirstCycle`、dot 339 那一段、`UpdateMinimumDrawCycles`。

**先写测试。** `test_the_fetch_takes_the_control_register_as_it_stands_on_the_fetch_dot`——把
`Suddenly Resize Sprite` 的判别条件写成单元测试：在两个槽位的取指 dot 之间翻转 `SPRITE_SIZE`，然后
断言哪一个最终落在范围内。上一次尝试缺的正是这条测试，也是它分不清"dot 错了"与"模型错了"的原因。
再加 `test_the_fetch_runs_with_the_sprite_enable_bit_clear`、
`test_a_shifter_activates_at_sprite_x_plus_one`、
`test_dot_339_hands_the_unreached_shifters_over_in_counting_mode`、
`test_the_shifter_list_is_sorted_at_dot_zero`。

**门。** AccuracyCoin `$0489` 仍通过（它前两项都要；HEAD 上它是通过的，而上次尝试把它弄坏了）、
`$0459` 保持通过、`$0458` 第 3 项通过、`sprite_hit_tests` 保持 11/11，而且 AccuracyCoin 那一轮仍然
跑到它全部的结果项。

**点名的风险。** (a) Mesen 在同一个 dot 里读两个图案平面，本内核差两个 dot；这会改动 MMC3 盯着的那条
总线地址时间线，所以 `mmc3_test_2/3-A12_clocking` 与 `4-scanline_timing` 也属于这一步的门——那里若
失败，说明移植在别处有错，因为 Mesen 是过得了这些 ROM 的。(b) `_spriteCount`/`_processSprites` 决定
像素里 sprite 那一半跑不跑，弄错的表现是 sprite 在某一行整片消失。(c) 命中的 dot 按构造就会挪（移位器
在 `spriteX + 1` 激活），所以 `sprite_hit` 的画面是判官。

### S4——缺掉的三次访问，以及奇数帧的 sprite 延迟

**改动。** dot 337 与 dot 339 的 nametable 读，以**实时** `IsRenderingEnabled()` 为门；dot 320 的
`LoadExtraSprites`；257-320 的二级 OAM 地址步进（`+= !((cycle - 1) & 4)`、257 时归零、321 时多一次
自增）；257-320 每个 dot 上 `_spriteRamAddr = 0`；预渲染 dot 0 的 `CorruptOamRow`（
`DisableOamAddrBug` 默认 false）；以及奇数帧上每个移位器 `+= 1 << 4` 配 `_dotSkipped = 3`。

**依据。** `case 0`/`case 2`/`case 4` 那个 switch、`_cycle == 337` 与 `_cycle == 339` 两段、
`LoadExtraSprites`、`ProcessScanlineFirstCycle`。

**门。** `mmc3_test_2/3-A12_clocking` 与 `4-scanline_timing` 通过；`ppu_vbl_nmi/10-even_odd_timing`
保持绿；`NES_A12_ROM` 显示每帧的 A12 上升次数不变；AccuracyCoin 不变。

### S5——`$2002` 的 sprite 标志

到 S3 结束时，两个标志**被置**的 dot 应当已经对了，所以这一步主要是验证：核对 `$048D` 的两项检查
（清标志的时序 `$E0 $E0 $80 $00`；置标志的时序要 `$60`，即同一次读里两个标志都在）、`1.Basics`
#4/#5 与 `2.Details` #4（跨越 VBlank 的标志寿命）。清标志的那个 dot 本身不动，除非有实测要求：本内核
在预渲染行 dot 1 清，并**通过了**读它的那项检查，而 Mesen 是在 dot 0 清 sprite 标志。

**若两个标志的置位时间都对上了、`$048D` 第 2 项仍然失败**，剩下的差异就是 ROM 自己注释里写的那个：
G 版 CPU 上这次读在 M2 升起时锁存 VBlank、在两个 sprite 位上晚约 1.875 个 PPU dot。那只是那两个 sprite
位上的一个读延迟——是 §8 要拍板的事，不是 S5 期间可以拧的旋钮，也绝不是去动 `ACCESS_DOTS` 的理由
（它被五张 ROM 量定为 2，见对齐文档 §4）。

### S6——收尾与文档

退休那些只服务于自己那一步的诊断（有价值的按对齐文档 §5 的清单留着）、给没有版本号的状态字段补上
`STATE_VERSION`、跑完整套验收，然后更新对齐文档：§2.1（求值不再是逐行）、§7（每一步一行，带数字）、
§9 的 C8 行、§10.2 的 PPU 表与 §10.5 的逐项表。若某一步按设计移动了像素，就再录一次金标准，并读
diff。

## 5. 交火规则

1. 一步一个提交；提交信息带上判定这一步用的数字。
2. 先写这一步的测试，并确认它们在**上一个提交**上失败。一条"改动前失败、改动后通过"的测试都没有的
   步骤，既无法度量，也就无法修。
3. 前后用同一套命令量，并把数字写进本文件。对齐文档已经吃过一次"把过期数字当成实测数字"的亏。
4. 出现回退时：用 `NES_REGLOG_ROM`/`NES_PPU_DOT_LOG` 找到第一处分歧，修掉或收窄，然后再决定。忠于
   Mesen 的回退连同证据记成有意偏差，**不要**回撤这一步。走不到门的步骤就拆开。
5. 不许只看 AccuracyCoin 的计数（开头的规则 3），也不许只看金标准：金标准吻合只说明"画面没动"，
   不说明"ROM 通过了"。
6. 每一步都重跑 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D
   warnings`、`cargo test --workspace`、sprite 套件、`blargg_mmc3`、`blargg_ppu_vbl_nmi`、
   `blargg_cpu_interrupts`、`blargg_apu`，以及 `frame_cost`（同一台机器、五次中位、只与这台上一次
   比较，不往前追）。
7. 确定性与存档是不变量：新状态进 `state_fields!`、配往返测试、并抬高 `STATE_VERSION`。
8. 别的东西一律不动（开头的规则 5）。

## 6. 完成的定义

- `sprite_overflow_tests` 按画面判决 5/5、`sprite_hit_tests` 11/11；`oam_read` 与
  `blargg_ppu_tests` 人眼读过并且没变。
- AccuracyCoin：`$0458`、`$0459`、`$048D` 通过、`$0489` 仍通过、其余 17 项不更差，并且整张表都在帧
  预算内跑到结果。
- §1.2 那张表的每一行都进了代码，且每一行都有一条尚未被覆盖的单元测试。
- 渲染期的 `$2004` 与 Mesen 一致，它读到的状态在存档里。
- 对齐文档 §2.1 不再说求值是逐行的，§9 的 C8 行带着数字收口。

## 7. 本计划不覆盖什么

- MMC5 的竖向分屏（已决定不做）与扩展音源那条线。
- PAL 的 sprite 求值规则（`_palSpriteEvalScanline`）：本地没有 PAL ROM 可判。
- `RemoveSpriteLimit`、`EnablePpuSpriteEvalBug` 与 `EnablePpuOamRowCorruption`：三者默认都是关的，
  所以本计划有意不实现（代码走到另一条分支的地方应当写明这一点）。
- `$2002` 的读延迟，直到 S5 的门判定它是唯一剩下的差异为止。

## 8. 边做边要拍板的问题

1. S5 的读延迟：把两个 sprite 位那约 1.875 个 dot 的延迟建模，还是把 `$048D` 第 2 项连同 ROM 自己的
   注释记成有意偏差？
2. Mesen 的 `_spriteTiles` 是 64 项，因为存在 `RemoveSpriteLimit`。关掉它时八项就够——但
   `LoadExtraSprites` 于是索引不到东西，而 `_lastVisibleSpriteAddr` 仍要维护。数组留八项并写明这处
   偏差，还是把 64 也移植过来？
3. 预渲染的 `CorruptOamRow` 是否需要在 Mesen 归零 `_secondaryOamAddr` 的那个 dot 上归零（在
   `ProcessScanlineFirstCycle` 的 `_prevRenderingEnabled` 分支里，也就是只在渲染开启时）？
4. `3.Timing` 后面的检查（#11/#12，"第九个 sprite 远远晚于第八个"）要求重对齐规则与 Mesen 逐字一致，
   包括用 `(_secondaryOamAddr & 0x03) == 0` 而不是 `spriteAddrL == 0` 来判断。写之前先对着 ROM 的源码
   （`roms/blargg/sprite_overflow_tests/source/3.Timing.a`）核一遍。
