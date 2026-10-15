# Mesen alignment: status, gaps, and how to continue

[简体中文](./mesen-alignment.zh-CN.md) · **English**

This is the working document for the accuracy work: **the emulator core is being aligned with
[Mesen2](https://github.com/SourMesen/Mesen2)'s NES core, and Mesen settles every ambiguity.**
`docs/architecture.md` stays the description of what the emulator *is* today (§15.1 there is its
summary of the known limits); this document is the plan, the measurements behind it, and what is
still different. **Start a new session with §9's ledger** — the one-page answer to what is done,
what is stuck and what to do next.

Two rules, in order:

1. **Mesen's implementation wins.** Where Mesen and this core disagree, the gap is a bug here
   unless §4 below lists it as a deliberate deviation. Read Mesen's source rather than guessing:
   the files that matter are `Core/NES/NesCpu.cpp`/`.h` (execution, DMA, interrupts),
   `Core/NES/NesPpu.cpp`/`.h` (the fetch pipeline), `Core/NES/BaseNesPpu.h` (PPU state),
   `Core/NES/NesMemoryManager.cpp`, `Core/NES/Mappers/A12Watcher.h` and the mapper files under
   `Core/NES/Mappers/`.
2. **Test-ROM baselines are evidence, not constraints.** Each suite's expected-fail list in
   `crates/nes-core/tests/rom_tests.rs` is asserted in both directions, so a change that moves a
   suite *must* update the list — but the list follows the measurements, and a regression that
   comes from a more faithful model is recorded and explained rather than reverted.

## 1. What is aligned

| Area | What Mesen does | What this core does now | Verified by |
| --- | --- | --- | --- |
| Clock | One master clock; `cpuDivider` 12 / `ppuDivider` 4 on NTSC feed both chips | `Region::cpu_divider()`/`ppu_divider()` (12/4, the real crystal), one master clock, `NesBus::run_ppu_to` runs the PPU to a target | `test_clock_dividers_agree_with_the_frame_geometry`, `test_the_master_clock_path_places_the_same_dots` |
| CPU execution | `MemoryRead`/`MemoryWrite` *are* the cycle (`StartCpuCycle`/`EndCpuCycle` around the access) | `Bus::read_cycle`/`write_cycle` are the cycle; `NesBus::begin_cpu_cycle`/`end_cpu_cycle` advance the PPU and APU around the access | `instr_timing` 2/2, `branch_timing` 3/3, `test_every_opcode_executes` |
| Operand fetches and discarded reads | `FetchOperand` spells out the dummies (zero-page indexed, indexed absolute/indirect, the implied/accumulator read) | `Cpu::fetch_operand` plus `AddressingMode::AbsoluteXWrite`/`AbsoluteYWrite`/`IndirectYWrite`, `RTS`'s two dummies, `RTI`/`PLA`/`PLP`'s one each | the same suites |
| Interrupt timing | Lines sampled every cycle; the decision reads the previous cycle's samples (`_prevRunIrq`/`_prevNeedNmi`) | `Cpu::end_cycle` keeps the same lag; `execute_instruction` decides after the instruction | `cpu_interrupts_v2/2-nmi_and_brk` green, `ppu_vbl_nmi` 9/10 |
| Vector choice | The vector is chosen at the status push; an NMI asserted by then takes it over (`IRQ()`, `BRK()`) | `Cpu::take_interrupt`/`Cpu::brk` do the same, including BRK's `_prevNeedNmi = false` | `test_nmi_hijacks_an_irq_sequence`, `test_nmi_takes_brks_vector_over` |
| A12 | `SetBusAddress` puts the *last real access*'s address on the bus and it is held; the filter is `(GetMasterClock() - _a12LowClock) >= 3` with the master clock being the CPU cycle count | `NesBus::bus_address` (held), fed by the addresses `PixelPipeline` *really read* on the dot it read them, `Mapper::on_ppu_bus(addr, cpu_cycles)` and `mapper4` measure the run in cycles, resetting on *every* rise | `2-details` (241 clockings) and `4-scanline_timing` green |
| Per-dot rendering | `ProcessScanlineImpl`: `LoadTileInfo` on `cycle % 8 == 1/3/5/7`, `ShiftTileRegisters` and `DrawPixel` every dot, off the single `_videoRamAddr` | `PixelPipeline`: the same fetch/load/shift/draw schedule off `v`/`t`/`x`, with the sprite phase's fetches and per-dot sprite pixels and sprite-0 hit | `4-scanline_timing`, `sprite_hit` 11/11, and byte-identical frames against the whole-line renderer on `nestest` |
| Open bus | A stamp per bit (`_openBusDecayStamp`), three frames of decay, `$2002`'s low five bits masked from it, and the PPU's own latch rather than the CPU's | the same: `Ppu::drive_open_bus`/`open_bus_decay_stamp`, `$2002` driving only its top three bits, `$2007`'s palette reads driving six | `ppu_open_bus` 1/1 |
| The pre-render line | The pipeline runs for `scanline < 240`, which includes the pre-render line | `drive_ppu_bus`'s `in_vblank` excludes only 240..=260 | the same |
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
`RenderTiming`, `ppu.render_v` and the whole-line renderer are gone.

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
  Dendy swap what the register's emphasis bits mean. What is *not* aligned is the RGB those
  nine bits resolve to: Mesen ORs `Red/Green/Blue` into the pixel value and its video filter
  looks the result up in a 512-entry palette, while this core converts per pixel with a
  three-quarter attenuation of the channels that are not emphasised.
- PAL's own PPU rules (`_nmiScanline`, `_vblankEnd`, PAL's OAM refresh) are still absent.

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
- Resampling is sample-and-hold decimation; Mesen uses a band-limited (blip) resampler.
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
   `7-dmc_basics` and `8-dmc_rates` green — apu_test 8/8.
4. ~~**§2.2's register details**~~ — done: open-bus decay (`ppu_open_bus`), the rendering-time
   `$2007` behaviour, the grayscale mask on palette reads and the PAL emphasis swap. OAM decay
   turned out to be an off-by-default Mesen setting, so not modelling it *is* the default. The
   one piece left is the RGB the nine colour bits resolve to (Mesen's 512-entry table, which
   its video filter owns).
5. **PAL**. The ratio is done — PAL runs 16/5 with the fractional dot accumulator, so a PAL
   frame is 33247.5 CPU cycles and the audio clock derives from the real 3.2 rather than from
   the NTSC-shaped constant. What is left is PAL's own PPU/APU rules (`_nmiScanline`,
   `_vblankEnd`, PAL's OAM refresh); none of it is verifiable without a PAL test ROM.
6. **Mappers** (§2.5): the NES 2.0 header and bus conflicts are in. What is left is dispatching
   on the submapper (which needs the per-ROM data Mesen keeps in its database, not the header),
   and the boards 64/158/185/115.
7. **Audio resampling** against Mesen's band-limited reference.

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

## 5. Instruments

Existing, and used for every measurement above:

- `cargo test -p nes-core --test rom_tests -- --nocapture` — the per-ROM pass/known lines.
- `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture` — frame cost
  (600 frames of nestest; run it a few times and compare medians).
- `cargo test -p nes-core --test cycle_trace -- --ignored --nocapture` — per-instruction traces
  (`NES_TRACE_ROM`) and the PPU bus address per line (`NES_A12_ROM`, `NES_A12_SKIP`,
  `NES_A12_LINE`).
- The test-ROMs ship their own sources and readmes under `roms/blargg/*/source/` and
  `readme.txt`; blargg's suites state their precision ("Timing is tested to an accuracy of one
  PPU clock") and print expected traces. Compare against those, not against the shape of ours.
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

## 6. Verification protocol

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p nes-core --test rom_tests -- --nocapture   # per-ROM pass/known lines
```

- ROMs are not committed: drop blargg's suites and AccuracyCoin under `roms/` (the layout and
  the download sources are in `docs/architecture.md` §13.1).
- A baseline entry that starts passing must be removed from the list in `tests/rom_tests.rs`; a
  failure outside the list fails the suite. A regression that comes from a more faithful model
  is added to the list *with the reason in the comment*.
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
| Mesen's DMC source | `Core/NES/APU/DeltaModulationChannel.cpp` read (the manifest listed the path; the file was fetched): status = `_bytesRemaining > 0`; the counter and the IRQ move in `SetDmcReadBuffer`, i.e. at the fetch; the register takes the buffer when `_bitsRemaining` hits 0, with a silence flag holding the DAC; `SetEnabled` delays its transfer 2/3 cycles by parity; PAL has its own period table | The Mesen-shaped implementation was tried and **withdrawn**: it puts `8-dmc_rates` at ~128 periods where the ROM's window wants ~135, and `7-dmc_basics` regresses to its 2nd check. The tree keeps the playback-based counter (135.3 periods) |
| DMC fetch charged twice | The channel's request latch stayed set until the byte was supplied, and the bus re-armed its pending transfer from it on every cycle the APU ticked — so the transfer's own four cycles armed a second one. Two four-cycle transfers per byte, and no CPU read of `$4000` ever saw the byte on the bus. The bus now *takes* the request (`Apu::take_dmc_dma_request`), which is Mesen's one-shot halt latch | **AccuracyCoin boots again**: 155 tests ran, 85 passed, 43 failing (the rows above recorded 46); every other suite unchanged; frame cost 1.72 → 1.70 ms; two new unit tests (the latch, and a hand-built ROM that polls `$4000` the way AccuracyCoin's boot-time sync does) |
| DMC is Mesen's | The length counter moves at the fetch rather than at the end of playback, a fetched byte waits in the buffer for the output unit's byte boundary (so the buffer holds one byte and a restart cannot queue a second), the register holds its level while silent instead of decaying over zeroes, and the timer's period is the table's value rather than one more | **`7-dmc_basics` and `8-dmc_rates` green** (apu_test 6/8 → **8/8**); AccuracyCoin 43 → **34 failing** (143 ran, 94 passed); nothing else moved; frame cost 1.70 → 1.74 ms; state version 10 → 11 |

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
- **`cpu_interrupts_v2`: the IRQ judged one cycle later (the sample from two cycles back
  instead of one).** Measured: `3-nmi_and_irq` passes, and `5-branch_delays_irq`'s `test_jmp`,
  `test_branch_not_taken` and `test_branch_taken_pagecross` tables match the readme
  **row for row** — the first time this emulator reproduces that table — with only the last
  sub-test (the commented "special case") still off (`05 05 03` where the readme has
  `05 01 00`). But `1-cli_latency` check 4 ("exactly one instruction after CLI should execute
  before IRQ is taken") and `4-scanline_timing` check 3 go red. Reverted: those two pin the
  ordinary IRQ judgement to the penultimate cycle, which is Mesen's rule too.
- **`cpu_interrupts_v2`: the `$4017` write's parity inverted.** Measured: `5-branch_delays_irq`
  is byte-identical (so its write's hold is not what is off), `3-nmi_and_irq`'s output changes,
  and AccuracyCoin drops 34 → 36. Reverted.

  **What that leaves.** All three remaining failures are the same ±1-cycle question about where
  the frame counter's flag, the IRQ judgement and the instruction stream sit relative to each
  other. Three sets of tests constrain it, and no single one-cycle knob satisfied all three:
  `4-jitter`, `5-len_timing` and `6-irq_flag_timing` pin the flag against the `$4017` write;
  `1-cli_latency` and `4-scanline_timing` pin the judgement to the penultimate cycle; and the
  three `cpu_interrupts_v2` tables want those two shifted by a cycle relative to the code
  (`4-irq_and_dma`'s own table is the readme's shifted one row as well: ours opens
  `0 +0 / 0 +1 / 1 +2`, the readme `0 +0 / 1 +1 / 1 +2`). The next attempt should measure the
  *chain* — the instruction-by-instruction cost of the delays in `5-branch_delays_irq`'s
  `begin` against the cycle the readme's table implies (its `test_jmp` row 0 wants the flag on
  the RTS's last cycle, i.e. write + 29831) — instead of moving the knobs again.

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

**How to use it**: every row is *status → evidence → next step*. Re-run the baseline from §6 first
(the numbers move with the machine; the set of suites should not), then pick a row. Nothing goes
into this document without a measurement behind it.

**The harness fails rather than skips when a ROM stops reporting**: `accuracy_coin_matches_its_baseline`
now distinguishes "the ROM is not on this checkout" from "the ROM never ran its suite", because the
second one is what a boot-time hang looks like (`$37` stays zero). If a future change hangs the ROM
before the menu, that test goes red instead of quietly passing — which is exactly how the DMC's
double-charged fetch hid here for several steps (§7).

**Current baseline** (last measured this session, `cargo test -p nes-core --test rom_tests --
--nocapture`): `instr_test-v5` 16/16, `instr_timing` 2/2, `branch_timing_tests` 3/3,
`blargg_ppu_tests` 5/5, `oam_read` 1/1, `oam_stress` 1/1, `ppu_open_bus` **1/1**,
`sprite_hit_tests` 11/11, `sprite_overflow_tests` 5/5, `apu_test` **8/8**, `cpu_interrupts_v2`
2/5, `mmc3_test_2` **5/6**, `ppu_vbl_nmi` 9/10, AccuracyCoin **34 failing** (143 ran, 94 passed,
0 skipped). Unit tests: 588. Frame cost: about 1.74 ms (release, 600 frames of nestest, median).
Branch:
`codex/mesen-alignment`.

| # | Item | Status | Evidence (commit / measurement) | Next step |
| --- | --- | --- | --- | --- |
| S0 | Public per-dot observation API (`Nes::ppu_bus_trace`, `step_dot`, exported from wasm and uniffi) | Done | `c8957a7`; four unit tests | Use it for a differential when one is needed |
| P1 | Per-dot PPU renderer (background + sprites), A12 from real fetches | Done | `5dc9e32`, `30b7267`: `4-scanline_timing` green; `RenderTiming`, `ppu.render_v` and the whole-line renderer deleted | The sprite *evaluation* is still per line (§2.1), when it is wanted |
| P2 | DMA inside `MemoryRead` (`process_pending_dma`) | Done | `8ec693d`: `4-irq_and_dma`'s trace is byte-identical | Nothing (DMC's prerequisite is in) — but it is also where the DMC's double-charged fetch came in; see the last §7 row |
| P3a | APU frame counter phase (`$4017` three cycles late, IRQ flag three cycles in a row) | Done | `a1fbaa3`: `4-jitter`, `5-len_timing`, `6-irq_flag_timing` green | Nothing |
| P3b | DMC | Done | `f9984d4` (two semantics bugs), the one-shot fetch latch (§7), and Mesen's channel shape: counter at the fetch, byte held in the buffer, silence flag, period = the table's value. **`7-dmc_basics` and `8-dmc_rates` green** (apu_test 8/8); AccuracyCoin 43 → 34 | Nothing for the roms; the mixing rate (P7) and the DMC IRQ's effect on the CPU's IRQ line are the neighbouring items |
| P4 | §2.2's register details: open-bus decay, rendering-time `$2007`, grayscale mask, PAL emphasis swap | Done | `af2f375`, `8247f10`: `ppu_open_bus` green; three unit tests | The RGB those nine colour bits resolve to (Mesen's 512-entry table lives in its video filter) |
| P5 | PAL 16/5, fractional dot accumulator, region-correct audio clock | Done (the ratio) | `dbdf7cc`: a PAL frame is 33,247.5 CPU cycles | PAL's own PPU/APU rules (`_nmiScanline`, `_vblankEnd`, PAL's OAM refresh) — unverifiable without a PAL ROM |
| P6 | Mappers: NES 2.0 header, bus conflicts | Partly done | `4e1c7a6`: conflicts for 2/3/7/11/66/71; five header and two conflict tests | Dispatching on the submapper (Mesen reads that from its CRC-keyed database, not the header); then boards 64/158/185/115 |
| P7 | Audio resampling | Not started | — | Band-limited (blip) resampling; no reference recording, so acceptance is "does not break what passes" |
| P3c | `cpu_interrupts_v2`'s three remaining tables | **In progress** | Two one-cycle experiments measured and withdrawn (the frame counter's flag one cycle later; the IRQ judged one cycle later). The second reproduces `5-branch_delays_irq`'s first three tables row for row, at the cost of `1-cli_latency` and `4-scanline_timing`. Both are written up in §7. | Measure the *chain* of `5-branch_delays_irq`'s `begin` against the cycle its readme table implies (its row 0 wants the flag on the RTS's last cycle, i.e. write + 29831) before moving any knob |
| — | The access-dot experiment | Done | `f5943b3`: 0 and 1 each cost five suites, 2 passes every one — so `ACCESS_DOTS = 2` stands | Nothing |

**How to get Mesen's sources** (`/tmp` copies may be gone; this is what worked here): the
repository is `SourMesen/Mesen2`; fetch a file at a time, e.g.
`curl -sSf -O https://raw.githubusercontent.com/SourMesen/Mesen2/master/Core/NES/APU/DeltaModulationChannel.cpp`.
Already read and written into this document: `Core/NES/NesPpu.cpp`,
`Core/NES/BaseNesPpu.{h,cpp}`, `Core/NES/NesCpu.cpp`, `Core/NES/NesMemoryManager.cpp`,
`Core/NES/APU/DeltaModulationChannel.{h,cpp}`, `Core/NES/APU/ApuFrameCounter.h`, and
`Core/NES/Mappers/` (`MMC3.h`, `Mapper4.h`, `BaseMapper.*`, `VRC6.h`). The repository root's
`mesen_files.json` is its whole-tree manifest: look a path up by its `name` field.

**Conventions to hand over**: ROMs are never committed (a suite with no `roms/` directory prints a
skip); a document change touches both languages (`*.zh-CN.md`); each step's numbers go into its
commit message and §7; and a regression that comes from being *more* faithful to Mesen is recorded
in §7's withdrawn-experiment list rather than reverted.
