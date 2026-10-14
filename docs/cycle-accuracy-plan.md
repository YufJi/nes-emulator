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

Two phases are committed and the third sits in the working tree; none of the three regressed
the baselines:

| Phase | Commit | What it did | Measured effect |
| --- | --- | --- | --- |
| L1 | `4018bba` | The machine advances one CPU cycle at a time: three PPU dots, an APU tick, the cartridge's view of the bus — interleaved, instead of batched per instruction | No ROM change at all (57 pass / 17 known, identical); frame cost 1.075 → 1.046 ms (release, 600 frames of nestest) |
| L2 | `49d9d29` | Interrupt lines sampled on the penultimate cycle of each instruction; the seven-cycle vector sequence (two discarded reads, three pushes, two vector halves) driven cycle by cycle | Baselines unchanged in count, four failing tests' timing traces moved toward the expected patterns; AccuracyCoin 78 → 79 passing |
| L3 step 1 | `a2b13e9` | Writes to `$2000-$401F` land on the cycle the chip drives them — the last cycle of a store, the cycle before it for a read-modify-write's discarded write — and the NMI a `$2000` write raises is taken up on that cycle | No ROM count change (57 pass / 17 known and AccuracyCoin 79/49 identical); `4-irq_and_dma`'s trace moved from the wrong shape to the documented one with the L3a offset (`53 53 53 0…` → `0 0 0 0 1 1 2 2 4…`, CRC `5FA88FED` → `218E154C`); `3-nmi_and_irq`'s CRC moved `90BC9498` → `7E5D001A`; two AccuracyCoin error codes changed (`$0468`/`$0469` 0x06 → 0x0A, still failing); frame cost unchanged (1.033 → 1.034 ms, release, 600 frames of nestest) |
| L3 step 2 | (this change) | The write also lands on a *dot* inside that cycle: the PPU's per-dot view sees it from the cycle's first dot, and the APU tick at the end of the cycle sees it | ROM baseline 57 pass / 17 known → **58 / 16** (`apu_test/5-len_timing`); AccuracyCoin 79/49 → **81 / 47** (`Frame Counter 4-step`, `Frame Counter 5-step`); `4-irq_and_dma`'s CRC moved `218E154C` → `7DE0C30F`, `5-branch_delays_irq`'s `BE443B12` → `29BB6E90`, `6-irq_flag_timing` moved a sub-test ("Flag first set too late" → "Flag last set too soon"); nothing that was green regressed; frame cost unchanged (1.034 → 1.032 ms, release, 600 frames of nestest); the dot itself is not discriminated by any suite (0, 1 and 2 are byte-identical) |
| L3 step 3a | (this change) | An NMI asserted during an interrupt sequence, before its status push, takes the vector over: the frame pushed stays the sequence's, the handler is the NMI's | No test flips and nothing regresses (58 pass / 16 known, AccuracyCoin 81/47); `3-nmi_and_irq`'s second column comes in line with its readme for the five "NMI occurs after CLC, interrupting IRQ" rows (`23` → `00`), and its first column there moves `27` → `23` against a documented `20` |

Current state: `cargo test --workspace` 820 green, ROM baseline **58 pass / 16 known**,
AccuracyCoin **81 passed / 47 failing**, `STATE_VERSION` 4.

Two experiments were implemented, measured and **reverted**; their evidence is the starting
point for what follows (§4). L3a's bus-side parking has since come back as L3 step 1, with
the dependents its revert was about moved along with it.

## 3. What is left

### 3.1 The accuracy backlog the ROM suites name

Fifteen cases remain. They are not all the same kind of problem:

| Suite | Cases | What they need |
| --- | --- | --- |
| `cpu_interrupts_v2` | `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq` | An NMI to take over an interrupt sequence in flight, and the exact CPU cycle the handler starts on — i.e. §4's work |
| `ppu_vbl_nmi` | `05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `08-nmi_off_timing`, `10-even_odd_timing` | One-PPU-dot accuracy for `$2002` reads and `$2000`/`$2001` writes — §4's work |
| `mmc3_test_2` | `4-scanline_timing` | The exact dot the first A12 rise of a frame lands on — §4, plus the existing dot-level A12 model |
| `ppu_open_bus` | the whole ROM | PPU data-bus decay, a **separate feature**, nothing to do with cycles |
| `apu_test` | `4-jitter`, `6-irq_flag_timing`, `7-dmc_basics`, `8-dmc_rates` | Frame-counter and DMC timing; `5-len_timing` turned out **not** to be a length-counter bug but a write-phase one, and step 2 fixed it |

`4-irq_and_dma` is the closest to green and the most useful yardstick: the plan's first
step - implemented - moved its trace to the documented shape with a constant offset (§4.2).

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

Step 1 below is that lesson applied: L3a's parking stays, and the two dependents it broke -
the immediate-NMI detection and, later, the mask's dot phase - move with it. Measured, the
parking alone (step 1) reproduces L3a's `4-irq_and_dma` trace **and** keeps all four lines
L3a had broken: baseline identical, 57 pass / 17 known, AccuracyCoin 79/49.

**L4a — polling the interrupt lines during a DMA stall.** §4.3's diagnosis for
`4-irq_and_dma` says its remaining offset is the IRQ poll during the OAM/DMC DMA stall "and
which cycle the DMA releases the CPU on". Both halves were implemented on top of step 2 -
the lines polled on every stalled cycle, so a request asserted during the DMA is latched and
its sequence starts the moment the CPU is released, plus the 513/514 parity read from the
other side of the write's cycle - and all four combinations were measured. `4-irq_and_dma`'s
head, the part its readme documents as `0 1 1 2 2 4 4 7 7 7 7 8 …`, is **byte-identical in
every combination**: ours (`0 0 0 0 0 1 1 2 2 4 4 7 7 …`, CRC `7DE0C30F`) does not depend on
the DMA at all. The only entry that moves is the trace's last one, `8` → `7` where the readme
documents `9` — away from it. Nothing else in any suite moved and no test flipped, so the
change is reverted (it was never committed).

What that leaves: the head's lag is the *interrupt source*'s phase, not the CPU's answer to
DMA — the frame counter, i.e. the `6-irq_flag_timing` item below, which the same change left
exactly where it was ("Flag last set too soon"). §4.3's bullet for `4-irq_and_dma` is
corrected accordingly.

**The ROMs document themselves.** `roms/blargg/*/readme.txt` comes with the downloads, and
`cpu_interrupts_v2/source/` with it: the expected trace of every failing case in §3.1 can be
read next to ours. The table under L3a above was built from one of those readmes, and the
`4-irq_and_dma` expected values above from another. Compare against the documented trace
rather than against the shape of ours.

### 4.2 L3 — put register writes on their cycle, and take their dependents along

**Step 1 (done): writes land on their cycle, and the immediate NMI is detected there.**

- Keep L3a's bus-side parking (writes to `$2000-$401F`, last cycle for a store, the cycle
  before for a read-modify-write's discarded write, and only for writes the CPU makes inside
  an instruction — a host poke stays immediate). *Done: `NesBus::write` parks them, and
  `NesBus::replay_deferred_writes` drains the queue from the end as the machine ticks the
  instruction's cycles; two slots cover every opcode, since only a read-modify-write writes
  twice.*
- Move the "did this instruction enable NMI while VBlank is set?" detection from before the
  instruction's cycles to *the cycle the write lands on*, and re-express `nmi_raised_late`
  in terms of that cycle. `ppu_vbl_nmi/04-nmi_control` is the test that decides whether this
  is right. *Done: the flag is gone. The PPU's staged NMI is taken up when the write lands
  (`NesBus::latch_nmi_line`), and because a store's write is its last cycle - one after the
  poll - the one-instruction delay falls out of the cycle order instead of being imposed.*
- Verified: `04-nmi_control` stays green; `4-irq_and_dma`'s trace took the documented shape
  (`0 0 0 0 1 1 2 2 4 …`, CRC `218E154C`, exactly L3a's); `3-nmi_and_irq`'s trace moved
  (`90BC9498` → `7E5D001A`); nothing else moved - `3-A12_clocking`, `2-len_table` and
  `oam_stress`, all of which L3a had broken, stayed green.
- What is left over from this step: a read-modify-write's discarded write lands on the poll
  cycle itself. It writes the *old* value, so today it cannot raise an NMI either way; step
  2's dot offset is where that stops being a coincidence.

**Step 2 (done): the write's dot inside its cycle.**

- `$2001` currently takes effect on a cycle boundary; the A12 model reads the mask per dot
  (§`NesBus::ppu_bus_address`). Decide and implement where inside the write's cycle the register
  takes effect — i.e. give the deferred write a *dot* offset, not just a cycle. *Done: the
  parked write is applied on the write cycle's first dot, before that cycle's dots are
  stepped (`NesBus::advance_one_cpu_cycle_with`).*
- The planning note above was half right. The visible effect is on the PPU's per-dot view,
  but the *ordering* that came with it is what the ROMs paid for: applying the write inside
  its cycle also puts it before the APU tick at the end of that cycle, and step 1's parking
  had left it after. Two things turned green on exactly that, both frame-counter/length-
  counter writes whose phase had been one cycle late:
  - `apu_test/5-len_timing` — filed above as a length-counter bug; it was not one.
  - AccuracyCoin's `Frame Counter 4-step` and `Frame Counter 5-step` (the `$0468`/`$0469`
    entries step 1 had already moved).
- Verified: baseline 57 pass / 17 known → **58 / 16**; AccuracyCoin 79/49 → **81 / 47**;
  `3-A12_clocking`, `2-len_table`, `oam_stress` and `04-nmi_control` all stayed green, so
  all four lines L3a had broken are still fine. `4-irq_and_dma`'s CRC moved `218E154C` →
  `7DE0C30F` and `5-branch_delays_irq`'s `BE443B12` → `29BB6E90`; `6-irq_flag_timing`
  moved a sub-test, from "Flag first set too late" to "Flag last set too soon".
- **Which dot is not measured.** With the write applied at dot 0, 1 or 2 the whole ROM
  suite is byte-identical — every baseline, trace and AccuracyCoin result. The suites vary
  the write's *cycle*, and a 1-2 dot shift cannot move a flip point that sits between
  cycles, so nothing here pins the dot down. Step 2 therefore settles the ordering and
  leaves the dot as a convention: dot 0, "the access belongs to that cycle, so everything
  the cycle does after it sees the write", which is the order the machine already uses for
  the reads an instruction performs before its cycles. Revisit it when a test that reads
  the dot appears - `10-even_odd_timing` and `4-scanline_timing` still fail for reasons
  this step did not touch.
- **A later dot costs a frame rate, not just tidiness.** Placing the write *inside* the
  cycle means stepping that cycle's three dots one at a time, which needs a second
  dot-stepping path next to `advance_one_cpu_cycle`; measured, its mere presence took the
  frame from 1.034 to 1.104 ms (release, 600 frames of nestest, ~7%) with no ROM result
  moving at all. Disabling the parking entirely still cost 1.11 ms, so the price is the
  optimiser giving up on the hot cycle loop, not the write handling. The landed shape keeps
  the loop as it was - one extra `Option` in an inlined helper - and lands the write on the
  cycle's first dot, which needs no per-dot stepping at all.

**Step 3: retry the NMI hijack (L2b) on top.** With writes and their dependents in place,
re-apply the parked hijack patch and measure: the expectation is that `3-nmi_and_irq` gains
from it and the AccuracyCoin test at `$0452` no longer loses. If it still does, that test is
the thing to understand before landing the patch: it is a "Dummy Read Cycles"-family test
(it uses `Clockslide_29780` to park on a frame boundary and then exercises `$2002` reads), so
it is itself a dot-timing test and may simply be waiting for step 1 and 2 to be right.

*Half done: the IRQ side is in.* An NMI asserted while a sequence is still on its way to the
status push takes its vector over (`Cpu::nmi_hijack`, sampled on the sequence's first four
cycles); the pushes stay the IRQ's, so the frame on the stack has B clear, and an NMI that
arrives after the push waits for the next instruction's poll as before. Both sides of that
window have a unit test. Measured: **no test flips, nothing regresses**, and
`3-nmi_and_irq`'s trace moves toward its readme for the five "NMI occurs after CLC,
interrupting IRQ" rows, whose second column goes from `23` to `00` - the documented value:

```
readme   23 00  21 00  21 00  20 00  20 00  20 00  20 00  20 00  20 00  25 20  25 20
before   22 00  22 00  23 00  23 00  27 23  27 23  27 23  27 23  27 23  27 23  27 23
after    22 00  22 00  23 00  23 00  23 00  23 00  23 00  23 00  23 00  27 23  27 23
```

*Still open: the BRK side.* `2-nmi_and_brk` (and AccuracyCoin's `$0462` "NMI Overlap BRK")
need BRK's own vector read to be hijackable, and BRK pushes *and* reads its vector inside the
instruction, at the instruction's start - so its vector has to move onto the instruction's
last cycles the way a register write did in step 1, which is the same "an instruction is not
atomic" work step 4 is about. The readme's expected rows are `27 36 00` / `26 36 00` /
`26 36 00` / `36 00 00` ×5 / `27 36 00` ×2.

*3b, landed without evidence.* BRK now leaves its vector provisional for the rest of the
instruction (`Cpu::brk_vector_pending`, cleared at the start of the next one) and an NMI
asserted before the instruction ends takes it over - one frame, B set - instead of the
machine stacking a second one; a unit test pins the frame count, the B flag and the handler.
On the ROMs it is **invisible**: `2-nmi_and_brk`'s trace and AccuracyCoin's `$0462` are
byte-identical with and without it, so no suite asserts an NMI inside BRK's own cycles. It
stays because it is what the hardware does in that corner and nothing regressed, but
`2-nmi_and_brk` still needs its own investigation: our trace (`02 36 00 …`) does not differ
from the readme's (`27 36 00 …`) by a constant, so that test measures something this window
is not - most likely its own synchronisation to the PPU.

*`2-nmi_and_brk`, investigated (source read at `cpu_interrupts_v2/source/2-nmi_and_brk.s`).*
Its report is three columns - `nmi_flag | 0x02`, `irq_flag`, `X` (`X` should be 0) - and the
rows that should show the NMI taking BRK's vector expect `36 00 00`: the NMI handler ran with
BRK's frame on top (`nmi_flag` = 0x34, B set) and the BRK handler never ran (`irq_flag` = 0).
Ours is `02 36 00` in **all ten rows**: `nmi_flag` = 0 (the NMI handler never stored) and
`irq_flag` = 0x36 (the BRK handler ran). The ten rows differ only in *when* the NMI arrives -
one CPU clock apart - so this is not about the hijack window at all: **the NMI never arrives
inside the ten-clock window**. It is not that NMI is off (`setb` is a plain `lda #imm` /
`sta`, and `ppu_vbl_nmi/04-nmi_control` passes, so the NMI path itself is fine). What does not
line up is the test's own synchronisation: `sync_vbl` tunes a `$2002`-read loop to one *PPU
clock* over many frames ("the loop will effectively read $2002 one PPU clock later each
frame", the frame being 29780.67 CPU clocks) and `delay 29709` then places the NMI against
that. Our reads still happen at the instruction's start, and a read that lands on the flag's
dot wins the race over it across the whole CPU cycle (`cycle <= 1`, up to three dots), so the
loop can be knocked a whole frame out of phase - or swallow the very frame it is waiting for.

That makes `2-nmi_and_brk` (and `3-nmi_and_irq`'s remaining rows) **step 4's work, not the
sequence's**: nothing on the interrupt-sequence side can move it until a `$2002` read happens
on its own dot. Its readme even warns about this test ("Occasionally fails on NES due to
PPU-CPU synchronization").

**Step 4: reads with side effects.** `$2002` reads clear the VBlank flag and `$2007` reads
move the VRAM address; both are still performed at the instruction's start. `02-vbl_set_time`
passes today by luck of the current granularity — `06-suppression` and `05-nmi_timing` do
not. Placing reads needs the addressing phase and the data access to happen on different
cycles, i.e. splitting `Cpu::execute_instruction` into "fetch operands" and "perform the
access and the operation", with the machine ticking the addressing cycles in between. Scope
it to the instructions that can touch `$2000-$3FFF` and `$4015-$4017` first.

**Step 4, first try (measured, reverted).** Putting a `$2002` read's *clearing* on the read's
own cycle while the value is still reported where the instruction asks for it - the split into
`Ppu::read_ppustatus` / `Ppu::clear_ppustatus`, with the clear applied on the instruction's
last cycle - moves exactly what the investigation above predicted, without the whole
instructions-are-not-atomic split:

- `2-nmi_and_brk` goes from ten rows of `02 36 00` (the NMI never arriving) to `27 36 00` x5,
  `26 36 00` x2, `36 00 00` x3 - the documented shapes, hijack rows included.
- `06-suppression`'s row 04 goes from `- -` to `- N` (CRC `2F686C63` -> `3FE15516`).
- AccuracyCoin's `$0406` "Dummy read cycles" error code moves 0x12 -> 0x06.
- Nothing else moves; 58 pass / 16 known and 81/47 still hold.

But it breaks one green line: `ppu_vbl_nmi/02-vbl_set_time`, whose trace becomes `00 - - ...`
where its readme documents `00 - V ...` - the clear is a cycle late for the pair of reads it
measures. Reverted, not landed. Both endpoints are now measured, though: with the clear at the
instruction's start `02-vbl_set_time` is green and `2-nmi_and_brk` is dead; with the clear on
the instruction's last cycle it is the other way round. So the next move is to bisect the
*cycle* the deferred clear lands on (and, if that is not enough, the dot inside it), with
`02-vbl_set_time`, `06-suppression` and `2-nmi_and_brk` as the yardsticks. The `cycle <= 1`
race window in `Ppu::clear_ppustatus` (up to three dots wide) is the other thing
`02-vbl_set_time` is sensitive to.

### 4.3 L4 — DMA and the renderer (after L3)

- **`4-irq_and_dma`**: *the guess that stood here — "keep polling while the CPU is stalled, and
  know which cycle the DMA releases the CPU on" — was implemented and measured, and neither
  half moves the documented head of the trace (§4.1's L4a).* The lag tracks the IRQ source's
  phase, so this case belongs with the frame-counter item below.
- **`7-dmc_basics`, `8-dmc_rates`, `4-jitter`, `6-irq_flag_timing`**: DMC fetch timing and the
  frame counter's exact phase relative to CPU cycles — which is also where `4-irq_and_dma`'s
  remaining offset lives.
- **sprite 0 hit to the dot**: the renderer draws a whole scanline at once; the hit is
  detected at the render point, not at the pixel. Fixing it means per-dot rendering, which is
  the same machinery the MMC5 split was declined for — so it should be decided explicitly.
- **PAL**'s 3.2:1 PPU:CPU ratio needs a fractional dot accumulator.

### 4.4 Ordering, and what each step is expected to buy

1. L3 step 1 — **done**: kept `04-nmi_control`, moved `4-irq_and_dma` to the documented
   shape. No ROM count change, as expected. The one-cycle offset L3a left is unchanged by
   this step; the DMA-side poll (§4.3, L4) is the next suspect for it.
2. L3 step 2 — **done**, after having been aimed at the wrong thing: the write's dot inside
   its cycle turned out not to be measurable with these ROMs, but moving the write inside
   the cycle also moved it before that cycle's APU tick, which fixed `5-len_timing` and two
   AccuracyCoin frame-counter tests. `10-even_odd_timing` and `4-scanline_timing` did not
   move at all: step 1 had already taken `10-even_odd_timing` from "skipped too late" to
   "skipped too soon" and the dot does not shift it, so both now look like they wait for the
   per-dot PPU work §6 decision 2 is about rather than for the write's placement.
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
