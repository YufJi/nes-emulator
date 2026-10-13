//! Mapper 4 - MMC3 (TxROM)
//!
//! Used by games like Super Mario Bros. 2, Super Mario Bros. 3, Kirby's Adventure.
//! Fine-grained bank switching: two 8 KB switchable PRG-ROM banks, two 2 KB + four
//! 1 KB CHR banks. Provides a scanline-counting IRQ for split-screen effects and
//! software-switchable H/V nametable mirroring.
//! See https://www.nesdev.org/wiki/MMC3

use crate::prelude::*;
use super::Mapper;

/// MMC3 mapper (Mapper 4)
#[derive(Debug)]
pub struct Mapper4 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,

    // Bank switching registers
    /// Command/bank select register ($8000)
    command: u8,
    /// PRG address select (bit 6 of $8000): 0=normal, 1=swapped
    prg_address_select: u8,
    /// CHR address select (bit 7 of $8000): 0=normal, 1=swapped
    chr_address_select: u8,

    // Bank registers
    /// R0-R7: Bank data registers
    bank_regs: [u8; 8],

    // IRQ registers
    /// IRQ counter
    irq_counter: u8,
    /// IRQ latch value (reload value)
    irq_latch: u8,
    /// IRQ enabled flag
    irq_enabled: bool,
    /// IRQ pending flag
    irq_pending: bool,

    // Mirroring
    /// 0=vertical, 1=horizontal
    mirroring: u8,

    // ROM sizes
    prg_rom_banks: usize,
    chr_rom_banks: usize,
}

impl Mapper4 {
    /// Create a new Mapper4 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        let prg_rom_banks = prg_rom.len() / 0x2000; // 8KB banks
        let chr_rom_banks = chr_rom.len() / 0x0400; // 1KB banks

        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0u8; 0x2000],
            chr_is_ram,
            command: 0,
            prg_address_select: 0,
            chr_address_select: 0,
            bank_regs: [0; 8],
            irq_counter: 0,
            irq_latch: 0,
            irq_enabled: false,
            irq_pending: false,
            mirroring: 0,
            prg_rom_banks: prg_rom_banks.max(2),
            chr_rom_banks: chr_rom_banks.max(8),
        }
    }

    /// Get PRG-ROM bank for 8KB region
    fn get_prg_bank(&self, region: u8) -> usize {
        // region: 0=$8000, 1=$A000, 2=$C000, 3=$E000
        let bank = match region {
            0 => {
                if self.prg_address_select == 0 {
                    // R6 at $8000
                    self.bank_regs[6] as usize
                } else {
                    // Second-to-last bank fixed at $8000
                    self.prg_rom_banks.saturating_sub(2)
                }
            }
            1 => {
                // R7 at $A000 (always switchable)
                self.bank_regs[7] as usize
            }
            2 => {
                if self.prg_address_select == 0 {
                    // Second-to-last bank fixed at $C000
                    self.prg_rom_banks.saturating_sub(2)
                } else {
                    // R6 at $C000
                    self.bank_regs[6] as usize
                }
            }
            3 => {
                // Last bank fixed at $E000
                self.prg_rom_banks.saturating_sub(1)
            }
            _ => 0,
        };
        bank % self.prg_rom_banks
    }

    /// Read from PRG-ROM at the given address
    fn read_prg_rom(&self, addr: u16) -> u8 {
        let region = ((addr - 0x8000) / 0x2000) as u8;
        let bank = self.get_prg_bank(region);
        let offset = (addr & 0x1FFF) as usize;
        let rom_addr = (bank * 0x2000 + offset) % self.prg_rom.len();
        self.prg_rom.get(rom_addr).copied().unwrap_or(0)
    }

    /// Get CHR-ROM bank for 1KB region
    fn get_chr_bank_1k(&self, slot: u8) -> usize {
        // slot 0-7 for 1KB regions $0000-$1FFF
        let reg = match slot {
            0 | 1 => {
                if self.chr_address_select == 0 {
                    // R0 for slots 0-1 (2KB bank)
                    (self.bank_regs[0] & 0xFE) as usize + (slot as usize & 1)
                } else {
                    // R2-R3 for slots 0-1 (1KB banks at $1000-$17FF in swapped mode)
                    self.bank_regs[(slot + 2) as usize] as usize
                }
            }
            2 | 3 => {
                if self.chr_address_select == 0 {
                    // R1 for slots 2-3 (2KB bank)
                    (self.bank_regs[1] & 0xFE) as usize + (slot as usize & 1)
                } else {
                    // R4-R5 for slots 2-3
                    self.bank_regs[(slot + 2) as usize] as usize
                }
            }
            4 | 5 | 6 | 7 => {
                if self.chr_address_select == 0 {
                    // R2-R5 for slots 4-7 (1KB banks)
                    self.bank_regs[(slot - 2) as usize] as usize
                } else {
                    // Swapped mode mirrors the two 2KB banks into the upper
                    // half: R0 covers $1000-$17FF and R1 covers $1800-$1FFF.
                    if slot < 6 {
                        (self.bank_regs[0] & 0xFE) as usize + (slot as usize & 1)
                    } else {
                        (self.bank_regs[1] & 0xFE) as usize + (slot as usize & 1)
                    }
                }
            }
            _ => 0,
        };
        reg % self.chr_rom_banks
    }

    /// Physical CHR address the 1KB slot holding `addr` maps to.
    fn chr_addr(&self, addr: u16) -> usize {
        let slot = (addr / 0x0400) as u8;
        let bank = self.get_chr_bank_1k(slot);
        let offset = (addr & 0x03FF) as usize;
        (bank * 0x0400 + offset) % self.chr_rom.len()
    }

    /// Read from CHR-ROM/RAM at the given address
    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_rom.get(self.chr_addr(addr)).copied().unwrap_or(0)
    }

    /// Write to CHR-RAM (only when the cartridge ships without CHR-ROM).
    fn write_chr(&mut self, addr: u16, value: u8) {
        if !self.chr_is_ram {
            return;
        }
        let chr_addr = self.chr_addr(addr);
        if let Some(byte) = self.chr_rom.get_mut(chr_addr) {
            *byte = value;
        }
    }

    /// Execute a bank select command
    fn execute_command(&mut self, cmd: u8, value: u8) {
        let cmd_idx = cmd as usize;
        match cmd {
            0 | 1 => {
                // Select 2KB CHR bank at $0000 (cmd 0) or $0800 (cmd 1)
                // Or at $1000/$1800 if chr_address_select is set
                self.bank_regs[cmd_idx] = value;
            }
            2 | 3 | 4 | 5 => {
                // Select 1KB CHR bank
                self.bank_regs[cmd_idx] = value;
            }
            6 => {
                // Select 8KB PRG bank at $8000 or $C000 depending on prg_address_select
                self.bank_regs[6] = value & 0x3F; // Only 6 bits for PRG
            }
            7 => {
                // Select 8KB PRG bank at $A000
                self.bank_regs[7] = value & 0x3F;
            }
            _ => {}
        }
    }

    /// Clock the IRQ counter (called once per scanline)
    pub fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
            if self.irq_counter == 0 && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }
}

impl Mapper for Mapper4 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, command,
        prg_address_select, chr_address_select, bank_regs, irq_counter, irq_latch,
        irq_enabled, irq_pending, mirroring, prg_rom_banks, chr_rom_banks);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index]
            }
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg_rom(addr),
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // $0000-$1FFF: CHR-RAM uploads land on the cartridge image; on
            // CHR-ROM boards the mapper drops them.
            0x0000..=0x1FFF => self.write_chr(addr, value),
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // MMC3 registers: $8000-$FFFF
            0x8000..=0xFFFF => {
                let reg = addr & 0xE001;
                match reg {
                    0x8000 => {
                        // Bank select
                        self.command = value & 0x07;
                        self.prg_address_select = (value >> 6) & 0x01;
                        self.chr_address_select = (value >> 7) & 0x01;
                    }
                    0x8001 => {
                        // Bank data
                        self.execute_command(self.command, value);
                    }
                    0xA000 => {
                        // Mirroring
                        self.mirroring = value & 0x01;
                    }
                    0xA001 => {
                        // PRG RAM protect (not implemented)
                    }
                    0xC000 => {
                        // IRQ latch
                        self.irq_latch = value;
                    }
                    0xC001 => {
                        // IRQ reload: the counter is reloaded from the latch
                        // on the next clock.
                        self.irq_counter = 0;
                    }
                    0xE000 => {
                        // IRQ disable
                        self.irq_enabled = false;
                        self.irq_pending = false;
                    }
                    0xE001 => {
                        // IRQ enable
                        self.irq_enabled = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// Clock the scanline counter once per rendered scanline.
    ///
    /// The hardware toggles A12 eight times per scanline; games only care about
    /// the resulting count, so clocking once per visible scanline is enough.
    /// Nothing is clocked while rendering is disabled, which is how games keep
    /// the counter under control.
    fn on_scanline(&mut self, scanline: u16, rendering: bool) {
        if rendering && scanline < 240 {
            self.clock_irq_counter();
        }
    }

    /// Battery-backed work RAM ($6000-$7FFF).
    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    /// Restore battery-backed work RAM.
    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }

    /// MMC3 mirroring register: 0 = vertical, 1 = horizontal.
    fn mirroring(&self) -> Option<crate::ppu::rendering::Mirroring> {
        Some(if self.mirroring & 0x01 == 0 {
            crate::ppu::rendering::Mirroring::Vertical
        } else {
            crate::ppu::rendering::Mirroring::Horizontal
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mapper() -> Mapper4 {
        let prg = vec![0u8; 0x80000]; // 512KB (64 x 8KB banks)
        let chr = vec![0u8; 0x20000]; // 128KB (32 x 4KB banks, 64 x 2KB banks, 128 x 1KB banks)
        Mapper4::new(prg, chr)
    }

    #[test]
    fn test_mapper4_new() {
        let mapper = create_test_mapper();
        assert_eq!(mapper.prg_rom_banks, 64);
        assert_eq!(mapper.chr_rom_banks, 128); // 128KB / 1KB = 128 banks
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending);
    }

    #[test]
    fn test_prg_ram_read_write() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Test PRG RAM read/write at $6000-$7FFF
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7000, 0xAB);
        assert_eq!(mapper.read(0x7000), 0xAB);

        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x7FFF), 0xEF);
    }

    #[test]
    fn test_bank_select_register() {
        let mut mapper = create_test_mapper();

        // Write to bank select ($8000)
        // command = 6 (PRG bank), prg_address_select = 1, chr_address_select = 1
        mapper.write(0x8000, 0xC6);
        assert_eq!(mapper.command, 6);
        assert_eq!(mapper.prg_address_select, 1);
        assert_eq!(mapper.chr_address_select, 1);
    }

    #[test]
    fn test_prg_bank_switching_normal_mode() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (16 x 8KB banks)
        for i in 0..16 {
            prg[i * 0x2000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Normal mode: prg_address_select = 0
        // $8000-$9FFF: R6 (switchable)
        // $A000-$BFFF: R7 (switchable)
        // $C000-$DFFF: Second-to-last bank (fixed)
        // $E000-$FFFF: Last bank (fixed)

        // Set R6 to bank 3
        mapper.write(0x8000, 0x06); // Select command 6
        mapper.write(0x8001, 3);    // Set bank to 3
        assert_eq!(mapper.bank_regs[6], 3);

        // Set R7 to bank 5
        mapper.write(0x8000, 0x07); // Select command 7
        mapper.write(0x8001, 5);    // Set bank to 5
        assert_eq!(mapper.bank_regs[7], 5);

        // Read from different regions
        assert_eq!(mapper.read(0x8000), 3); // R6
        assert_eq!(mapper.read(0xA000), 5); // R7
        assert_eq!(mapper.read(0xC000), 14); // Second-to-last (bank 14)
        assert_eq!(mapper.read(0xE000), 15); // Last (bank 15)
    }

    #[test]
    fn test_prg_bank_switching_swapped_mode() {
        let mut prg = vec![0u8; 0x20000];
        for i in 0..16 {
            prg[i * 0x2000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Swapped mode: prg_address_select = 1
        // $8000-$9FFF: Second-to-last bank (fixed)
        // $A000-$BFFF: R7 (switchable)
        // $C000-$DFFF: R6 (switchable)
        // $E000-$FFFF: Last bank (fixed)

        // Set prg_address_select = 1
        mapper.write(0x8000, 0x40); // command = 0, prg_address_select = 1

        // Set R6 to bank 3
        mapper.write(0x8000, 0x46); // command = 6, prg_address_select = 1
        mapper.write(0x8001, 3);

        // Set R7 to bank 5
        mapper.write(0x8000, 0x47); // command = 7, prg_address_select = 1
        mapper.write(0x8001, 5);

        // Read from different regions
        assert_eq!(mapper.read(0x8000), 14); // Second-to-last (fixed)
        assert_eq!(mapper.read(0xA000), 5);  // R7
        assert_eq!(mapper.read(0xC000), 3);  // R6
        assert_eq!(mapper.read(0xE000), 15); // Last (fixed)
    }

    #[test]
    fn test_chr_bank_switching() {
        let prg = vec![0u8; 0x20000];
        let mut chr = vec![0u8; 0x10000]; // 64KB (16 x 4KB banks)
        // Mark 1KB banks
        for i in 0..64 {
            chr[i * 0x0400] = i as u8;
        }
        let mut mapper = Mapper4::new(prg, chr);

        // Normal mode: chr_address_select = 0
        // R0: 2KB bank at $0000
        // R1: 2KB bank at $0800
        // R2-R5: 1KB banks at $1000-$1FFF

        // Set R0 to bank 2 (2KB bank, so uses banks 2 and 3)
        mapper.write(0x8000, 0x00); // command = 0
        mapper.write(0x8001, 2);    // Set to bank 2 (even)

        // Set R2 to bank 10 (1KB bank at $1000)
        mapper.write(0x8000, 0x02); // command = 2
        mapper.write(0x8001, 10);

        // Read CHR
        assert_eq!(mapper.read(0x0000), 2);  // R0 first 1KB
        assert_eq!(mapper.read(0x0400), 3);  // R0 second 1KB
        assert_eq!(mapper.read(0x1000), 10); // R2 at $1000
    }

    #[test]
    fn test_chr_bank_switching_swapped_mode_maps_both_2kb_banks() {
        let mut chr = vec![0u8; 0x40000];
        for bank in 0..256 {
            for byte in &mut chr[bank * 0x0400..(bank + 1) * 0x0400] {
                *byte = bank as u8;
            }
        }
        let mut mapper = Mapper4::new(vec![0u8; 0x80000], chr);

        // CHR mode 1 puts the two 2KB banks at $1000-$17FF (R0) and
        // $1800-$1FFF (R1). Slots 6-7 must follow R1, not R2/R3.
        mapper.write(0x8000, 0x00);
        mapper.write(0x8001, 12); // R0
        mapper.write(0x8000, 0x01);
        mapper.write(0x8001, 20); // R1
        mapper.write(0x8000, 0x80); // CHR mode 1, command 0

        assert_eq!(mapper.read(0x1000), 12);
        assert_eq!(mapper.read(0x1400), 13);
        assert_eq!(mapper.read(0x1800), 20);
        assert_eq!(mapper.read(0x1C00), 21);
    }

    #[test]
    fn test_irq_counter() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // $C000 sets the latch, $C001 requests a reload.
        mapper.write(0xC000, 3);
        assert_eq!(mapper.irq_latch, 3);
        mapper.write(0xC001, 0);
        assert_eq!(mapper.irq_counter, 0, "reload clears the counter");

        // Enable IRQ
        mapper.write(0xE001, 0);
        assert!(mapper.irq_enabled);

        // Clock the counter down to 0
        mapper.clock_irq_counter(); // reload to 3
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 3 -> 2
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 1 -> 0, triggers IRQ
        assert!(mapper.irq_pending());
    }

    #[test]
    fn test_irq_trigger() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Set IRQ latch to 2
        mapper.write(0xC000, 2);

        // Enable IRQ
        mapper.write(0xE001, 0);

        // Counter starts at 0, first clock reloads to 2
        mapper.clock_irq_counter();
        assert!(!mapper.irq_pending());

        // 2 -> 1
        mapper.clock_irq_counter();
        assert!(!mapper.irq_pending());

        // 1 -> 0, triggers IRQ
        mapper.clock_irq_counter();
        assert!(mapper.irq_pending());
    }

    #[test]
    fn test_irq_disable() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Set up and trigger IRQ
        mapper.write(0xC000, 1);
        mapper.write(0xE001, 0);
        mapper.clock_irq_counter(); // reload to 1
        mapper.clock_irq_counter(); // 1 -> 0, triggers
        assert!(mapper.irq_pending());

        // Disable IRQ
        mapper.write(0xE000, 0);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending()); // Writing to $E000 clears pending IRQ
    }

    #[test]
    fn test_mirroring_select() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper4::new(prg, chr);

        // Default mirroring
        assert_eq!(mapper.mirroring, 0);

        // Set to horizontal mirroring
        mapper.write(0xA000, 1);
        assert_eq!(mapper.mirroring, 1);

        // Set to vertical mirroring
        mapper.write(0xA000, 0);
        assert_eq!(mapper.mirroring, 0);
    }

    #[test]
    fn test_chr_is_ram() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![]; // Empty CHR triggers CHR-RAM mode
        let mapper = Mapper4::new(prg, chr);
        assert!(mapper.chr_is_ram);
        assert_eq!(mapper.chr_rom.len(), 8192);
    }

    #[test]
    fn test_chr_ram_write_is_visible_to_reads() {
        let prg = vec![0u8; 0x8000];
        let mut mapper = Mapper4::new(prg, Vec::new()); // Empty CHR -> CHR-RAM
        assert!(mapper.chr_is_ram);

        // Whatever bank the 1KB slot selects, a byte written through the
        // mapper must read back at the address the PPU renders from.
        for addr in [0x0000u16, 0x03FF, 0x0800, 0x1000, 0x1FFF] {
            mapper.write(addr, 0x5A);
            assert_eq!(mapper.read(addr), 0x5A, "CHR-RAM write lost at {addr:#06X}");
        }
    }

    #[test]
    fn test_chr_rom_write_is_ignored() {
        let prg = vec![0u8; 0x8000];
        let chr = vec![0x11u8; 0x2000];
        let mut mapper = Mapper4::new(prg, chr);
        assert!(!mapper.chr_is_ram);

        mapper.write(0x0000, 0x5A);
        assert_eq!(mapper.read(0x0000), 0x11);
    }

    #[test]
    fn test_save_state_keeps_chr_ram() {
        use crate::state::{StateReader, StateWriter};

        let prg = vec![0u8; 0x8000];
        let mut mapper = Mapper4::new(prg, Vec::new()); // Empty CHR -> CHR-RAM
        mapper.write(0x0000, 0x5A);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        mapper.write(0x0000, 0x00);
        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);
        assert_eq!(mapper.read(0x0000), 0x5A, "CHR-RAM was not restored");
    }

    #[test]
    fn test_on_scanline_clocks_only_while_rendering() {
        let mut mapper = Mapper4::new(vec![0u8; 0x20000], vec![0u8; 8192]);

        mapper.write(0xC000, 2); // latch
        mapper.write(0xC001, 0); // reload
        mapper.write(0xE001, 0); // enable

        // Rendering disabled: the counter must not move.
        mapper.on_scanline(10, false);
        mapper.on_scanline(10, false);
        assert!(!mapper.irq_pending());

        // VBlank scanlines are not clocked either.
        mapper.on_scanline(250, true);
        assert!(!mapper.irq_pending());

        // Visible scanlines advance the counter: the first clock reloads from
        // the latch (2), then it counts 2 -> 1 -> 0 and raises the IRQ.
        mapper.on_scanline(10, true); // reload to 2
        mapper.on_scanline(11, true); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.on_scanline(12, true); // 1 -> 0 -> IRQ
        assert!(mapper.irq_pending());
    }
}
