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
