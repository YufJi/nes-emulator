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

The work has landed in steps. The table is the record of what each one did and what it
measured; none of them regressed the baselines:

| Phase | Commit | What it did | Measured effect |
| --- | --- | --- | --- |
| L1 | `4018bba` | The machine advances one CPU cycle at a time: three PPU dots, an APU tick, the cartridge's view of the bus — interleaved, instead of batched per instruction | No ROM change at all (57 pass / 17 known, identical); frame cost 1.075 → 1.046 ms (release, 600 frames of nestest) |
| L2 | `49d9d29` | Interrupt lines sampled on the penultimate cycle of each instruction; the seven-cycle vector sequence (two discarded reads, three pushes, two vector halves) driven cycle by cycle | Baselines unchanged in count, four failing tests' timing traces moved toward the expected patterns; AccuracyCoin 78 → 79 passing |
| L3 step 1 | `a2b13e9` | Writes to `$2000-$401F` land on the cycle the chip drives them — the last cycle of a store, the cycle before it for a read-modify-write's discarded write — and the NMI a `$2000` write raises is taken up on that cycle | No ROM count change (57 pass / 17 known and AccuracyCoin 79/49 identical); `4-irq_and_dma`'s trace moved from the wrong shape to the documented one with the L3a offset (`53 53 53 0…` → `0 0 0 0 1 1 2 2 4…`, CRC `5FA88FED` → `218E154C`); `3-nmi_and_irq`'s CRC moved `90BC9498` → `7E5D001A`; two AccuracyCoin error codes changed (`$0468`/`$0469` 0x06 → 0x0A, still failing); frame cost unchanged (1.033 → 1.034 ms, release, 600 frames of nestest) |
| L3 step 2 | `d4bf4ce` | The write also lands on a *dot* inside that cycle: the PPU's per-dot view sees it from the cycle's first dot, and the APU tick at the end of the cycle sees it | ROM baseline 57 pass / 17 known → **58 / 16** (`apu_test/5-len_timing`); AccuracyCoin 79/49 → **81 / 47** (`Frame Counter 4-step`, `Frame Counter 5-step`); `4-irq_and_dma`'s CRC moved `218E154C` → `7DE0C30F`, `5-branch_delays_irq`'s `BE443B12` → `29BB6E90`, `6-irq_flag_timing` moved a sub-test ("Flag first set too late" → "Flag last set too soon"); nothing that was green regressed; frame cost unchanged (1.034 → 1.032 ms, release, 600 frames of nestest); the dot itself is not discriminated by any suite (0, 1 and 2 are byte-identical) |
| L3 step 3a | `32864f1` | An NMI asserted during an interrupt sequence, before its status push, takes the vector over: the frame pushed stays the sequence's, the handler is the NMI's | No test flips and nothing regresses (58 pass / 16 known, AccuracyCoin 81/47); `3-nmi_and_irq`'s second column comes in line with its readme for the five "NMI occurs after CLC, interrupting IRQ" rows (`23` → `00`), and its first column there moves `27` → `23` against a documented `20` |
| Fix: the branch poll | (this change) | Drops the pre-L1 "a taken branch that stays in its page skips that poll" latch: the sample belongs to the branch's own penultimate cycle, so handing it to the next instruction means a one-instruction wait loop never samples at all | No suite ever measured the latch — ROM baseline stays **62 / 12** and AccuracyCoin **82 / 46**, and `5-branch_delays_irq` keeps its CRC `29BB6E90`; `cargo test --workspace` 826 → 827; 热血格斗 (mapper 4) boots again, its VBlank wait being `$F86F: D0 FE` (a `BNE self` loop with NMI enabled) that took **0** NMIs in 600k traced instructions |
| L3 step 4 | (this change) | `$2000-$3FFF` reads land on their own cycle *and dot* - the machine runs the handler two dots into the cycle the chip performs the access on - the VBlank suppression window moves to the dot before the flag, and the NMI line is sampled once per CPU cycle instead of being set at the dot the flag rises on | ROM baseline 58 pass / 16 known → **62 / 12** (`05-nmi_timing`, `06-suppression`, `07-nmi_on_timing`, `08-nmi_off_timing`); AccuracyCoin 81/47 → **82 / 46**; `2-nmi_and_brk`'s first four rows are the readme's (`27 26 26 36`), `3-nmi_and_irq`'s second column matches its readme for the whole first half; frame cost 1.11 → 1.16 ms (release, 600 frames of nestest, both measured in one sitting), almost all of it the per-cycle line sample |
| Phase 1 | (this change) | The CPU stops being instruction-atomic and drives its own cycles the way Mesen's does: every access (`Bus::read_cycle`/`write_cycle`) *is* a cycle, the addressing's discarded reads and the handler's accesses are written out where the chip makes them, and the interrupt lines are sampled per cycle with the decision one cycle behind | ROM baseline stays **62 pass / 12 known**, but the composition moves: `2-nmi_and_brk` turns green (the vector is chosen at the status push, so the NMI takes it over) and `5-len_timing` moves onto the list, because the `$4015` read now lands on its own cycle and the sync error it was absorbing moved (§4.3 predicted exactly this); `instr_timing` 2/2 and `branch_timing` 3/3 stay green, which is what says the cycle counts survived; AccuracyCoin 82/46 unchanged; frame cost 1.204 → 1.204 ms (release, 600 frames of nestest, 5 runs each) — the parked-write machinery and the post-hoc cycle replay are gone, and the access-driven form pays for itself |
| Phase 0 | (this change) | The machine gets Mesen's clock shape: one master clock with `cpuDivider`/`ppuDivider` (`Region::cpu_divider()`/`ppu_divider()`), and a cycle's dots can be either a counted loop or a run to a master-clock target | No ROM change at all (every suite identical, AccuracyCoin 82/46, `4-irq_and_dma` `7DE0C30F`); frame cost 1.206 → 1.210 ms (release, 600 frames of nestest, 5 runs each) - the first version, which did the clock arithmetic every cycle, cost 1.266 ms and was replaced by the counted fast path |

Current state: `cargo test --workspace` 829 green, ROM baseline **62 pass / 12 known**,
AccuracyCoin **82 passed / 46 failing**, `STATE_VERSION` 6.

### 2.1 Phase 0: the clock model the rest of the work needs

The steps above made the *machine* cycle-driven while leaving the CPU instruction-atomic and
the renderer scanline-at-a-time. Taking either of those further needs one thing first: a
clock the two chips are derived from, which is what Mesen's NES core is built around
(`NesCpu::StartCpuCycle`/`EndCpuCycle` advancing one master clock and running the PPU to
`_masterClock - _ppuOffset`; `ppuDivider` 4 and `cpuDivider` 12 on NTSC, 5 and 16 on PAL).
Phase 0 is that structure, with no behaviour change:

- `Region::cpu_divider()`/`Region::ppu_divider()` are the ratio's only home. NTSC's 12/4 is
  the real 21.477 MHz crystal divided down; PAL models 15/5 rather than the real 16/5 until
  PAL has test ROMs to measure.
- `NesBus::run_ppu_to` runs the PPU to a master-clock target, and an access is placed by a
  *tick* offset (`ACCESS_DOTS * ppu_divider`) instead of "the third of three steps", so the
  placement survives a cycle that is not exactly three dots long.
- A whole ratio runs a counted dot loop instead of doing that arithmetic per cycle: the
  general form measured 4% of the frame for nothing, and the two are held together by
  `test_the_master_clock_path_places_the_same_dots`. The other test,
  `test_the_whole_ratio_flags_agree_with_the_dividers`, is what fails if PAL's numbers are
  flipped without flipping `has_whole_dot_ratio` with them.
- The master clock is derived from the cycle count rather than stored, so save states are
  untouched - the reason this landed without a `STATE_VERSION` bump.
- `Nes::cpu_cycles()` / `master_clock()` / `ppu_dots()` / `ppu_position()` / `cpu_pc()` are
  new, and `tests/cycle_trace.rs` prints a per-instruction trace from them for diffing
  against the traces the ROM readmes document.

### 2.2 Phase 1: the CPU drives its own cycles

Landed. `Cpu::execute_instruction` no longer returns a cycle count for the machine to tick:
it fetches operands through `Bus::read_cycle`, its handlers read and write through the same
call, and the discarded reads the chip spends its internal cycles on are written out in
`Cpu::fetch_operand` and in the handlers that need them (`RTS`'s two, `RTI`'s, `PLA`/`PLP`'s,
the NOPs' operand read, the branch's two). `NesBus::begin_cpu_cycle`/`end_cpu_cycle` are
Mesen's `StartCpuCycle`/`EndCpuCycle`: the PPU runs to the point in the cycle the access
belongs at, the access happens, and the rest of the cycle follows.

What that deleted: `Access`/`plan_access`/`PendingAccess`, the parked register writes and
their replay, `defer_writes`/`defer_register_reads`, the machine-side interrupt polling and
the hand-driven seven-cycle sequence, and `irq_inhibit` - the one-cycle lag in the interrupt
decision is what gives CLI/SEI/PLP their delay, so the special case went with the rest. The
interrupt latches moved to the CPU, which is where Mesen keeps them.

Two things worth calling out:

- **The opcode table's cycle counts are now only a cross-check.** Nothing ticks them; the
  tests pin them (`test_every_opcode_executes`, `instr_timing`, `branch_timing`). The
  conversion was driven by them: `instr_timing` named the opcodes whose addressing mode was
  the read form of an indexed write (`1E`, `3E`, `5E`, `7E`, `DE`, `FE`, `91`, `99`, `9D`,
  `13`, `33`, `1B`, `3B`, `1F`, `3F`), which is what `AddressingMode::AbsoluteXWrite` and
  friends are for - a store or a read-modify-write always makes the discarded read, a load
  only when the index carries.
- **KIL keeps this project's behaviour, not Mesen's.** Mesen treats a KIL as an unrecoverable
  crash and clears the interrupt samples so the hang cannot be interrupted; the hardware -
  and this core, which reports the hang to the host through `Nes::is_cpu_halted` - lets an
  interrupt restart it. The PC rewind is Mesen's; the sample clearing is not.

### 2.3 The odd frame's decision moved to dot 339 (first step of the per-dot work)

Mesen decides the odd frame's short frame while it is processing pre-render dot **339** — it
jumps its counter to 340 so that 340 is the dot that goes missing — and the check reads the
rendering bits as they are at that dot. The core used to decide on 340 itself, which counts a
`$2001` write that lands in between; the decision now happens a dot earlier, on 339, with the
310 / `skip_odd_frame_dot` latch carrying it to the dot it skips (the latch is state, so
`STATE_VERSION` went 5 → 6).

Measured: no ROM moves (`10-even_odd_timing` still reports "skipped too late", byte for byte
the same output), and the frame cost is unchanged. That is the useful result: the write's dot
*inside* its cycle and the decision's dot are now both where Mesen puts them, so what is left
of that test is not this boundary — it is the fetch-level behaviour of the visible line, which
is the per-dot renderer.

Then the renderer becomes a per-dot pipeline (§6's decision 2, which is also what A12,
sprite 0 and the odd-frame skip are waiting for), and the frame counter's phase is the next
thing the APU needs.

### 2.4 A12 cannot be reconstructed on its own (experiment, reverted)

The obvious cheaper route to `mmc3_test_2/4-scanline_timing` is to leave the whole-scanline
renderer in place and reconstruct the *fetch address sequence* per dot in `ppu_bus_address`,
the way the odd-frame dot was fixed: Mesen's `LoadTileInfo` reads the nametable byte on
`cycle % 8 == 1`, the attribute byte on 3 and the pattern bytes on 5 and 7, and the sprite
phase's garbage nametable and attribute reads on `(cycle - 257) % 8 == 0` and `2` are what
lets A12 go low between the sprite pattern fetches.

Implemented and measured: `4-scanline_timing` moved from its second sub-test to its fourth
(so the pre-render and sprite-phase behaviour is right with it), and **`2-details` regressed**
- "Counter should be clocked 241 times in PPU frame" - because the sequence and the counter's
filter are one mechanism: with the real addresses A12 is low for four dots between fetches,
and it is the nine-dot low run across a scanline boundary (dots 337-340 plus the next line's
0-4) that is meant to be the frame's only qualifying rise, one per scanline. Feeding the
filter a more accurate sequence without the fetches that go with it moves that count.

So it is reverted, and this is now the concrete argument for the per-dot renderer: the
addresses *and* the dot they land on have to come from the same place, which is the fetch
itself.

### 2.5 What the A12 work landed, and the instrument it now needs

Landed since that revert, each measured against the MMC3 suite:

- the cartridge is handed the address of the *last real access* and it is held between
  accesses (Mesen's `SetBusAddress`/`_ppuBusAddress`), instead of following the VRAM address
  register - which is what keeps the long low run over vertical blanking intact, and what
  makes the pre-render line's fetches matter;
- only the dots that really fetch drive it (Mesen calls `SetBusAddress` from `ReadVram`, and
  nowhere else), so an address a CPU access left on the bus through `$2007` survives until the
  pipeline's next fetch;
- the counter's filter is `_cyclesDown`'s rule: a rise resets the run whether or not it
  counted (counting across runs clocks a scanline counter many times per line).

Measured: `2-details` ("clocked 241 times") green, and `4-scanline_timing` from its second
sub-test to its twelfth - where `$2000=$10` leaves the last visible line's IRQ one dot early.

**The instrument this now needs.** `tests/cycle_trace.rs`'s `trace_a12_for_a_scanline` prints
the bus address per *instruction*, which is too coarse: the filter's threshold is nine dots (a
third of a cycle's worth of resolution is missing), and several fetches can land inside one
instruction. What the next step needs is a dot-level sampler: either a `Nes::step_dot()`
alongside `step_instruction` (a debugger-grade primitive, which is what `run_cycles` /
`step_instruction` already are), or a `Nes::ppu_bus_trace()` that the core fills in. With that,
the last sub-test is a comparison rather than an argument: which line's rise is one dot early,
and whether it is the boundary run (dots 337-340 plus the next line's 0-4) or the first pattern
fetch after the sprite phase's long low run.

Three experiments were implemented, measured and **reverted**; their evidence is the
starting point for what follows (§4). L3a's bus-side parking has since come back as L3 step
1, with the dependents its revert was about moved along with it, and the step-4 read
experiment is now step 4 itself (the bisect it produced is what pinned the read's dot down).

## 3. What is left

### 3.1 The accuracy backlog the ROM suites name

Eleven cases remain. They are not all the same kind of problem:

| Suite | Cases | What they need |
| --- | --- | --- |
| `cpu_interrupts_v2` | `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq` | The interrupt sequence's own cycle, now that the reads and writes around it are placed: `4-irq_and_dma`'s remaining offset tracks the frame counter's phase (§4.3), and the other three are down to one or two instructions |
| `ppu_vbl_nmi` | `10-even_odd_timing` | The dot the odd-frame clock skip is decided on — the same per-dot rendering `4-scanline_timing` waits for |
| `mmc3_test_2` | `4-scanline_timing` | The exact dot the first A12 rise of a frame lands on — §4, plus the existing dot-level A12 model |
| `ppu_open_bus` | the whole ROM | PPU data-bus decay, a **separate feature**, nothing to do with cycles |
| `apu_test` | `4-jitter`, `6-irq_flag_timing`, `7-dmc_basics`, `8-dmc_rates` | Frame-counter and DMC timing; `5-len_timing` turned out **not** to be a length-counter bug but a write-phase one, and step 2 fixed it |

Four more have left the list: §4.2 steps 1-4 placed the register writes and the `$2002`
reads on their own cycles and dots and gave the NMI line its sampling, which is what
`05-nmi_timing`, `06-suppression`, `07-nmi_on_timing` and `08-nmi_off_timing` were
waiting for.

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

### 4.1 What the reverted experiments taught

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
- **Step 4 moved that convention to dot 2**, the point in the cycle the chip performs an
  access — the same dot the `$2002` reads landed on once their own placement was measured
  against the tests. The write's own dot is still not discriminated by anything (dot 0 and
  dot 2 measure identically), but the two accesses now sit at the same point in the cycle
  by construction rather than by accident.
- **A later dot costs a frame rate, not just tidiness.** Placing the write *inside* the
  cycle means stepping that cycle's three dots one at a time, which needs a second
  dot-stepping path next to `advance_one_cpu_cycle`; measured, its mere presence took the
  frame from 1.034 to 1.104 ms (release, 600 frames of nestest, ~7%) with no ROM result
  moving at all. Disabling the parking entirely still cost 1.11 ms, so the price is the
  optimiser giving up on the hot cycle loop, not the write handling. The landed shape keeps
  the loop as it was - one extra `Option` in an inlined helper - and lands the write on the
  cycle's first dot, which needs no per-dot stepping at all. (Step 4 moved it two dots in,
  to the point in the cycle the chip performs an access; see below.)

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

**Step 4 (done): reads with side effects, and the NMI line.** `$2002` reads clear the VBlank
flag and `$2007` reads move the VRAM address; both used to happen at the instruction's start,
three cycles early and blind to the dot. `02-vbl_set_time` passed by luck of that
granularity; `06-suppression` and `05-nmi_timing` did not. Three things moved, and each was
measured on its own.

*The read lands on its own cycle and dot.* The obvious way in was to split
`Cpu::execute_instruction` into "fetch operands" and "perform the access"; the landed shape
is smaller, and does not need the split: the CPU still does all the addressing in one go,
and a read of the PPU window is left to the machine the way a write is. `Cpu::plan_access`
works out which cycle
of the instruction the handler's access belongs to — the last cycle for a load (one later
for one that pays a page-crossing penalty), two before the end for a read-modify-write's
read, which is what `Access` encodes from the opcode's mnemonic — and `PendingAccess` hands
the handler over. The machine runs it two dots into that cycle
(`NesBus::advance_one_cpu_cycle_with_read`).

*Two dots is measured, not chosen.* The step-4 first try — clearing on the read's own cycle
but leaving the value where the instruction asked for it — put the clear a cycle late for
`02-vbl_set_time`'s pair of reads (`00 - -` where the readme has `00 - V`) and was reverted;
it also showed the read's *value* has to move with its clearing, which no amount of
re-scheduling the clear can do. Bisecting the read's dot instead, with `05-nmi_timing`,
`06-suppression` and `02-vbl_set_time` as the yardsticks, gave a clean one-row-per-dot
gradient: at the cycle's start the NMI boundary falls two rows early, at its end three rows
late, and the truth is in the middle. The same window is what the VBlank suppression race
became: one dot wide, a read standing on dot 0 of scanline 241, whose successor is the dot
the flag is set on.

*The NMI line is sampled once per cycle.* `NesBus::nmi_pending` used to be set on the dot the
flag rose on, which made `06-suppression`'s `05`/`06` rows fire an NMI the hardware does not
(the read that cleared the flag in the same cycle could not take the line back). It is now a
latch that a sample at the end of every CPU cycle leaves behind: a line that comes and goes
inside one cycle is never seen, and a line that is seen stays latched until the CPU services
it. That is what separates `05 V -` from `07 V N` in `06-suppression`, and it is also why
`07-nmi_on_timing`'s rows 02-04 keep their NMI - a `$2000` write raised the line in a cycle
whose end the sample saw it high at, and the `$2002` read that cleared the flag later in the
same instruction cannot take that back.

*The window is the PPU's.* `$2000-$3FFF` is deferred; the APU's `$4015` is not, yet: blargg's
`apu_test` synchronises through it and the frame-counter phase it measures is a cycle or two
out anyway (§4.3), so moving that read now only moves that error around. It is the one thing
left to revisit here.

Measured: ROM baseline 58 pass / 16 known → **62 / 12** (`05-nmi_timing`,
`06-suppression`, `07-nmi_on_timing`, `08-nmi_off_timing` — blargg's `ppu_vbl_nmi` is 9/10,
with only the odd-frame skip left); AccuracyCoin 81 passed / 47 failing → **82 / 46**;
nothing that was green regressed, `02-vbl_set_time`, `04-nmi_control`, `3-A12_clocking`,
`5-len_timing` and `oam_stress` included. `2-nmi_and_brk`'s first four rows are now the
readme's (`27 26 26 36` against the documented `27 26 26 36 00 …`) and `3-nmi_and_irq`'s
second column matches its readme's `00` for the whole first half; both are still short of
the full table, which is the hijack work of step 3 meeting the test's own frame-by-frame
drift rather than anything the read's placement can reach. Cost: the per-cycle sample
is worth about 4% of the frame (1.11 → 1.16 ms/frame, both measured in one sitting, 600
frames of nestest in release); placing the read is free, and moving the *write* from the
cycle's first dot to the same dot as the read costs nothing measurable - it is what makes
the two accesses symmetric, and `5-len_timing` and AccuracyCoin's frame-counter tests stay
green with it.

### 4.3 L4 — DMA and the renderer (after L3)

- **`4-irq_and_dma`**: *the guess that stood here — "keep polling while the CPU is stalled, and
  know which cycle the DMA releases the CPU on" — was implemented and measured, and neither
  half moves the documented head of the trace (§4.1's L4a).* The lag tracks the IRQ source's
  phase, so this case belongs with the frame-counter item below.
- **`7-dmc_basics`, `8-dmc_rates`, `4-jitter`, `6-irq_flag_timing`**: DMC fetch timing and the
  frame counter's exact phase relative to CPU cycles — which is also where `4-irq_and_dma`'s
  remaining offset lives.

  **The frame counter's phase, measured and not landed.** A `$4017` write restarts the
  counter, and *when* the restart lands is what everything else hangs off. Three pieces were
  built and measured together:
  - the restart is delayed: the write's mode and IRQ inhibit apply at once, but the counter
    passes its boundaries three CPU cycles later - four when the write falls on the other
    half of the APU clock, which is the free-running divide-by-two `4-jitter` measures both
    ways;
  - the IRQ flag is set three cycles after the step that asks for it, and the sequence asks
    again on its last two ticks - the "set three times in a row" of `6-irq_flag_timing`; and
  - `$4015` reads are placed like the PPU's (two dots into their own cycle) rather than at
    the instruction's start, which the same test's `sync_apu` needs to see the phase at all.

  Measured, that turns `4-jitter` and `6-irq_flag_timing` green ("Frame irq is set too
  soon" → Passed, "Flag first set too soon" → Passed) and keeps `5-len_timing` - but
  AccuracyCoin loses three: `Frame Counter 4-step`/`Frame Counter 5-step`
  (`$0468`/`$0469`) fail again, and `$0460`/`$0461`'s error codes move (0x06 → 0x0A and
  0x22 → 0x06). Two cases for three is a regression by this plan's rule, so it is reverted,
  not landed. The APU-side read placement on its own is innocent (AccuracyCoin stays at
  82/46 with it), so the disagreement is in the counter's phase - one of the two suites is
  reading a different event than the other, and finding out which is the first thing this
  item needs.
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
3. L3 step 3 — **done for the IRQ side and the BRK vector, blocked on the read side**: the
   hijack itself was landed in 3a/3b, and the rows still failing in `2-nmi_and_brk` and
   `3-nmi_and_irq` turned out to be the test's own `$2002` synchronisation, which is step 4.
4. L3 step 4 — **done**: all four `ppu_vbl_nmi` cases turned green. The two `cpu_interrupts_v2`
   cases step 3 was waiting on moved closer with it (step 4's measurements above) but are not
   there yet; what is left of them is the interrupt sequence's own cycle, not the read.
5. L4 — the DMA/frame-counter group, then sprite 0 timing.

Each step is one commit, and each is only kept if the ROM baseline does not regress: the
suites carry an expected-fail list and assert *both* directions (`tests/rom_tests.rs`), and
AccuracyCoin carries a failing-count ratchet (currently 46).

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
  `cargo test -p nes-core --release --test frame_cost -- --ignored --nocapture`, a few runs
  and compare medians) — L1's net effect was a small *speed-up*, and the L3 steps are the
  first ones expected to cost something. `tests/cycle_trace.rs` prints a per-instruction
  trace the same way, for diffing against the traces the ROM readmes document.
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
