# The sprite phase and `$2002`'s sprite flags: how to land them

[简体中文](./sprite-phase-plan.zh-CN.md) · **English**

This is the execution plan for the two items the alignment document calls **C8** (§2.1, §9) and
**§10.7 item 4**: the PPU's sprite *evaluation* is still a line at a time, and `$2002`'s two sprite
flags do not yet have Mesen's lifetimes. Read the alignment document's §2.1, §7 and §9 first — they
carry the two withdrawn attempts and the measurements that narrowed the problem down. This file adds
only the order of work, the gates it may be judged by, and the instruments it is measured with.

The alignment document's two rules apply unchanged (Mesen's source wins; ROM results are evidence,
not constraints). This workstream adds three, and they exist because of how the previous two
attempts ended:

3. **A stage is never judged by AccuracyCoin's failing count alone.** That count is a lagging
   indicator, and while a port of this size lands it moves in both directions (the attempts went
   111/17 → 119/22 → 116/27). The leading indicators are *which check* each failing test stops at,
   and the on-screen verdicts of the suites that never write `$6000`.
4. **No stage is reverted as a whole.** A regression is diagnosed with the instruments below and
   either fixed, narrowed, or recorded as a deviation with its evidence. A stage that cannot reach
   its gate is *split*: the part that holds is committed, the rest becomes its own stage.
5. **Nothing else moves while this lands.** `ACCESS_DOTS`, the register state machine's dot
   placement, the DMA state machine and the APU's place in the CPU cycle are all measured and pinned
   by tests; a stage that needs one of those changed is a different workstream.

## 1. What "aligned" means here

### 1.1 The shape in the tree today

- `NesBus::ppu_step` (`crates/nes-core/src/nes.rs`) drives the sprite half on **every** line, with no
  rendering gate and no "not the pre-render line" gate; `SpriteEvaluator::evaluate`
  (`crates/nes-core/src/ppu/sprite.rs`) fills secondary OAM for the next line **in one call at dot
  65** and raises the overflow flag there and then.
- The pixel pipeline (`crates/nes-core/src/ppu/pipeline.rs`) latches eight slots at dot 257, reads the
  two pattern planes at dots 261 and 263 (`(cycle - 257) % 8 == 4 / 6`), and draws each sprite pixel
  by recomputing it from the slot's X range and the fetched bytes.
- Three of Mesen's bus accesses never happen: the nametable reads at dots **337** and **339**, and
  `LoadExtraSprites` at dot **320**.
- `$2004` during rendering is a plain OAM read/write with an unconditional increment.

### 1.2 The dot schedule to implement (MesenCE `NesPpu::ProcessScanlineImpl`)

Line numbers are dots. "Rendering" below means `_prevRenderingEnabled` — the mask as of the end of
the previous dot — unless the row says otherwise.

| Dots | MesenCE | In the tree today |
| --- | --- | --- |
| 0 | `ProcessScanlineFirstCycle`: roll the line; on a line below 240 (`_scanline < 240`) sort `_spriteShifterList[0..8]` and restart the sweep at its head; arriving on the pre-render line: clear `SpriteOverflow` and `Sprite0Hit`, and `CorruptOamRow(_spriteRamAddr >> 3, _secondaryOamAddr & 0x1F)` when rendering is on (`DisableOamAddrBug` defaults to false); arriving on line 240: `SendFrame()` | nothing (no sort, no walk state, no row copy) |
| 1-64 | clear secondary OAM: `_oamCopybuffer = $FF`, `_secondarySpriteRam[_secondaryOamAddr & 0x1F] = $FF`, `_secondaryOamAddr += !(_cycle & 1)` | nothing |
| 65 | `ProcessSpriteEvaluationStart`: `_sprite0Added`/`_spriteInRange` false, `_secondaryOamAddr = 0`, `_overflowBugCounter = 0`, `_oamCopyDone` false, `_firstVisibleSpriteAddr = _spriteRamAddr & $FC` | the one-shot `evaluate()` starts here |
| odd 65-255 | `_oamCopybuffer = ReadSpriteRam(_spriteRamAddr)` | — (inside `evaluate`) |
| even 66-256 | decide: the range test first (`_scanline >= buf && _scanline < buf + height`), then the room branch (copy the byte into secondary OAM, with the misalignment resync rule) or the overflow branch (the diagonal step, `_overflowBugCounter`); write `_spriteRamAddr` back | — (inside `evaluate`; the flag is raised when the ninth sprite is *found by the sweep*, not on the dot it is judged) |
| 1-256 | `DrawPixel()` (background from the shifters, sprites from the shifter list, the sprite-0 hit inside it), `ShiftTileRegisters()`, `ProcessSpriteShifters()` | pixel drawn; no shifter list |
| `(cycle & 7) == 0` | `IncHorizontalScrolling()`; at 256 also `IncVerticalScrolling()` | present |
| 257 | `_spriteIndex = 0`, `_spriteCount = 0`; copy horizontal scroll from `t`; `_secondaryOamAddr = 0` | partially (scroll only) |
| 257-320, every dot | `_sprite0Visible = _sprite0Added`; `_spriteRamAddr = 0`; `_secondaryOamAddr += !((_cycle - 1) & 4)` | none of the three |
| `(cycle - 257) % 8 == 0` and `== 2` | two garbage nametable reads that still reach the cartridge (this is part of what the MMC3 watches) | present |
| `(cycle - 257) % 8 == 4` (261, 269, … 317) | `LoadSpriteTileInfo`: `_spriteIndex = (_cycle - 257) >> 3`, read the four bytes out of secondary OAM, `LoadSprite` reads **both** pattern planes in that one dot; the range test is `scanline8Bit - spriteY` with `scanline8Bit = 261` on the pre-render line, flipped by bit 7 of the attributes; out of range → zero bytes and `SpriteShifterDone`, in range → `_spriteShifterList[i] = ((spriteX + 1) << 4) \| i`, `_spriteCount++`, `_processSprites = true` | low at 261+8k, high at 263+8k; the in-range decision was already made by `evaluate()` |
| 280-304 (pre-render only) | copy vertical scroll from `t` | present |
| 320 | `LoadExtraSprites()` | missing (a no-op under `RemoveSpriteLimit = false`, but part of the schedule) |
| 321-336 | `_secondaryOamAddr++` at 321; `IncHorizontalScrolling()` at 328 and 336; `ShiftTileRegisters()`; `LoadTileInfo()` | the last two are present |
| 337 | `if (IsRenderingEnabled()) _tile.TileAddr = ReadVram(GetNameTableAddr())` — the **live** mask, not the delayed state | missing |
| 339 | `_activeSpriteShifters = 0`; if rendering: the same nametable read into `_tile.TileAddr`, and every shifter that is neither done nor expired enters **counting** mode; on an odd NTSC 2C02 pre-render frame `_cycle = 340`, `_dotSkipped = 3`, every shifter `+= 1 << 4`, `_nextSpriteShifterCycle++`; then every shifter not in counting mode becomes **active**; `UpdateProcessSpritesFlag()` | only the odd frame's skipped dot |

The shifter's own lifecycle, which the table above refers to:

- a loaded shifter is `((spriteX + 1) << 4) | index` — the dot it is *activated* on, one past the
  sprite's leftmost pixel;
- `ProcessSpriteShifters` walks the list in X order and moves each shifter from counting to active on
  its own dot, marking it expired;
- at dot 0 of a new line the list is sorted and the sweep restarts at its head; at dot 339 the ones
  that never came round go into counting mode, and the rest are activated so their pixels start at
  X 0;
- the pixel is taken from the shifted bytes, and the sprite-0 hit is raised where the pixel is
  output (`_cycle != 256`, both enables on, neither plane clipped).

### 1.3 `$2002`

| | MesenCE | In the tree today |
| --- | --- | --- |
| a read | bits 7/6/5 from the flags; `UpdateStatusFlag()` clears **VBlank and the CPU's NMI latch only**; bits 4-0 from the PPU's own open bus; a read at `_scanline == _nmiScanline && _cycle == 0` sets `_preventVblFlag` | already aligned (C3) |
| when the sprite flags are *cleared* | `ProcessScanlineFirstCycle` on the pre-render line, i.e. dot 0 (Mesen); AccuracyCoin's own comment says dot 1, and the core clears at dot 1 and passes the test that measures it | dot 1, with VBlank |
| when they are *set* | `SpriteOverflow`: on the even dot the walk judges the ninth in-range sprite. `Sprite0Hit`: in `GetPixelColor`, on the dot the overlapping pixel is output | overflow at dot 65 of the line, whether or not rendering is on and whether or not the ninth sprite is anywhere near that dot |
| read latency | AccuracyCoin's `$048D` comment: on a revision-G CPU the VBlank bit is latched when M2 rises and the sprite bits ~1.875 PPU dots later, when M2 falls | the whole register is sampled at one instant (`ACCESS_DOTS = 2`, §4 of the alignment document) |

### 1.4 Defaults that decide what *not* to build

From `NesConfig`'s defaults (alignment document §10.1): `RemoveSpriteLimit = false` (no
`_spriteTiles[8..64]`, no extra sprites drawn), `EnablePpuSpriteEvalBug = false` (so the
`_oamCopyDone` re-read branch *is* taken), `EnablePpuOamRowCorruption = false` (no row corruption on
`$2003`/state changes), `DisableOamAddrBug = false` (so the pre-render `CorruptOamRow` **is**
emulated), `ForceBackgroundFirstColumn`/`ForceSpritesFirstColumn = false`,
`RestrictPpuAccessOnFirstFrame = false`. None of these is a knob to add.

## 2. The baseline, measured on 2026-10-16 (HEAD `8ed8713`)

**AccuracyCoin.** `NES_ACCURACYCOIN=1 cargo test --release -p nes-core --test cycle_trace --
--ignored --nocapture` → 146 tests ran, 111 passed, 17 failing — in **7.4 s**. The same run in a
debug build takes 63 s, so the loop for this workstream is `--release`.

**The ROM suite.** It was out of the tree (b4caaf6; the snapshot is at
`/tmp/nes-baseline-snapshot-20261016`), and restoring it at HEAD read almost entirely red — *not*
because the core had regressed:

- the goldens predate every rendering change that has landed since they were recorded (B1's sprite
  high plane, B2's attribute coarse X, the per-dot pipeline, the register state machine, the
  forced-blank palette path), so a ROM whose `$6000` says "Passed" is still reported as "frames moved
  from the goldens";
- the 37 ROMs added in §10.6 have no goldens at all, and are failures by construction.

So every frame-judged ROM was read off its own screen first (dump a late frame, read it), and only
then were the goldens re-recorded — see S0 below for what that took. The verdicts, which are what
this plan is written against:

| Suite | Verdict at HEAD, and how it is read |
| --- | --- |
| `blargg/instr_test-v5` (16) | 16/16 through `$6000` |
| `blargg/instr_timing` (2) | 2/2 through `$6000` |
| `blargg/branch_timing_tests` (3) | 3/3 by picture: `PASSED` |
| `blargg/cpu_interrupts_v2` (5) | 5/5 through `$6000` |
| `blargg/ppu_vbl_nmi` (10) | 10/10 through `$6000` |
| `blargg_ppu_tests` (5) | 4/5 by picture: `$01` on `palette_ram`, `sprite_ram`, `vbl_clear_time` and `vram_access`; `power_up_palette` prints `$02` — the power-on palette, which §4 lists as a deliberate deviation |
| `blargg/oam_read` (1) | 1/1 through `$6000` |
| `blargg/oam_stress` (1) | 1/1 through `$6000` (it needs ~1700 frames) |
| `blargg/ppu_open_bus` (1) | 1/1 through `$6000` |
| `blargg/sprite_hit_tests` (11) | **11/11** by picture: `PASSED` |
| `blargg/sprite_overflow_tests` (5) | 2/5 by picture at S0 (`1.Basics` `FAILED: #6`, `3.Timing` `FAILED: #4`, `5.Emulator` `FAILED: #3`); **4/5 after S1** — see that stage below |
| `blargg/mmc3_test_2` (6) | 5/6 through `$6000`; `6-MMC3_alt` wants MMC3 revision A, which Mesen selects from its database and therefore fails too (§10.6) |
| `blargg/apu_test` (8) | 8/8 through `$6000` |
| `nes-test-roms/blargg_apu_2005.07.30` (11) | 9/11 by picture: `$01` on nine; `10.len_halt_timing` prints `$03`, `11.len_reload_timing` prints `$04` — the length counter's halt/reload timing (§10.3 item 8) |
| `nes-test-roms/cpu_dummy_reads` (1) | 1/1 by picture: `Passed` (it needs >600 frames to get there) |
| `nes-test-roms/cpu_dummy_writes` (2) | 2/2 through `$6000` (also >600 frames) |
| `nes-test-roms/cpu_exec_space` (2) | `ppuio` 1/1 through `$6000`; `apu` stops at `$6000 = 2`, the APU page's open bus (§10.3 item 4) |
| `nes-test-roms/cpu_reset` (2) | 0/2: both print the values they asked for and then wait for a *human* to press reset |
| `nes-test-roms/dmc_dma_during_read4` (5) | 1/5 by picture: `dma_2007_write` `Passed`; `dma_4016_read` and `read_write_2007` `Failed`; `dma_2007_read` and `double_2007_read` print a checksum one cycle off the accepted values |
| `nes-test-roms/dmc_tests` (4) | 0/4: all four wait forever on the DMC (PC in a tight loop by frame 9000) |
| `nes-test-roms/full_palette` (3) | picture only, and no still picture either — they are animations (`jmp begin_frame`), so their per-frame goldens are the whole net |
| `nes-test-roms/ppu_read_buffer` (1) | 1/1 by picture: `Passed` |
| `nes-test-roms/read_joy3` (4) | 0/4: all four wait for scripted input |
| `nes-test-roms/sprdma_and_dmc_dma` (2) | 0/2: both print `Failed` (the sprite/DMC overlap phase) |
| AccuracyCoin | 146 ran, 111 passed, 17 failing |

The three `sprite_overflow` failures should be stated plainly, because the alignment document's §9
records that suite as 5/5: that number came from goldens recorded on a build whose pictures already
contained these failures, so the suite has been failing since before the alignment pass began. All
three are in this workstream, and they are the sharpest instruments available:
`1.Basics #6` is "must not be set when all rendering is off" (it sets `$2001 = 0`),
`3.Timing #4` is "set too early for the first scanline" (the ROM brackets the dot with two
consecutive `$2002` reads and tolerates ±1-2 CPU clocks), and `5.Emulator #3` is "disabling rendering
did not recalculate the flag time". The core fails all three for one reason: `ppu_step` drives the
walk on every line with no rendering gate, and `evaluate()` raises the flag at dot 65.

### The targets, and the guards

| Test | At HEAD | Role here |
| --- | --- | --- |
| `sprite_overflow_tests/1.Basics` | FAILED #6 | target: #6 falls in S1, #7/#8 in S2 |
| `sprite_overflow_tests/3.Timing` | FAILED #4 | target: S2 — the flag has to appear on the dot the walk judges the ninth sprite |
| `sprite_overflow_tests/5.Emulator` | FAILED #3 | target: S2 |
| `sprite_overflow_tests/2.Details`, `4.Obscure` | PASSED | guard (lifetimes and the overflow-bug diagonal) |
| `sprite_hit_tests` (11) | PASSED | guard: S3 is the stage that can break them |
| AccuracyCoin `$0458` | check 3 | target: S2/S3 (`_sprite0Added` and a non-zero `$2003`) |
| AccuracyCoin `$0459` | check 2 | target: S2. The last attempt *did* fix this one — and broke `$0489` doing it |
| AccuracyCoin `$0489` | **passing** | guard: the last attempt turned this green → red, so both fetch-dot timing and the phase have to keep it |
| AccuracyCoin `$048D` | check 2 | target: S3/S5 (Sprite0Hit and SpriteOverflow set inside the same read) |
| AccuracyCoin, whole table | 146 ran, 111 passed, 17 failing | never worse than 111/17 except where a stage's pre-declared regression says so; and the run must always reach all 146 results |

## 3. Stage 0 — done, 2026-10-16

The suite is back in the tree, the goldens have been re-recorded deliberately, the baseline lists
carry their reasons, and the four diagnostics exist. `cargo test --workspace --release` is green
(the ROM suite is 90 s of that in release; the unit tests are 613 and take a second).

**What was done.**

1. Restored `tests/rom_tests.rs`, `tests/common/` and `tests/golden/` from the snapshot
   (`git checkout b4caaf6^ -- crates/nes-core/tests/…` gets the same files back).
2. Read every frame-judged ROM's own screen — `NES_DUMP_ROM` at a late frame, with the top rows of a
   suite stacked into one image so sixteen verdicts read in one glance — and wrote the result into
   §2's table. That is how the three failing `sprite_overflow` checks were found.
3. Re-recorded the goldens for every suite (`NES_GOLDEN_RECORD=1 … --test rom_tests`) and re-ran the
   suite unrecorded to confirm it matches: 26 tests, 0 failed, 90 s.
4. Taught the baseline about reasons. An entry in `expected_failures` is now
   `"<file>.nes — <why>"`, and a ROM on the list **needs no golden**: one that hangs on a missing
   feature or waits for a button has no verdict to pin, and pinning a picture of a hang would claim a
   pass nobody measured. When a listed ROM *does* have goldens and they move, that is still reported
   — it is the signal to go and read the picture. A listed ROM that starts reporting a pass through
   `$6000` still retires itself loudly (`FIXED`).
5. Fixed the frame budgets: `nes-test-roms`'s `DMC_FRAMES = 1800` for the suites that measure a DMC
   fetch against a read (`dmc_dma_during_read4`, `sprdma_and_dmc_dma`, `cpu_dummy_reads`,
   `cpu_dummy_writes`, `cpu_exec_space`), because 600 frames cut all five off before their verdicts.
   `BASELINE_FAILURES` in the AccuracyCoin test is 17.
6. Added the observation API the tools need — deliberately read-only, and none of it in the save
   state:
   - `PpuBusSample` now carries the registers and the scroll (`ctrl`, `mask`, `status`, `oam_addr`,
     `v`, `t`, `x`) with every dot of the bus trace, so a dot can be read without `peek` (which would
     *perform* the read: a `$2002` peek clears VBlank and changes what the trace is measuring).
   - `Nes::ppu_write_log` / `set_ppu_write_log_capacity` (default off), `clear_ppu_write_log`,
     `ppu_write_log_capacity`: every write to a PPU register with the scanline, dot, value and `$2002`
     it landed on.
7. Four diagnostics, all `#[ignore]`d, in `crates/nes-core/tests/cycle_trace.rs`:
   - `NES_PPU_DOT_LOG=<rom>` + `NES_DOT_WINDOW=<line>:<dot>-<line>:<dot>` (+ `NES_DOT_SKIP`): one
     line per dot — `$2000`, `$2001`, `$2002`, `$2003`, `v`, `t`, `x`, the bus address, and any
     register write that landed on that dot. It answers "did that `$2000`/`$2001` write arrive before
     or after the fetch dot" directly; S3 adds the sprite shifters to what it prints.
   - `NES_REGLOG_ROM=<rom>` (+ `NES_REGLOG_FRAMES`, `NES_REGLOG_OUT`): every register write as
     `frame / line / dot / $addr = $value / $2002`, in a file meant to be diffed between two builds.
   - `NES_RESULTLOG=1` (+ `NES_ACCURACYCOIN_FRAMES`): every change to AccuracyCoin's result bytes,
     with the frame and the check it means — the first test a change moves, named.
   - `NES_PROGRESS_ROM=<rom>` (+ `NES_PROGRESS_FRAMES`, `NES_PROGRESS_ADDR`, `NES_PROGRESS_STALL`,
     `NES_PROGRESS_MENU`, `NES_PROGRESS_START`): a progress counter printed when it changes, and a
     stall line after `NES_PROGRESS_STALL` frames without a change — the "the ROM stopped reaching
     its table" failure mode, caught on the frame it starts.

**The loop for every stage below.** All four commands are `--release`; the ROM suite on a machine
*without* ROMs skips itself, so none of this is a fresh-clone cost.

```bash
# ~7 s: the whole AccuracyCoin table, 146 ran / 111 passed / 17 failing today
NES_ACCURACYCOIN=1 cargo test --release -p nes-core --test cycle_trace -- --ignored --nocapture
# ~15 s: the sprite suites, the ones this workstream can break
cargo test --release -p nes-core --test rom_tests sprite -- --nocapture
# ~5 s: the neighbouring suites a sprite-phase change can reach
cargo test --release -p nes-core --test rom_tests mmc3 -- --nocapture
cargo test --release -p nes-core --test rom_tests vbl_nmi -- --nocapture
# 90 s: everything, at a stage boundary
cargo test --release -p nes-core --test rom_tests -- --nocapture
```

## 4. The stages

One stage per commit, with the measured numbers in the commit message (the alignment document's §6
asks for this, and §7 is where they are collected). A stage's gate is its leading indicator; the
AccuracyCoin count is checked afterwards, and a count that moves the wrong way while every leading
indicator is green is a finding to write down, not a reason to revert.

### S1 — gate the walk the way Mesen gates it — **done, 2026-10-16**

**What changed.** `NesBus::ppu_step` drives the walk only while the picture is being drawn: it hangs
off `rendering_enabled_delayed` (Mesen's `_prevRenderingEnabled`, the mask as of the end of the
previous dot) and it no longer runs on the pre-render line at all. The pre-render line keeps the
*fetch*, and the latch at dot 257 now hands the pipeline the slots straight out of whatever secondary
OAM the last visible line's walk left — `latch_sprite_slots` is its own function, gated the same way,
because Mesen's whole dots-257-320 window sits inside the same `if(_prevRenderingEnabled)`.

**One thing had to come with it.** Gating the walk is what makes line 0 take its sprites from stale
secondary OAM, and the old slot path drew them: `sprite_hit_tests/07.screen_bottom` fell from
`PASSED` to `FAILED: #4` ("should always miss when Y = 255"), and AccuracyCoin's `Sprite0Hit_Behavior`
(`$0457`, check 13) appeared on the failing list. Both were the same missing piece — Mesen's
`LoadSprite` decides *at the fetch dot* whether the sprite is on the line being fetched for:

```cpp
const uint8_t scanline8Bit = (uint8_t)(_scanline >= 0 ? _scanline : (_region == ConsoleRegion::Ntsc ? 261 : 311));
uint16_t rangeResult = scanline8Bit - spriteY;
```

The line is truncated to eight bits (261 *and* -1 both become 5) and the difference is widened
signed, so a sprite whose Y sits below that line comes out huge and is rejected. `PixelPipeline::sprite_fetch`
does exactly that now, and takes its row and its 8x16 second-tile bit from the same `rangeResult` —
which is what puts `07.screen_bottom` back at `PASSED` and `$0457` back off the list. Bringing it in
with S1 rather than S3 is the plan's own rule (§5.4): narrow the regression, do not revert the stage.
S3 moves *where* the decision lives (the shifter list); this arithmetic stays as it is.

**Tests first** (all three fail on the previous commit): `test_the_sprite_walk_is_skipped_while_rendering_is_off`,
`test_the_pre_render_line_does_not_walk_oam`, `test_line_zero_is_fetched_from_the_last_walks_secondary_oam`.

**Measured.**

| Gate | Before S1 | After S1 |
| --- | --- | --- |
| `sprite_overflow_tests/1.Basics` | `FAILED: #6` | **`PASSED`** |
| `sprite_overflow_tests/5.Emulator` | `FAILED: #3` | **`PASSED`** |
| `sprite_overflow_tests/3.Timing` | `FAILED: #4` | `FAILED: #4` — that is S2's gate |
| `sprite_overflow_tests`, overall | 2/5 | **4/5** |
| `sprite_hit_tests` | 11/11 | 11/11 — after the fetch fix; without it `07.screen_bottom` regressed |
| AccuracyCoin | 146 ran / 111 passed / 17 failing | 146 / 111 / 17, the same 17 addresses |
| `mmc3_test_2`, `ppu_vbl_nmi`, `cpu_interrupts_v2`, `apu_test` | green | green (`5/6`, `10/10`, `5/5`, `8/8`) |
| unit tests | 613 | **616** |

`cargo test --workspace --release` is green. The goldens for `1.Basics` and `5.Emulator` were
re-recorded after their new screens were read (both `PASSED`), and both left the baseline.

### S2 — the walk becomes dot-exact

> **Status, 2026-10-16: ported, measured, and blocked on S4.** The port is in
> `/tmp/s2-walk-dot-exact.diff` (with `…-sprite.rs` and `…-test-ppu.rs` for the two files it
> rewrites wholesale); read the measurement table below before re-applying it.

**What was ported.** The faithful `ProcessSpriteEvaluation` from `/tmp/newsrc4` replacing
`evaluate()` — dots 1-64 clear secondary OAM, dot 65 starts the walk, odd dots read primary OAM at
`$2003`, even dots decide (the range test outside the room test, the misalignment resync, the
diagonal overflow-bug step, the `_oamCopyDone` re-read) — driven per dot from `ppu_step`; plus
Mesen's `OAMADDR = 0` over dots 257-320 (of a *drawing* line: `Exec` runs `ProcessScanlineImpl` when
`_scanline < 240`, so the pre-render line is in and vertical blanking is out, which is what keeps a
program's own `$2003` alive during vblank); plus Mesen's rendering-time `$2004` — a read answers
`_oamCopybuffer`, re-read out of secondary OAM on dots 257-340 and dot 0, and a write only performs
the glitchy increment. 617 unit tests and 55 integration tests, all green.

**Four measurements**, AccuracyCoin at 3600 frames and the sprite suites:

| Configuration | AccuracyCoin | `sprite_overflow_tests/3.Timing` | `sprite_hit_tests` |
| --- | --- | --- | --- |
| walk + `$2004` only | 155 ran / 18 failing | **check 4 → check 10** | broken: 8 pictures move to `FAILED` |
| + `$2003 = 0` over 257-320 | 144 ran / 24 failing | check 4, unchanged | 11/11 |
| + `$2003 = 0`, no write-back | 155 ran / 19 failing | **check 4 → check 8** | broken: sprite 0 stops hitting |
| all three (Mesen's own shape) | 144 ran / 24 failing | check 4 | 11/11 |

**What that says.** The walk's `$2003` write-back is load-bearing for sprite 0 here — with it off,
`sprite_hit_tests` reports "isn't working at all" across the board — while with it on, Mesen's
`$2003 = 0` over 257-320 costs eleven AccuracyCoin items at once (`Address2004_Behavior`,
`OAM_Corruption`, `2004_Stress`, `FrozenOAM2Inc`, `HybridAddresses`, `ALERead`,
`StaleBGShiftRegs`, `StaleBGShiftRegisters`, `RenderingFlagBehavior`, `BGSerialIn`, `2007_Stress`).
Those eleven are the tests that watch `$2003`/`$2004` *while rendering*, and the write-back only
becomes observable the way Mesen intends once the rest of the OAM address bookkeeping is in: the
`$2004` write landing a dot or two after the write ends, the OAM row corruption rows, and the DMA's
own address timing. **That is S4's work, so S2 is blocked on S4** rather than on anything in S2
itself; the order to do them in is S4's bookkeeping first, then this write-back on top.

The part with a green gate is the walk's *timing*, and it moves where it counts: with the write-back
off, `3.Timing` goes from check 4 to check 8 — checks 4 and 5, "set too early/too late for the first
scanline", pass — and AccuracyCoin's `$0459 Sprite overflow behavior` passes in every configuration.

**Costs recorded**: `$0457 Sprite0Hit_Behavior`, `$0489 Suddenly Resize Sprite` and
`$0494 Misaligned OAM DMA` pass with the write-back and fail without it; the sprite-suite pictures
that move to `FAILED` in the write-back-off column are the sprite-0 breakage itself.

**One more piece of S4, tried and ruled out** (2026-10-16): `UpdateState` also carries an
unconditional glitch — switching rendering *off* during dots 65-256 of a drawing line bumps the OAM
address by one ("disabling rendering during OAM evaluation will trigger a glitch causing the current
address to be incremented by 1"). It was implemented and measured against the Mesen-shaped
configuration: identical, 144 ran / 24 failing with the same failing addresses. So the eleven items
want something else from S4 — the `$2004` write's landing and the DMA's address timing are the
candidates left, not this glitch.

**Change.** `SpriteEvaluator` (the port in `/tmp/newsrc4/sprite.rs`) replaces `evaluate()`: dots 1-64
clear, dot 65 starts, odd dots read `$2003`'s byte, even dots decide (the range test outside the room
test, the misalignment resync rule, the diagonal overflow-bug step, the `_oamCopyDone` re-read
branch), `$2003` is written back by the walk, and the overflow flag is handed to the caller **on the
dot it is raised**. `$2004` during rendering answers `_oamCopybuffer` (re-read from secondary OAM on
dots 257-340 and on dot 0) and a `$2004` write only moves the address (`+4 & 0xFC`).

**Oracle.** `ProcessSpriteEvaluation`, `ProcessSpriteEvaluationStart`, and `ReadRam`/`WriteRam`'s
`SpriteData` cases.

**Tests first.** The three port bugs the first attempt found, as tests that fail on the current tree:
the range test outside the room test (a ninth in-range sprite must be found with secondary OAM full),
the walk not running on the pre-render line, and the `$2004` write replacing — not adding to — the
normal store. Plus `test_overflow_is_raised_on_the_dot_the_ninth_sprite_is_judged`,
`test_a_misaligned_oam_addr_copies_a_window_shifted_sprite`, `test_the_walk_writes_oam_addr_back`.

**Gate.** `sprite_overflow_tests/3.Timing` passes #4/#5, `5.Emulator` passes #3, `1.Basics` gets past
#6/#7, `2.Details` and `4.Obscure` still pass, and AccuracyCoin `$0459` check 2 passes.

### S3 — the sprite phase: the fetch dots and the shifter list

**Change.** The dot-257 slot latch and the per-pixel recomputation give way to Mesen's machinery: a
shifter list with the `SpriteShifterDone` sentinel, `LoadSpriteTileInfo` on `(cycle - 257) % 8 == 4`
reading the four bytes from secondary OAM and **both** pattern planes in that one dot, `_spriteIndex`/
`_spriteCount` reset at 257, `_sprite0Visible` copied from `_sprite0Added` every dot of 257-320, the
fetch window gated on rendering alone (the sprite enable bit is not consulted), the sweep
(`ProcessSpriteShifters`) activating each shifter at `spriteX + 1`, the sort at dot 0, the dot-339
handoff, `_processSprites`/`UpdateProcessSpritesFlag`, the three `_minimumDraw*` cycle thresholds
(computed at frame start **and** on every `$2001` write, as `SetMaskRegister` does), and
`DrawPixel`/`GetPixelColor` reading the shifted bytes instead of recomputing a pixel.

**Oracle.** `LoadSprite`, `LoadSpriteTileInfo`, `ProcessSpriteShifters`, `GetPixelColor`,
`ProcessScanlineFirstCycle`, the dot-339 block, `UpdateMinimumDrawCycles`.

**Tests first.** `test_the_fetch_takes_the_control_register_as_it_stands_on_the_fetch_dot` — the
`Suddenly Resize Sprite` discriminator, expressed as a unit test: flip `SPRITE_SIZE` between two
slots' fetch dots and assert which of them ends up in range. This is the test the last attempt did
not have, and it is why that attempt could not tell a wrong dot from a wrong model. Also
`test_the_fetch_runs_with_the_sprite_enable_bit_clear`,
`test_a_shifter_activates_at_sprite_x_plus_one`,
`test_dot_339_hands_the_unreached_shifters_over_in_counting_mode`,
`test_the_shifter_list_is_sorted_at_dot_zero`.

**Gate.** AccuracyCoin `$0489` still passes (both of its first two checks — it passed at HEAD and the
last attempt broke it), `$0459` stays passing, `$0458` check 3 passes, `sprite_hit_tests` stays
11/11, and the AccuracyCoin run still reaches all of its results.

**Risks, named.** (a) Mesen reads both pattern planes in one dot where the core reads them two dots
apart; that changes the bus-address timeline the MMC3 watches, so `mmc3_test_2/3-A12_clocking` and
`4-scanline_timing` are part of this stage's gate too — a failure there means the port is wrong
somewhere else, since Mesen passes those ROMs. (b) `_spriteCount`/`_processSprites` decide whether
the sprite half of the pixel runs at all, so getting them wrong makes sprites vanish on a line.
(c) The hit's dot moves by construction (the shifter activates at `spriteX + 1`), so the `sprite_hit`
pictures are the judge.

### S4 — the three missing accesses, and the odd frame's sprite delay

**Change.** The dot-337 and dot-339 nametable reads, gated on the **live** `IsRenderingEnabled()`;
`LoadExtraSprites` at dot 320; the secondary-OAM address walk of 257-320 (`+= !((cycle - 1) & 4)`,
zeroed at 257, an extra increment at 321); `_spriteRamAddr = 0` on every dot of 257-320;
`CorruptOamRow` at pre-render dot 0 (`DisableOamAddrBug` is false by default); and the odd frame's
`+= 1 << 4` on every shifter with `_dotSkipped = 3`.

**Oracle.** The `case 0`/`case 2`/`case 4` switch, the `_cycle == 337` and `_cycle == 339` blocks,
`LoadExtraSprites`, `ProcessScanlineFirstCycle`.

**Gate.** `mmc3_test_2/3-A12_clocking` and `4-scanline_timing` pass; `ppu_vbl_nmi/10-even_odd_timing`
stays green; `NES_A12_ROM` shows the same number of A12 risings per frame; AccuracyCoin unchanged.

### S5 — `$2002`'s sprite flags

By the end of S3 the two flags should already be *set* on the right dots, so this stage is mostly a
verification: check `$048D`'s two checks (`$E0 $E0 $80 $00` for the clear timing, and `$60` — both
flags in the same read — for the set timing), `1.Basics` #4/#5 and `2.Details` #4 (the flags' life
across VBlank). The clear dot itself stays where it is unless a measurement asks: the core clears at
dot 1 of the pre-render line and passes the test that reads it, while Mesen clears the sprite flags at
dot 0.

**If `$048D` check 2 still fails with both set-times right**, the remaining difference is the one the
ROM's own comment describes: on a revision-G CPU the read latches VBlank when M2 rises and the sprite
bits about 1.875 PPU dots later. That is a read latency on the two sprite bits only — a decision for
§8, not a knob to turn during S5, and never a reason to touch `ACCESS_DOTS` (measured at 2 against
five ROMs, alignment document §4).

### S6 — cleanup and the docs

Retire the diagnostics that outlived their stage (keeping the ones with continuing value, as the
alignment document's §5 lists them), bump `STATE_VERSION` if a stage added state without one, re-run
the whole protocol, and update the alignment document: §2.1 (the evaluation is no longer line at a
time), §7 (a row per stage with its numbers), §9's C8 row, §10.2's PPU table and §10.5's per-test
table. Re-record the goldens again if a stage legitimately moved pixels, and read the diff.

## 5. Rules of engagement

1. One stage per commit; the commit message carries the numbers the stage was judged by.
2. Write the stage's tests first and check that they fail on the previous commit. A stage with no test
   that fails before it and passes after it is not measurable, and therefore not fixable.
3. Measure before and after with the same commands, and write the numbers into this file. The
   alignment document has already been bitten once by a stale number read as a live one.
4. When something regresses: find the first divergence with `NES_REGLOG_ROM`/`NES_PPU_DOT_LOG`, fix it
   or narrow it, and only then decide. Record a Mesen-faithful regression as a deviation with its
   evidence; do not revert the stage. A stage that cannot reach its gate is split.
5. Never judge by AccuracyCoin's count alone (rule 3 at the top), and never judge a suite by its
   goldens alone: a matching golden says "the picture did not move", not "the ROM passed".
6. Every stage re-runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
   warnings`, `cargo test --workspace`, the sprite suites, `blargg_mmc3`, `blargg_ppu_vbl_nmi`,
   `blargg_cpu_interrupts`, `blargg_apu`, and `frame_cost` (same machine, five medians, compared with
   the previous stage on that machine and no further back).
7. Determinism and save states are invariants: new state goes into `state_fields!`, gets a round-trip
   test, and bumps `STATE_VERSION`.
8. Nothing else moves (rule 5 at the top).

## 6. Definition of done

- `sprite_overflow_tests` 5/5 by picture verdict and `sprite_hit_tests` 11/11; `oam_read` and
  `blargg_ppu_tests` read by hand and unchanged.
- AccuracyCoin: `$0458`, `$0459`, `$048D` passing, `$0489` still passing, the rest of the 17 not
  worse, and the whole table reached inside the frame budget.
- Every row of §1.2's table is in the code, with a unit test per row that is not already covered.
- `$2004` behaves as Mesen's during rendering, and the state it reads is in the saved state.
- The alignment document's §2.1 no longer says the evaluation is line at a time, and §9's C8 row
  closes with the numbers.

## 7. What this plan does not cover

- MMC5's vertical split (decided: not done) and the extension-audio work.
- PAL's sprite-evaluation rules (`_palSpriteEvalScanline`): no PAL ROM exists to judge them.
- `RemoveSpriteLimit`, `EnablePpuSpriteEvalBug` and `EnablePpuOamRowCorruption`: all default-off, so
  the plan deliberately does not build them (and the code should say so where it takes the other
  branch).
- The `$2002` read latency, until S5's gate says it is the only difference left.

## 8. Open questions to settle while doing it

1. S5's read latency: model the sprite bits' ~1.875-dot latency, or record `$048D` check 2 as a
   deviation with the ROM's own comment as the evidence?
2. `_spriteTiles` is 64 entries in Mesen because `RemoveSpriteLimit` exists. With it off, eight are
   enough — but `LoadExtraSprites` then indexes nothing, and `_lastVisibleSpriteAddr` still has to be
   maintained. Keep the array at eight and write down the divergence, or port the 64?
3. Does the pre-render `CorruptOamRow` need `_secondaryOamAddr` zeroed at the dot Mesen zeroes it
   (in the `_prevRenderingEnabled` branch of `ProcessScanlineFirstCycle`, i.e. only when rendering is
   on)?
4. `3.Timing`'s later checks (#11/#12, "the ninth sprite is way after the eighth") need the walk's
   realignment rule to be exactly Mesen's, including the `(_secondaryOamAddr & 0x03) == 0` test rather
   than `spriteAddrL == 0`. Confirm that reading against the ROM's source
   (`roms/blargg/sprite_overflow_tests/source/3.Timing.a`) before writing it.
