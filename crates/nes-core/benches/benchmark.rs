//! Performance Benchmarks for NES Emulator Core
//!
//! This benchmark suite measures the performance of the NES emulator
//! and calculates FPS (frames per second) metrics.
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nes_core::apu::Apu;
use nes_core::cpu::Cpu;
use nes_core::mapper::create_mapper;
use nes_core::ppu::Ppu;
use nes_core::{Bus, Mapper};

/// Simple Bus implementation for benchmarking
struct BenchBus {
    cpu_ram: [u8; 0x800],
    mapper: Box<dyn Mapper>,
    ppu: Ppu,
    apu: Apu,
}

impl BenchBus {
    fn new(mapper: Box<dyn Mapper>) -> Self {
        Self {
            cpu_ram: [0; 0x800],
            mapper,
            ppu: Ppu::new(),
            apu: Apu::new(),
        }
    }

    /// Create a test bus with a simple ROM
    fn create_test_bus() -> Self {
        let prg_rom = vec![0xEAu8; 16384]; // NOP instructions
        let chr_rom = vec![0u8; 8192];
        let mapper = create_mapper(0, prg_rom, chr_rom).unwrap();
        let mut bus = Self::new(mapper);
        bus.ppu.reset();
        bus.apu.reset();
        bus
    }

    /// Create a bus loaded with nestest ROM if available
    fn create_nestest_bus() -> Option<Self> {
        let rom_path = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?)
            .parent()?
            .parent()?
            .parent()?
            .join("roms/nestest/nestest.nes");

        if !rom_path.exists() {
            return None;
        }

        let data = std::fs::read(rom_path).ok()?;
        let (header, data_offset) = parse_rom_header(&data).ok()?;

        let prg_start = data_offset;
        let prg_end = prg_start + header.prg_rom_size;
        let prg_rom = data[prg_start..prg_end].to_vec();

        let chr_start = prg_end;
        let chr_end = chr_start + header.chr_rom_size;
        let chr_rom = if data.len() >= chr_end {
            data[chr_start..chr_end].to_vec()
        } else {
            vec![]
        };

        let mapper = create_mapper(header.mapper_number, prg_rom, chr_rom).ok()?;
        let mut bus = Self::new(mapper);
        bus.ppu.reset();
        bus.apu.reset();
        Some(bus)
    }

    /// Step PPU for a given number of CPU cycles
    ///
    /// The real machine advances the PPU inside `NesBus::advance_cycles`, which
    /// is private; this benchmark drives its own copy of the wiring, so the dot
    /// counter it needs lives here.
    fn step_ppu(&mut self, cpu_cycles: u32) {
        for _ in 0..(cpu_cycles * 3) {
            self.ppu.cycle += 1;
            if self.ppu.cycle > 340 {
                self.ppu.cycle = 0;
                self.ppu.scanline += 1;
                if self.ppu.scanline > 261 {
                    self.ppu.scanline = 0;
                    self.ppu.frame += 1;
                }
            }
        }
    }

    /// Step APU for a given number of CPU cycles
    fn step_apu(&mut self, cpu_cycles: u32) {
        for _ in 0..cpu_cycles {
            self.apu.tick();
        }
    }
}

impl Bus for BenchBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(addr),
            0x4000..=0x4015 => self.apu.read_register(addr),
            0x4016..=0x4017 => 0, // Controller input
            _ => self.mapper.read(addr),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr, value),
            0x4000..=0x4015 => self.apu.write_register(addr, value),
            0x4016..=0x4017 => {} // Controller strobe
            _ => self.mapper.write(addr, value),
        }
    }

    fn on_cpu_cycle(&mut self, _cycles: u8) {}

    fn nmi_pending(&self) -> bool {
        self.ppu.nmi_pending()
    }

    fn clear_nmi(&mut self) {
        self.ppu.clear_nmi();
    }

    fn irq_pending(&self) -> bool {
        self.mapper.irq_pending()
    }
}

/// ROM header structure for loading test ROMs
struct RomHeader {
    prg_rom_size: usize,
    chr_rom_size: usize,
    mapper_number: u16,
}

fn parse_rom_header(data: &[u8]) -> Result<(RomHeader, usize), Box<dyn std::error::Error>> {
    if data.len() < 16 || &data[0..4] != b"NES\x1a" {
        return Err("Invalid ROM header".into());
    }

    let prg_rom_size = data[4] as usize * 16384;
    let chr_rom_size = data[5] as usize * 8192;
    let flags6 = data[6];
    let flags7 = data[7];
    let has_trainer = (flags6 & 0x04) != 0;
    let mapper_number = ((flags7 & 0xF0) | ((flags6 & 0xF0) >> 4)) as u16;

    let header_size = 16;
    let trainer_size = if has_trainer { 512 } else { 0 };
    let data_offset = header_size + trainer_size;

    Ok((
        RomHeader {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
        },
        data_offset,
    ))
}

/// Execute one full frame of emulation
///
/// An NTSC frame consists of:
/// - 262 scanlines (0-261)
/// - 341 PPU cycles per scanline
/// - 3 PPU cycles = 1 CPU cycle
///
/// Total: ~29780 CPU cycles per frame
fn execute_frame(cpu: &mut Cpu, bus: &mut BenchBus) {
    const CYCLES_PER_FRAME: u32 = 29780;
    let mut cycles = 0u32;

    while cycles < CYCLES_PER_FRAME {
        // Execute one CPU instruction
        let opcode = bus.read(cpu.pc());
        let cpu_cycles = cpu.execute_instruction(bus, opcode) as u32;
        cycles += cpu_cycles;

        // Step PPU (3 PPU cycles per CPU cycle)
        bus.step_ppu(cpu_cycles);

        // Step APU (1 APU cycle per CPU cycle)
        bus.step_apu(cpu_cycles);

        // Handle NMI
        if bus.ppu.nmi_pending() {
            bus.ppu.clear_nmi();
        }
    }
}

/// Execute multiple CPU instructions
fn execute_instructions(cpu: &mut Cpu, bus: &mut BenchBus, count: usize) {
    for _ in 0..count {
        let opcode = bus.read(cpu.pc());
        cpu.execute_instruction(bus, opcode);
    }
}

// =============================================================================
// Benchmark Functions
// =============================================================================

/// Benchmark full frame execution
fn bench_frame_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_execution");

    group.throughput(Throughput::Elements(1));
    group.bench_function("full_frame", |b| {
        let mut bus = BenchBus::create_test_bus();
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        b.iter(|| {
            execute_frame(&mut cpu, &mut bus);
            black_box(bus.ppu.scanline());
        });
    });

    // Benchmark with nestest ROM if available
    if let Some(mut bus) = BenchBus::create_nestest_bus() {
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        group.bench_function("full_frame_nestest", |b| {
            b.iter(|| {
                execute_frame(&mut cpu, &mut bus);
                black_box(bus.ppu.scanline());
            });
        });
    }

    group.finish();
}

/// Benchmark CPU instruction execution
fn bench_cpu_instructions(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_instructions");

    for count in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::new("execute_instructions", count),
            count,
            |b, &count| {
                let mut bus = BenchBus::create_test_bus();
                let mut cpu = Cpu::new();
                cpu.reset(&mut bus);
                b.iter(|| {
                    execute_instructions(&mut cpu, &mut bus, count);
                    black_box(cpu.pc());
                });
            },
        );
    }

    group.finish();
}

/// Benchmark PPU rendering operations
fn bench_ppu_rendering(c: &mut Criterion) {
    let mut group = c.benchmark_group("ppu_rendering");

    // Benchmark PPU register access
    group.bench_function("ppu_register_read", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for addr in (0x2000..=0x2007).cycle().take(1000) {
                black_box(bus.ppu.read_register(addr));
            }
        });
    });

    group.bench_function("ppu_register_write", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for (i, addr) in (0x2000..=0x2007).cycle().take(1000).enumerate() {
                bus.ppu.write_register(addr, i as u8);
            }
            black_box(&bus.ppu);
        });
    });

    // Benchmark PPU VRAM access
    group.bench_function("ppu_vram_access", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for i in 0..1000u16 {
                bus.ppu.write_register(0x2006, (i >> 8) as u8);
                bus.ppu.write_register(0x2006, i as u8);
                bus.ppu.write_register(0x2007, i as u8);
            }
            black_box(&bus.ppu);
        });
    });

    group.finish();
}

/// Benchmark memory access operations
fn bench_memory_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_access");

    // CPU RAM access
    group.bench_function("cpu_ram_read_write", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for i in 0..1000u16 {
                bus.write(i & 0x07FF, i as u8);
                black_box(bus.read(i & 0x07FF));
            }
        });
    });

    // Mapper read/write
    group.bench_function("mapper_read_write", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for i in 0..100u16 {
                bus.mapper.write(0x8000 + i, i as u8);
                black_box(bus.mapper.read(0x8000 + i));
            }
        });
    });

    group.finish();
}

/// Benchmark mapper operations including bank switching
fn bench_mapper_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapper_access");

    // Test different mappers
    for mapper_num in [0, 1, 2, 4].iter() {
        let prg_rom = vec![0u8; 32768];
        let chr_rom = vec![0u8; 8192];

        if create_mapper(*mapper_num, prg_rom, chr_rom).is_ok() {
            group.bench_with_input(
                BenchmarkId::new("bank_switching", format!("mapper_{}", mapper_num)),
                mapper_num,
                |b, mapper_num| {
                    let prg_rom = vec![0u8; 32768];
                    let chr_rom = vec![0u8; 8192];
                    let mut mapper = create_mapper(*mapper_num, prg_rom, chr_rom).unwrap();
                    b.iter(|| {
                        // Simulate bank switching writes
                        for bank in 0..8u8 {
                            mapper.write(0x8000, bank);
                            mapper.write(0xA000, bank);
                            black_box(mapper.read(0x8000));
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

/// Benchmark APU operations
fn bench_apu_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("apu_operations");

    group.bench_function("apu_tick", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for _ in 0..1000 {
                bus.apu.tick();
            }
            black_box(&bus.apu);
        });
    });

    group.bench_function("apu_register_access", |b| {
        let mut bus = BenchBus::create_test_bus();
        b.iter(|| {
            for i in 0..100u16 {
                bus.apu.write_register(0x4000 + (i % 0x18), i as u8);
                black_box(bus.apu.read_register(0x4015));
            }
        });
    });

    group.finish();
}

/// FPS Calculation Benchmark
///
/// This benchmark measures the actual FPS achieved by running
/// multiple frames and calculating the throughput.
fn bench_fps_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fps_calculation");

    // Run 10 frames and measure time
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(10));

    group.bench_function("fps_10_frames", |b| {
        let mut bus = BenchBus::create_test_bus();
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        b.iter(|| {
            for _ in 0..10 {
                execute_frame(&mut cpu, &mut bus);
            }
            black_box(&bus);
        });
    });

    // Calculate throughput in frames per second
    group.throughput(Throughput::Elements(1));
    group.bench_function("single_frame_throughput", |b| {
        let mut bus = BenchBus::create_test_bus();
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        b.iter(|| {
            execute_frame(&mut cpu, &mut bus);
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    benches,
    bench_frame_execution,
    bench_cpu_instructions,
    bench_ppu_rendering,
    bench_memory_access,
    bench_mapper_access,
    bench_apu_operations,
    bench_fps_calculation
);

criterion_main!(benches);
