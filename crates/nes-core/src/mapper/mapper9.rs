//! Mapper 9 - MMC2 (PNROM / PEEROM)
//!
//! Used exclusively by Mike Tyson's Punch-Out!! (and Punch-Out!!).
//! Features tile-triggered CHR bank switching with two independent 4KB CHR latches
//! that automatically swap between two banks when the PPU fetches specific tiles ($FD/$FE).
//!
//! PRG-ROM layout:
//! - $8000-$9FFF: 8KB switchable bank
//! - $A000-$FFFF: Three 8KB fixed banks (last three of the ROM)
//!
//! CHR-ROM layout:
//! - $0000-$0FFF: 4KB bank controlled by latch0 ($FD or $FE)
//! - $1000-$1FFF: 4KB bank controlled by latch1 ($FD or $FE)
//!
//! Registers:
//! - $A000-$AFFF: PRG bank select (8KB bank at $8000)
//! - $B000-$BFFF: CHR bank for $0000 when latch0 = $FD
//! - $C000-$CFFF: CHR bank for $0000 when latch0 = $FE
//! - $D000-$DFFF: CHR bank for $1000 when latch1 = $FD
//! - $E000-$EFFF: CHR bank for $1000 when latch1 = $FE
//! - $F000-$FFFF: Mirroring (bit 0: 0=vertical, 1=horizontal)
//!
//! Latch behavior:
//! - Latch 0 triggers on addresses $0FD8 (sets to $FD) and $0FE8 (sets to $FE)
//! - Latch 1 triggers on addresses $1FD8-$1FDF (sets to $FD) and $1FE8-$1FEF (sets to $FE)
//! - The latch switches AFTER the data has been read
//!
//! See https://www.nesdev.org/wiki/MMC2

use super::Mapper;
use crate::prelude::*;

/// MMC2 mapper (Mapper 9)
#[derive(Debug)]
pub struct Mapper9 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8KB PRG-RAM at $6000-$7FFF (battery-backed)
    prg_ram: [u8; 0x2000],
    /// Current 8KB PRG bank at $8000 (0-15)
    prg_bank: u8,
    /// CHR bank for $0000 when latch0 = $FD
    chr_bank_fd0: u8,
    /// CHR bank for $0000 when latch0 = $FE
    chr_bank_fe0: u8,
    /// CHR bank for $1000 when latch1 = $FD
    chr_bank_fd1: u8,
    /// CHR bank for $1000 when latch1 = $FE
    chr_bank_fe1: u8,
    /// Latch 0 state ($FD or $FE)
    latch0: u8,
    /// Latch 1 state ($FD or $FE)
    latch1: u8,
    /// Mirroring: false = vertical, true = horizontal
    mirroring: bool,
    irq_pending: bool,
}

impl Mapper9 {
    /// Create a new Mapper9 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0u8; 0x2000],
            prg_bank: 0,
            chr_bank_fd0: 0,
            chr_bank_fe0: 0,
            chr_bank_fd1: 0,
            chr_bank_fe1: 0,
            latch0: 0xFE,
            latch1: 0xFE,
            mirroring: false, // Default to vertical
            irq_pending: false,
        }
    }

    /// Get PRG-ROM size in 8KB banks
    fn prg_rom_banks(&self) -> usize {
        self.prg_rom.len() / 0x2000
    }

    /// Get the current CHR bank index for $0000 based on latch0 state
    fn current_chr_bank_0(&self) -> u8 {
        if self.latch0 == 0xFD {
            self.chr_bank_fd0
        } else {
            self.chr_bank_fe0
        }
    }

    /// Get the current CHR bank index for $1000 based on latch1 state
    fn current_chr_bank_1(&self) -> u8 {
        if self.latch1 == 0xFD {
            self.chr_bank_fd1
        } else {
            self.chr_bank_fe1
        }
    }

    /// Read from CHR-ROM with latch update
    /// This should be called by the PPU when fetching pattern data
    fn read_chr_with_latch(&mut self, addr: u16) -> u8 {
        let value = self.read_chr(addr);
        self.update_latch(addr);
        value
    }

    /// Read from CHR-ROM without latch update
    fn read_chr(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0FFF => {
                let bank = self.current_chr_bank_0() as usize;
                let offset = (bank * 0x1000) + (addr as usize);
                self.chr_rom.get(offset).copied().unwrap_or(0)
            }
            0x1000..=0x1FFF => {
                let bank = self.current_chr_bank_1() as usize;
                let offset = (bank * 0x1000) + ((addr - 0x1000) as usize);
                self.chr_rom.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Update latches based on CHR address being accessed
    /// Called after the data has been read (latch switches after read)
    fn update_latch(&mut self, addr: u16) {
        match addr {
            // Latch 0 triggers on exactly $0FD8 (sets to $FD) and $0FE8 (sets to $FE)
            0x0FD8 if self.latch0 != 0xFD => self.latch0 = 0xFD,
            0x0FE8 if self.latch0 != 0xFE => self.latch0 = 0xFE,
            // Latch 1 triggers on $1FD8-$1FDF (sets to $FD) and $1FE8-$1FEF (sets to $FE)
            0x1FD8..=0x1FDF if self.latch1 != 0xFD => self.latch1 = 0xFD,
            0x1FE8..=0x1FEF if self.latch1 != 0xFE => self.latch1 = 0xFE,
            _ => {}
        }
    }
}

impl Mapper for Mapper9 {
    crate::state::mapper_state!(
        chr_rom,
        prg_ram,
        prg_bank,
        chr_bank_fd0,
        chr_bank_fe0,
        chr_bank_fd1,
        chr_bank_fe1,
        latch0,
        latch1,
        mirroring,
        irq_pending,
    );

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM: $0000-$1FFF (with latch update)
            0x0000..=0x1FFF => self.read_chr_with_latch(addr),
            // PRG-RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index]
            }
            // PRG-ROM bank 0: $8000-$9FFF (switchable 8KB)
            0x8000..=0x9FFF => {
                let bank = (self.prg_bank as usize) % self.prg_rom_banks().max(1);
                let offset = (bank * 0x2000) + ((addr - 0x8000) as usize);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            // PRG-ROM banks 1-3: $A000-$FFFF (fixed to last three 8KB banks)
            0xA000..=0xFFFF => {
                let total_banks = self.prg_rom_banks();
                let bank_offset = match addr {
                    0xA000..=0xBFFF => total_banks.saturating_sub(3),
                    0xC000..=0xDFFF => total_banks.saturating_sub(2),
                    0xE000..=0xFFFF => total_banks.saturating_sub(1),
                    _ => 0,
                };
                let bank = bank_offset % total_banks.max(1);
                let offset = (bank * 0x2000) + ((addr - 0xA000) % 0x2000) as usize;
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // PRG-RAM writes
            0x6000..=0x7FFF => {
                let index = (addr - 0x6000) as usize;
                self.prg_ram[index] = value;
            }
            // CHR-RAM is not supported on MMC2 (ROM only)
            0x0000..=0x1FFF => {}
            // Mapper registers at $8000-$FFFF
            0x8000..=0xFFFF => {
                match addr & 0xF000 {
                    0xA000 => {
                        // $A000-$AFFF: PRG bank select
                        self.prg_bank = value & 0x0F;
                    }
                    0xB000 => {
                        // $B000-$BFFF: CHR bank for $0000 when latch0 = $FD
                        self.chr_bank_fd0 = value & 0x1F;
                    }
                    0xC000 => {
                        // $C000-$CFFF: CHR bank for $0000 when latch0 = $FE
                        self.chr_bank_fe0 = value & 0x1F;
                    }
                    0xD000 => {
                        // $D000-$DFFF: CHR bank for $1000 when latch1 = $FD
                        self.chr_bank_fd1 = value & 0x1F;
                    }
                    0xE000 => {
                        // $E000-$EFFF: CHR bank for $1000 when latch1 = $FE
                        self.chr_bank_fe1 = value & 0x1F;
                    }
                    0xF000 => {
                        // $F000-$FFFF: Mirroring (bit 0: 0=vertical, 1=horizontal)
                        self.mirroring = (value & 0x01) != 0;
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

    /// Battery-backed work RAM ($6000-$7FFF).
    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    /// Restore battery-backed work RAM.
    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mapper() -> Mapper9 {
        let prg = vec![0u8; 131072]; // 128KB = 16 x 8KB banks
        let chr = vec![0u8; 65536]; // 64KB = 16 x 4KB banks
        Mapper9::new(prg, chr)
    }

    #[test]
    fn test_mapper9_new() {
        let mapper = create_test_mapper();
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.latch0, 0xFE);
        assert_eq!(mapper.latch1, 0xFE);
        assert!(!mapper.mirroring);
    }

    #[test]
    fn test_prg_bank_select() {
        let mut mapper = create_test_mapper();

        // Fill PRG-ROM with identifiable patterns
        for i in 0..mapper.prg_rom.len() {
            mapper.prg_rom[i] = (i / 0x2000) as u8;
        }

        // Default bank 0 at $8000
        assert_eq!(mapper.read(0x8000), 0);

        // Switch to bank 5
        mapper.write(0xA000, 0x05);
        assert_eq!(mapper.prg_bank, 5);
        assert_eq!(mapper.read(0x8000), 5);

        // Switch to bank 15
        mapper.write(0xA000, 0x0F);
        assert_eq!(mapper.prg_bank, 15);
        assert_eq!(mapper.read(0x8000), 15);
    }

    #[test]
    fn test_fixed_prg_banks() {
        let mut mapper = create_test_mapper();

        // Fill PRG-ROM with identifiable patterns
        for i in 0..mapper.prg_rom.len() {
            mapper.prg_rom[i] = (i / 0x2000) as u8;
        }

        // Fixed banks at $A000-$FFFF should be last three banks
        let last_bank = (mapper.prg_rom.len() / 0x2000) - 1;
        assert_eq!(mapper.read(0xE000), last_bank as u8);
        assert_eq!(mapper.read(0xC000), (last_bank - 1) as u8);
        assert_eq!(mapper.read(0xA000), (last_bank - 2) as u8);
    }

    #[test]
    fn test_chr_bank_registers() {
        let mut mapper = create_test_mapper();

        // Set CHR banks
        mapper.write(0xB000, 0x01); // chr_bank_fd0
        mapper.write(0xC000, 0x02); // chr_bank_fe0
        mapper.write(0xD000, 0x03); // chr_bank_fd1
        mapper.write(0xE000, 0x04); // chr_bank_fe1

        assert_eq!(mapper.chr_bank_fd0, 0x01);
        assert_eq!(mapper.chr_bank_fe0, 0x02);
        assert_eq!(mapper.chr_bank_fd1, 0x03);
        assert_eq!(mapper.chr_bank_fe1, 0x04);
    }

    #[test]
    fn test_latch0_trigger_fd() {
        let mut mapper = create_test_mapper();

        // Set up different banks for FD and FE states
        mapper.chr_bank_fd0 = 0x01;
        mapper.chr_bank_fe0 = 0x02;

        // Initially latch0 = FE
        assert_eq!(mapper.latch0, 0xFE);

        // Read from $0FD8 to trigger FD state
        mapper.read(0x0FD8);

        // Latch should now be FD
        assert_eq!(mapper.latch0, 0xFD);
    }

    #[test]
    fn test_latch0_trigger_fe() {
        let mut mapper = create_test_mapper();

        // Set latch to FD first
        mapper.latch0 = 0xFD;
        mapper.chr_bank_fd0 = 0x01;
        mapper.chr_bank_fe0 = 0x02;

        // Read from $0FE8 to trigger FE state
        mapper.read(0x0FE8);

        // Latch should now be FE
        assert_eq!(mapper.latch0, 0xFE);
    }

    #[test]
    fn test_latch1_trigger_fd() {
        let mut mapper = create_test_mapper();

        // Set up different banks for FD and FE states
        mapper.chr_bank_fd1 = 0x03;
        mapper.chr_bank_fe1 = 0x04;

        // Initially latch1 = FE
        assert_eq!(mapper.latch1, 0xFE);

        // Read from $1FD8 to trigger FD state
        mapper.read(0x1FD8);

        // Latch should now be FD
        assert_eq!(mapper.latch1, 0xFD);
    }

    #[test]
    fn test_latch1_trigger_fe() {
        let mut mapper = create_test_mapper();

        // Set latch to FD first
        mapper.latch1 = 0xFD;
        mapper.chr_bank_fd1 = 0x03;
        mapper.chr_bank_fe1 = 0x04;

        // Read from $1FE8 to trigger FE state
        mapper.read(0x1FE8);

        // Latch should now be FE
        assert_eq!(mapper.latch1, 0xFE);
    }

    #[test]
    fn test_latch1_range_fd() {
        let mut mapper = create_test_mapper();

        // Test that all addresses $1FD8-$1FDF trigger FD
        for addr in 0x1FD8..=0x1FDF {
            mapper.latch1 = 0xFE; // Reset latch
            mapper.read(addr);
            assert_eq!(mapper.latch1, 0xFD, "Failed at address ${:04X}", addr);
        }
    }

    #[test]
    fn test_latch1_range_fe() {
        let mut mapper = create_test_mapper();

        // Test that all addresses $1FE8-$1FEF trigger FE
        for addr in 0x1FE8..=0x1FEF {
            mapper.latch1 = 0xFD; // Reset latch
            mapper.read(addr);
            assert_eq!(mapper.latch1, 0xFE, "Failed at address ${:04X}", addr);
        }
    }

    #[test]
    fn test_mirroring_register() {
        let mut mapper = create_test_mapper();

        // Default: vertical mirroring
        assert!(!mapper.mirroring);

        // Set horizontal mirroring
        mapper.write(0xF000, 0x01);
        assert!(mapper.mirroring);

        // Set vertical mirroring
        mapper.write(0xF000, 0x00);
        assert!(!mapper.mirroring);
    }

    #[test]
    fn test_prg_ram() {
        let mut mapper = create_test_mapper();

        // Write to PRG-RAM
        mapper.write(0x6000, 0xAA);
        mapper.write(0x7FFF, 0xBB);

        // Read from PRG-RAM
        assert_eq!(mapper.read(0x6000), 0xAA);
        assert_eq!(mapper.read(0x7FFF), 0xBB);
    }

    #[test]
    fn test_chr_bank_switching() {
        let mut mapper = create_test_mapper();

        // Fill CHR-ROM with identifiable patterns
        for i in 0..mapper.chr_rom.len() {
            mapper.chr_rom[i] = (i / 0x1000) as u8;
        }

        // Set up banks
        mapper.chr_bank_fd0 = 0x05;
        mapper.chr_bank_fe0 = 0x06;
        mapper.latch0 = 0xFE; // Start with FE state

        // Read from $0000 should use bank 6 (FE state)
        assert_eq!(mapper.read(0x0000), 6);

        // Trigger FD state
        mapper.read(0x0FD8);

        // Read from $0000 should now use bank 5 (FD state)
        // Note: The latch update happens AFTER the read, so we need to read again
        assert_eq!(mapper.read(0x0000), 5);
    }

    #[test]
    fn test_irq_methods() {
        let mut mapper = create_test_mapper();

        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }
}
