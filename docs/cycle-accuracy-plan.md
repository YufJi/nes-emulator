# Cycle-Accurate Interrupts and Bus Writes: Plan

[简体中文](./cycle-accuracy-plan.zh-CN.md) · **English**

This is a working plan, not documentation of shipped behaviour: it says what is left in the
accuracy backlog, how far the cycle-granularity work has come, and how to continue it. The
architecture document (`docs/architecture.md`) stays the description of what the emulator is
today; §15.1 there lists the same limits in summary form.

## 1. Why this work exists

blargg's PPU and CPU suites state their precision up front: "Timing is tested to an accuracy
of one PPU clock" (`ppu_vbl_nmi/readme.txt`). Everything still failing in
`docs/architecture.md` §13.1 needs the emulator to know *which PPU dot* a CPU action lands
on. The emulator currently knows which *instruction* it lands in, and — since phase L1/L2 —
which *CPU cycle*.

## 2. Where the emulator stands

Two phases are done, each a commit of its own, both with zero regressions:

| Phase | Commit | What it did | Measured effect |
| --- | --- | --- | --- |
| L1 | `4018bba` | The machine advances one CPU cycle at a time: three PPU dots, an APU tick, the cartridge's view of the bus — interleaved, instead of batched per instruction | No ROM change at all (57 pass / 17 known, identical); frame cost 1.075 → 1.046 ms (release, 600 frames of nestest) |
| L2 | `49d9d29` | Interrupt lines sampled on the penultimate cycle of each instruction; the seven-cycle vector sequence (two discarded reads, three pushes, two vector halves) driven cycle by cycle | Baselines unchanged in count, four failing tests' timing traces moved toward the expected patterns; AccuracyCoin 78 → 79 passing |

Current state: `cargo test --workspace` 814 green, ROM baseline **57 pass / 17 known**,
AccuracyCoin **79 passed / 49 failing**, `STATE_VERSION` 3.

Two experiments were implemented, measured and **reverted**; their evidence is the starting
point for what follows (§4).

## 3. What is left

### 3.1 The accuracy backlog the ROM suites name

Sixteen cases remain. They are not all the same kind of problem:

| Suite | Cases | What they need |
| --- | --- | --- |
| `cpu_interrupts_v2` | `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq` | An NMI to take over an interrupt sequence in flight, and the exact CPU cycle the handler starts on — i.e. §4's work |
| `ppu_vbl_nmi` | `05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `08-nmi_off_timing`, `10-even_odd_timing` | One-PPU-dot accuracy for `$2002` reads and `$2000`/`$2001` writes — §4's work |
| `mmc3_test_2` | `4-scanline_timing` | The exact dot the first A12 rise of a frame lands on — §4, plus the existing dot-level A12 model |
| `ppu_open_bus` | the whole ROM | PPU data-bus decay, a **separate feature**, nothing to do with cycles |
| `apu_test` | `4-jitter`, `5-len_timing`, `6-irq_flag_timing`, `7-dmc_basics`, `8-dmc_rates` | Frame-counter and DMC timing; `5-len_timing` is a length-counter bug, the rest are cycle/DMA work |

`4-irq_and_dma` is the closest to green and the most useful yardstick: the plan's first
step moved its trace to the documented shape with a constant offset (§4.2).

Two cases are **not** cycle problems at all and should be filed separately:

- `mmc3_test_2/6-MMC3_alt` wants mapper 4's "alternate" IRQ revision, which is selected by a
  NES 2.0 submapper this project deliberately does not parse (see `docs/architecture.md`
  §15.6). Either add a per-ROM override table or accept the case as permanently failing.
- `ppu_open_bus` wants the PPU data bus latch to decay over time.

### 3.2 MMC5

- **Missing**: extended-attribute mode (`$5104` = 3) needs its own byte layout decoded; the
  `$5105` mapping, ExRAM nametables and fill mode are wired already.
- **Out of scope by decision**: the hardware vertical split (§15.6). A game asking for one
  currently gets a static split.

### 3.3 Mappers

117 numbers are implemented (`supported_mappers()`). Missing, with reasons in §8:
`115` (extended `$6000`/`$6001` switching plus its own IRQ), `116`/`176` (behaviour selected
by NES 2.0 submappers), `158` (needs RAMBO-1 — i.e. mapper 64 — first), `185` (a CHR window
onto nametable RAM, which needs the renderer to ask the cartridge for pattern data).

### 3.4 Everything else on the list

From §15: save-state fingerprint; audio resampling quality; zero-copy WASM picture; a front
end that exposes `RenderTiming`; IPS patching; Zapper and four-player; bus conflicts for the
discrete boards (2/3/7 and friends); FDS and NSF.

## 4. The plan

### 4.1 What the two reverted experiments taught

**L2b — letting an NMI take over a sequence.** Implemented `poll_hijack` plus a
hijack-aware vector: `3-nmi_and_irq`'s trace moved further toward the documented pattern
(CRC `90BC9498` → `14B79C72`) and a unit test pinned the B-flag behaviour, but **no blargg
case turned green** and AccuracyCoin lost one test (79 → 78, identical across three runs, so
not a race). Narrowing the window from "before the vector fetch" to "before the status push"
changed nothing. Parked until step 4.2.3, when the write cycles are in place and the
AccuracyCoin test can be re-measured against a model that is right in more places.

**L3a — deferring register writes to their real cycle.** Writes to `$2000-$401F` were parked
and applied on the instruction's last cycle (the discarded write of a read-modify-write one
cycle before). This moved `4-irq_and_dma` from a wrong trace to the *documented shape with a
constant offset*:

```
expected   0 +0   1 +1   1 +2   2 +3   2 +4   4 +5   4 +6   7 +7 ...
after L3a  0 +0   0 +1   0 +2   0 +3   1 +4   1 +5   2 +6   2 +7   4 +8 ...
before     53 +0 53 +1  53 +2   0 +3   0 +4   0 +5   0 +6   0 +7   1 +8 ...
```

but it broke four lines that were green: `ppu_vbl_nmi/04-nmi_control` (the immediate-NMI path
is detected *before* the instruction's cycles, i.e. before the write now lands),
`mmc3_test_2/3-A12_clocking` (the per-dot A12 model reads the `$2001` mask, whose effective
time moved), plus `apu_test/2-len_table` and `oam_stress`; AccuracyCoin 79 → 76.

The lesson, and the shape of the remaining work: **a register write cannot move alone.** Its
dependents have to move with it.

### 4.2 L3 — put register writes on their cycle, and take their dependents along

**Step 1: writes land on their cycle, and the immediate NMI is detected there.**

- Keep L3a's bus-side parking (writes to `$2000-$401F`, last cycle for a store, the cycle
  before for a read-modify-write's discarded write, and only for writes the CPU makes inside
  an instruction — a host poke stays immediate).
- Move the "did this instruction enable NMI while VBlank is set?" detection from before the
  instruction's cycles to *the cycle the write lands on*, and re-express `nmi_raised_late`
  in terms of that cycle. `ppu_vbl_nmi/04-nmi_control` is the test that decides whether this
  is right.
- Verify: `04-nmi_control` stays green; `4-irq_and_dma`'s trace keeps its shape; nothing else
  moves.

**Step 2: the mask's per-dot phase.**

- `$2001` currently takes effect on a cycle boundary; the A12 model reads the mask per dot
  (§`NesBus::ppu_bus_address`). Decide and implement where inside the write's cycle the register
  takes effect — i.e. give the deferred write a *dot* offset, not just a cycle.
- This is what `mmc3_test_2/3-A12_clocking` and `10-even_odd_timing` measure.
- Verify with `mmc3_test_2` and `ppu_vbl_nmi`, and re-check `apu_test/2-len_table` and
  `oam_stress`, which regressed in the L3a experiment for the same reason.

**Step 3: retry the NMI hijack (L2b) on top.** With writes and their dependents in place,
re-apply the parked hijack patch and measure: the expectation is that `3-nmi_and_irq` gains
from it and the AccuracyCoin test at `$0452` no longer loses. If it still does, that test is
the thing to understand before landing the patch: it is a "Dummy Read Cycles"-family test
(it uses `Clockslide_29780` to park on a frame boundary and then exercises `$2002` reads), so
it is itself a dot-timing test and may simply be waiting for step 1 and 2 to be right.

**Step 4: reads with side effects.** `$2002` reads clear the VBlank flag and `$2007` reads
move the VRAM address; both are still performed at the instruction's start. `02-vbl_set_time`
passes today by luck of the current granularity — `06-suppression` and `05-nmi_timing` do
not. Placing reads needs the addressing phase and the data access to happen on different
cycles, i.e. splitting `Cpu::execute_instruction` into "fetch operands" and "perform the
access and the operation", with the machine ticking the addressing cycles in between. Scope
it to the instructions that can touch `$2000-$3FFF` and `$4015-$4017` first.

### 4.3 L4 — DMA and the renderer (after L3)

- **`4-irq_and_dma`**: the IRQ poll during OAM/DMC DMA stalls. The DMA stall is already a
  cycle count; it needs to keep polling while the CPU is stalled, and to know which cycle the
  DMA releases the CPU on.
- **`7-dmc_basics`, `8-dmc_rates`, `4-jitter`, `6-irq_flag_timing`**: DMC fetch timing and the
  frame counter's exact phase relative to CPU cycles.
- **sprite 0 hit to the dot**: the renderer draws a whole scanline at once; the hit is
  detected at the render point, not at the pixel. Fixing it means per-dot rendering, which is
  the same machinery the MMC5 split was declined for — so it should be decided explicitly.
- **PAL**'s 3.2:1 PPU:CPU ratio needs a fractional dot accumulator.

### 4.4 Ordering, and what each step is expected to buy

1. L3 step 1 — keeps `04-nmi_control`; shrinks the `4-irq_and_dma` offset. No ROM count change expected.
2. L3 step 2 — `3-A12_clocking` (already green today) must survive, `10-even_odd_timing` and
   `4-scanline_timing` are the ones that should move.
3. L3 step 3 — `2-nmi_and_brk`, `3-nmi_and_irq`.
4. L3 step 4 — `05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `08-nmi_off_timing`.
5. L4 — the DMA/frame-counter group, then sprite 0 timing.

Each step is one commit, and each is only kept if the ROM baseline does not regress: the
suites carry an expected-fail list and assert *both* directions (`tests/rom_tests.rs`), and
AccuracyCoin carries a failing-count ratchet (currently 49).

## 5. Verification protocol (used by every phase so far)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p nes-core --test rom_tests -- --nocapture   # per-ROM pass/known lines
```

- ROMs are not committed: drop blargg's suites and AccuracyCoin under `roms/` (§13.1 has the
  layout and the sources).
- A baseline entry that starts passing must be removed from the list in
  `tests/rom_tests.rs`; a failure outside the list fails the suite.
- Performance: measure release-mode frame cost before and after (`600 frames of nestest`,
  timing printed by a scratch test) — L1's net effect was a small *speed-up*, and the L3
  steps are the first ones expected to cost something.
- Keep `docs/architecture.md` and its `zh-CN` twin in step with the code, and record each
  phase's measurements in the commit message: the numbers in this document came from those
  messages.

## 6. Decisions the next project has to make

1. **How far to take L3**: step 1 alone (write cycles, cheap, no ROM gain expected) or
   steps 1-4 (the dot-precision group, several commits). Step 1 is a prerequisite for all of
   it, so it is a safe first commit either way.
2. **Sprite 0 hit / PAL / MMC5 split**: all three need per-dot rendering. Decide once whether
   that is in scope, rather than three times.
3. **`6-MMC3_alt`**: add a small per-ROM override table for the alternate IRQ revision, or
   keep it as a documented permanent failure (the NES 2.0 decision blocks the principled fix).
4. **Mappers**: whether to add mapper 64 (RAMBO-1) so that 158 follows, and whether 185's
   pattern-from-VRAM window is worth the renderer hook.
5. **Non-cycle work** (§3.4) is independent of all of the above and can be interleaved:
   the cheapest high-value items are IPS patching, bus conflicts for the discrete boards, and
   the save-state fingerprint.
