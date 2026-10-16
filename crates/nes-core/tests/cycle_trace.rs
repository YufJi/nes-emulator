//! Per-instruction cycle trace, for diffing our timing against the trace a test
//! ROM prints for itself.
//!
//! The accuracy work needs to know *which cycle* an access lands on, and the
//! suites that measure it (blargg's `cpu_interrupts_v2`, `ppu_vbl_nmi`) print
//! their own measurements, not ours. This tool prints the machine's side so the
//! two can be read side by side; it is a diagnostic, not a check, so it is
//! `#[ignore]`d and does nothing unless it is pointed at a ROM:
//!
//! ```text
//! NES_TRACE_ROM=roms/blargg/cpu_interrupts_v2/rom_singles/4-irq_and_dma.nes \
//! NES_TRACE_FRAMES=20 \
//!   cargo test -p nes-core --test cycle_trace -- --ignored --nocapture
//! ```
//!
//! Columns: the instruction's program counter, the CPU cycle it started on,
//! the cycles it took, the PPU position it left behind (scanline, dot), and the
//! PPU dot count the master clock has reached.

use std::path::PathBuf;

use nes_core::Nes;

#[test]
#[ignore = "a diagnostic tool: set NES_ACCURACYCOIN=1 to run the suite and print its failures"]
fn run_accuracy_coin_and_report() {
    if std::env::var_os("NES_ACCURACYCOIN").is_none() {
        println!("set NES_ACCURACYCOIN=1 to run AccuracyCoin and print its result table");
        return;
    }
    const MENU_FRAMES: u32 = 90;
    const RESULTS: std::ops::Range<u16> = 0x0400..0x0500;

    let path = resolve_rom_path("roms/accuracycoin/AccuracyCoin.nes");
    if !path.exists() {
        println!("skipped: no roms/accuracycoin/AccuracyCoin.nes");
        return;
    }
    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    // The ROM is menu-driven: let the menu come up, then press Start to run
    // every test.
    for _ in 0..MENU_FRAMES {
        nes.frame();
    }
    nes.set_button(1, nes_core::nes::BUTTON_START, true);
    nes.frame();
    nes.frame();
    nes.set_button(1, nes_core::nes::BUTTON_START, false);
    let budget: u32 = std::env::var("NES_ACCURACYCOIN_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3600);
    for _ in 0..budget {
        nes.frame();
    }

    let mut passed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();
    for addr in RESULTS {
        match nes.peek(addr) {
            0 => {}
            0xFF => skipped += 1,
            value if value & 1 == 1 => passed += 1,
            value => failures.push((addr, value)),
        }
    }
    println!(
        "AccuracyCoin: {} tests ran, {passed} passed, {skipped} skipped, {} failing",
        nes.peek(0x37),
        failures.len()
    );
    for (addr, value) in &failures {
        // The byte is `(check << 2) | 2`, so the failing check is `(v >> 2) + 1`.
        println!(
            "  fail at {addr:#06X} = {value:#04X} (check {})",
            (value >> 2) + 1
        );
    }
}

/// Follow a ROM's blargg-style status while it runs.
///
/// A suite that never reports `$6000 = $80` is either still running or stuck,
/// and the difference matters: this prints the status byte and the report text
/// every so many frames so the two can be told apart.
///
/// ```text
/// NES_STATUS_ROM=roms/nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma.nes \
///   cargo test -p nes-core --test cycle_trace dump_status_for_a_rom -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_STATUS_ROM to the .nes file to follow"]
fn dump_status_for_a_rom() {
    let Some(requested) = std::env::var("NES_STATUS_ROM").ok() else {
        println!("set NES_STATUS_ROM=<path to a .nes file> to follow its status");
        return;
    };
    let path = resolve_rom_path(&requested);
    let frames: u32 = std::env::var("NES_STATUS_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(600);
    let every: u32 = std::env::var("NES_STATUS_EVERY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    for frame in 1..=frames {
        nes.frame();
        if frame % every == 0 || frame <= 3 {
            let status = nes.peek(0x6000);
            let mut text = String::new();
            for offset in 0..64u16 {
                let byte = nes.peek(0x6004 + offset);
                if byte == 0 {
                    break;
                }
                text.push(if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                });
            }
            println!(
                "frame {frame:5}: $6000={status:#04X} pc={:#06X} halted={} {text}",
                nes.cpu_pc(),
                nes.is_cpu_halted()
            );
        }
    }
}

#[test]
#[ignore = "a diagnostic tool: set NES_DUMP_ROM to the .nes file to dump frames"]
fn dump_frames_of_a_rom() {
    let Some(requested) = std::env::var("NES_DUMP_ROM").ok() else {
        println!("set NES_DUMP_ROM=<path to a .nes file> to dump its frames");
        return;
    };
    let path = resolve_rom_path(&requested);
    let frames: u32 = std::env::var("NES_DUMP_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120);
    let every: u32 = std::env::var("NES_DUMP_EVERY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(frames);
    let directory = std::env::var("NES_DUMP_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame".to_string());

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    // The frame buffer holds `0xAARRGGBB` words, so on a little-endian host the
    // bytes land as B, G, R, A — read in that order here, which is what keeps
    // red and blue from swapping.
    for frame in 1..=frames {
        nes.frame();
        if frame % every != 0 {
            continue;
        }
        let mut ppm = b"P6\n256 240\n255\n".to_vec();
        for pixel in nes.frame_buffer() {
            ppm.push((pixel >> 16) as u8);
            ppm.push((pixel >> 8) as u8);
            ppm.push(*pixel as u8);
        }
        let out = format!("{directory}/{stem}_f{frame:04}.ppm");
        std::fs::write(&out, ppm).expect("write the frame");
        println!("wrote {out}");
    }
}

/// The path a `NES_*_ROM` variable names, resolved from the repository root when
/// it is written the way the docs write it (`roms/...`).
fn resolve_rom_path(requested: &str) -> PathBuf {
    if std::path::Path::new(requested).exists() {
        return PathBuf::from(requested);
    }
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root.pop();
    root.push(requested);
    root
}

#[test]
#[ignore = "a diagnostic tool: set NES_TRACE_ROM to the .nes file to trace"]
fn trace_a_rom() {
    let Some(requested) = std::env::var("NES_TRACE_ROM").ok() else {
        println!("set NES_TRACE_ROM=<path to a .nes file> to trace it");
        return;
    };
    // Cargo runs a test with the crate as the working directory, so a path
    // written the way the docs write it (`roms/...`) has to be resolved from
    // the repository root.
    let path = if std::path::Path::new(&requested).exists() {
        PathBuf::from(&requested)
    } else {
        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        root.pop();
        root.pop();
        root.push(&requested);
        root
    };
    let frames: u64 = std::env::var("NES_TRACE_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    println!(
        "{}: region {:?}, tracing {frames} frames",
        path.display(),
        nes.region()
    );
    println!(
        "{:>4} {:>10} {:>3} {:>3} {:>3} {:>10}",
        "pc", "cpu_cycle", "cyc", "ln", "dot", "ppu_dots"
    );

    let mut instructions = 0u64;
    let mut frame = nes.frame_number();
    for _ in 0..frames {
        // Instruction by instruction until the picture advances, so the trace
        // starts and ends on a frame boundary like the ROM's own report does.
        while nes.frame_number() == frame {
            let pc = nes.cpu_pc();
            let cycle = nes.cpu_cycles();
            let cycles = nes.step_instruction();
            let (scanline, dot) = nes.ppu_position();
            println!(
                "{pc:04X} {cycle:>10} {cycles:>3} {scanline:>3} {dot:>3} {:>10}",
                nes.ppu_dots()
            );
            instructions += 1;
        }
        frame = nes.frame_number();
    }

    println!("{instructions} instructions traced");
}

/// Dump the PPU bus address the cartridge is being handed, one line per
/// instruction.
///
/// This is the view an A12-sensitive mapper has: which address is on the bus,
/// and when it changes. The pipeline drives it at fixed dots, so instruction
/// granularity (a third of a dot-per-cycle rate) is enough to see which dots a
/// rise lands on and how long the low run before it was.
///
/// ```text
/// NES_A12_ROM=roms/blargg/mmc3_test_2/rom_singles/4-scanline_timing.nes \
/// NES_A12_FRAMES=2 NES_A12_LINE=239 \
///   cargo test -p nes-core --test cycle_trace -- --ignored --nocapture a12
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_A12_ROM to the .nes file to trace"]
fn trace_a12_for_a_scanline() {
    let Some(requested) = std::env::var("NES_A12_ROM").ok() else {
        println!("set NES_A12_ROM=<path to a .nes file> to trace its PPU bus");
        return;
    };
    let mut path = std::path::PathBuf::from(&requested);
    if !path.exists() {
        let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        root.pop();
        root.pop();
        path = root.join(&requested);
    }
    let frames: u64 = std::env::var("NES_A12_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let want: i64 = std::env::var("NES_A12_LINE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(239);

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    println!(
        "{}: tracing line {want} of {frames} frame(s)",
        path.display()
    );
    println!(
        "{:>6} {:>3} {:>3} {:>6} {:>4}",
        "pc", "ln", "dot", "a12addr", "a12"
    );

    // The frame that matters is the one the test measures, which is after it
    // has synchronised - so the tool can run the ROM forward without printing.
    let skip: u64 = std::env::var("NES_A12_SKIP")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    for _ in 0..skip {
        nes.frame();
    }

    let until = nes.frame_number() + frames;
    let mut printed = 0;
    let mut last = u16::MAX;
    while nes.frame_number() < until {
        let (scanline, dot) = nes.ppu_position();
        let addr = nes.ppu_bus_address();
        if scanline as i64 == want && addr != last {
            println!(
                "{:04X} {scanline:>3} {dot:>3} {addr:04X} {:>4}",
                nes.cpu_pc(),
                if addr & 0x1000 != 0 { "high" } else { "low" }
            );
            last = addr;
            printed += 1;
        }
        nes.step_instruction();
    }
    println!("{printed} bus changes on that line");
}

/// One line per PPU dot inside a window: the registers, the scroll, the bus
/// address, and every register write that landed on that dot.
///
/// This is the tool for "that write landed on the wrong side of a fetch": a
/// sprite slot's pattern fetch is a *dot*, so a `$2000`/`$2001` write has to be
/// read against the dot it arrived on rather than against the instruction it
/// came from. `docs/sprite-phase-plan.md` §3 is where it came from; S3 adds the
/// sprite shifters to what is printed.
///
/// ```text
/// NES_PPU_DOT_LOG=roms/accuracycoin/AccuracyCoin.nes \
/// NES_DOT_WINDOW=100:250-100:290 NES_DOT_SKIP=200 \
///   cargo test --release -p nes-core --test cycle_trace -- --ignored --nocapture dots
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_PPU_DOT_LOG and NES_DOT_WINDOW"]
fn log_every_ppu_dot() {
    let Some(requested) = std::env::var("NES_PPU_DOT_LOG").ok() else {
        println!("set NES_PPU_DOT_LOG=<path to a .nes file> to log its PPU dots");
        return;
    };
    let Some(window_text) = std::env::var("NES_DOT_WINDOW").ok() else {
        println!("set NES_DOT_WINDOW=<line>:<dot>-<line>:<dot>, e.g. 100:250-100:290");
        return;
    };
    let Some(window) = parse_dot_window(&window_text) else {
        panic!("NES_DOT_WINDOW should be <line>:<dot>-<line>:<dot>, e.g. 100:250-100:290");
    };
    let path = resolve_rom_path(&requested);
    let skip: u64 = env_number("NES_DOT_SKIP", 0);

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");

    for _ in 0..skip {
        nes.frame();
    }

    // A whole frame of samples fits (an NTSC frame is 89,342 dots), so the
    // window can be anywhere on the frame being logged.
    const FRAME_DOTS: usize = 90_000;
    nes.set_ppu_bus_trace_capacity(FRAME_DOTS);
    nes.set_ppu_write_log_capacity(4096);
    nes.clear_ppu_bus_trace();
    nes.clear_ppu_write_log();
    nes.frame();

    println!(
        "{}: dots {window_text} of frame {}",
        path.display(),
        nes.frame_number()
    );
    println!(
        "{:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>6} {:>6} {:>3} {:>6}  writes",
        "line", "dot", "$2000", "$2001", "$2002", "$2003", "v", "t", "x", "bus"
    );

    let writes: Vec<nes_core::nes::PpuWriteSample> = nes.ppu_write_log().collect();
    let mut printed = 0;
    for sample in nes.ppu_bus_trace() {
        let position = (sample.scanline, sample.dot);
        if position < window.0 || position > window.1 {
            continue;
        }
        let landed = writes
            .iter()
            .filter(|write| (write.scanline, write.dot) == position)
            .map(|write| format!("${:04X}=${:02X}", write.addr, write.value))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{:>4} {:>4} {:>02X} {:>02X} {:>02X} {:>02X} {:>6} {:>6} {:>3} {:>6}  {landed}",
            sample.scanline,
            sample.dot,
            sample.ctrl,
            sample.mask,
            sample.status,
            sample.oam_addr,
            format!("{:04X}", sample.v),
            format!("{:04X}", sample.t),
            sample.x,
            format!("{:04X}", sample.address),
        );
        printed += 1;
    }
    println!("{printed} dots printed");
}

/// Every write to a PPU register, with the frame, scanline and dot it landed
/// on, written to a file meant to be diffed between two builds: the *first*
/// write that differs is worth more than any count.
///
/// ```text
/// NES_REGLOG_ROM=roms/accuracycoin/AccuracyCoin.nes NES_REGLOG_FRAMES=400 \
///   cargo test --release -p nes-core --test cycle_trace -- --ignored --nocapture reglog
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_REGLOG_ROM to the .nes file to log"]
fn log_ppu_register_writes() {
    let Some(requested) = std::env::var("NES_REGLOG_ROM").ok() else {
        println!("set NES_REGLOG_ROM=<path to a .nes file> to log its PPU register writes");
        return;
    };
    let path = resolve_rom_path(&requested);
    let frames: u64 = env_number("NES_REGLOG_FRAMES", 120);
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rom".to_string());
    let out = std::env::var("NES_REGLOG_OUT").unwrap_or_else(|_| format!("/tmp/reglog-{stem}.txt"));

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");
    nes.set_ppu_write_log_capacity(4096);

    let mut log = String::new();
    let mut printed = 0u64;
    while nes.frame_number() < frames {
        nes.step_instruction();
        // One line per write, in the order the machine made them; the log is
        // drained per instruction so nothing can be missed by the ring filling.
        for write in nes.ppu_write_log() {
            log.push_str(&format!(
                "frame {:>5} line {:>3} dot {:>3} ${:04X} = ${:02X}  $2002={:02X}\n",
                nes.frame_number(),
                write.scanline,
                write.dot,
                write.addr,
                write.value,
                write.status,
            ));
            printed += 1;
        }
        nes.clear_ppu_write_log();
    }
    std::fs::write(&out, log).expect("write the log");
    println!("{printed} writes to {out}");
}

/// Every change to AccuracyCoin's result table, with the frame it happened on.
///
/// The suite's verdicts land one test at a time; this names the first test a
/// change moves, which is how the last two attempts located their first
/// divergence (the alignment document's §7).
///
/// ```text
/// NES_RESULTLOG=1 cargo test --release -p nes-core --test cycle_trace \
///   -- --ignored --nocapture results
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_RESULTLOG=1"]
fn log_accuracy_coin_results() {
    if std::env::var_os("NES_RESULTLOG").is_none() {
        println!("set NES_RESULTLOG=1 to follow AccuracyCoin's result table");
        return;
    }
    const MENU_FRAMES: u32 = 90;
    const RESULTS: std::ops::Range<u16> = 0x0400..0x0500;

    let path = resolve_rom_path("roms/accuracycoin/AccuracyCoin.nes");
    if !path.exists() {
        println!("skipped: no roms/accuracycoin/AccuracyCoin.nes");
        return;
    }
    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");
    for _ in 0..MENU_FRAMES {
        nes.frame();
    }
    nes.set_button(1, nes_core::nes::BUTTON_START, true);
    nes.frame();
    nes.frame();
    nes.set_button(1, nes_core::nes::BUTTON_START, false);

    let budget: u64 = env_number("NES_ACCURACYCOIN_FRAMES", 3600);
    let mut previous: Vec<u8> = RESULTS.clone().map(|addr| nes.peek(addr)).collect();
    let mut counter = nes.peek(0x37);
    for frame in 1..=budget {
        nes.frame();
        let seen = nes.peek(0x37);
        if seen != counter {
            println!("frame {frame:5}: $37 {counter} -> {seen}");
            counter = seen;
        }
        for (index, addr) in RESULTS.enumerate() {
            let value = nes.peek(addr);
            if value != previous[index] {
                let how = match value {
                    0xFF => "marked to skip".to_string(),
                    value if value & 1 == 1 => "passing".to_string(),
                    value => format!("failing at check {}", (value >> 2) + 1),
                };
                println!(
                    "frame {frame:5}: {addr:#06X}: {:#04X} -> {value:#04X} ({how})",
                    previous[index]
                );
                previous[index] = value;
            }
        }
    }
    println!("followed {budget} frames");
}

/// A ROM's own progress counter, printed when it changes — the tool for "the
/// ROM stopped running its table".
///
/// The second attempt at the sprite phase lost nine of AccuracyCoin's tests to
/// a hang that only showed up at 9000 frames; a stall has to be visible on the
/// frame it starts. `NES_PROGRESS_ADDR` defaults to `$37`, AccuracyCoin's test
/// counter; point it at any byte the ROM under study increments.
///
/// ```text
/// NES_PROGRESS_ROM=roms/accuracycoin/AccuracyCoin.nes NES_PROGRESS_FRAMES=9000 \
///   cargo test --release -p nes-core --test cycle_trace -- --ignored --nocapture progress
/// ```
#[test]
#[ignore = "a diagnostic tool: set NES_PROGRESS_ROM to the .nes file to follow"]
fn log_rom_progress() {
    let Some(requested) = std::env::var("NES_PROGRESS_ROM").ok() else {
        println!("set NES_PROGRESS_ROM=<path to a .nes file> to follow its progress");
        return;
    };
    let path = resolve_rom_path(&requested);
    let frames: u64 = env_number("NES_PROGRESS_FRAMES", 9000);
    let stall: u64 = env_number("NES_PROGRESS_STALL", 600);
    let counter_addr: u16 = env_number("NES_PROGRESS_ADDR", 0x37) as u16;
    let menu_frames: u64 = env_number("NES_PROGRESS_MENU", 90);

    let data = std::fs::read(&path).expect("read the ROM");
    let mut nes = Nes::new();
    nes.load_rom(&data).expect("load the ROM");
    for _ in 0..menu_frames {
        nes.frame();
    }
    if std::env::var_os("NES_PROGRESS_START").is_some() {
        nes.set_button(1, nes_core::nes::BUTTON_START, true);
        nes.frame();
        nes.frame();
        nes.set_button(1, nes_core::nes::BUTTON_START, false);
    }

    println!(
        "{}: {frames} frames, following ${counter_addr:04X}",
        path.display()
    );
    let mut last = nes.peek(counter_addr);
    let mut still_since = 1u64;
    for frame in 1..=frames {
        nes.frame();
        let counter = nes.peek(counter_addr);
        if counter != last {
            println!(
                "frame {frame:5}: ${counter_addr:04X} = {:#04X}, pc = {:#06X}",
                counter,
                nes.cpu_pc()
            );
            last = counter;
            still_since = frame;
        } else if frame - still_since == stall {
            println!(
                "frame {frame:5}: stalled for {stall} frames at ${counter_addr:04X} = {:#04X}, pc = {:#06X}",
                counter,
                nes.cpu_pc()
            );
        }
    }
}

/// `<line>:<dot>`.
fn parse_position(text: &str) -> Option<(u16, u16)> {
    let (line, dot) = text.split_once(':')?;
    Some((line.trim().parse().ok()?, dot.trim().parse().ok()?))
}

/// `<line>:<dot>-<line>:<dot>`, inclusive at both ends.
fn parse_dot_window(text: &str) -> Option<((u16, u16), (u16, u16))> {
    let (from, to) = text.split_once('-')?;
    Some((parse_position(from)?, parse_position(to)?))
}

/// A `u64` from the environment, or `default` when it is unset or unreadable.
fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
