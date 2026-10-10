//! Mapper 119 - TQROM (MMC3 variant)
//!
//! Used by games like Pin-Bot and High Speed (both by Rare).
//! Identical to standard MMC3 except: bit 6 of the CHR bank register values
//! selects between CHR ROM (0) and CHR RAM (1). Bits 0-5 specify the bank
//! within the selected chip, allowing up to 64KB CHR ROM and 8KB CHR RAM.
//! A 74HC32 ORs PPU A13 with CHR A16 (bit 6) to generate the ROM chip-enable,
//! while CHR A16 directly enables the RAM chip.
//! See https://www.nesdev.org/wiki/INES_Mapper_119

use super::Mapper;

/// Mapper 119 - TQROM (MMC3 variant)
#[derive(Debug)]
pub struct Mapper119 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 8KB PRG RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// 8KB CHR RAM (separate from CHR ROM)
    chr_ram: [u8; 0x2000],

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

    // CHR ROM/RAM tracking
    /// Tracks which CHR RAM bank (0-7) is mapped at each 1KB PPU slot (0-7),
    /// or None if CHR ROM is there.
    chr_ram_slots: [Option<u8>; 8],

    // ROM sizes
    prg_rom_banks: usize,
    chr_rom_banks: usize,
}

impl Mapper119 {
    /// Create a new Mapper119 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let prg_rom_banks = prg_rom.len() / 0x2000; // 8KB banks
        // CHR ROM banks are 1KB each, but bit 6 selects ROM/RAM
        // So we have up to 64 banks of ROM (bit 6 = 0)
        let chr_rom_banks = chr_rom.len() / 0x0400; // 1KB banks

        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0u8; 0x2000],
            chr_ram: [0u8; 0x2000],
            command: 0,
            prg_address_select: 0,
            chr_address_select: 0,
            bank_regs: [0; 8],
            irq_counter: 0,
            irq_latch: 0,
            irq_enabled: false,
            irq_pending: false,
            mirroring: 0,
            chr_ram_slots: [None; 8],
            prg_rom_banks: prg_rom_banks.max(2),
            chr_rom_banks: chr_rom_banks.max(1),
        }
    }

    /// Get PRG-ROM bank for 8KB region
    fn get_prg_bank(&self, region: u8) -> usize {
        // region: 0=$8000, 1=$A000, 2=$C000, 3=$E000
        let bank = match region {
            0 => {
                if self.prg_address_select == 0 {
                    // R6 at $8000
                    self.bank_regs[6] as usize & 0x3F
                } else {
                    // Second-to-last bank fixed at $8000
                    self.prg_rom_banks.saturating_sub(2)
                }
            }
            1 => {
                // R7 at $A000 (always switchable)
                self.bank_regs[7] as usize & 0x3F
            }
            2 => {
                if self.prg_address_select == 0 {
                    // Second-to-last bank fixed at $C000
                    self.prg_rom_banks.saturating_sub(2)
                } else {
                    // R6 at $C000
                    self.bank_regs[6] as usize & 0x3F
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

    /// Get the CHR source (ROM or RAM) and bank for a 1KB slot
    fn get_chr_source(&self, slot: u8) -> (bool, usize) {
        // Returns (is_ram, bank_number)
        // is_ram: true = CHR RAM, false = CHR ROM
        // bank_number: the bank index within the selected memory
        let reg_idx = match slot {
            0 | 1 => {
                if self.chr_address_select == 0 {
                    0 // R0 for slots 0-1
                } else {
                    slot + 2 // R2-R3 for slots 0-1
                }
            }
            2 | 3 => {
                if self.chr_address_select == 0 {
                    1 // R1 for slots 2-3
                } else {
                    slot + 2 // R4-R5 for slots 2-3
                }
            }
            4 | 5 | 6 | 7 => {
                if self.chr_address_select == 0 {
                    slot - 2 // R2-R5 for slots 4-7
                } else {
                    if slot < 6 {
                        slot - 4 // R0-R1 for slots 4-5
                    } else {
                        slot - 4 // R6-R7 for slots 6-7
                    }
                }
            }
            _ => 0,
        };

        let reg_value = self.bank_regs[reg_idx as usize];
        let is_ram = (reg_value & 0x40) != 0; // Bit 6 selects RAM (1) or ROM (0)

        // For 2KB banks (R0, R1 in normal mode or R0, R1 in swapped mode for slots 4-5),
        // we need to handle the bank number appropriately
        let bank = match slot {
            0 | 1 => {
                if self.chr_address_select == 0 {
                    // R0 is a 2KB bank, mask bit 0 for alignment
                    if is_ram {
                        (((reg_value & 0x06) >> 1) as usize * 2) + (slot as usize & 1)
                    } else {
                        (((reg_value & 0x3E) >> 1) as usize * 2) + (slot as usize & 1)
                    }
                } else {
                    // 1KB banks
                    if is_ram {
                        (reg_value & 0x07) as usize
                    } else {
                        (reg_value & 0x3F) as usize
                    }
                }
            }
            2 | 3 => {
                if self.chr_address_select == 0 {
                    // R1 is a 2KB bank
                    if is_ram {
                        (((reg_value & 0x06) >> 1) as usize * 2) + (slot as usize & 1)
                    } else {
                        (((reg_value & 0x3E) >> 1) as usize * 2) + (slot as usize & 1)
                    }
                } else {
                    if is_ram {
                        (reg_value & 0x07) as usize
                    } else {
                        (reg_value & 0x3F) as usize
                    }
                }
            }
            4 | 5 => {
                if self.chr_address_select == 0 {
                    // R2-R3 are 1KB banks
                    if is_ram {
                        (reg_value & 0x07) as usize
                    } else {
                        (reg_value & 0x3F) as usize
                    }
                } else {
                    // R0-R1 are 2KB banks in swapped mode
                    if is_ram {
                        (((reg_value & 0x06) >> 1) as usize * 2) + (slot as usize & 1)
                    } else {
                        (((reg_value & 0x3E) >> 1) as usize * 2) + (slot as usize & 1)
                    }
                }
            }
            6 | 7 => {
                if self.chr_address_select == 0 {
                    // R4-R5 are 1KB banks
                    if is_ram {
                        (reg_value & 0x07) as usize
                    } else {
                        (reg_value & 0x3F) as usize
                    }
                } else {
                    // R6-R7 are 1KB banks in swapped mode
                    if is_ram {
                        (reg_value & 0x07) as usize
                    } else {
                        (reg_value & 0x3F) as usize
                    }
                }
            }
            _ => 0,
        };

        (is_ram, bank)
    }

    /// Read from CHR-ROM/RAM at the given address
    fn read_chr(&self, addr: u16) -> u8 {
        let slot = (addr / 0x0400) as u8;
        let offset = (addr & 0x03FF) as usize;

        if let Some(ram_bank) = self.chr_ram_slots[slot as usize] {
            // Reading from CHR RAM
            let ram_addr = (ram_bank as usize * 0x0400 + offset) % self.chr_ram.len();
            self.chr_ram[ram_addr]
        } else {
            // Reading from CHR ROM
            let (is_ram, bank) = self.get_chr_source(slot);
            if is_ram {
                // This shouldn't happen if chr_ram_slots is properly maintained
                // But handle it just in case
                let ram_addr = (bank * 0x0400 + offset) % self.chr_ram.len();
                self.chr_ram[ram_addr]
            } else {
                let rom_addr = (bank * 0x0400 + offset) % self.chr_rom.len().max(1);
                self.chr_rom.get(rom_addr).copied().unwrap_or(0)
            }
        }
    }

    /// Write to CHR-RAM at the given address
    fn write_chr(&mut self, addr: u16, value: u8) {
        let slot = (addr / 0x0400) as u8;
        let offset = (addr & 0x03FF) as usize;

        if let Some(ram_bank) = self.chr_ram_slots[slot as usize] {
            // Writing to CHR RAM
            let ram_addr = (ram_bank as usize * 0x0400 + offset) % self.chr_ram.len();
            self.chr_ram[ram_addr] = value;
        }
        // If CHR ROM is mapped, writes are ignored (ROM is read-only)
    }

    /// Execute a bank select command
    fn execute_command(&mut self, cmd: u8, value: u8) {
        let cmd_idx = cmd as usize;
        match cmd {
            0 | 1 => {
                // Select 2KB CHR bank at $0000 (cmd 0) or $0800 (cmd 1)
                self.bank_regs[cmd_idx] = value;
                self.update_chr_slots();
            }
            2 | 3 | 4 | 5 => {
                // Select 1KB CHR bank
                self.bank_regs[cmd_idx] = value;
                self.update_chr_slots();
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

    /// Update CHR RAM slot tracking based on current bank registers
    fn update_chr_slots(&mut self) {
        // For each 1KB slot, determine if it should use CHR RAM or ROM
        for slot in 0..8u8 {
            let (is_ram, bank) = self.get_chr_source(slot);
            if is_ram {
                self.chr_ram_slots[slot as usize] = Some(bank as u8 % 8);
            } else {
                self.chr_ram_slots[slot as usize] = None;
            }
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

    /// Check if a CHR address is writable (mapped to CHR RAM)
    pub fn is_chr_writable(&self, addr: u16) -> bool {
        if addr >= 0x2000 {
            return false;
        }
        let slot = (addr / 0x0400) as usize;
        self.chr_ram_slots[slot].is_some()
    }

    /// Get the CHR RAM slot for a given address (None if ROM)
    pub fn get_chr_ram_slot(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 {
            return None;
        }
        let slot = (addr / 0x0400) as usize;
        self.chr_ram_slots[slot]
    }
}

impl Mapper for Mapper119 {
    crate::state::mapper_state!(
        prg_ram,
        chr_ram,
        command,
        prg_address_select,
        chr_address_select,
        bank_regs,
        irq_counter,
        irq_latch,
        irq_enabled,
        irq_pending,
        mirroring,
        chr_ram_slots,
        prg_rom_banks,
        chr_rom_banks,
    );

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
            // CHR-RAM writes (if enabled for this slot)
            0x0000..=0x1FFF => {
                self.write_chr(addr, value);
            }
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
                        self.update_chr_slots();
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
                        // IRQ counter
                        self.irq_counter = value;
                    }
                    0xC001 => {
                        // IRQ latch
                        self.irq_latch = value;
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

    fn create_test_mapper() -> Mapper119 {
        let prg = vec![0u8; 0x80000]; // 512KB (64 x 8KB banks)
        let chr = vec![0u8; 0x10000]; // 64KB (64 x 1KB banks)
        Mapper119::new(prg, chr)
    }

    #[test]
    fn test_mapper119_new() {
        let mapper = create_test_mapper();
        assert_eq!(mapper.prg_rom_banks, 64);
        assert_eq!(mapper.chr_rom_banks, 64);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending);
    }

    #[test]
    fn test_prg_ram_read_write() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x4000];
        let mut mapper = Mapper119::new(prg, chr);

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
        mapper.write(0x8000, 0xC6);
        assert_eq!(mapper.command, 6);
        assert_eq!(mapper.prg_address_select, 1);
        assert_eq!(mapper.chr_address_select, 1);
    }

    #[test]
    fn test_prg_bank_switching() {
        let mut prg = vec![0u8; 0x20000]; // 128KB (16 x 8KB banks)
        for i in 0..16 {
            prg[i * 0x2000] = i as u8;
        }
        let chr = vec![0u8; 0x4000];
        let mut mapper = Mapper119::new(prg, chr);

        // Set R6 to bank 3
        mapper.write(0x8000, 0x06);
        mapper.write(0x8001, 3);

        // Set R7 to bank 5
        mapper.write(0x8000, 0x07);
        mapper.write(0x8001, 5);

        // Read from different regions
        assert_eq!(mapper.read(0x8000), 3); // R6
        assert_eq!(mapper.read(0xA000), 5); // R7
        assert_eq!(mapper.read(0xC000), 14); // Second-to-last
        assert_eq!(mapper.read(0xE000), 15); // Last
    }

    #[test]
    fn test_chr_rom_selection() {
        let prg = vec![0u8; 0x20000];
        let mut chr = vec![0u8; 0x10000]; // 64KB CHR ROM
        // Mark 1KB banks
        for i in 0..64 {
            chr[i * 0x0400] = i as u8;
        }
        let mut mapper = Mapper119::new(prg, chr);

        // Normal mode: chr_address_select = 0
        // Set R0 to bank 2 (bit 6 = 0, so CHR ROM)
        mapper.write(0x8000, 0x00); // command = 0
        mapper.write(0x8001, 0x02); // bank 2, CHR ROM

        // Verify CHR ROM is selected (not RAM)
        assert!(!mapper.is_chr_writable(0x0000));
        assert_eq!(mapper.get_chr_ram_slot(0x0000), None);

        // Read should come from CHR ROM
        assert_eq!(mapper.read(0x0000), 2);
    }

    #[test]
    fn test_chr_ram_selection() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x10000]; // 64KB CHR ROM
        let mut mapper = Mapper119::new(prg, chr);

        // Set R0 to CHR RAM bank 1 (bit 6 = 1, bits 1-0 = 01)
        // In normal mode, R0 is a 2KB bank at $0000
        mapper.write(0x8000, 0x00); // command = 0
        mapper.write(0x8001, 0x42); // bit 6 = 1 (RAM), bank = 1 (0x02 << 1 = 0x04, but we use 0x42)

        // Verify CHR RAM is selected
        assert!(mapper.is_chr_writable(0x0000));
        assert!(mapper.get_chr_ram_slot(0x0000).is_some());

        // Write to CHR RAM
        mapper.write(0x0000, 0xAA);
        assert_eq!(mapper.read(0x0000), 0xAA);

        mapper.write(0x0400, 0xBB);
        assert_eq!(mapper.read(0x0400), 0xBB);
    }

    #[test]
    fn test_chr_ram_1kb_banks() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x10000];
        let mut mapper = Mapper119::new(prg, chr);

        // Set R2 (1KB bank at $1000 in normal mode) to CHR RAM bank 3
        mapper.write(0x8000, 0x02); // command = 2
        mapper.write(0x8001, 0x43); // bit 6 = 1 (RAM), bank = 3

        // Verify CHR RAM is selected at $1000
        assert!(mapper.is_chr_writable(0x1000));

        // Write and read back
        mapper.write(0x1000, 0xCC);
        assert_eq!(mapper.read(0x1000), 0xCC);
    }

    #[test]
    fn test_chr_rom_write_ignored() {
        let prg = vec![0u8; 0x20000];
        let mut chr = vec![0u8; 0x10000];
        chr[0] = 0x11;
        let mut mapper = Mapper119::new(prg, chr);

        // Set R0 to CHR ROM (bit 6 = 0)
        mapper.write(0x8000, 0x00);
        mapper.write(0x8001, 0x00); // CHR ROM bank 0

        // Verify CHR ROM is selected
        assert!(!mapper.is_chr_writable(0x0000));

        // Write should be ignored
        let original_value = mapper.read(0x0000);
        mapper.write(0x0000, 0xFF);
        assert_eq!(mapper.read(0x0000), original_value);
    }

    #[test]
    fn test_mixed_chr_rom_ram() {
        let prg = vec![0u8; 0x20000];
        let mut chr = vec![0u8; 0x10000];
        for i in 0..64 {
            chr[i * 0x0400] = 0x80 | i as u8;
        }
        let mut mapper = Mapper119::new(prg, chr);

        mapper.write(0x8000, 0x00);
        mapper.write(0x8001, 0x04);

        mapper.write(0x8000, 0x02);
        mapper.write(0x8001, 0x42);

        mapper.write(0x1000, 0x33);

        assert_eq!(mapper.read(0x0000), 0x84);
        assert_eq!(mapper.read(0x0400), 0x85);
        assert_eq!(mapper.read(0x1000), 0x33);

        assert!(!mapper.is_chr_writable(0x0000));
        assert!(mapper.is_chr_writable(0x1000));
    }

    #[test]
    fn test_chr_address_select_swap() {
        let prg = vec![0u8; 0x20000];
        let mut chr = vec![0u8; 0x10000];
        for i in 0..64 {
            chr[i * 0x0400] = i as u8;
        }
        let mut mapper = Mapper119::new(prg, chr);

        // Set chr_address_select = 1 (swapped mode)
        mapper.write(0x8000, 0x80);

        // In swapped mode, R2-R5 are at $0000-$0FFF
        // Set R2 to CHR ROM bank 10
        mapper.write(0x8000, 0x82); // command = 2, chr_address_select = 1
        mapper.write(0x8001, 0x0A); // CHR ROM bank 10

        // R2 should now be at $0000 in swapped mode
        assert_eq!(mapper.read(0x0000), 10);
    }

    #[test]
    fn test_irq_counter() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x4000];
        let mut mapper = Mapper119::new(prg, chr);

        // Set IRQ latch
        mapper.write(0xC001, 5);
        assert_eq!(mapper.irq_latch, 5);

        // Set IRQ counter directly
        mapper.write(0xC000, 3);
        assert_eq!(mapper.irq_counter, 3);

        // Enable IRQ
        mapper.write(0xE001, 0);
        assert!(mapper.irq_enabled);

        // Clock the counter down to 0
        mapper.clock_irq_counter(); // 3 -> 2
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.clock_irq_counter(); // 1 -> 0, triggers IRQ
        assert!(mapper.irq_pending());
    }

    #[test]
    fn test_irq_disable() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x4000];
        let mut mapper = Mapper119::new(prg, chr);

        // Set up and trigger IRQ
        mapper.write(0xC001, 1);
        mapper.write(0xE001, 0);
        mapper.clock_irq_counter(); // reload to 1
        mapper.clock_irq_counter(); // 1 -> 0, triggers
        assert!(mapper.irq_pending());

        // Disable IRQ
        mapper.write(0xE000, 0);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_mirroring_select() {
        let prg = vec![0u8; 0x20000];
        let chr = vec![0u8; 0x4000];
        let mut mapper = Mapper119::new(prg, chr);

        // Default mirroring
        assert_eq!(mapper.mirroring, 0);

        // Set to horizontal mirroring
        mapper.write(0xA000, 1);
        assert_eq!(mapper.mirroring, 1);

        // Set to vertical mirroring
        mapper.write(0xA000, 0);
        assert_eq!(mapper.mirroring, 0);
    }
}
