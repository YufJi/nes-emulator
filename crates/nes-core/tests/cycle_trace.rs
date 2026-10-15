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
