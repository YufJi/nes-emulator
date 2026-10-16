# Mesen alignment: status, gaps, and how to continue

[简体中文](./mesen-alignment.zh-CN.md) · **English**

This is the working document for the accuracy work: **the emulator core is being aligned with
[Mesen2](https://github.com/SourMesen/Mesen2)'s NES core, and Mesen settles every ambiguity.**
`docs/architecture.md` stays the description of what the emulator *is* today (§15.1 there is its
summary of the known limits); this document is the plan, the measurements behind it, and what is
still different. **Start a new session with §9's ledger** — the one-page answer to what is done,
what is stuck and what to do next — and with **§10, the file-by-file comparison against MesenCE's
NES core** (2026-10-16): it lists what is still different, what is already identical (so it is not
redone) and which ROM proves each item.

Two rules, in order:

1. **Mesen's implementation wins.** Where Mesen and this core disagree, the gap is a bug here
   unless §4 below lists it as a deliberate deviation. Read Mesen's source rather than guessing:
   the files that matter are `Core/NES/NesCpu.cpp`/`.h` (execution, DMA, interrupts),
   `Core/NES/NesPpu.cpp`/`.h` (the fetch pipeline), `Core/NES/BaseNesPpu.h` (PPU state),
   `Core/NES/NesMemoryManager.cpp`, `Core/NES/Mappers/A12Watcher.h` and the mapper files under
   `Core/NES/Mappers/`.
2. **Test-ROM baselines are evidence, not constraints.** Each suite's expected-fail list (in
   `crates/nes-core/tests/rom_tests.rs`, out of the tree for this pass — §6) was asserted in both
   directions, so a change that moved a suite *had* to update the list — but the list follows the
   measurements, and a regression that comes from a more faithful model is recorded and explained
   rather than reverted.

## 1. What is aligned

| Area | What Mesen does | What this core does now | Verified by |
| --- | --- | --- | --- |
| Clock | One master clock; `cpuDivider` 12 / `ppuDivider` 4 on NTSC feed both chips | `Region::cpu_divider()`/`ppu_divider()` (12/4, the real crystal), one master clock, `NesBus::run_ppu_to` runs the PPU to a target | `test_clock_dividers_agree_with_the_frame_geometry`, `test_the_master_clock_path_places_the_same_dots` |
| CPU execution | `MemoryRead`/`MemoryWrite` *are* the cycle (`StartCpuCycle`/`EndCpuCycle` around the access) | `Bus::read_cycle`/`write_cycle` are the cycle; `NesBus::begin_cpu_cycle`/`end_cpu_cycle` advance the PPU and APU around the access | `instr_timing` 2/2, `branch_timing` 3/3, `test_every_opcode_executes` |
| Operand fetches and discarded reads | `FetchOperand` spells out the dummies (zero-page indexed, indexed absolute/indirect, the implied/accumulator read) | `Cpu::fetch_operand` plus `AddressingMode::AbsoluteXWrite`/`AbsoluteYWrite`/`IndirectYWrite`, `RTS`'s two dummies, `RTI`/`PLA`/`PLP`'s one each | the same suites |
| Interrupt timing | Lines sampled every cycle; the decision reads the previous cycle's samples (`_prevRunIrq`/`_prevNeedNmi`) | `Cpu::end_cycle` keeps the same lag; `execute_instruction` decides after the instruction | `cpu_interrupts_v2` 5/5, `ppu_vbl_nmi` 9/10 |
| Where the chips sit in the cycle | `StartCpuCycle` runs the PPU's dots up to the access and then `ProcessCpuClock` (the APU and the cartridges); `EndCpuCycle` runs the rest of the dots and the CPU samples the lines | `NesBus::begin_cpu_cycle` runs the dots, then the APU tick, before the access; `end_cpu_cycle` runs the remaining dots | `test_the_apu_clocks_before_the_cycles_access`, and `cpu_interrupts_v2` 5/5 — its last three tables read this one cycle |
| Vector choice | The vector is chosen at the status push; an NMI asserted by then takes it over (`IRQ()`, `BRK()`) | `Cpu::take_interrupt`/`Cpu::brk` do the same, including BRK's `_prevNeedNmi = false` | `test_nmi_hijacks_an_irq_sequence`, `test_nmi_takes_brks_vector_over` |
| A12 | `SetBusAddress` puts the *last real access*'s address on the bus and it is held; the filter is `(GetMasterClock() - _a12LowClock) >= 3` with the master clock being the CPU cycle count | `NesBus::bus_address` (held), fed by the addresses `PixelPipeline` *really read* on the dot it read them, `Mapper::on_ppu_bus(addr, cpu_cycles)` and `mapper4` measure the run in cycles, resetting on *every* rise | `2-details` (241 clockings) and `4-scanline_timing` green |
| Per-dot rendering | `ProcessScanlineImpl`: `LoadTileInfo` on `cycle % 8 == 1/3/5/7`, `ShiftTileRegisters` and `DrawPixel` every dot, coarse X stepping on the group's last dot (`(_cycle & 0x07) == 0`), all off the single `_videoRamAddr` | `PixelPipeline`: the same fetch/load/shift/draw schedule, with coarse X moving at the reload dot, off `v`/`t`/`x`, plus the sprite phase's fetches and per-dot sprite pixels and sprite-0 hit | `4-scanline_timing`, `sprite_hit` 11/11, and byte-identical frames against the whole-line renderer on `nestest` (that check cannot see the attribute fetch — see §7's last row) |
| Open bus | A stamp per bit (`_openBusDecayStamp`), three frames of decay, `$2002`'s low five bits masked from it, and the PPU's own latch rather than the CPU's | the same: `Ppu::drive_open_bus`/`open_bus_decay_stamp`, `$2002` driving only its top three bits, `$2007`'s palette reads driving six | `ppu_open_bus` 1/1 |
| The pre-render line | The pipeline runs for `scanline < 240`, which includes the pre-render line | The pipeline's own `visible` test, `scanline < 240 \|\| scanline == pre_render` | the same |
| The odd frame's skipped dot | Decided while processing dot 339 (`_cycle = 340`), so dot 340 is the one that goes missing | the same, carried by `NesBus::skip_odd_frame_dot` | `test_a_register_write_is_in_time_for_the_odd_frame_decision` |

## 2. What is not aligned yet

### 2.1 PPU: the sprite *evaluation* is still a line at a time

Rendering is per dot (`PixelPipeline`): a background tile is fetched every eight dots,
shifted out one pixel per dot and drawn on its own dot, off the chip's own `v`/`t`/`x`; the
sprite phase's pattern fetches run on their dots (257-320), and the sprite pixels and the
sprite-0 hit are drawn per dot from the fetched bytes. What is left of Mesen's
`ProcessScanlineImpl` is the evaluation itself: `ProcessSpriteEvaluation` (65-256) is still a
single call that fills secondary OAM for the next line, rather than the chip's odd/even OAM
read pattern with `SetOamCorruptionFlags`/`ProcessOamCorruption`.

Consequences still outstanding:

- **A sprite can only be shifted into secondary OAM once**: the OAM-address corruption the
  hardware does when `$2003` is written mid-evaluation is not modelled.
- MMC5's vertical split remains declined.
- Sprite pixels and the sprite-0 hit follow the fetches, so a mid-line CHR bank switch now
  affects sprites on the dot it lands on.

Landed with the per-dot background and sprites: real fetch addresses took `4-scanline_timing`
green (the reconstructed run before the first sprite-phase fetch was a dot long), and
`RenderTiming` and the whole-line renderer are gone. (`ppu.render_v` is *not*: the field survives,
unused but serialized — §10.3.)

What Mesen has that is missing here: `_lowBitShift`/`_highBitShift`, `TileInfo`, `LoadTileInfo`,
`LoadSprite`/`LoadExtraSprites`, `ShiftTileRegisters`, `GetPixelColor`, `DrawPixel`,
`_minimumDrawBgCycle`/`_minimumDrawSpriteCycle`, and `ProcessSpriteEvaluation`
(`ProcessSpriteEvaluationStart`/`End`, the OAM address behaviour, `SetOamCorruptionFlags` /
`ProcessOamCorruption`).

### 2.2 PPU: the bus, OAM and the registers around rendering

- ~~**OAM decay**~~ Not a gap: Mesen models it behind `EnableOamDecay`, a setting whose default
  is *off* (`_enableOamDecay = false` in `BaseNesPpu.h`), and the rule itself is a 3000-cycle
  per-eight-byte-row stamp whose decayed value is the row's own address
  (`NesPpu::ReadSpriteRam`). Not modelling it matches Mesen's default.
- ~~**`$2007` during rendering**~~ Modelled from `UpdateVideoRamAddr` and `ReadRegister`: on a
  drawing line with rendering on a `$2007` access advances the *scroll* — a coarse X increment
  and a Y increment together — instead of the address, and a second `$2007` access within six
  PPU dots is ignored and reads the open bus (`_ignoreVramRead = 6`). Off a drawing line (or
  with rendering off) the documented 1/32 increment stands.
- ~~**Emphasis/grayscale**~~ Aligned to `UpdateColorBitMasks`: the grayscale bit masks a palette
  value with $30 on its way out of `$2007` as well as on its way to the screen, and PAL and
  Dendy swap what the register's emphasis bits mean.
- ~~**The RGB the nine colour bits resolve to**~~ Aligned to Mesen's default video filter
  (`NesDefaultVideoFilter.cpp`): the pixel value is `palette | Red<<6 | Green<<7 | Blue<<8`
  (Mesen's `_intensifyColorBits`), and a 512-entry table resolves it. Setting an emphasis bit
  damps the *other* two channels to 84% of their value, truncating, and the `$xE`/`$xF` columns
  are left alone — Mesen's `GenerateFullColorPalette`. The 64-colour base this core already had
  turns out to be Mesen's 2C02 table (`_ppuPaletteArgb[0]`) entry for entry, so all that moved
  was the emphasis: 0.75 → 0.84, the black-column rule, and a lookup instead of per-pixel float
  arithmetic.
- PAL's own PPU rules (`_nmiScanline`, `_vblankEnd`, PAL's OAM refresh) are still absent.
- **What turns the background fetch on.** Mesen's `LoadTileInfo` hangs off `IsRenderingEnabled()`
  (`BackgroundEnabled | SpritesEnabled`): with the background off and sprites on it still fetches
  background tiles, still loads the shift registers and still steps `v` — this core hangs all of
  that off `SHOW_BG`. The visible consequences are A12 (MMC3 misses a batch of clocks with the
  background off) and the scroll advancing while the background is off; no local ROM covers it.

### 2.3 CPU: DMA is applied after the instruction

Mesen runs `ProcessPendingDma` at the start of *every* `MemoryRead`: the halt and dummy-read
cycles, the abort logic (`StopDmcTransfer`, `_abortDmcDma`), the `$4016`/`$4017` bit-loss
behaviour and `ProcessDmaRead`'s value merging, and the DMC/OAM address conflicts. This core
adds a single `dma_stall_cycles` after the instruction (`NesBus::step_instruction`), so the
stall is not interleaved with the instruction's own accesses and the interrupt lines are not
sampled during it. This is the biggest CPU-side gap, and the DMC work depends on it.

Smaller CPU differences: Mesen runs eight warm-up cycles after reset and sets its cycle counter
to `-1` (this core ticks neither); Mesen has a `_ppuOffset` (1 by default) and an optional
randomised CPU/PPU alignment (this core has offset 0).

### 2.4 APU: DMC, resampling and expansion audio

- ~~The frame counter's phase.~~ Done: a `$4017` restart holds the counter 3 cycles (4 on the
  other half of the APU clock), and the frame IRQ flag goes up for three cycles in a row
  starting 29831 clocks after the write — both taken from blargg's own measured cycles in
  `roms/blargg/apu_test/source/`, not from a guess. `4-jitter`, `5-len_timing` and
  `6-irq_flag_timing` are green.
- ~~**Where the APU sits in the CPU cycle**~~ Done: the chip clocks the APU at the *start* of
  a CPU cycle, before that cycle's own access (Mesen's `ProcessCpuClock`, called from
  `StartCpuCycle`), where this core used to tick it at the *end*. That single cycle is what was
  left of `cpu_interrupts_v2`: a `$4017` restart is first seen by the *next* cycle's tick, so
  the frame IRQ flag comes up 29831 cycles after the write **cycle** rather than 29830, and
  every sample of the interrupt line sees the APU one cycle later. All three of the suite's
  remaining tables read that cycle off their interrupt landing spots, and all three are green
  with the APU clocked at the front (§7).
- ~~**DMC**~~ Done: the channel is Mesen's now
  (`Core/NES/APU/DeltaModulationChannel.cpp`), and `7-dmc_basics` and `8-dmc_rates` are both
  green (apu_test 6/8 → **8/8**). The earlier reading of the two roms as contradicting the
  source was wrong — the contradiction was a bus-side defect (a fetched byte halted the CPU
  *twice*, §7) — and with that fixed the three moving parts line up:
  * The length counter moves in `SetDmcReadBuffer` — when a byte is **read out of memory** —
    and the status bit is `_bytesRemaining > 0`, so the sample reads as over while its last
    byte is still on its way out of the register, and the IRQ flag goes up at that same fetch
    (`7-dmc_basics` check 19 is exactly this).
  * A fetched byte **waits in the buffer**, and the output unit takes it at the next byte
    boundary (`Run` does the move). The reader is fed from that boundary, so the buffer never
    holds more than one byte and a second `$4015` write cannot queue another behind a sample
    that is still playing.
  * The timer's period is the table's value, not one cycle more.
  * `8-dmc_rates`'s 135 periods come from those plus the wave's own sync: `sync_dmc_fast`
    leaves a byte of its own on the register, the new sample's first fetch waits for that
    byte's boundary, and the seven bits it has left — clocked at the *new* rate — are the 7P
    between 128P and 135P.
- ~~**DMC's PAL period table**~~ Done: `DmcChannel::set_region` picks Mesen's
  `_dmcPeriodLookupTablePal` (`398, 354, ... 50`) on PAL, and the console hands the region
  down when a cartridge is loaded.
- ~~Resampling~~ Done: the audio path is a band-limited resampler now, the shape blargg's
  `blip_buf` has and the one Mesen's NES mixer runs (`NesSoundMixer` feeds two `blip_t`
  buffers): a change in the mixer's level is a step, a step at a fractional output position
  adds a 32-phase, linearly interpolated, sixteen-tap impulse response, the output is the
  running sum of those contributions, and a one-pole high-pass keeps it AC coupled (blip's
  `bass_shift = 9`). One deviation, and it is deliberate: Mesen runs blip at 96 kHz and then
  Hermite-resamples that to the host's rate, where this core resamples straight to the host's
  rate — one less stage, and the band limit lands exactly on the rate the host asked for
  instead of on an intermediate one.
- Expansion audio (VRC6, MMC5, Sunsoft 5B) exists but has never been compared against a
  reference recording; N163 and FDS are missing.

### 2.5 Mappers

- ~~**NES 2.0 submappers are not parsed.**~~ Parsed now: byte 7's bits 2-3 spot the version,
  byte 8 carries the submapper and the mapper's top four bits, byte 9's nibbles carry the sizes'
  high bits, byte 12 states the region, and the exponent-multiplier size encoding is refused
  rather than mis-read (`Nes::rom_submapper` exposes it). What is *not* done is dispatching on
  it: the titles that need that carry plain iNES headers, where the choice lives in a database
  rather than in the file — `6-MMC3_alt` is the example, and 116/176 need the same treatment.
- ~~**Bus conflicts are not modelled**~~ Modelled: `Mapper::has_bus_conflicts`, applied where
  every write reaches a cartridge (`NesBus::write_now`) as the bitwise AND of the CPU's value
  and the byte the board returns for that address — Mesen's `HasBusConflicts`. The discrete
  boards that have them say so: 2, 3, 7, 11, 66 and 71.
- Missing boards: 64 (RAMBO-1, and 158 with it), 115, 116, 176, 185 (needs the
  pattern-data-from-VRAM window).
- MMC5: extended attribute mode (`$5104 = 3`) is missing; the vertical split is declined.
- Hook granularity: `on_ppu_bus` is now per dot, but `on_scanline` and `on_cpu_cycles` are still
  approximations where Mesen calls `ProcessCpuClock` every CPU cycle.

## 3. Priority, and what each step should buy

1. ~~**The per-dot PPU renderer** (§2.1)~~ — done: `4-scanline_timing` is green, A12 and the
   sprite-0 hit are produced rather than reconstructed, and the whole-line renderer is gone.
   `ppu_vbl_nmi/10-even_odd_timing` did not move, and the sprite *evaluation* is the piece of
   §2.1 that is still line-at-a-time.
2. ~~**DMA inside `MemoryRead`** (§2.3)~~ — done, and it moved no ROM: `4-irq_and_dma`'s trace is
   byte-identical, which matches §7's L4a finding. What it buys is the shape DMC needs.
3. ~~**The APU frame counter's phase, then DMC** (§2.4)~~ — both done: the frame counter turned
   `4-jitter`, `5-len_timing` and `6-irq_flag_timing` green, and Mesen's DMC shape (counter at
   the fetch, byte held in the buffer, silence flag, the table's own period) turned
   `7-dmc_basics` and `8-dmc_rates` green — apu_test 8/8. The item's third piece was where the
   APU sits *inside* the cycle (§2.4): clocking it at the start rather than the end is what
   took `cpu_interrupts_v2`'s last three cases green.
4. ~~**§2.2's register details**~~ — done: open-bus decay (`ppu_open_bus`), the rendering-time
   `$2007` behaviour, the grayscale mask on palette reads and the PAL emphasis swap. OAM decay
   turned out to be an off-by-default Mesen setting, so not modelling it *is* the default. The
   RGB the nine colour bits resolve to is in as well — Mesen's 512-entry table, its 84%
   truncation and the black-column rule (§2.2/§7).
5. **PAL**. The ratio is done — PAL runs 16/5 with the fractional dot accumulator, so a PAL
   frame is 33247.5 CPU cycles and the audio clock derives from the real 3.2 rather than from
   the NTSC-shaped constant. What is left is PAL's own PPU/APU rules (`_nmiScanline`,
   `_vblankEnd`, PAL's OAM refresh); none of it is verifiable without a PAL test ROM.
6. **Mappers** (§2.5): the NES 2.0 header and bus conflicts are in. What is left is dispatching
   on the submapper (which needs the per-ROM data Mesen keeps in its database, not the header),
   and the boards 64/158/185/115.
7. ~~**Audio resampling**~~ — done: the band-limited resampler above (§2.4/§7).

## 4. Deliberate deviations (do not "fix" these without a decision)

- **KIL/JAM.** Mesen treats a KIL as an unrecoverable crash and clears the interrupt samples so
  the hang cannot be interrupted. This core keeps the hardware's behaviour — an interrupt or a
  reset restarts the CPU — because the host reports the hang (`Nes::is_cpu_halted`, and the web
  front end surfaces it). The PC rewind is Mesen's; the sample clearing is not.
- **The access dot inside a cycle.** `ACCESS_DOTS = 2`: the chip's access lands after the cycle's
  first two dots, where Mesen's `_startClockCount`/`_endClockCount` (6/6 of 12) put it about one
  dot in. This is *measured*, and the per-dot renderer made the measurement sharp: with the
  pipeline fetching at the dot, `ACCESS_DOTS = 1` (and 0) each cost five ROMs —
  `ppu_vbl_nmi`'s `05-nmi_timing`, `06-suppression` and `07-nmi_on_timing`,
  `cpu_interrupts_v2/2-nmi_and_brk` and `mmc3_test_2/4-scanline_timing` — while 2 passes every
  suite. The earlier note here said no suite discriminated between 0, 1 and 2; that was true of
  the whole-line renderer and is not true any more.
- **Power-on contents.** RAM and the PPU's palette RAM come up all-zero here, where Mesen's
  default is `RamPowerOnState = Random` (and its non-random path seeds the palette with a fixed
  32-byte boot pattern for blargg's old palette ROM). Random power-on contents are one of the
  three settings Mesen offers — `AllZeros`, `AllOnes`, `Random` — so zeros are a Mesen
  configuration, and they are the one that keeps this core's determinism invariant
  (`frame()`/`run_cycles()`/`step_instruction()` reaching byte-identical state). A ROM that
  reads RAM before writing it can therefore differ from Mesen's default run.

## 5. Instruments

Existing, and used for every measurement above. **The ROM suites are out of the tree for this
pass** (§6); the ROMs are still under `roms/`, and the diagnostics below are what replaces the
per-ROM lines while the alignment runs:

- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture` with `NES_STATUS_ROM`
  (follow `$6000`/`$6004` and the PC) or `NES_DUMP_ROM` (write the frame out as a PPM, for the
  ROMs whose verdict is a screen) — the spot-check for a ROM a change is meant to move.
- `cargo test -p nes-core --test rom_tests -- --nocapture` — the per-ROM pass/known lines, back
  when the suites are (§10.1 and §13.1 of `architecture.md` still describe them).
- `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture` — frame cost
  (600 frames of nestest; run it a few times and compare medians).
- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture` — per-instruction traces
  (`NES_TRACE_ROM`) and the PPU bus address per line (`NES_A12_ROM`, `NES_A12_SKIP`,
  `NES_A12_LINE`).
- The test-ROMs ship their own sources and readmes under `roms/blargg/*/source/` and
  `readme.txt`; blargg's suites state their precision ("Timing is tested to an accuracy of one
  PPU clock") and print expected traces. Compare against those, not against the shape of ours.
- Mind the byte order before writing a raw frame buffer out as a PNG:
  `PixelFormat::Argb8888`'s `0xAARRGGBB` lands on a little-endian host as **B, G, R, A**, so a
  script that reads `A, R, G, B` swaps red and blue — correct colours then look like a broken
  palette (this bullet is here because that cost a session).
- `Nes::ppu_bus_trace()` / `Nes::clear_ppu_bus_trace()` / `Nes::set_ppu_bus_trace_capacity()`:
  the PPU's bus timeline, one sample per dot, filled by the machine as the dots happen (so it is
  in machine order even though the CPU runs whole instructions). It is the same view the
  cartridge has, and `test_a12_clockings_per_frame` counts qualifying A12 rises a frame at a
  time through it. `Nes::step_dot()` is the pacing companion — a debugger's dot counter, with
  `step_instruction()`'s semantics one chip over.

Missing, and worth adding when it is needed:

- PAL test ROMs, an MMC5 suite, a bus-conflict ROM, and a golden-audio comparison for the
  expansion chips;
- an oracle comparison against Mesen itself (its debugger can emit cycle traces), which would
  give per-instruction diffs instead of end-of-ROM verdicts.

### 5.1 Mesen's own test suite is *not* this one (checked 2026-10-16)

Worth knowing before chasing a baseline, because the two sets answer different questions and a
wrong assumption here turns a fix into a loop:

- **Where the maintained Mesen lives.** `SourMesen/Mesen2`'s README points at
  `nesdev-org/MesenCE`: that fork is the one with CI, releases and tests. Its source layout is
  the same, so the paths this document uses still resolve, but `MesenCE` is what to read.
- **MesenCE's accuracy CI is `.github/workflows/tests.yml`**, and it works by checking out
  `nesdev-org/MesenTests` into `Tests/` and running `PGOHelper <Tests folder> citests`, which
  runs every `.mtp` file it finds (multi-threaded, screenshots uploaded when something fails).
  `SourMesen/Mesen2`'s own CI is *build only*: no ROM is run.
- **A `.mtp` is a "recorded ROM test"** (`Core/Shared/RecordedRomTest`): a played-back input log
  plus **per-frame screenshot hashes** to compare against (`_screenshotHashes`, `_badFrameCount`).
  The suite is organised by system (`NES/`, `GB/`, `SNES/`, `GBA/`, …), and it ships **no ROMs**:
  the runner looks each one up by **filename and CRC32** in the tester's own game folders
  (`Core/Shared/RomFinder.h`).
- **Ours are self-reporting accuracy ROMs**: blargg's suites print their verdict and write it to
  `$6000`, AccuracyCoin writes a result byte per test into `$0400`, and our CI runs
  `cargo test --workspace` with those suites *skipping* when `roms/` is absent — so this list is
  a local instrument, not something CI enforces.

What follows from that: **AccuracyCoin's count is our baseline, not Mesen's** — Mesen's CI never
runs it. So for each failing item the oracle has to be **Mesen's source** (rule 1), with the ROM's
verdict as evidence rather than as the target. AccuracyCoin's own README says its tests are
written for an RP2A03G + RP2C02G and that some are revision-specific (it dropped one test because
revision-G PPUs failed it), so a few of the 34 may be cases where the ROM wants a behaviour Mesen
models differently — those belong in §4 as deliberate deviations, with the reading that justifies
them, not as a reason to bend the emulator. Its README also documents every error code, which is
the fastest way to see *what* a failing check means. The stronger oracle, if it is ever wanted, is
to run MesenCE itself over `.mtp`-style recordings and compare golden frames; that needs Mesen
built and the ROMs present, so it is not free.

### 5.2 The baseline now judges pixels too — and 26 of its "passes" were empty

Adopting Mesen's *mechanism* turned up a hole in ours, so this is the state of the local suites as
of 2026-10-16. **The suites and their goldens came out of the tree the same day** (§6/§9), so this
section is a record of the mechanism and of what it found, not a description of what runs now; the
harness itself is `tests/common/frames.rs` in the git history (`b6c06a1`).

- **Every suite's frames are hashed** (`tests/common/frames.rs`): SHA1 over the raw frame buffer
  (`SHA1::GetHash(frame.FrameBuffer, frame.FrameBufferSize)`, the same hash and the same bytes
  Mesen's `.mtp` tests use), run-length encoded into `tests/golden/<suite>/<rom>.hashes`. To save
  the expensive hash, a frame is hashed when a cheap fingerprint says the picture moved — a change
  that changes back within one frame goes unnoticed, which is the price of a suite that finishes.
  Re-record deliberately with `NES_GOLDEN_RECORD=1` and read the diff.
- **The hole it found**: the `$6000` protocol. A ROM that never writes it leaves that byte at zero,
  and "the status is not `$80`" was being read as a pass — so **26 ROMs were counted as passing
  without a test ever running**: all of `sprite_hit_tests_2005.10.05` (11),
  `sprite_overflow_tests` (5), `blargg_ppu_tests_2005.09.15b` (5), `branch_timing_tests` (3),
  `oam_stress` (1) and one of `instr_timing`'s two (measured by running each ROM and watching
  `$6000`). Those suites are the *older* blargg ROMs, which print their verdict to the screen
  instead.
- **The fix**: a ROM that never reports itself is judged by its frames — the goldens are the whole
  verdict, which is how Mesen judges every ROM it runs — and a ROM that does report must actually
  have been seen running before its result counts. Their frame budgets are kept to what the ROMs
  need (the picture settles about thirty frames in; `oam_stress` is the documented exception).
- **It works**: re-introducing the B1 bug (the sprite pattern's high plane read from the low
  plane's address) makes `blargg_sprite_hit` fail with "frames moved from the goldens" and nothing
  else changes. That is the class of bug that passed every suite in this directory for a day.
- **What it is not**: a verdict about correctness. A golden says a ROM's picture did not move, not
  that it is right — and these are *our* hashes, not Mesen's. The step that would make them Mesen's
  is a `.mtp` reader (a zip holding `TestMovie.mmo` plus the hash table) whose hashes come out of
  MesenCE; §5.1 has the format.

## 6. Verification protocol

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- **The ROM suites are not in the tree during this pass** (removed 2026-10-16, §10.6): a full
  run costs minutes, and the alignment work needs a loop measured in seconds. The ROMs stay
  under `roms/`, the suites come back from git when the pass ends, and until then the
  verification is `cargo test --workspace` (604 unit tests) plus the `#[ignore]`d diagnostics
  in `tests/cycle_trace.rs`. Spot checks against the ROMs a change is supposed to move are
  expected — that is what `NES_STATUS_ROM` and `NES_DUMP_ROM` are for.
- When the suites are back, the rules they carried still hold: ROMs are not committed (the
  layout and sources are in `docs/architecture.md` §13.1); a baseline entry that starts passing
  must be removed from the list; a failure outside the list fails the suite; and a regression
  that comes from being *more* faithful to Mesen is added to the list *with the reason in the
  comment* rather than reverted.
- Determinism is an invariant, not a feature: `frame()` / `run_cycles()` / `step_instruction()`
  must reach byte-identical state at the same frame boundary, and save states must round-trip.
- Keep `docs/architecture.md` and its `zh-CN` twin in step with the code, and record each
  step's measurements in the commit message.

## 7. History and measurements

The work landed in steps, each measured against the suite before it. The numbers are what the
commit messages recorded; `frame cost` is release mode, 600 frames of nestest, medians of
several runs.

| Step | What it did | Measured |
| --- | --- | --- |
| L1 | The machine advances one CPU cycle at a time instead of batching per instruction | No ROM change; frame cost 1.075 → 1.046 ms |
| L2 | Interrupt lines sampled on the penultimate cycle; the seven-cycle vector sequence driven cycle by cycle | AccuracyCoin 78 → 79 passing |
| L3 step 1 | Writes to `$2000-$401F` land on the cycle the chip drives them | `4-irq_and_dma`'s trace moved to the documented shape; frame cost 1.033 → 1.034 ms |
| L3 step 2 | The write lands on a dot inside that cycle, before the cycle's APU tick | ROM 57/17 → **58/16** (`5-len_timing`); AccuracyCoin 79/49 → 81/47 |
| L3 step 3 | An NMI asserted before the status push takes a running sequence's vector over; BRK's vector stays provisional | No suite flips; two rows of `3-nmi_and_irq` come in line with its readme |
| L3 step 4 | `$2000-$3FFF` reads land on their own cycle *and* dot; the VBlank suppression window is one dot wide; the NMI line is sampled once a cycle | ROM 58/16 → **62/12**; AccuracyCoin 81/47 → 82/46; frame cost 1.11 → 1.16 ms |
| Phase 0 | One master clock with `cpuDivider`/`ppuDivider`; the PPU runs to a target; the access's dot is a tick offset; `run_ppu_to` and the counted fast path | No ROM change; frame cost 1.206 → 1.210 ms (the first version, doing clock arithmetic every cycle, cost 1.266 ms) |
| Phase 1 | The CPU drives its own cycles: every access is a cycle, the discarded reads are written out, the interrupt lines are sampled per cycle with the decision one cycle behind, the interrupt sequence is the CPU's | ROM composition moves: `2-nmi_and_brk` green, `5-len_timing` onto the list because the `$4015` read now lands on its own cycle; `instr_timing` and `branch_timing` stay green; frame cost 1.204 → 1.204 ms |
| The odd frame's dot | The skip is decided on dot 339, as Mesen decides it | No ROM moves (`10-even_odd_timing` byte-identical), which is what rules this boundary out |
| A12, part 1 | The cartridge is handed the last real access's address and it is held between accesses | `4-scanline_timing` from its 2nd to its 4th sub-test |
| A12, part 2 | The pre-render line runs the fetch pipeline | from its 3rd to its 4th sub-test, direction flipped |
| A12, part 3 | The filter measures the low run in CPU cycles, and *every* rise ends it | **`2-details` green** ("clocked 241 times"); `4-scanline_timing` to its 12th sub-test |
| Per-dot background | `PixelPipeline` replaces the whole-line renderer: fetches on `cycle % 8 == 1/3/5/7`, shift registers, a pixel per dot, off the real `v`/`t`; the cartridge gets the addresses really read; `RenderTiming` and `ppu.render_v` removed | **`4-scanline_timing` green** (the reconstruction was a dot long); rest of the baseline unchanged (AccuracyCoin 46); frame cost 1.32 → 1.80 ms |
| Per-dot sprites | The sprite phase's pattern fetches, per-dot pixels, and the sprite-0 hit on its own dot; the whole-line renderer (`BackgroundRenderer`, `SpriteRenderer`, `ScanlineProcessor`) is deleted | No ROM moved (baseline unchanged, AccuracyCoin 46); frame cost 1.80 → 1.68 ms |
| DMA inside `MemoryRead` | `NesBus::process_pending_dma` (Mesen's `ProcessPendingDma`): a `$4014` write and a DMC request only leave a pending transfer, spent at the CPU's next access in real machine cycles | No ROM moved — `4-irq_and_dma`'s trace is byte-identical, which matches the L4a finding that its offset is the frame counter's phase |
| Open-bus decay | The PPU's own bus latch; a per-bit three-frame stamp; `$2007` driving the byte it returns; palette reads driving six bits and taking two from the bus; `$2002` driving three | **`ppu_open_bus` green** (0/1 → 1/1) |
| Frame counter phase | `$4017` holds the counter 3 cycles (4 on the odd APU phase); the frame IRQ flag goes up for three cycles in a row starting 29831 clocks after the write | **`4-jitter`, `5-len_timing` and `6-irq_flag_timing` green** (apu_test 3/8 → 6/8); rest of the baseline unchanged |
| DMC fixes | `write_4015` now sets the enable flag (`$4015` bit 4 was never set at all), a read of `$4015` no longer clears the DMC IRQ flag, and the byte counter follows *played* bytes with a two-stage buffer | `7-dmc_basics` from its 2nd check to its 19th; `8-dmc_rates` still on the baseline (rate 0's window) |
| PAL 16/5 | `cpu_divider` 15 → 16, `has_whole_dot_ratio` PAL → false so the master-clock path runs, and the APU's rate derives from `dots_per_cpu_cycle()` instead of a 3.0 constant | No ROM moved (no PAL ROMs); a PAL frame is 33,247.5 CPU cycles and the audio clock lands within 10 Hz of the published rate |
| The access dot | Ran `ACCESS_DOTS` at 0, 1 and 2 against the whole ROM baseline now that the pipeline fetches at the dot | **2 is the measured value**: 0 and 1 each cost five ROMs (`05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `2-nmi_and_brk`, `4-scanline_timing`); 2 passes every suite |
| NES 2.0 header + bus conflicts | The header parses the version, the submapper, byte 9's size bits and byte 12's region (and refuses the exponent-multiplier encoding); `Mapper::has_bus_conflicts` makes a cartridge write the AND of the CPU's value and the board's byte, for 2/3/7/11/66/71 | No ROM moved (the local suites use mappers 0/1/4, none of which has the conflict trap); `Nes::rom_submapper` exposes what was parsed |
| PPU register details (§2.2) | Read from Mesen's own sources (`NesPpu.cpp`, `BaseNesPpu.cpp`): a `$2007` access on a drawing line advances the scroll both ways, a second `$2007` access within six dots is ignored, palette reads take the grayscale mask, PAL/Dendy swap the emphasis bits; OAM decay turns out to be off by default | No ROM moved (`blargg_ppu_tests` covers the `$2007` paths); three unit tests pin the behaviours |
| The nine colour bits' RGB (§2.2) | Mesen's default video filter read (`NesDefaultVideoFilter.cpp`): the pixel value is `palette \| Red<<6 \| Green<<7 \| Blue<<8`, looked up in a 512-entry table whose emphasis entries damp the other two channels to **84%** and leave the `$xE`/`$xF` columns alone. The base table this core had is Mesen's 2C02 one, entry for entry | No ROM moves (no local ROM looks at rendered colour); four unit tests pin the table, the 84% truncation, the black columns and the PAL swap, one end-to-end test pins a rendered pixel to `0xFF6493D6`; frame cost 1.74 → 1.69 ms |
| Mesen's DMC source | `Core/NES/APU/DeltaModulationChannel.cpp` read (the manifest listed the path; the file was fetched): status = `_bytesRemaining > 0`; the counter and the IRQ move in `SetDmcReadBuffer`, i.e. at the fetch; the register takes the buffer when `_bitsRemaining` hits 0, with a silence flag holding the DAC; `SetEnabled` delays its transfer 2/3 cycles by parity; PAL has its own period table | The Mesen-shaped implementation was tried and **withdrawn**: it puts `8-dmc_rates` at ~128 periods where the ROM's window wants ~135, and `7-dmc_basics` regresses to its 2nd check. The tree keeps the playback-based counter (135.3 periods) |
| DMC fetch charged twice | The channel's request latch stayed set until the byte was supplied, and the bus re-armed its pending transfer from it on every cycle the APU ticked — so the transfer's own four cycles armed a second one. Two four-cycle transfers per byte, and no CPU read of `$4000` ever saw the byte on the bus. The bus now *takes* the request (`Apu::take_dmc_dma_request`), which is Mesen's one-shot halt latch | **AccuracyCoin boots again**: 155 tests ran, 85 passed, 43 failing (the rows above recorded 46); every other suite unchanged; frame cost 1.72 → 1.70 ms; two new unit tests (the latch, and a hand-built ROM that polls `$4000` the way AccuracyCoin's boot-time sync does) |
| DMC is Mesen's | The length counter moves at the fetch rather than at the end of playback, a fetched byte waits in the buffer for the output unit's byte boundary (so the buffer holds one byte and a restart cannot queue a second), the register holds its level while silent instead of decaying over zeroes, and the timer's period is the table's value rather than one more | **`7-dmc_basics` and `8-dmc_rates` green** (apu_test 6/8 → **8/8**); AccuracyCoin 43 → **34 failing** (143 ran, 94 passed); nothing else moved; frame cost 1.70 → 1.74 ms; state version 10 → 11 |
| Band-limited resampling (§2.4) | The audio path became blip's shape: a level change is a step, 32 phases × 16 taps with linear interpolation between them, an integrator for the output and a one-pole high-pass (Mesen's `NesSoundMixer` + `blip_buf`), at the host's rate directly rather than through Mesen's 96 kHz intermediate | ROM baseline unchanged (no ROM listens); unit tests 592 → 597, including the point of the exercise — a 30 kHz input, whose 18 kHz fold is at full strength under sample-and-hold, comes out ≥ 20 dB down; frame cost 1.69 → 1.82 ms; state version 11 → 12 |
| The APU clocks at the start of the cycle | The chip runs the APU before the cycle's own access (Mesen's `ProcessCpuClock`, from `StartCpuCycle`) instead of after it. Measured from the ROM first: with the tick trailing the access, `5-branch_delays_irq`'s row 0 interrupted the `RTS` that ends `begin` two cycles *after* the flag went up, where the readme wants the `NOP` behind it, four cycles after — the same single cycle in each of the four rows of that table that differed | **`cpu_interrupts_v2` 2/5 → 5/5**: `3-nmi_and_irq`, `4-irq_and_dma` and `5-branch_delays_irq` all pass. Nothing else moved — `apu_test` 8/8, `ppu_vbl_nmi` 9/10, `mmc3_test_2` 5/6 and AccuracyCoin's 34 failing are unchanged, and the three suites that pin the flag against the `$4017` write stay green because the flag moved *with* the APU. Unit tests 597 → 598 (one of them pins the APU's place in the cycle); frame cost 1.82 → 1.86 ms (median of five runs, spread ±0.06) |
| The sprite pattern fetch's second plane | The sprite phase read `addr + row` for the high plane as well as the low one, so the low plane came in twice. A sprite pixel whose planes are (0, 1) is colour 2; read back as (0, 0) it is *transparent*, so two of a sprite's four colours were holes — a game character drawn with see-through patches. The high plane is the tile's second eight bytes (`addr + row + 8`), which is what the background fetch has always done. **No suite saw it**: no local ROM looks at the pixels it draws, so `sprite_hit` 11/11, `sprite_overflow` 5/5, every blargg suite and AccuracyCoin's 34 sat exactly where they did even with the bug in | No ROM moved (the whole baseline was run before and after: byte-identical verdicts). The regression test renders a sprite whose planes differ and rejects the old code with the pixel at `0xFF000000` — the backdrop, the sprite invisible there. Frame cost 1.83 ms (median of five; the difference from 1.86 before is inside the run-to-run spread) |
| The SHx stores' address corruption | `SHA`/`SHS`/`SHX`/`SHY` keep the index's carry into the high byte — only the addressing's discarded read happens at the un-carried address — and then AND that high byte with the register pair being written, so the store can land in a page the arithmetic never mentions; the value is the pair ANDed with the *base's* high byte plus one, and a DMA that interrupts the cycle before the write lifts that mask (Mesen's `SyaSxaAxa` and its `hadDma`). The old code kept the base's page and left the address alone, which is none of the four behaviours different 6502 revisions have — and is exactly what AccuracyCoin's behaviour probe is looking for | AccuracyCoin's five `UnOp_` tests (`SHA $93`/`$9F`, `SHS $9B`, `SHY $9C`, `SHX $9E`) all move from failing check 3 to check 15 at once, and their failing count stays at 34 because that last check is inside the tests' DMA section (its own comment: "error codes 7, 8, 9, A, B, and C" — from there on, a DMA has to be right). Every blargg suite unchanged; unit tests 602 → 603 |
| Coarse X and the attribute fetch | Coarse X used to step right after the nametable read; Mesen steps it on the group's *last* dot (`(_cycle & 0x07) == 0`, the reload dot here). The attribute fetch comes two dots after the nametable one and reads `v`'s coarse X bit 1 (which palette pair) and bits 2-4 (which attribute byte), so every background tile drew with its **right-hand neighbour's** palette, and one four-tile boundary in four read the neighbour's byte as well. The pattern fetches were untouched (their row comes from fine Y). The symptom lands only on palette boundaries: in SMB's 1-1 the right halves of the `?` blocks and the tail of the brick rows went green (the sky's pair), and the title screen's last "E" in "1 PLAYER GAME" went green | **No ROM saw it** — no local ROM looks at the pixels it draws, so every blargg suite and AccuracyCoin's 143 ran / 94 passed / 34 failing sat exactly where they did. Pixel-comparing against the whole-line renderer: `894cc07` (before the per-dot background pipeline) is **byte-identical** to the fixed code, while `5dc9e32` (the per-dot background pipeline itself) is already 304 pixels off — the defect came in with it, and `nestest`'s byte-identity check could not see it. Three regression tests (the increment's dot, the pair, the attribute byte's address) all fail on the old code. SMB's title screen and 1-1 render correctly again (248 and 283 pixels change on the same frames, all of them on palette boundaries). Frame cost: median of three on one host 3.68 ms, 3.84 ms unfixed (spread ±0.3 ms; this host is running about 2× the table's absolute numbers today) |

| The five quick ones from §10.3, in one step | NES 2.0's byte 8 read Mesen's way (`(Byte8 & 0x0F) << 8` for the mapper, `(Byte8 & 0xF0) >> 4` for the submapper, plus the archaic iNES whose mapper lives in byte 6 alone); a `$2002` read clears *only* VBlank (`UpdateStatusFlag`) and leaves the two sprite flags for pre-render; the mixer takes Mesen's constants and channel weights; power-up and soft resets are two different things (`SP = $FD` and `A/X/Y = 0` at power-on, `SP -= 3` and `I` at the button) followed by the eight dummy cycles; and the odd frame's skipped dot reads the rendering state as of the *end* of dot 338 (Mesen's `_renderingEnabled` lags a dot behind `$2001`) | **`ppu_vbl_nmi` 9/10 → 10/10** (`10-even_odd_timing` passes, printing the `08 08 09 07` its own comment predicts) and **AccuracyCoin 34 → 32**: `Suddenly Resize Sprite` and `Misaligned OAM DMA` stop failing. `cpu_reset/registers` gets past its check 2 and now reads `A X Y P S = 00 00 00 34 FD`, exactly the ROM's power-on expectation, where it used to print `S=$F7` and fail. Every reporting ROM re-measured in one pass: `instr_test-v5` 16/16, `instr_timing` 2/2, `cpu_interrupts_v2` 5/5, `apu_test` 8/8, `mmc3_test_2` 5/6 (the documented one), `oam_read`, `ppu_open_bus`, `cpu_dummy_writes` 2/2, `cpu_exec_space/ppuio`, and the five `ACCESS_DOTS = 2` ROMs (`05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `2-nmi_and_brk`, `4-scanline_timing`) all still green. Unit tests 604 → 605 |

**One stale number, corrected.** From `8ec693d` (DMA inside `MemoryRead`) until the row above,
AccuracyCoin never ran: the ROM hung inside its own DMA-sync loop while booting, `$37` stayed
zero, and the harness read that as "no ROM here" and skipped itself — so the rows in between
repeated the 46 it had at `30b7267` without measuring it (re-measuring at `0934d3a`, the commit
before this session, puts the menu back on screen, so the hang came from the DMA move, not from
the per-dot renderer). The harness now fails when the ROM reports nothing, and the number in this
row is measured.

### Experiments that were implemented, measured and reverted

Their value is the evidence; do not retry them without reading this first.

- **L2b — an NMI taking over an IRQ sequence (pre-Phase-3).** No blargg case turned green and
  AccuracyCoin lost one test. It came back later, in step 3, once the write cycles were in place.
- **L3a — deferring register writes to their real cycle, alone.** Moved `4-irq_and_dma` to the
  documented shape but broke four lines that were green, because *a register write cannot move
  alone*: the immediate-NMI detection, the A12 mask, and two length-counter lines had to move
  with it. Landed later as step 1 *with* its dependents.
- **L4a — polling the interrupt lines during a DMA stall.** `4-irq_and_dma`'s trace head is
  byte-identical with and without it; the lag is the interrupt source's phase (the frame
  counter), not the CPU's answer to DMA.
- **The frame counter's phase, implemented as three pieces.** Turned `4-jitter` and
  `6-irq_flag_timing` green and lost three AccuracyCoin tests (the two frame-counter tests and
  two error codes), so two cases for three is a regression by this document's rule. The
  disagreement is in the counter's phase — one suite is reading a different event than the
  other — and finding out which is the first thing that item needs.
- **A12 reconstructed from the dot counter, without real fetches.** `4-scanline_timing` moved
  forward but `2-details` ("clocked 241 times") regressed, because the address sequence and the
  counter's filter are one mechanism: with real addresses A12 is low for four dots between
  fetches, and it is the low run across a scanline boundary that is meant to be the only
  qualifying rise. This is the concrete argument for the per-dot renderer.
- **Open-bus decay (Mesen's per-bit, three-frame model).** It took three changes, in this
  order, to turn `ppu_open_bus` green: the `$2000-$3FFF` reads had to return the *PPU's* own
  latch rather than the CPU-side bus latch (the first thing to check, as the earlier note here
  predicted); a `$2007` read had to drive that latch with the byte it returned; and a palette
  read had to take its top two bits from the open bus while driving only its low six, with
  `$2002` driving only its top three so its low five keep decaying.

- **DMC: Mesen's model, implemented and withdrawn (§2.4).** Reading the source makes the
  model explicit, so it was implemented: the length counter and the IRQ follow the fetch,
  the register takes the buffered byte only when the bit counter reaches zero, a silence flag
  holds the DAC between samples, and the enable delays its transfer 2 cycles (3 on an odd
  cycle). It moved `8-dmc_rates` from "period is too short" to a *timeout* and put
  `7-dmc_basics` back to its second check, so it is worse than the current playback-based
  counter on both: the counter/IRQ placement cannot be aligned before the ~7-period
  disagreement above is understood. Withdrawn; the tree keeps the playback-based version and
  the ROMs' verdicts it produces.
- **DMC: the memory reader one clock ahead (§2.4).** With the length counter moving at the byte
  boundary, each fetched byte's transfer latency is added to the sample, which is the
  hundred-odd cycles that put `8-dmc_rates`'s rate-0 sample just past its window. Moving the
  reader to `bits_remaining == 1` — fetch while the last bit is on its way out, so the buffer
  is filled before the register runs dry — is the right shape, and in isolation it brings a
  standing 17-byte sample to 134.3 periods; but the ROM's verdict does not move at all (still
  past the window), which means the excess is not per-byte transfer latency, and one unit test
  (`test_reading_4015_does_not_clear_the_dmc_irq`) stopped seeing its IRQ. Withdrawn: the
  measurement says the model needs diagnosing from the bus's side (how long a DMC fetch really
  takes in `NesBus::process_pending_dma`) rather than from the channel's.
- **The frame counter's phase, twice withdrawn (§2.4).** Two rounds of guessing at Mesen's
  ordering moved one suite forward and the other back. What settled it was reading blargg's
  *source*, which is in `roms/blargg/apu_test/source/`: `6-irq_flag_timing.s` says in its
  header that the flag "is set three times in a row 29831 clocks after writing $00 to $4017",
  and its checks read `$4015` at 29830/29831/29832/29833 to confirm it — so the flag is not
  set once but held for three cycles, starting 29831 after the write. `4-jitter.s` then shows
  the restart's jitter: its read at 29831 has to see the same value on an even clock and a
  different one on an odd clock, which is what holding the counter for 3 cycles (4 on the odd
  phase) produces. The lesson for the next attempt here: read the ROM's source and its
  comments first, and print the cycle the event lands on.
- **The DMC's ~7-period disagreement, diagnosed (not an experiment).** Both DMC verdicts were
  reading the bus side and the channel side at once: the channel was handing the CPU *two*
  four-cycle transfers per fetched byte (see §7's last row), so a byte cost 440 CPU cycles
  instead of 432 and every sample ran long. With the transfer charged once, the channel's own
  numbers are the thing to compare again — which is why Mesen's read-time counter (§2.4) is
  worth a second attempt instead of another guess.
- **`cpu_interrupts_v2`: the frame counter's flag one cycle later (`$4017` hold 4/3 → 5/4).**
  Measured: `3-nmi_and_irq` **passes**, and `5-branch_delays_irq`'s `test_jmp` table comes out
  *exactly* as the test's readme prints it. But `4-jitter` ("Odd jitter not handled properly"),
  `5-len_timing` ("First length of mode 0 is too late") and `6-irq_flag_timing` ("Flag first set
  too late") all go red, and AccuracyCoin drops 34 → 36 failing. Reverted: those three pin the
  flag's cycle against the `$4017` write, so the missing cycle is not in the flag.
  (It was not: the flag is late by one cycle because the *APU* was late by one cycle — the tick
  sat behind the cycle's access. Moving the tick moves the flag with everything still attached
  to it, which is why that experiment cost three suites and this one costs nothing.)
- **`cpu_interrupts_v2`: the IRQ judged one cycle later (the sample from two cycles back
  instead of one).** Measured: `3-nmi_and_irq` passes, and `5-branch_delays_irq`'s `test_jmp`,
  `test_branch_not_taken` and `test_branch_taken_pagecross` tables match the readme
  **row for row** — the first time this emulator reproduces that table — with only the last
  sub-test (the commented "special case") still off (`05 05 03` where the readme has
  `05 01 00`). But `1-cli_latency` check 4 ("exactly one instruction after CLI should execute
  before IRQ is taken") and `4-scanline_timing` check 3 go red. Reverted: those two pin the
  ordinary IRQ judgement to the penultimate cycle, which is Mesen's rule too.
  (Which is what the tables turned out to want as well: what they needed was for the *APU* to
  move, not for the judgement to.)
- **`cpu_interrupts_v2`: the `$4017` write's parity inverted.** Measured: `5-branch_delays_irq`
  is byte-identical (so its write's hold is not what is off), `3-nmi_and_irq`'s output changes,
  and AccuracyCoin drops 34 → 36. Reverted.

  **What settled it.** All three failures were one ±1-cycle question, and the answer was not in
  the flag, the judgement or the write's parity but in where the *APU* sits inside the CPU
  cycle: every sample of the interrupt line was one cycle early because the APU ticked after the
  cycle's access instead of before it. Measuring the chain first is what found it — the
  cycle-by-cycle trace of `5-branch_delays_irq`'s `begin` showed the flag going up on the
  `RTS`'s *fifth* cycle (write + 29830) where the readme's row 0 wants it on the sixth (write +
  29831, the RTS's last cycle) — and clocking the APU at the front (Mesen's `ProcessCpuClock`)
  is one change with no knob in it: all three tables go green together, and the three suites
  that pin the flag (`4-jitter`, `5-len_timing`, `6-irq_flag_timing`) stay green because they
  moved *with* it. The lesson for the next ±1-cycle hunt: when three groups of tests disagree
  about a cycle, look for a chip that is not where Mesen keeps it, not for a number to add.
- **The DMA's dummy reads, first attempt (the 13 AccuracyCoin DMA cases).** Reading the ROM's
  source first paid off: `DMA + $2007 Read` check 3 is `CMP #$03 / BMI fail` over the byte a
  `LDA $2007` returns after a DMA lands in its last cycle, which only comes out right if the
  DMA's **dummy cycles are real reads at the address the CPU was about to use** — Mesen's
  `ProcessPendingDma` reads `readAddress` in the halt cycle, in the DMC's extra dummy-read
  cycle and in the OAM transfer's alignment cycles (ours performs bare cycles there, so a
  `$2007` in that window never moves `v` and the value comes back 0/1/2).
  The first implementation of that was measured and **withdrawn**: Mesen's loop gives the DMC
  and the sprite transfer a fixed priority and consumes exactly one of `_needHalt` /
  `_needDummyRead` per branch, and a version that approximated that handshake moved
  `4-irq_and_dma`'s table (its `+526` row became instruction 9 where the readme has 8) and
  **hung AccuracyCoin's boot** — 0 tests ran, which the harness's "fail instead of skip" caught
  (every unit test still passed). The next attempt should port `ProcessPendingDma` verbatim:
  the `(_state.CycleCount & 0x01) == 0` parity, the two-flag handshake, `_abortDmcDma`, and
  `skipDummyReads`/`skipFirstInputClock` for `$4016`/`$4017` — with the OAM total staying at
  513/514 cycles.

  A second attempt did exactly that much — the parity, the two-flag handshake, `skipDummyReads`
  and the OAM bytes going in through `$2004` — and was measured and withdrawn as well: the same
  `4-irq_and_dma` row moved, and AccuracyCoin booted into its own trap this time. `$37` stayed
  zero (0 tests ran) and the ROM came to rest on `$80DD`, a `JMP $80DD` it reaches right after
  `JSR $F109` (`LDA $F0 / ORA #$80 / STA $F0 / STA $2000`) and `INC $EC` — i.e. inside the boot,
  before the menu ever appears. What that narrows it to: with the dummy reads in place the DMC's
  fetch takes 3 or 4 cycles depending on the parity, where the model we have always charged 4,
  and AccuracyCoin's boot *synchronises against the DMC's byte* (an earlier row here records a
  hang with the same signature) — so the byte's delivery cycle has to move together with the
  dummy reads. `dmc_sync_rom`, which is a simplified model of that sync, still passed, so the
  next attempt should write the *real* sync loop as its test and check the channel's
  byte-boundary delivery against it, not just the DMA's length. Two smaller things belong in the
  same port: our machine does not sample the interrupt lines during DMA cycles (Mesen's
  `StartCpuCycle`/`EndCpuCycle` do), and `$4016`/`$4017` also need `skipFirstInputClock` and
  `ProcessDmaRead`'s internal-register merge.

## 8. Decisions still open

**Decided**: the renderer went all the way (background *and* sprites, §2.1); PAL runs 16/5 with
the fractional dot accumulator (§4); the expansion chips keep their unit tests rather than a
golden-audio comparison; the access dot (§4) waits for an experiment with `ppu_bus_trace`; and
`6-MMC3_alt` needs a per-ROM override list or a CRC-keyed database rather than a submapper — its
header is plain iNES, which §2.5 now records.

1. **The access dot** (§4): run the `05-nmi_timing`/`06-suppression`/`02-vbl_set_time` experiment
   with `Nes::ppu_bus_trace` and keep `ACCESS_DOTS = 2` unless it says otherwise.
2. ~~**The APU frame counter's phase** (§2.4)~~ — done: the numbers come from the measured
   cycles blargg's own sources state (§7), and `4-jitter`, `5-len_timing` and
   `6-irq_flag_timing` are green.
3. ~~**DMC** (§2.4)~~ — decided and landed: the ~7 periods were not a disagreement between
   models but the artefact of the bus charging a fetch twice (§7). Mesen's shape passes both
   checks.

---

## 9. The ledger (start a new session here)

**How to use it**: every row is *status → evidence → next step*. Re-run the verification from §6
first, then pick a row. Nothing goes into this document without a measurement behind it.

**The suites are out of the tree right now (2026-10-16), on purpose.** A full run cost minutes, and
the alignment work (§10) needs a loop measured in seconds: `cargo test --workspace` is down to
~11 s (604 unit tests). The ROMs stay under `roms/` and every suite comes back from git — with the
rules it carried: it failed rather than skipped when a ROM stopped reporting, because a ROM that
never runs its suite is what a boot-time hang looks like, and that is exactly how the DMC's
double-charged fetch hid here for several steps (§7). Re-add them when the pass ends, together with
the suites §10.6 lists.

**Last measured baseline** (2026-10-16, before the removal): `instr_test-v5` 16/16, `instr_timing` 2/2, `branch_timing_tests` 3/3,
`blargg_ppu_tests` 5/5, `oam_read` 1/1, `oam_stress` 1/1, `ppu_open_bus` **1/1**,
`sprite_hit_tests` 11/11, `sprite_overflow_tests` 5/5, `apu_test` **8/8**, `cpu_interrupts_v2`
**5/5**, `mmc3_test_2` **5/6**, `ppu_vbl_nmi` 9/10, AccuracyCoin **34 failing** (140 ran, 94 passed, 0 skipped — the same 34 as
before the APU move; §10.5 has the per-item list). Unit tests: 604. Frame cost: about 1.86 ms (release, 600 frames of nestest,
median of five; spread ±0.06). **Frame cost is a same-run comparison only**: this machine has
reported 1.82 ms (1.815/1.825/1.826, idle) and 3.68 ms (the 10-15 round, with other load on it)
for the same test, so the difference is the load rather than the code — the 5–8% in `0520b59` is
the Android front end turning the PPU bus trace off for itself
(`set_ppu_bus_trace_capacity(0)`); the core's default is unchanged. Branch: `main` (the alignment
work and the front-end work merged here on 10-16; `codex/mesen-alignment` stops at `226bd01`).

| # | Item | Status | Evidence (commit / measurement) | Next step |
| --- | --- | --- | --- | --- |
| S0 | Public per-dot observation API (`Nes::ppu_bus_trace`, `step_dot`, exported from wasm and uniffi) | Done | `c8957a7`; four unit tests | Use it for a differential when one is needed |
| P1 | Per-dot PPU renderer (background + sprites), A12 from real fetches | Done | `5dc9e32`, `30b7267`: `4-scanline_timing` green; `RenderTiming`, `ppu.render_v` and the whole-line renderer deleted | The sprite *evaluation* is still per line (§2.1), when it is wanted |
| P2 | DMA inside `MemoryRead` (`process_pending_dma`) | Done | `8ec693d`: `4-irq_and_dma`'s trace is byte-identical | Nothing (DMC's prerequisite is in) — but it is also where the DMC's double-charged fetch came in; see the last §7 row |
| P3a | APU frame counter phase (`$4017` three cycles late, IRQ flag three cycles in a row) | Done | `a1fbaa3`: `4-jitter`, `5-len_timing`, `6-irq_flag_timing` green | Nothing |
| P3b | DMC | Done | `f9984d4` (two semantics bugs), the one-shot fetch latch (§7), and Mesen's channel shape: counter at the fetch, byte held in the buffer, silence flag, period = the table's value. **`7-dmc_basics` and `8-dmc_rates` green** (apu_test 8/8); AccuracyCoin 43 → 34 | Nothing for the roms; the mixing rate (P7) and the DMC IRQ's effect on the CPU's IRQ line are the neighbouring items |
| P4 | §2.2's register details: open-bus decay, rendering-time `$2007`, grayscale mask, PAL emphasis swap, and the nine colour bits' RGB | Done | `af2f375`, `8247f10`, and the 512-entry emphasis table (§7): `ppu_open_bus` green; eight unit tests | Nothing — the sprite *evaluation* being per line (P1) is the neighbouring item |
| P5 | PAL 16/5, fractional dot accumulator, region-correct audio clock | Done (the ratio) | `dbdf7cc`: a PAL frame is 33,247.5 CPU cycles | PAL's own PPU/APU rules (`_nmiScanline`, `_vblankEnd`, PAL's OAM refresh) — unverifiable without a PAL ROM |
| P6 | Mappers: NES 2.0 header, bus conflicts | Partly done | `4e1c7a6`: conflicts for 2/3/7/11/66/71; five header and two conflict tests | Dispatching on the submapper (Mesen reads that from its CRC-keyed database, not the header); then boards 64/158/185/115 |
| P7 | Audio resampling | Done | `e33ebeb`, the §7 row: the band-limited resampler; four unit tests on the filter itself (step level, AC coupling, in-band amplitude, the out-of-band fold) plus the machine-level silence test | Nothing for the local ROMs; a reference recording would be the only way to judge the remaining difference from Mesen's two-stage chain |
| P3c | `cpu_interrupts_v2`'s three remaining tables | **Done** | The chain was measured cycle by cycle (§7) and the missing cycle was the APU's place in it: the chip clocks the APU at the *start* of a CPU cycle (Mesen's `ProcessCpuClock`), not at the end. **`cpu_interrupts_v2` 2/5 → 5/5** — `3-nmi_and_irq`, `4-irq_and_dma` and `5-branch_delays_irq` all pass, and everything that pinned the flag against the `$4017` write stayed green. §2.4; the two withdrawn knob experiments are explained in §7 | Nothing; the neighbouring item is the sprite *evaluation* being per line (P1) |
| B1 | A game's sprites drawn with holes ("transparent" characters) | **Done** | The sprite pattern fetch's high plane came from the low plane's address (§7), so colour 2 pixels read as colour 0 and vanished. Fixed to `addr + row + 8`; unit test `test_a_sprites_high_plane_is_the_tiles_second_half` fails on the old code | Nothing. Worth remembering: the ROM baseline is blind to drawn pixels — a game, or a unit test, is what catches this class |
| B3 | The unofficial `SHx` stores' address corruption (5 AccuracyCoin cases) | Partly done | Modelled on Mesen's `SyaSxaAxa`: the index's carry into the high byte is kept, that byte is ANDed with the register pair, the value is masked by the base's high byte plus one, and a DMA in the cycle before the write lifts the mask (§7). The five `UnOp_SHA`/`SHS`/`SHY`/`SHX` tests move from check 3 to check 15 | That last check is in the tests' own DMA section — it goes with the next cut, the 13 DMA cases |
| B4 | AccuracyCoin's DMA group (13 cases) | **In progress** | The mechanism is identified: the DMA's halt and dummy cycles have to be *reads at the address the CPU was about to use* (`DMA + $2007 Read` check 3 counts them). Two implementations were measured and withdrawn: both moved `4-irq_and_dma`'s table and hung AccuracyCoin's boot (the second one reaching the ROM's own trap at `$80DD`); §7 has both sets of evidence and the narrowed next step | Port Mesen's `ProcessPendingDma` **verbatim**: the parity, the two-flag handshake, `_abortDmcDma`, and the `$4016`/`$4017` skip rules — with the OAM transfer still 513/514 cycles |
| B2 | Background tiles drawn with their right-hand neighbour's palette (`?` blocks, the tail of brick rows and of title text going green) | **Done** | Coarse X stepped on before the attribute fetch (§7's row): fixed at the reload dot, with three regression tests that fail on the old code | Nothing; the neighbouring item is §2.2's fetch enable (Mesen keeps fetching with the background off) |
| — | The access-dot experiment | Done | `f5943b3`: 0 and 1 each cost five suites, 2 passes every one — so `ACCESS_DOTS = 2` stands | Nothing |
| C1 | **The file-by-file comparison with MesenCE (§10)** | Done | The whole `Core/NES` tree was fetched and read against this core (551 files, ~275 boards). Twelve defects found, including the NES 2.0 nibble swap, `$2002` clearing the sprite flags, the CPU's reset `SP`, and the controller ports' open bus; twelve areas confirmed identical; AccuracyCoin's 34 re-read as "which check failed" (`(value >> 2) + 1`, which moves the §7/§9 labels by one) | Follow §10.7: the DMA port → `$4016`/`$4017` with `read_joy3` → the PPU's register state machines → `$2002`'s flags and per-dot sprite evaluation |
| C2 | **The ROM suites removed for the alignment pass** (§6/§9) | Done | `tests/rom_tests.rs`, `tests/common/` and `tests/golden/` deleted on 10-16 (recoverable from git: `b6c06a1` and earlier; a copy sits in `/tmp/nes-baseline-snapshot-20261016`). `cargo test --workspace` 130 s → ~11 s, 604 unit tests green. The suites §10.6 lists were staged under `roms/nes-test-roms/` (37 ROMs, gitignored) and measured once each | Re-add everything when the alignment pass ends, with the suites §10.6 lists: at that measurement `cpu_dummy_writes` (2/2) and `cpu_exec_space`'s `ppuio` ROM already reported "Passed", `cpu_exec_space_apu` failed at open-bus check 2, and `cpu_reset/registers` failed at check 2 with `SP=$F7` where the ROM wants `$FD` — the defect §10.3 #3 fixes |
| C3 | The five quick §10.3 items: NES 2.0 nibbles, `$2002` flag lifetime, Mesen's mixer, the reset split + eight warm-up cycles, and the one-dot-lagged rendering state for the odd frame's skip | **Done** | §7's row has the whole measurement. `ppu_vbl_nmi` 10/10, AccuracyCoin 32 failing, `cpu_reset/registers` past check 2 with the expected `S=$FD`; the whole reporting-ROM set re-measured with no regressions; unit tests 605 | §10.3's remaining items, in §10.7's order: the controller ports and `$4016`'s write delay (#4/#5), then the PPU's register state machines (#6), then the DMA port (#7) — with the APU edge timings (#8) and the dead `addressing.rs` (#10) as sidecars |

**One thing to get right first**: AccuracyCoin's count is *our* baseline, not Mesen's — it is not
part of Mesen's test suite (§5.1). Every item's oracle has to be Mesen's source, with the ROM's
verdict as evidence rather than as the target; a few of them may be cases where the ROM wants a
behaviour Mesen deliberately does not model, and those belong in §4 as deliberate deviations
rather than as reasons to bend the emulator.

**AccuracyCoin's 34 have been sorted by name** (13 DMA edge cases, 7 CPU instruction details, 5
unofficial `SHx` opcodes, 3 sprites, 3 APU, 2 controllers, 1 PPU); the list is in
`docs/architecture.md` §13.1. The thirteen DMA ones fail on the same two checks, which says they
are *one* mechanism — that is the first cut worth making.

**How to get Mesen's sources**: take the whole tree rather than single files — it is 3.8 MB and one
command (§10.1 has the recipe: a blob-filtered clone plus a sparse checkout of `Core/NES`,
`Core/Shared`, `UI/Dependencies` and `UI/Config`). Reading files one at a time
(`curl -sSf -O https://raw.githubusercontent.com/SourMesen/Mesen2/master/Core/NES/NesPpu.cpp`) is what
earlier sessions did, and it is how the `SourMesen/Mesen2` and `nesdev-org/MesenCE` trees got mixed
up: they differ by 5–100 lines per file. **`nesdev-org/MesenCE` is the one to read** (§5.1), and
`UI/Dependencies/MesenNesDB.txt` comes with it. What has been read and written into this document is
listed in §10.1.

**Conventions to hand over**: ROMs are never committed (a suite with no `roms/` directory prints a
skip); a document change touches both languages (`*.zh-CN.md`); each step's numbers go into its
commit message and §7; and a regression that comes from being *more* faithful to Mesen is recorded
in §7's withdrawn-experiment list rather than reverted.

---

## 10. File-by-file comparison with MesenCE's NES core (2026-10-16)

§2 lists the gaps that were known; this section is the result of reading the two trees side by side,
subsystem by subsystem, so it also catches the things nobody had looked for. It is the reference to
take a task from: every row names both sides, and §10.4 names what is *already* identical so the
work is not repeated.

### 10.1 What was compared, and how to get the same tree

Mesen's maintained fork is `nesdev-org/MesenCE` (§5.1). This pass read it as a real checkout, not as
per-file copies:

```bash
git clone --depth 1 --filter=blob:none https://github.com/nesdev-org/MesenCE.git /tmp/MesenCE
cd /tmp/MesenCE
git sparse-checkout set --cone Core/NES Core/Shared UI/Dependencies UI/Config
git checkout                      # fetches the blobs the sparse set needs
```

`HEAD` at the time was `4b8a196`. `Core/NES` is 551 files: the top-level translation units are
10,644 lines (`NesPpu.cpp` 1,769, `BaseMapper.cpp` 1,383, `NesCpu.h` 914, `NesConsole.cpp` 775,
`MapperFactory.cpp` 670, `NesCpu.cpp` 612, `NesTypes.h` 527, `GameDatabase.cpp` 346,
`NesControlManager.cpp` 303, `NesHeader.cpp` 250), and `Core/NES/Mappers/` is another 296 files
(1.3 MB), with 275 board headers included and 326 `case` labels in `MapperFactory::GetMapperFromID`.
The files this core has an opinion about were read in full:
`NesCpu.{h,cpp}`, `NesPpu.{h,cpp}`, `BaseNesPpu.{h,cpp}`, `DefaultNesPpu.h`, `NesConsole.{h,cpp}`,
`NesMemoryManager.{h,cpp}`, `OpenBusHandler.h`, `InternalRamHandler.h`, `NesControlManager.{h,cpp}`,
`Input/NesController.h`, `NesConstants.h`, `NesHeader.cpp`, `Loaders/iNesLoader.cpp`,
`APU/*` (all eleven), `NesSoundMixer.{h,cpp}`, `Shared/Audio/SoundResampler.cpp`,
`NesDefaultVideoFilter.cpp`, `BaseMapper.{h,cpp}`, `Mappers/Nintendo/MMC3.h`, `Mappers/A12Watcher.h`,
`Shared/SettingTypes.h` (`NesConfig`'s defaults) and `UI/Dependencies/MesenNesDB.txt`.

Two consequences of doing it this way, both worth remembering:

- Earlier sessions read single files from `SourMesen/Mesen2`. The trees are close but **not
  identical** (the same files differ by 5-100 lines: `ApuFrameCounter.h` 236 vs 256,
  `NesPpu.cpp` 1,614 vs 1,769), so every claim has to say which fork it came from. The rows below
  are MesenCE's.
- `NesConfig`'s defaults are the definition of "Mesen's behaviour" (a setting that is off by default
  is not something to match). The ones this core cares about: `EnableOamDecay = false`,
  `EnablePpuOamRowCorruption = false`, `EnablePpu2000ScrollGlitch = false`,
  `EnablePpu2006ScrollGlitch = false`, `DisablePpu2004Reads = false`, `DisablePaletteRead = false`,
  `RestrictPpuAccessOnFirstFrame = false`, `RemoveSpriteLimit = false`,
  `ForceBackgroundFirstColumn = false`, `ForceSpritesFirstColumn = false`,
  `EnableDmcSampleDuplicationGlitch = false`, `SwapDutyCycles = false`,
  `ReverseDpcmBitOrder = false`, `SilenceTriangleHighFreq = false`, `ReduceDmcPopping = false`,
  `IsFullColorPalette = false`, `RandomizeCpuPpuAlignment = false`,
  `RandomizeMapperPowerOnState = false`, **`RamPowerOnState = Random`**, `InputScanline = 241`,
  `ConsoleType = Nes001`, `Region = Auto`, and the overclocking scanline counts at 0.

Mesen's own numbers, for the arithmetic later steps need: clock 1,789,773 / 1,662,607 Hz
(`NesConstants.h`); `cpuDivider`/`ppuDivider` 12/4 NTSC, 16/5 PAL, 15/5 Dendy (`NesCpu::Reset`,
`NesPpu::UpdateTimings`); the CPU cycle's access split `_startClockCount`/`_endClockCount` 6/6 NTSC,
8/8 PAL, 7/8 Dendy (`NesCpu::SetMasterClockDivider`); `_ppuOffset = 1`; `_nmiScanline` 241/241/291
and `_vblankEnd` 260/310/310; 341 dots a line; the APU's `_stepCycles` and the DMC's period tables.

### 10.2 What is still different

#### Machine and CPU

| | MesenCE | This core | What it costs |
| --- | --- | --- | --- |
| Reset | `NesCpu::Reset`: `CycleCount = -1`, a master-clock offset of `cpuDivider`, `_ppuOffset = 1`, then **eight dummy cycles** before the first instruction; power-up sets `SP = $FD`, `A/X/Y = 0`, `PS = I` and does *not* subtract 3 from SP (only a soft reset does) | `Cpu::reset` (`cpu.rs`) subtracts 3 from an SP that starts at `$FD` - so a power-up here leaves **`SP = $FA`** - sets the cycle count to 0, and runs no warm-up cycles | `cpu_reset`-style ROMs; the PPU/APU's phase when the first instruction runs |
| The access' place inside a cycle | `_startClockCount`/`_endClockCount` per region: the access sits 5 of 12 master clocks into an NTSC cycle, 7 of 16 on PAL, 6 of 15 on Dendy | `ACCESS_DOTS = 2` dots, the same number in every region - on PAL that is 10 of 16 ticks, not 7 | NTSC is measured and deliberate (§4); PAL's position has never been measured, and there is no PAL ROM |
| DMA | `NesCpu::ProcessPendingDma` reads `readAddress` on the halt cycle, the DMC's dummy cycle and the OAM alignment cycle; the DMC fetch is **3 or 4 cycles by parity**; DMC and sprite transfers share a two-flag handshake; `StopDmcTransfer`/`_abortDmcDma` cancel at defined points; `$4016`/`$4017` have skip rules; a DMC DMA ending next to a bit-counter reset can re-request (config-gated) | `NesBus::process_pending_dma` (`nes.rs`): a fixed 4-cycle DMC fetch, no bus read on any halt/dummy cycle, no abort path, no parity | The 13 AccuracyCoin DMA items, `4-irq_and_dma`, `Misaligned OAM DMA`, `DMC DMA + OAM DMA`, `Explicit`/`Implicit DMA Abort` |
| Interrupt lines during DMA | DMA cycles are ordinary `StartCpuCycle`/`EndCpuCycle` pairs, so `_prevRunIrq`/`_needNmi` keep being sampled | `advance_one_cpu_cycle` is the bus's own begin/end pair, and `Cpu::end_cycle` - the only place the lines are sampled - is not called, so an interrupt arriving inside a stalled transfer is never latched | `cpu_interrupts_v2`, `DMA + Open Bus` |
| Dendy | A third region: `cpuDivider` 15, `ppuDivider` 5, `_nmiScanline` 291, `_vblankEnd` 310, `startClockCount` 7 | No such region; a Dendy cartridge runs as NTSC | Unverifiable here (no Dendy ROM); it is the one region a header can name that the core cannot represent |
| Unofficial `KIL`/`JAM` | `NesCpu::HLT` parks the CPU *and* clears the interrupt samples, so a hang cannot be interrupted | The CPU parks but stays interruptible (`Cpu::jam`), so a host can report and recover the hang | Deliberate (§4) |
| `cpu/addressing.rs` | - | 760 lines of addressing plus a whole test module that **no longer runs**: `instructions.rs::fetch_operand` is the live implementation and nothing outside the module references `addressing::` | Not a fidelity gap, a trap: those tests pin a model the machine does not use, and one of them pinned the wrong SHx behaviour before it was replaced |

#### PPU

| | MesenCE | This core | What it costs |
| --- | --- | --- | --- |
| `$2002` read | Clears **only** the VBlank flag and the CPU's NMI latch (`UpdateStatusFlag`); sprite overflow and sprite 0 hit are cleared at pre-render cycle 0 | `ppu.rs` clears VBlank **and** sprite overflow **and** sprite 0 hit on every read | Any ROM that reads `$2002` before it is done with the other two flags; AccuracyCoin reads bit 5 and then expects the flag's state to still mean something |
| `$2002` bits 0-4 | Open bus, `openBusMask = 0x1F`, plus the 2C05 variants (`ProcessStatusRegOpenBus`) | The same five bits from the PPU's own latch | Aligned; the 2C05 variants only matter to VS System games |
| `$2004` | Reads while rendering return `_oamCopybuffer`, refilled from secondary OAM on dots 257-340 and 0; writes while rendering are *dropped* and bump OAMADDR's high six bits (`+4 & 0xFC`); attribute bytes read back masked `& 0xE3`; the increment has PAL parity rules | A plain OAM read/write with an unconditional increment | OAM traffic inside a line; `Misaligned OAM DMA` (Mesen's transfer writes go through `$2004` and inherit these rules) |
| `$2007` read | Returns the buffer, and the buffer refills **five PPU cycles later** (`_ppuMemoryDataReadStateMachine = 5`); `v` increments one cycle after that (`_needVideoRamIncrement`); a second read within six dots returns open bus | Immediate buffer swap and immediate `v` increment; the six-dot window is modelled (`_ignoreVramRead`) | `ppu_read_buffer`, `full_palette`; the DMA items that read `$2007` |
| `$2007` write | The value is latched and written five PPU cycles later, and **during rendering the value is thrown away and the address' low byte is written instead** | Written immediately and always with the real value | The same suites; any game that writes VRAM while rendering |
| `$2006` | `v` is updated **three PPU cycles later** (`_updateVramAddrDelay = 3`), and `t` follows | `v = t` on the write's own cycle | Mid-frame splits that reprogram `v` just before the increment |
| Bus address to the cartridge | `SetBusAddress` is called from every `ReadVram`/`WriteVram` **and** from non-fetch places: `UpdateVideoRamAddr` while not rendering, the start of vblank, scanline 0's first cycle, a mid-screen rendering change, the delayed `$2006` update | Only the addresses the pipeline really read (`take_bus_access`), fed once per dot | An A12 filter sees fewer transitions than Mesen's - which is why the A12 work had to be measured dot by dot (§7) |
| Sprite evaluation | Dot by dot, 65-256: secondary OAM cleared at 1-64, `ProcessSpriteEvaluationStart` at 65 with `_sprite0Added` decided at dot 66, the OAM address bug (`spriteAddrH`/`spriteAddrL` moving together), the `_oamCopyDone` realignment and the pseudo-overflow counter | One call at dot 65 fills the next line's secondary OAM | `oam_flicker_test*`; AccuracyCoin's `$2002 flag timing` check 2 (`$55 == $60`, sprite 0 and overflow in the same read); `Arbitrary Sprite zero`; `Suddenly Resize Sprite` |
| Sprite pixels | Eight hardware shifters plus `_dotSkipped`, with counting/active/expired modes and extra sprites for `RemoveSpriteLimit`; the odd frame's skipped dot delays every sprite by one pixel (`_spriteShifterList[i] += 1 << 4`) | Recomputed per dot from the per-line slots | The odd frame's one-pixel sprite delay is missing; the sprite-limit options do not exist |
| Background fetches with the background off | `LoadTileInfo` is gated on `IsRenderingEnabled()` (background **or** sprites), so tiles are still fetched and shifted and `v` still advances while only sprites are on | The whole background path is gated on `SHOW_BG` | A12 while only sprites are enabled (MMC3 clocks differ); no local ROM covers it |
| Left-column masks | Three thresholds, `_minimumDrawBgCycle`/`_minimumDrawSpriteCycle`/`_minimumDrawSpriteStandardCycle`, each 8 or 300 depending on the mask and the "force first column" settings | `x < 8` checks against the two mask bits | Equivalent for the default settings; the three thresholds are not the same number in every case |
| Forced blank | With `v` in `$3F00-$3FFF` and rendering off, `DrawPixel` shows `paletteRam[v & 0x1F]` | Shows the backdrop | `full_palette`-style ROMs |
| Reset state | Palette RAM takes a fixed 32-byte boot pattern (or random, since `RamPowerOnState = Random` is the default); `v` is *not* cleared by a reset, only at power-on; `_scanline = -1`, `_cycle = 340`, `_frameCount = 1` | Palette, VRAM and OAM are zeroed, `v = 0`, `scanline = 0`, `cycle = 0`, `frame = 0` | `power_up_palette` and `palette_ram` in `blargg_ppu_tests` (green today because they write what they test) |
| Frame boundary | `SendFrame` runs at scanline 240, cycle 0, so the frontend's frame starts at vblank | The frame swaps when the pre-render line wraps | Host-visible only: our boundary is one line earlier |
| OAM decay | `ReadSpriteRam` with per-row stamps, **off by default** (`EnableOamDecay = false`); the stamp's window is `OamDecayCycleCount = 4500` (~40 scanlines) | Not modelled | Nothing: "not done" is Mesen's default (§2.2). §2.2's "3000" is `SourMesen/Mesen2`'s value — an example of the two trees disagreeing (4500 here) |

#### APU and audio

| | MesenCE | This core | What it costs |
| --- | --- | --- | --- |
| Length counter load | `LoadLengthCounter` only records `_reloadValue`/`_previousValue`; `ReloadCounter` applies it **after the frame counter has been run for that cycle**, and only when `_counter == _previousValue`; the halt flag is applied there too | `LengthCounter::load` writes the counter immediately on the register write | Blargg's `len_reload_timing` tests 4 and 5 - the ROM Mesen's own comment cites |
| Frame counter | Six-entry tables (`...29828, 29829, 29830` and `...37281, 37282`), a `_newValue` pending write with a 3/4-cycle delay, `_blockFrameCounterTick = 2` (a tick blocks the next two cycles' writes from clocking), and `GetIrqFlag` **auto-clears the flag at the start of the next APU cycle after it is read** | Five-entry tables with a `hold` and a three-cycle `frame_counter_irq_window` | `4-jitter`/`5-len_timing`/`6-irq_flag_timing` pass; AccuracyCoin's `Frame Counter IRQ` and `Controller Strobing` (check 3 reads the put/get transition) do not |
| DMC | `SetEnabled` delays the effect by 2/3 cycles (`_disableDelay`) and the transfer start by 2/3 (`_transferStartDelay`), aborts a transfer that has not started, and has the sample-duplication and one-byte-sample cases (config-gated); the timer's period is stored as the table's value minus one | `set_enabled` is immediate; the period is the table's value | `dmc_dma_start_test`, `dmc_dma_start_test_v2`, AccuracyCoin's `DMC Reload Timing`, and the DMC half of the DMA group |
| Mixer | Non-linear with its own constants: `95.88 * 5000 / (8128 / square + 100)` and `159.79 * 5000 / (22638 / tnd + 100)`, `tnd = dmc + 2.7516713261 * triangle + 1.8493587125 * noise`, all in the channels' raw 0-15 units, then a factor of 4 into blip | `95.52 / (8128 / pulse + 100)` and `163.67 / (24329 / tnd + 100)` with `tnd = dmc + 3 * triangle + 2 * noise`, in floats | A few percent of channel balance: at full volume the triangle lands at 0.2464 (Mesen) against 0.2555 (here), so the triangle is 3.7% and the noise 3.0% louder while the pulses are 0.4% quieter. `apu_mixer` and `apu_mixer_recordings` are the ROMs that can judge it |
| Expansion audio mixing | Linear deltas with fixed weights: FDS 20, MMC5 14, N163 20, 5B 15, VRC6 5, VRC7 1 | Each expansion source has its own path | Audible only on those boards; no goldens here |
| Resampling | blip at 96 kHz (`bass_shift = 9`), then a Hermite resampler chasing the sound card's real rate | One band-limited stage straight to the device rate | Deliberate, documented (§2.4/§7) |

#### Memory, open bus and input

| | MesenCE | This core | What it costs |
| --- | --- | --- | --- |
| Data bus | Two latches, `NesCpuBusType::Internal`/`External` (`OpenBusHandler.h`): reads update both, a `$4015` read updates the internal one *only*, and a DMA read can update one without the other | One `open_bus` latch on the bus, written on every read and every write | AccuracyCoin's `Internal Data Bus`, `Open Bus`, and the `$4015` half of `APU Register Activation` |
| Internal RAM | `RamPowerOnState = Random` by default, and the Famicom Box's 8 KB variant exists (`InternalRamHandler<0x1FFF>`) | Zeroed 2 KB | `cpu_reset`-style ROMs that read RAM before writing it |
| `$4016`/`$4017` read | `openBus(GetOpenBusMask(port)) \| device`, where the mask depends on the console type ($E0 for a NES-001, $E4 for port 1 of a NES-101, $F8 for an HVC-001); a read of the same address within one CPU cycle returns the previous value (`ReadDevice`); after eight reads the controller's shift register fills with 1s | `bit \| 0x40` every time, always shifting, and the register empties to 0 after eight reads | `read_joy3`; AccuracyCoin's `Open Bus` (check 2), `APU Register Activation` (check 2) and `Controller Clocking` (check 2) |
| `$4016` write | The strobe and OUT pins are updated at the next PUT cycle (`_writePending = (masterClock & 1) ? 1 : 2`, applied from `ProcessCpuClock`) | Applied on the write's own cycle | AccuracyCoin's `Controller Strobing` |
| Input sampling | The control devices are refreshed once a frame at PPU scanline `InputScanline = 241` | The host's button state is whatever it last set | A host that changes a button mid-frame sees it at once instead of at vblank |

#### Cartridge, loaders and video

| | MesenCE | This core | What it costs |
| --- | --- | --- | --- |
| MMC3's IRQ revision | `_forceMmc3RevAIrqs = DatabaseInfo.Chip.substr(0,5) == "MMC3A"` - per game, from the database | Neither revision: the rev-B condition only | `6-MMC3_alt` (see §10.6 - **Mesen fails it too**) and the games whose board says `MMC3A` |
| `$C001` | `_irqCounter = 0; _irqReload = true` | `irq_counter \|= 0x80` - blargg's "pathological" behaviour, which Mesen does not implement | Nothing observable: every counted rise reloads from the latch either way |
| MMC6 | `SubMapperID == 1` switches the work RAM to four $400-byte banks with their own read/write enables | Not modelled | mapper 4 submapper 1 games - and it needs the NES 2.0 fix below to even be visible |
| A12 for non-MMC3 boards | `A12Watcher::UpdateVramAddress(addr, frameCycle)` with `minDelay = 10` **frame cycles** (PPU dots), used by the MMC4/VRC/FME7-style boards | `Mapper::on_ppu_bus(addr, cpu_cycles)` with a three-CPU-cycle rule | Only MMC3 has the filter here; the other boards' watchers do not exist |
| Bus conflicts | `BaseMapper::WriteRam` ANDs **only on register addresses** (`_isWriteRegisterAddr[addr]`), and the database can force the flag per game (`BusConflictType`) | `NesBus::write_now` ANDs on every cartridge write when the board declares conflicts | PRG RAM writes on a conflict board would be corrupted here (no such board in the local set) |
| Mapper hooks | `EnableCpuClockHook()` -> `ProcessCpuClock()` **every CPU cycle**, declared by Sunsoft FME7/3/4, IremH3001, Kaiser 202/7017, Mapper 40/42/43/50/83/106/253, Tf1201, Fk23C, Smb2j, CityFighter, T230, Yoko, DripGame and Rainbow | `on_cpu_cycles(cycles)` once per instruction, plus `on_scanline` | A mapper IRQ that should fire mid-instruction fires up to six cycles late (mappers 21/24/16/18/5/69 here are the ones that use it) |
| Board coverage | 296 mapper files, ~275 `case`s in `MapperFactory::GetMapperFromID`, plus the database overrides | 44 mapper files | Boards 64 (+158), 115, 116, 176, 185, MMC5's extended-attribute mode, VRC7, Namco 163, FDS, EPSM, VS System, NSF, HD packs - the long tail, deliberately after the CPU/PPU work |
| NES 2.0 header | `GetMapperID()` takes the mapper's high nibble from byte 8's **low** nibble and `GetSubMapper()` byte 8's **high** nibble; exponent-multiplier sizes implemented; byte 13's console/VS/PPU model and byte 15's input type parsed | **The two nibbles are swapped** (`nes.rs`): the mapper takes byte 8's high nibble and the "submapper" is the low one - a NES 2.0 file with submapper 1 loads as mapper 260, and the submapper reads 0 | No local ROM has a NES 2.0 header, so nothing moved; any NES 2.0 file with a nonzero submapper does |
| Game database | `UI/Dependencies/MesenNesDB.txt`: 10,655 entries keyed by PRG+CHR CRC32, 18 fields (board, PCB, chip, mapper, sizes, mirroring, input, bus conflicts, submapper, VS type...). This is where "MMC3A" and mirroring overrides come from | Nothing | Commercial-game quirks; **none of the 82 ROMs under `roms/` is in the database** (checked by CRC), so no override applies to this baseline (§10.6) |
| Video | 2C02/2C03/2C04-A-D/2C05-1-5 palettes, a user palette, the emphasis generation, and optional NTSC composite filters (`NesNtscFilter`, `BisqwitNtscFilter`) | The 2C02 table, the emphasis generation, `nes_color` | The palette and emphasis are **identical** (§10.4); the RGB-PPU palettes and the composite filters do not exist here |

### 10.3 The defects this pass found (each with its fix)

**Status:** items 1, 2, 3 and 9 are fixed (commit C3's step, §7 and §9), and with them the
odd frame's skip moved to the one-dot-lagged rendering state (§10.2's PPU rows) — the four
remaining groups are 4/5 (the controller ports), 6 (the PPU's register state machines), 7 (DMA)
and 8 (the APU's edge timings), plus the two cleanups 10 and 11.

1. **NES 2.0's mapper and submapper nibbles are swapped** (`crates/nes-core/src/nes.rs`, the
   `RomHeader` parse). Mesen: `MapperID = (Byte8 & 0x0F) << 8 | (Byte7 & 0xF0) | (Byte6 >> 4)` and
   `SubMapper = (Byte8 & 0xF0) >> 4`; here the mapper takes `(flags8 & 0xF0) << 4` and the submapper
   `flags8 & 0x0F`. Nothing in `roms/` carries a NES 2.0 header, which is how six suites of ROMs
   never noticed.
2. **`$2002` clears sprite overflow and sprite 0 hit** (`ppu.rs`, the `0x2002` arm). Mesen clears
   only VBlank there, and the other two at pre-render cycle 0. A read in the middle of a frame takes
   those two flags away here and leaves them there.
3. **CPU reset**: `SP -= 3` runs on power-up too (a fresh machine has `SP = $FA`, not `$FD`), the
   cycle count starts at 0 rather than -1, and the eight dummy cycles are missing.
4. **The controller ports answer `bit | 0x40`** (`nes.rs`, `ControllerState::read`): no console
   open-bus mask, no "same address twice in a cycle" rule, and the shift register empties to 0
   instead of filling with 1s.
5. **The `$4016` write reaches the controllers immediately** instead of at the next PUT cycle.
6. **The PPU's register state machines do not exist**: `$2007`'s delayed read buffer and delayed
   write (including the rendering-time "write the address' low byte"), `$2006`'s three-cycle delay,
   `$2004`'s rendering-time behaviour, and the forced-blank `palette[v & 0x1F]` path.
7. **DMA's shape** (§2.3/B4): the halt and dummy cycles make no bus access, the DMC fetch is always
   four cycles, there is no abort path, and the cycles do not sample the interrupt lines.
8. **The APU's edge timings**: the length counter's deferred reload, the frame IRQ flag's real
   lifetime, and the DMC's 2/3-cycle enable and disable delays.
9. **The mixer's constants** are the wiki's, not Mesen's (§10.2's numbers).
10. **`cpu/addressing.rs` is dead code** - 760 lines and its whole test module. Either delete it or
    make it the one implementation; leaving it is how a future session "fixes" a model that is not
    running.
11. **`ppu.render_v` still exists** (`ppu.rs`, and it is in `state_fields!`, so it is serialized)
    although §2.1 says it was deleted.
12. **§1 still credits `NesBus::drive_ppu_bus`** in the pre-render row; the function is gone.

### 10.4 Verified identical - do not re-open these

Read on both sides, same answer:

- The 2C02 palette table and the emphasis generation: extracting MesenCE's `_ppuPaletteArgb[0]` row
  and this core's 64-entry table and comparing them mechanically gives *identical* - and
  `GenerateFullColorPalette` multiplies the other two channels by **0.84** with truncation and skips
  columns `$xE`/`$xF`, exactly what §7's nine-bit row recorded.
- Palette reads: six bits from palette RAM, high two from open bus, the grayscale mask applied on the
  way out (`_paletteRamMask`), and the `$3F00/04/08/0C` <-> `$3F10/14/18/1C` aliasing.
- Open bus on the PPU side: `_openBusDecayStamp` per bit, cleared after **3 frames**, `$2002`
  driving only three bits, `$2007` driving the byte it returns.
- `_ignoreVramRead = 6` for a second `$2007` access, and the `>= $3F00` palette path.
- The odd frame's skipped dot: decided at the pre-render line's dot 339, the skipped dot being 340,
  NTSC-only and 2C02-only.
- OAM decay being off by default in Mesen (4500 cycles per row *when enabled*) - not modelling it is
  matching Mesen's default.
- MMC3's A12 filter: `_a12LowClock > 0 && GetMasterClock() - _a12LowClock >= 3`, with `GetMasterClock()`
  being the CPU's cycle count and every rise clearing the low run (`NotifyVramAddressChange`) - which
  is what `mapper4.rs` does. `5-MMC3` is green on both sides.
- Bus conflict semantics (the AND itself; the register-address restriction is the one difference
  above).
- blip's `bass_shift = 9`; the DMC's byte-at-fetch, held-buffer and silence-flag shape; the DMC's
  NTSC and PAL period tables, value for value.
- `SetBusAddress` on every `ReadVram` (the per-fetch half of the bus-address row above).
- The `$2002`-read VBlank suppression window: AccuracyCoin's `$2002 flag timing` check 1 (the
  `$E0 $E0 $80 $00` answer key) passes with `ACCESS_DOTS = 2`, which is independent evidence for
  §4's deliberate deviation.
- `UpdateStatusFlag`'s special case (`_preventVblFlag` on the NMI scanline's cycle 0) is the same race
  the suppression window models.
- The one-dot lag of the rendering state (`_renderingEnabled` is refreshed by `UpdateState` after
  each dot's work): the odd frame's skipped dot now reads the state as of the end of dot 338, which
  is what `10-even_odd_timing` measures with its four writes across that boundary.

### 10.5 AccuracyCoin's 34, and what each one actually demands

The harness prints the byte the ROM stores, which is `(ErrorCode << 2) | 2` (`TEST_Fail` in
`AccuracyCoin.asm`). **The failing check is therefore `(value >> 2) + 1`**, one less than the
`value / 2` that §7's and §9's rows for the `SHx` group and the DMA group used. Measured on this
checkout (`cargo test -p nes-core --test rom_tests accuracy -- --nocapture`: 140 ran, 94 passed,
34 failing):

| Address | Test | Failing check | What the check wants |
| --- | --- | --- | --- |
| `$0408` | Open Bus | 2 | Reading open bus with `LDA absolute` returns the operand's high byte |
| `$0446`-`$044A` | `UnOp_SHA_93`, `_SHA_9F`, `_SHS_9B`, `_SHY_9C`, `_SHX_9E` | 8 | The `SHx` stores inside the tests' DMA section (their address corruption is in, §7) |
| `$044C` | DMA + `$2007` Read | 3 | The halt/alignment cycles must increment `v` enough times |
| `$044F` | DMA + `$2007` Write | 2 | The DMA must be delayed by the write cycle |
| `$0458` | Arbitrary Sprite zero | 3 | Sprite 0 from a non-zero OAM address |
| `$0459` | Sprite overflow behavior | 2 | Nine sprites on a line leave bit 5 of `$2002` set when read after `CLV` |
| `$045C` | APU Register Activation | 2 | No unexpected bits when reading a controller port |
| `$045D` | DMA + `$4015` Read | 3 | - |
| `$045E` | DMA + `$4016` Read | 2 | - |
| `$045F` | Controller Strobing | 2 | Any value with bit 0 set written to `$4016` strobes |
| `$0460` | Instruction Timing | 2 | The DMA timing is accurate enough to sync against it |
| `$0461` | Interrupt flag latency | 11 | - |
| `$0467` | Frame Counter IRQ | 8 | Switching to 5-step mode after the flag was set must not clear it |
| `$046A` | Delta Modulation Channel | 20 | - |
| `$046B` | DMC DMA Bus Conflicts | 2 | The DMC DMA's bus conflict with the APU registers |
| `$046C` | DMA + Open Bus | 3 | - |
| `$046D` | Implied Dummy Reads | 4 | Open bus (or `JSR`'s cycles are out of order) |
| `$0477` | DMC DMA + OAM DMA | 2 | The overlapping DMAs' cycle count |
| `$0478`/`$0479` | Implicit/Explicit DMA Abort | 2 | The aborted DMA's cycle count |
| `$047A` | Controller Clocking | 2 | A read while strobed must not disturb the shift register |
| `$047C` | JSR Edge Cases | 3 | Open bus |
| `$0488` | DMA + `$2002` Read | 3 | - |
| `$0489` | Suddenly Resize Sprite | 2 | A `$2000` write at the start of HBlank resizing an in-range sprite |
| `$048B` | Branch Dummy Reads | 6 | - |
| `$048D` | `$2002` flag timing | 2 | Sprite 0 and overflow must be set in the same read (`$55 == $60`) |
| `$0490` | Internal Data Bus | 2 | A `$4015` read must not touch the external bus |
| `$0494` | Misaligned OAM DMA | 2 | The transfer starts at the OAM address that is set |
| `$0496` | DMA Landing on Write | 3 | A DMC DMA must not land on a write cycle - it waits for a read |
| `$0497` | DMC Reload Timing | 2 | - |

Every "DMA + ..." row is the same missing mechanism (§10.2's DMA row); the two controller groups,
`Internal Data Bus` and `Open Bus` are the `$4016`/`$4015` open-bus work. Nothing here needs
AccuracyCoin to be right - it is a checklist that happens to be free.

**Re-measured after §10.3's first four fixes (same day): 32 failing.** The two that stopped are
`Suddenly Resize Sprite` (`$0489`) and `Misaligned OAM DMA` (`$0494`); every other row above is
still failing on the same check, which is the expected shape — the four groups left are the
controller ports, the PPU's register state machines, DMA, and the APU's edge timings. The
failing-check arithmetic above is what makes that readable: the values in the table are the
bytes the ROM prints.

### 10.6 Baseline suites: the ones Mesen's own source develops against

Mesen names its oracles in comments, which makes the missing suites easy to choose. All of these are
in the `christopherpow/nes-test-roms` collection (checked out at `/tmp/nes-test-roms` here) and none
is in `crates/nes-core/tests/rom_tests.rs` today:

| Suite | Cited by | Pins down |
| --- | --- | --- |
| `blargg_apu_2005.07.30/11.len_reload_timing.nes` (also `pal_apu_tests/`) | `APU/NesApu.cpp:166`, "This fixes the test len_reload_timing (tests 4 & 5)" | The length counter's deferred reload |
| `full_palette/` | `NesPpu.cpp:1589`, "This allows the full_palette tests to properly display single-pixel glitches" | `v`'s increment lagging a `$2007` access by a dot |
| `ppu_read_buffer/` | - | `$2007`'s read buffer and its delay |
| `read_joy3/` | - | `$4016`: the strobe, the double read, bit deletion, the post-8-read 1s |
| `sprdma_and_dmc_dma/` (+ `_512`) | `NesCpu.cpp:397`, "Fixes dmc_dma_start_test_v2 case C" | Overlapping sprite and DMC DMA |
| `dmc_tests/` (`latency`, `status`, `status_irq`, `buffer_retained`) | `DeltaModulationChannel.cpp:268`, "Allows behavior to match dmc_dma_start_test" | DMC latency and status |
| `dmc_dma_during_read4/` | - | The DMC fetch landing in a read's last cycle |
| `cpu_dummy_reads/`, `cpu_dummy_writes/`, `cpu_exec_space/`, `cpu_reset/` | - | The CPU's cycle model: dummy reads, `SP` after reset |
| `vbl_nmi_timing/`, `nmi_sync/`, `scanline/` | - | NMI timing breadth beyond `ppu_vbl_nmi` |
| `mmc3_irq_tests/`, `exram/`, `mmc5test_v2/` | - | The cartridge's IRQ and expansion RAM |
| `apu_mixer/` + `apu_mixer_recordings/` | - | The mixer's constants - the only audio goldens available |
| `pal_apu_tests/`, `tvpassfail/` | - | PAL, which has no ROM at all today (P5) |

Two ROMs Mesen cites are **not** in that collection and would have to be fetched separately:
`oam_flicker_test_reenable` (`NesPpu.cpp:1177`) and `dmc_dma_start_test`/`_v2` (`NesCpu.cpp:397`).

**Staged, and measured once.** All of these were copied into `roms/nes-test-roms/` (37 ROMs, not
committed) and run once through the harness before it came out (§6), which is where these
verdicts come from — they are the first things to gate on when the suites go back in:

| Suite | What it said at that measurement |
| --- | --- |
| `cpu_dummy_writes` | both ROMs reported `$6000 = 0` (passed) |
| `cpu_exec_space` | `ppuio`: "Passed"; `apu`: failed at check 2, the open-bus one (§10.3 #4) |
| `cpu_reset` | `registers`: **failed at check 2**, printing `S=$F7` where the ROM wants `$FD` — §10.3 #3. `ram_after_reset` asks the *operator* to press reset ("Press reset AFTER this message disappears"), so it cannot pass under a harness that never resets |
| `read_joy3`, `full_palette`, `ppu_read_buffer`, `blargg_apu_2005`, `dmc_tests`, `dmc_dma_during_read4`, `cpu_dummy_reads` | no `$6000` report inside the frame budget — their verdict is a screen or a hang, so the harness had nothing to judge (that is what the frame-dump diagnostic and the golden frames were for). The DMC/DMA ones draw **nothing at all**: `sprdma_and_dmc_dma` spins inside its `$E200`/`$E220`/`$E240` measurement sequence (the delay at `$E22D` plus the `$4015` waits) and never advances its own clock counter — its DMA phase never lines up, which is §10.3 #7 in the act |

**And one baseline item is not a defect at all.** `mmc3_test_2`'s `6-MMC3_alt` asks for the MMC3
**revision A** IRQ condition, which Mesen only applies when its database says the board's chip is an
`MMC3A` (`MMC3.h:209`). The database (`UI/Dependencies/MesenNesDB.txt`, 10,655 entries keyed by
PRG+CHR CRC32) has **no entry for any of the blargg MMC3 ROMs** - verified by computing each ROM's
CRC32 - so Mesen runs `6-MMC3_alt` with the rev-B condition and fails it exactly as this core does.
The honest handling is to keep the entry with a comment saying so, rather than to add a per-ROM table
the oracle does not have.

### 10.7 What to do next, in order

1. **The DMA port** (`ProcessPendingDma` verbatim, §10.2's DMA row) with the two things this pass
   added to the diagnosis: the DMA cycles must sample the interrupt lines, and the DMC's fetch is 3
   or 4 cycles by parity. That is the 13 AccuracyCoin items plus `4-irq_and_dma`.
2. **`$4016`/`$4017`** - the open-bus masks, the PUT-cycle strobe, the same-cycle double read - with
   `read_joy3` added first. Small, self-contained, and it moves four AccuracyCoin items (Open Bus 2,
   APU Register Activation 2, Controller Clocking 2, Controller Strobing 2).
3. **The PPU's register state machines** (`$2004`/`$2006`/`$2007`, the rendering-time write) with
   `ppu_read_buffer` and `full_palette`.
4. **`$2002`'s flag lifetimes** and the sprite evaluation at dot granularity (with its overflow-bug
   counter) - the three sprite items and `$2002 flag timing` check 2.
5. **The APU's edge timings**, with `len_reload_timing` in the baseline first.
6. **The mixer's constants**, judged against `apu_mixer`.
7. **CPU reset and warm-up**, the NES 2.0 nibbles, the bus-conflict scope, Dendy.
8. **Mappers** (64/158/185/115/116/176, MMC5's extended attributes, per-CPU-cycle hooks) - the long
   tail, and the only part where the gap is counted in boards rather than in behaviour.
