//! Mapper 74 - Waixing MMC3 with CHR ROM *and* CHR RAM (43-393 / 43-406 / 860908C)
//!
//! Waixing built this board for its Chinese localizations — Metal Max (机甲战士,
//! the "重装机兵" cartridges), 甲A - China Soccer League, Robot War IV, 風雲 —
//! which need runtime-generated tiles: Chinese dialogue cannot be pre-rendered
//! per sentence the way a CHR-ROM-only game would.
//!
//! An MMC3 clone provides the usual two switchable 8 KB PRG banks, two 2 KB plus
//! four 1 KB CHR banks, the scanline IRQ and H/V mirroring. On top of that, a
//! 74LS138/74LS139 watches the 1 KB CHR bank number and redirects banks #8 and #9
//! to 2 KB of CHR RAM; every other bank still comes from CHR ROM. Because the
//! redirect looks at the bank *number*, CHR-ROM banks 8 and 9 are unreachable and
//! the RAM is not mirrored: bank 8 is the RAM's first 1 KB, bank 9 its second.
//!
//! Mirroring is plain MMC3 (bit 0 of $A000 selects vertical or horizontal). The
//! NESdev wiki notes that Nestopia reads writes of 2/3 there as 1-screen A/B
//! mirroring for Waixing's MMC3 clones; FCEUX's mapper 74 board keeps the standard
//! MMC3 behavior, which is what this implementation follows.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_074

use super::Mapper;
use crate::prelude::*;

/// First 1 KB CHR bank that lives in CHR RAM instead of CHR ROM.
const CHR_RAM_FIRST_BANK: usize = 8;
/// How many 1 KB banks of CHR RAM the board carries.
const CHR_RAM_BANKS: usize = 2;

/// Mapper 74 (Waixing MMC3 with CHR ROM + CHR RAM)
#[derive(Debug)]
pub struct Mapper74 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 2KB CHR RAM, reached through CHR bank numbers 8 and 9
    chr_ram: [u8; CHR_RAM_BANKS * 0x0400],
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],

    // Bank switching registers
    /// Command/bank select register ($8000)
    command: u8,
    /// PRG address select (bit 6 of $8000): 0=normal, 1=swapped
    prg_address_select: u8,
    /// CHR address select (bit 7 of $8000): 0=normal, 1=swapped
    chr_address_select: u8,

    /// R0-R7: bank data registers
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

    /// 0=vertical, 1=horizontal
    mirroring: u8,

    /// Number of 8KB PRG-ROM banks
    prg_rom_banks: usize,
}

impl Mapper74 {
    /// Create a new Mapper74 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x2000; // 8KB banks

        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0u8; CHR_RAM_BANKS * 0x0400],
            prg_ram: [0u8; 0x2000],
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
        }
    }

    /// Offset into the CHR RAM a 1 KB CHR bank number lands on, if it is one of
    /// the banks the board redirects.
    fn chr_ram_offset(bank: usize) -> Option<usize> {
        let ram_bank = bank.checked_sub(CHR_RAM_FIRST_BANK)?;
        (ram_bank < CHR_RAM_BANKS).then_some(ram_bank * 0x0400)
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

    /// 1 KB CHR bank number the bank registers select for `slot` (0-7).
    ///
    /// The raw number is returned, not a bank index into CHR ROM: the board
    /// decides between ROM and RAM from the number itself, so banks 8 and 9 must
    /// survive whatever size of CHR ROM the cartridge happens to carry.
    fn get_chr_bank_1k(&self, slot: u8) -> usize {
        match slot {
            0 | 1 => {
                if self.chr_address_select == 0 {
                    // R0 for slots 0-1 (2KB bank)
                    (self.bank_regs[0] & 0xFE) as usize + (slot as usize & 1)
                } else {
                    // R2-R3 for slots 0-1 (1KB banks in swapped mode)
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
            4..=7 => {
                if self.chr_address_select == 0 {
                    // R2-R5 for slots 4-7 (1KB banks)
                    self.bank_regs[(slot - 2) as usize] as usize
                } else if slot < 6 {
                    // R0 for slots 4-5 (2KB bank)
                    (self.bank_regs[0] & 0xFE) as usize + (slot as usize & 1)
                } else {
                    // R1 for slots 6-7 (2KB bank): the pair is the odd half of
                    // the mirrored layout, not R2/R3.
                    (self.bank_regs[1] & 0xFE) as usize + (slot as usize & 1)
                }
            }
            _ => 0,
        }
    }

    /// Read the byte the PPU sees at a pattern-table address.
    fn read_chr(&self, addr: u16) -> u8 {
        let bank = self.get_chr_bank_1k((addr / 0x0400) as u8);
        let offset = (addr & 0x03FF) as usize;

        if let Some(ram_offset) = Self::chr_ram_offset(bank) {
            self.chr_ram[ram_offset + offset]
        } else if self.chr_rom.is_empty() {
            0
        } else {
            let rom_addr = (bank * 0x0400 + offset) % self.chr_rom.len();
            self.chr_rom[rom_addr]
        }
    }

    /// Write to CHR-RAM; only the two redirected banks are writable, and writes
    /// aimed at CHR-ROM banks are dropped.
    fn write_chr(&mut self, addr: u16, value: u8) {
        let bank = self.get_chr_bank_1k((addr / 0x0400) as u8);
        let offset = (addr & 0x03FF) as usize;

        if let Some(ram_offset) = Self::chr_ram_offset(bank) {
            self.chr_ram[ram_offset + offset] = value;
        }
    }

    /// Execute a bank select command
    fn execute_command(&mut self, cmd: u8, value: u8) {
        match cmd {
            // 2KB CHR bank at $0000 (0) or $0800 (1), $1000/$1800 when swapped
            0..=5 => self.bank_regs[cmd as usize] = value,
            // 8KB PRG bank, 6 bits wide
            6 => self.bank_regs[6] = value & 0x3F,
            7 => self.bank_regs[7] = value & 0x3F,
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

impl Mapper for Mapper74 {
    crate::state::mapper_state!(
        chr_ram,
        prg_ram,
        command,
        prg_address_select,
        chr_address_select,
        bank_regs,
        irq_counter,
        irq_latch,
        irq_enabled,
        irq_pending,
        mirroring,
        prg_rom_banks
    );

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg_rom(addr),
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // $0000-$1FFF: CHR-RAM uploads land on the two redirected banks.
            0x0000..=0x1FFF => self.write_chr(addr, value),
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            // MMC3 registers: $8000-$FFFF
            0x8000..=0xFFFF => match addr & 0xE001 {
                0x8000 => {
                    self.command = value & 0x07;
                    self.prg_address_select = (value >> 6) & 0x01;
                    self.chr_address_select = (value >> 7) & 0x01;
                }
                0x8001 => self.execute_command(self.command, value),
                0xA000 => self.mirroring = value & 0x01,
                0xA001 => {
                    // PRG RAM protect (not implemented)
                }
                0xC000 => self.irq_latch = value,
                0xC001 => {
                    // IRQ reload: the counter is reloaded from the latch on the
                    // next clock.
                    self.irq_counter = 0;
                }
                0xE000 => {
                    // IRQ disable, and the pending flag goes with it
                    self.irq_enabled = false;
                    self.irq_pending = false;
                }
                0xE001 => self.irq_enabled = true,
                _ => {}
            },
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// Clock the scanline counter once per rendered scanline, like MMC3.
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

    /// 512KB PRG (64 x 8KB banks) and 256KB CHR, the sizes the board ships with.
    fn create_test_mapper() -> Mapper74 {
        Mapper74::new(vec![0u8; 0x80000], vec![0u8; 0x40000])
    }

    /// Point one 1 KB CHR slot at `bank` (normal mode uses slots 4-7).
    fn select_chr_bank(mapper: &mut Mapper74, slot: u8, bank: u8) {
        assert!((4..=7).contains(&slot), "pick a 1KB slot");
        mapper.write(0x8000, slot - 2);
        mapper.write(0x8001, bank);
    }

    #[test]
    fn test_mapper74_new() {
        let mapper = create_test_mapper();
        assert_eq!(mapper.prg_rom_banks, 64);
        assert_eq!(mapper.chr_ram.len(), 2048);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_prg_bank_switching_normal_mode() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (16 x 8KB banks)
        for bank in 0..16 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut mapper = Mapper74::new(prg, vec![0u8; 0x40000]);

        // $8000/$C000 are swappable, $A000 is R7, $E000 is the last bank.
        mapper.write(0x8000, 0x06);
        mapper.write(0x8001, 3);
        mapper.write(0x8000, 0x07);
        mapper.write(0x8001, 5);

        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xA000), 5);
        assert_eq!(mapper.read(0xC000), 14);
        assert_eq!(mapper.read(0xE000), 15);

        // Swapped mode: R6 moves down to $C000 and the fixed bank to $8000.
        mapper.write(0x8000, 0x46);
        mapper.write(0x8001, 3);
        assert_eq!(mapper.read(0x8000), 14);
        assert_eq!(mapper.read(0xC000), 3);
    }

    #[test]
    fn test_chr_banks_read_from_rom() {
        let mut chr = vec![0u8; 0x40000];
        for bank in 0..256 {
            for byte in &mut chr[bank * 0x0400..(bank + 1) * 0x0400] {
                *byte = bank as u8;
            }
        }
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        // Power-on registers are zero, so every slot starts on bank 0.
        assert_eq!(mapper.read(0x0000), 0);
        assert_eq!(mapper.read(0x1000), 0);

        // Slot 4 ($1000-$13FF) follows R2.
        select_chr_bank(&mut mapper, 4, 2);
        assert_eq!(mapper.read(0x1000), 2);
        select_chr_bank(&mut mapper, 4, 20);
        assert_eq!(mapper.read(0x1000), 20);
        assert_eq!(mapper.read(0x13FF), 20);
    }

    #[test]
    fn test_chr_ram_banks_8_and_9_are_writable() {
        let mut chr = vec![0xFFu8; 0x40000];
        chr[8 * 0x0400] = 0x11; // the ROM bytes the RAM shadows
        chr[9 * 0x0400] = 0x22;
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        select_chr_bank(&mut mapper, 4, 8);
        assert_eq!(mapper.read(0x1000), 0x00, "bank 8 reads CHR RAM, not ROM");
        mapper.write(0x1000, 0x5A);
        assert_eq!(mapper.read(0x1000), 0x5A);

        // Bank 9 is the second half of the same 2KB RAM.
        select_chr_bank(&mut mapper, 4, 9);
        assert_eq!(mapper.read(0x1000), 0x00);
        mapper.write(0x1000, 0xA5);
        assert_eq!(mapper.read(0x1000), 0xA5);
        assert_eq!(
            (mapper.chr_ram[0], mapper.chr_ram[0x400]),
            (0x5A, 0xA5),
            "banks 8 and 9 must hit separate halves: the RAM is not mirrored"
        );
    }

    #[test]
    fn test_chr_rom_banks_8_and_9_are_shadowed_by_ram() {
        let mut chr = vec![0u8; 0x40000];
        chr[8 * 0x0400] = 0x11;
        chr[9 * 0x0400] = 0x22;
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        // The redirect happens on the bank number, so the ROM that lives there
        // is unreachable no matter what the registers select.
        select_chr_bank(&mut mapper, 4, 8);
        assert_ne!(mapper.read(0x1000), 0x11);
        select_chr_bank(&mut mapper, 4, 9);
        assert_ne!(mapper.read(0x1000), 0x22);
    }

    #[test]
    fn test_chr_ram_banks_work_with_short_chr_rom() {
        // 8KB of CHR: a naive `bank % banks` would fold bank 8 onto bank 0 and
        // silently turn the RAM into ROM. The board does not care about the ROM
        // size, so neither does the redirect.
        let mut chr = vec![0u8; 0x2000];
        chr[0] = 0x77;
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        select_chr_bank(&mut mapper, 4, 8);
        assert_ne!(mapper.read(0x1000), 0x77);
        mapper.write(0x1000, 0x5A);
        assert_eq!(mapper.read(0x1000), 0x5A);
    }

    #[test]
    fn test_two_kb_chr_register_maps_both_halves_to_ram() {
        let mut mapper = create_test_mapper();

        // R0 selects a 2KB bank at $0000: writing 8 covers banks 8 and 9, i.e.
        // both halves of the CHR RAM.
        mapper.write(0x8000, 0x00);
        mapper.write(0x8001, 8);
        mapper.write(0x0000, 0x12);
        mapper.write(0x0400, 0x34);
        assert_eq!(mapper.read(0x0000), 0x12);
        assert_eq!(mapper.read(0x0400), 0x34);

        // The odd low bit is ignored, exactly like MMC3's 2KB banks.
        mapper.write(0x8001, 9);
        assert_eq!(mapper.read(0x0000), 0x12);
        assert_eq!(mapper.read(0x0400), 0x34);
    }

    #[test]
    fn test_swapped_mode_maps_both_2kb_chr_banks() {
        let mut chr = vec![0u8; 0x40000];
        for bank in 0..256 {
            for byte in &mut chr[bank * 0x0400..(bank + 1) * 0x0400] {
                *byte = bank as u8;
            }
        }
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        // CHR mode 1 (bit 7) with R0 = 12 and R1 = 20: the two 2KB banks move to
        // $1000-$17FF and $1800-$1FFF respectively. Reading R2/R3 there instead
        // would show banks 2/3 and break sprite patterns, which is what Metal Max
        // exposed.
        mapper.write(0x8001, 12); // R0 (register 0 is selected after reset)
        mapper.write(0x8000, 0x80 | 0x01);
        mapper.write(0x8001, 20); // R1
        mapper.write(0x8000, 0x80); // back to CHR mode 1, command 0

        assert_eq!(mapper.read(0x1000), 12);
        assert_eq!(mapper.read(0x1400), 13);
        assert_eq!(mapper.read(0x1800), 20);
        assert_eq!(mapper.read(0x1C00), 21);
    }

    #[test]
    fn test_chr_ram_reachable_through_swapped_mode() {
        let mut mapper = create_test_mapper();

        // CHR mode 1 (bit 7) swaps the 1KB registers into $0000-$0FFF.
        mapper.write(0x8000, 0x80 | 0x02);
        mapper.write(0x8001, 8);
        mapper.write(0x0000, 0x5A);
        assert_eq!(mapper.read(0x0000), 0x5A);
    }

    #[test]
    fn test_chr_rom_writes_are_ignored() {
        let mut chr = vec![0x11u8; 0x40000];
        chr[20 * 0x0400] = 0x33;
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        select_chr_bank(&mut mapper, 4, 20);
        mapper.write(0x1000, 0x5A);
        assert_eq!(mapper.read(0x1000), 0x33);
        assert_eq!(mapper.chr_ram[0], 0x00, "the write leaked into CHR RAM");
    }

    #[test]
    fn test_out_of_range_chr_bank_wraps_into_rom() {
        let mut chr = vec![0u8; 0x40000]; // 256 x 1KB banks
        for bank in 0..256 {
            chr[bank * 0x0400] = bank as u8;
        }
        let mut mapper = Mapper74::new(vec![0u8; 0x80000], chr);

        // Only banks 8 and 9 are RAM; the rest keep wrapping into the ROM image
        // instead of aliasing the RAM.
        select_chr_bank(&mut mapper, 4, 10);
        assert_eq!(mapper.read(0x1000), 10);
        select_chr_bank(&mut mapper, 4, 255);
        assert_eq!(mapper.read(0x1000), 255);
        select_chr_bank(&mut mapper, 4, 0);
        assert_eq!(mapper.read(0x1000), 0);
    }

    #[test]
    fn test_prg_ram_and_battery_backup() {
        let mut mapper = create_test_mapper();

        mapper.write(0x6000, 0x42);
        mapper.write(0x7FFF, 0xEF);
        assert_eq!(mapper.read(0x6000), 0x42);
        assert_eq!(mapper.read(0x7FFF), 0xEF);

        let saved = mapper
            .save_ram()
            .expect("MMC3 boards carry work RAM")
            .to_vec();
        assert_eq!(saved[0], 0x42);

        mapper.write(0x6000, 0x00);
        mapper.load_save_ram(&saved);
        assert_eq!(mapper.read(0x6000), 0x42);
    }

    #[test]
    fn test_irq_counter() {
        let mut mapper = create_test_mapper();

        mapper.write(0xC000, 3);
        assert_eq!(mapper.irq_latch, 3);
        mapper.write(0xC001, 0);
        assert_eq!(mapper.irq_counter, 0, "reload clears the counter");

        mapper.write(0xE001, 0);
        assert!(mapper.irq_enabled);

        mapper.clock_irq_counter(); // reload to 3
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 3 -> 2
        mapper.clock_irq_counter(); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 1 -> 0, triggers
        assert!(mapper.irq_pending());

        // Disabling clears the pending flag.
        mapper.write(0xE000, 0);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_on_scanline_clocks_only_while_rendering() {
        let mut mapper = create_test_mapper();

        mapper.write(0xC000, 2);
        mapper.write(0xC001, 0);
        mapper.write(0xE001, 0);

        mapper.on_scanline(10, false);
        mapper.on_scanline(10, false);
        mapper.on_scanline(250, true); // VBlank is not clocked either
        assert!(!mapper.irq_pending());

        mapper.on_scanline(10, true); // reload to 2
        mapper.on_scanline(11, true); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.on_scanline(12, true); // 1 -> 0 -> IRQ
        assert!(mapper.irq_pending());
    }

    #[test]
    fn test_mirroring_select() {
        let mut mapper = create_test_mapper();

        let vertical = Some(crate::ppu::rendering::Mirroring::Vertical);
        let horizontal = Some(crate::ppu::rendering::Mirroring::Horizontal);

        assert_eq!(mapper.mirroring(), vertical);
        mapper.write(0xA000, 1);
        assert_eq!(mapper.mirroring(), horizontal);
        mapper.write(0xA000, 0);
        assert_eq!(mapper.mirroring(), vertical);

        // The register only has one bit, like MMC3.
        mapper.write(0xA000, 0xFE);
        assert_eq!(mapper.mirroring(), vertical);
    }

    #[test]
    fn test_save_state_keeps_chr_ram() {
        use crate::state::{StateReader, StateWriter};

        let mut mapper = create_test_mapper();
        select_chr_bank(&mut mapper, 4, 8);
        mapper.write(0x1000, 0x5A);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        mapper.write(0x1000, 0x00);
        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);
        assert_eq!(mapper.read(0x1000), 0x5A, "CHR-RAM was not restored");
    }
}
