//! Mapper 5 - MMC5 (ExROM)
//!
//! Used by games like Castlevania III, Just Breed, Uncharted Waters, Metal Slader Glory.
//! The most complex Nintendo mapper. Flexible PRG/CHR banking (up to 1 MB each),
//! expansion audio (2 pulse + PCM), 8x8 hardware multiplier, 1 KB ExRAM for extended
//! nametable attributes, vertical split screen, and scanline-counting IRQ.
//! See https://www.nesdev.org/wiki/MMC5

use super::Mapper;
use crate::apu::mmc5::Mmc5Audio;

/// MMC5 mapper (Mapper 5)
#[derive(Debug)]
pub struct Mapper5 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    /// 64KB PRG RAM (up to two 32KB chips)
    prg_ram: [u8; 0x10000],
    /// 1KB ExRAM (internal to MMC5)
    exram: [u8; 0x400],
    /// The board's expansion audio (two pulses + PCM)
    audio: Mmc5Audio,
    /// Whether CHR is RAM (writable) instead of ROM
    chr_is_ram: bool,

    // PRG banking
    /// PRG mode (0=32K, 1=16K+16K, 2=16K+8K+8K, 3=8K+8K+8K+8K)
    prg_mode: u8,
    /// PRG bank registers ($5113-$5117)
    prg_bank_reg: [u8; 5],
    /// PRG RAM write protection A ($5102)
    prg_ram_protect_a: u8,
    /// PRG RAM write protection B ($5103)
    prg_ram_protect_b: u8,

    // CHR banking
    /// CHR mode (0=8K, 1=4K, 2=2K, 3=1K)
    chr_mode: u8,
    /// CHR bank set A ($5120-$5127) - sprite banks
    chr_bank_a: [u16; 8],
    /// CHR bank set B ($5128-$512B) - background banks
    chr_bank_b: [u16; 4],
    /// Upper CHR bank bits ($5130)
    chr_upper_bits: u8,
    /// Which CHR set was last written (0=A, 1=B)
    last_chr_write: u8,

    // Nametable mapping
    /// Nametable mapping ($5105) - 2-bit field for each of 4 slots
    nt_mapping: [u8; 4],

    // ExRAM mode
    /// ExRAM mode (0=nametable, 1=ext attributes, 2=RAM, 3=read-only)
    exram_mode: u8,

    // Fill mode
    /// Fill-mode tile ($5106)
    fill_tile: u8,
    /// Fill-mode attribute ($5107)
    fill_attr: u8,

    // Scanline IRQ
    /// IRQ target scanline ($5203)
    irq_target: u8,
    /// IRQ enabled ($5204 bit 7)
    irq_enabled: bool,
    /// IRQ pending status flag ($5204 bit 7). Latches when the scanline
    /// counter reaches the target, independently of [`Mapper5::irq_enabled`];
    /// it is cleared by reading $5204.
    irq_pending: bool,
    /// In-frame flag
    in_frame: bool,
    /// IRQ scanline counter
    irq_counter: u8,

    // Hardware multiplier
    /// Multiplier operand A ($5205)
    mult_a: u8,
    /// Multiplier operand B ($5206)
    mult_b: u8,

    // ROM sizes
    prg_rom_banks: usize,
    chr_rom_banks: usize,
}

impl Mapper5 {
    /// Create a new Mapper5 instance
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram {
            vec![0u8; 8192] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        let prg_rom_banks = prg_rom.len() / 0x2000; // 8KB banks
        let chr_rom_banks = chr_rom.len() / 0x0400; // 1KB banks

        let mut mapper = Self {
            prg_rom,
            chr_rom,
            prg_ram: [0u8; 0x10000],
            exram: [0u8; 0x400],
            audio: Mmc5Audio::new(),
            chr_is_ram,
            prg_mode: 3, // Power-on default: mode 3 (8K banks)
            prg_bank_reg: [0; 5],
            prg_ram_protect_a: 0x03, // Reset to protected
            prg_ram_protect_b: 0x03,
            chr_mode: 3, // Power-on default: mode 3 (1K banks)
            chr_bank_a: [0; 8],
            chr_bank_b: [0; 4],
            chr_upper_bits: 0,
            last_chr_write: 0,
            nt_mapping: [0; 4],
            exram_mode: 0,
            fill_tile: 0,
            fill_attr: 0,
            irq_target: 0,
            irq_enabled: false,
            irq_pending: false,
            in_frame: false,
            irq_counter: 0,
            mult_a: 0,
            mult_b: 0,
            prg_rom_banks: prg_rom_banks.max(1),
            chr_rom_banks: chr_rom_banks.max(8),
        };

        // $5117 defaults to last page (0xFF)
        mapper.prg_bank_reg[4] = 0xFF;

        mapper
    }

    /// Check if PRG RAM writes are enabled
    fn is_prg_ram_writable(&self) -> bool {
        self.prg_ram_protect_a == 0x02 && self.prg_ram_protect_b == 0x01
    }

    /// Read from PRG-ROM/RAM at $8000-$FFFF
    fn read_prg(&self, addr: u16) -> u8 {
        if self.prg_mode == 0 {
            // Mode 0: One 32K ROM bank at $8000-$FFFF, controlled by $5117.
            // addr is always in range $8000-$FFFF, so no check needed.
            let reg = self.prg_bank_reg[4];
            let bank = ((reg & 0x7C) >> 2) as usize; // 32K page
            let offset = (addr - 0x8000) as usize;
            let rom_addr = (bank * 0x8000 + offset) % self.prg_rom.len();
            return self.prg_rom.get(rom_addr).copied().unwrap_or(0);
        }

        let slot = self.prg_slot(addr);
        // prg_bank_reg[0] is for $5113 (PRG RAM at $6000)
        // prg_bank_reg[1..=4] are for $5114-$5117 (PRG ROM at $8000-$FFFF)
        let reg_idx = slot + 1;
        let reg = self.prg_bank_reg[reg_idx];
        let base = 0x8000 + (slot as u16 * 0x2000);

        // $5117 is always ROM; $5114-$5116 use bit 7 for RAM/ROM select
        let is_ram = slot < 3 && (reg & 0x80) == 0;

        if is_ram {
            // Read from PRG RAM
            let bank = (reg & 0x07) as usize;
            let offset = (addr - base) as usize;
            let ram_addr = (bank * 0x2000 + offset) & 0xFFFF;
            self.prg_ram[ram_addr]
        } else {
            // Read from PRG ROM
            let bank = (reg & 0x7F) as usize;
            let offset = (addr - base) as usize;
            let rom_addr = (bank * 0x2000 + offset) % self.prg_rom.len();
            self.prg_rom.get(rom_addr).copied().unwrap_or(0)
        }
    }

    /// Which of the four 8K PRG slots ($8000/$A000/$C000/$E000) holds `addr`.
    ///
    /// Only meaningful for PRG modes 1-3; mode 0 is a single 32K ROM bank and
    /// is handled directly in [`Mapper5::read_prg`] / [`Mapper5::write_prg_space`].
    fn prg_slot(&self, addr: u16) -> usize {
        match self.prg_mode {
            1 => {
                // Mode 1: Two 16K banks ($5115 at $8000, $5117 at $C000).
                if addr < 0xC000 {
                    1
                } else {
                    3
                }
            }
            2 => {
                // Mode 2: 16K + 8K + 8K.
                if addr < 0xC000 {
                    1
                } else if addr < 0xE000 {
                    2
                } else {
                    3
                }
            }
            _ => {
                // Mode 3: Four 8K banks.
                if addr < 0xA000 {
                    0
                } else if addr < 0xC000 {
                    1
                } else if addr < 0xE000 {
                    2
                } else {
                    3
                }
            }
        }
    }

    /// Read from PRG RAM at $6000-$7FFF
    fn read_prg_ram(&self, addr: u16) -> u8 {
        let bank = (self.prg_bank_reg[0] & 0x07) as usize;
        let offset = (addr - 0x6000) as usize;
        let ram_addr = (bank * 0x2000 + offset) & 0xFFFF;
        self.prg_ram[ram_addr]
    }

    /// Write to PRG RAM
    fn write_prg_ram(&mut self, addr: u16, value: u8) {
        if !self.is_prg_ram_writable() {
            return;
        }
        let bank = (self.prg_bank_reg[0] & 0x07) as usize;
        let offset = (addr - 0x6000) as usize;
        let ram_addr = (bank * 0x2000 + offset) & 0xFFFF;
        self.prg_ram[ram_addr] = value;
    }

    /// Write to the $8000-$FFFF window.
    ///
    /// In PRG modes 1-3, bank registers $5114-$5116 with bit 7 clear map their
    /// slot to PRG RAM; those bytes are part of the battery-backed work RAM and
    /// must accept writes (still gated by the $5102/$5103 write protection).
    /// Mode 0 is always ROM, and $5117 (slot 3) can never select RAM.
    fn write_prg_space(&mut self, addr: u16, value: u8) {
        if self.prg_mode == 0 {
            return;
        }

        let slot = self.prg_slot(addr);
        if slot >= 3 {
            return;
        }

        let reg = self.prg_bank_reg[slot + 1];
        if reg & 0x80 != 0 || !self.is_prg_ram_writable() {
            return;
        }

        let bank = (reg & 0x07) as usize;
        let base = 0x8000 + (slot as u16 * 0x2000);
        let offset = (addr - base) as usize;
        let ram_addr = (bank * 0x2000 + offset) & 0xFFFF;
        self.prg_ram[ram_addr] = value;
    }

    /// Resolve a pattern-table address ($0000-$1FFF) to an offset in CHR memory.
    ///
    /// Reads and writes share this mapping so CHR-RAM uploads land on exactly
    /// the bytes the PPU will fetch back.
    fn chr_addr(&self, addr: u16) -> usize {
        // Use CHR bank set A (simplified - full implementation would track BG/sprite phase)
        let slot = (addr / 0x0400) as usize;
        let bank = match self.chr_mode {
            0 => {
                // 8K mode: $5127 selects an 8K page
                (self.chr_bank_a[7] & 0xFF) as usize * 2 + (addr as usize / 0x2000)
            }
            1 => {
                // 4K mode: $5123 selects 4K at $0000, $5127 selects 4K at $1000
                let half = addr as usize / 0x1000;
                if half == 0 {
                    (self.chr_bank_a[3] & 0xFF) as usize
                } else {
                    (self.chr_bank_a[7] & 0xFF) as usize
                }
            }
            2 => {
                // 2K mode: $5121/$5123/$5125/$5127 each select 2K
                let quarter = addr as usize / 0x0800;
                let reg_idx = quarter * 2 + 1;
                (self.chr_bank_a[reg_idx] & 0x1FF) as usize
            }
            3 | _ => {
                // 1K mode: $5120-$5127 each select a 1K page
                (self.chr_bank_a[slot] & 0x3FF) as usize
            }
        };

        let offset = (addr & 0x03FF) as usize;
        (bank * 0x0400 + offset) % self.chr_rom.len()
    }

    /// Read from CHR-ROM/RAM
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

    /// Clock the IRQ counter (called once per scanline)
    pub fn clock_irq_counter(&mut self) {
        if !self.in_frame {
            // Pre-render scanline: entering active rendering
            self.in_frame = true;
            self.irq_counter = 0;
            return;
        }

        // Visible scanlines: increment counter
        self.irq_counter = self.irq_counter.wrapping_add(1);

        // Check for IRQ trigger
        // $5203 value of 0 is a special case that never matches
        if self.irq_target != 0 && self.irq_counter == self.irq_target {
            self.irq_pending = true;
        }
    }

    /// Read from ExRAM
    fn read_exram(&self, addr: u16) -> u8 {
        // Readable in modes 2 and 3 only
        if self.exram_mode >= 2 {
            let offset = (addr - 0x5C00) as usize;
            self.exram[offset & 0x3FF]
        } else {
            0 // Open bus behavior
        }
    }

    /// Write to ExRAM
    fn write_exram(&mut self, addr: u16, value: u8) {
        let offset = (addr - 0x5C00) as usize;
        match self.exram_mode {
            0 | 1 => {
                // Modes 0/1: writable only during rendering (in-frame)
                // If not in-frame, $00 is written instead
                self.exram[offset & 0x3FF] = if self.in_frame { value } else { 0 };
            }
            2 => {
                // Mode 2: general-purpose RAM, always writable
                self.exram[offset & 0x3FF] = value;
            }
            3 => {
                // Mode 3: read-only, writes have no effect
            }
            _ => {}
        }
    }
}

impl Mapper for Mapper5 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, exram, audio,
        prg_mode, prg_bank_reg, prg_ram_protect_a, prg_ram_protect_b, chr_mode, chr_bank_a,
        chr_bank_b, chr_upper_bits, last_chr_write, nt_mapping, exram_mode, fill_tile,
        fill_attr, irq_target, irq_enabled, irq_pending, in_frame, irq_counter, mult_a,
        mult_b, prg_rom_banks, chr_rom_banks);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CHR-ROM/RAM: $0000-$1FFF
            0x0000..=0x1FFF => self.read_chr(addr),
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => self.read_prg_ram(addr),
            // ExRAM: $5C00-$5FFF
            0x5C00..=0x5FFF => self.read_exram(addr),
            // PRG-ROM: $8000-$FFFF
            0x8000..=0xFFFF => self.read_prg(addr),
            // Expansion audio registers
            0x5015 => self.audio.read_status(),
            0x5204 => {
                // IRQ status
                let mut val = 0;
                if self.irq_pending {
                    val |= 0x80;
                }
                if self.in_frame {
                    val |= 0x40;
                }
                // Reading $5204 acknowledges (clears) the IRQ pending flag
                self.irq_pending = false;
                val
            }
            0x5205 => {
                // Multiplier result low byte
                (self.mult_a as u16 * self.mult_b as u16) as u8
            }
            0x5206 => {
                // Multiplier result high byte
                ((self.mult_a as u16 * self.mult_b as u16) >> 8) as u8
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        // The MMC5's pulse channels and PCM DAC live at $5000-$5015.
        if (0x5000..=0x5015).contains(&addr) {
            self.audio.write_register(addr, value);
            // $5010 stays part of the mapper's own register set.
            if (0x5000..=0x5007).contains(&addr) || addr == 0x5011 || addr == 0x5015 {
                return;
            }
        }

        match addr {
            // CHR-RAM writes (if enabled)
            0x0000..=0x1FFF => {
                self.write_chr(addr, value);
            }
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                self.write_prg_ram(addr, value);
            }
            // ExRAM: $5C00-$5FFF
            0x5C00..=0x5FFF => {
                self.write_exram(addr, value);
            }
            // PRG writes: $8000-$FFFF (may be RAM in some modes)
            0x8000..=0xFFFF => {
                self.write_prg_space(addr, value);
            }
            // MMC5 registers
            0x5100 => {
                // PRG banking mode
                self.prg_mode = value & 0x03;
            }
            0x5101 => {
                // CHR banking mode
                self.chr_mode = value & 0x03;
            }
            0x5102 => {
                // PRG RAM write protection A
                self.prg_ram_protect_a = value & 0x03;
            }
            0x5103 => {
                // PRG RAM write protection B
                self.prg_ram_protect_b = value & 0x03;
            }
            0x5104 => {
                // ExRAM mode
                self.exram_mode = value & 0x03;
            }
            0x5105 => {
                // Nametable mapping
                let mut v = value;
                self.nt_mapping[0] = v & 0x03;
                v >>= 2;
                self.nt_mapping[1] = v & 0x03;
                v >>= 2;
                self.nt_mapping[2] = v & 0x03;
                v >>= 2;
                self.nt_mapping[3] = v & 0x03;
            }
            0x5106 => {
                // Fill-mode tile
                self.fill_tile = value;
            }
            0x5107 => {
                // Fill-mode attribute
                self.fill_attr = value & 0x03;
            }
            0x5113 => {
                // PRG RAM bank for $6000-$7FFF
                self.prg_bank_reg[0] = value & 0x07;
            }
            0x5114..=0x5117 => {
                // PRG bank registers $5114-$5117
                let idx = (addr - 0x5113) as usize; // 1-4
                if idx < 5 {
                    self.prg_bank_reg[idx] = value;
                }
            }
            0x5120..=0x5127 => {
                // CHR bank set A ($5120-$5127)
                let reg = (addr - 0x5120) as usize;
                if reg < 8 {
                    self.chr_bank_a[reg] = ((self.chr_upper_bits as u16) << 8) | (value as u16);
                    self.last_chr_write = 0;
                }
            }
            0x5128..=0x512B => {
                // CHR bank set B ($5128-$512B)
                let reg = (addr - 0x5128) as usize;
                if reg < 4 {
                    self.chr_bank_b[reg] = ((self.chr_upper_bits as u16) << 8) | (value as u16);
                    self.last_chr_write = 1;
                }
            }
            0x5130 => {
                // Upper CHR bank bits
                self.chr_upper_bits = value & 0x03;
            }
            0x5203 => {
                // IRQ target scanline
                self.irq_target = value;
            }
            0x5204 => {
                // IRQ enable
                self.irq_enabled = (value & 0x80) != 0;
                // The pending flag is not cleared here: if it already latched,
                // enabling now asserts the IRQ line immediately.
            }
            0x5205 => {
                // Multiplier operand A
                self.mult_a = value;
            }
            0x5206 => {
                // Multiplier operand B
                self.mult_b = value;
            }
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        // $5204 bit 7 latches the pending status even while IRQ generation is
        // disabled, but the CPU's IRQ line only asserts when it is enabled.
        // Enabling after the flag has already latched raises the IRQ at once.
        self.irq_enabled && self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// MMC5 counts scanlines: the counter restarts on the pre-render line and
    /// increments once per visible scanline, raising an IRQ when it reaches
    /// the value written to $5203.
    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    /// The MMC5's pulse channels run at the CPU clock rate.
    fn on_cpu_cycles(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.audio.tick();
        }
    }

    fn on_scanline(&mut self, scanline: u16, _rendering: bool) {
        if scanline >= 261 {
            // Pre-render line (261 on NTSC, 311 on PAL): start of frame.
            self.in_frame = true;
            self.irq_counter = 0;
        } else if scanline < 240 {
            self.irq_counter = self.irq_counter.wrapping_add(1);
            if self.irq_target != 0 && self.irq_counter == self.irq_target {
                self.irq_pending = true;
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mapper() -> Mapper5 {
        let prg = vec![0u8; 0x100000]; // 1MB (128 x 8KB banks)
        let chr = vec![0u8; 0x40000]; // 256KB (256 x 1KB banks)
        Mapper5::new(prg, chr)
    }

    #[test]
    fn test_mapper5_new() {
        let mapper = create_test_mapper();
        assert_eq!(mapper.prg_mode, 3);
        assert_eq!(mapper.chr_mode, 3);
        assert_eq!(mapper.prg_bank_reg[4], 0xFF);
        assert!(!mapper.irq_enabled);
    }

    #[test]
    fn test_prg_mode_select() {
        let mut mapper = create_test_mapper();

        mapper.write(0x5100, 0);
        assert_eq!(mapper.prg_mode, 0);

        mapper.write(0x5100, 1);
        assert_eq!(mapper.prg_mode, 1);

        mapper.write(0x5100, 2);
        assert_eq!(mapper.prg_mode, 2);

        mapper.write(0x5100, 3);
        assert_eq!(mapper.prg_mode, 3);
    }

    #[test]
    fn test_chr_mode_select() {
        let mut mapper = create_test_mapper();

        mapper.write(0x5101, 0);
        assert_eq!(mapper.chr_mode, 0);

        mapper.write(0x5101, 1);
        assert_eq!(mapper.chr_mode, 1);

        mapper.write(0x5101, 2);
        assert_eq!(mapper.chr_mode, 2);

        mapper.write(0x5101, 3);
        assert_eq!(mapper.chr_mode, 3);
    }

    #[test]
    fn test_prg_ram_write_protection() {
        let mut mapper = create_test_mapper();

        // Default: write-protected
        assert!(!mapper.is_prg_ram_writable());

        // Enable writes
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);
        assert!(mapper.is_prg_ram_writable());

        // Disable writes
        mapper.write(0x5102, 0x03);
        assert!(!mapper.is_prg_ram_writable());
    }

    #[test]
    fn test_prg_ram_read_write() {
        let mut mapper = create_test_mapper();

        // Enable writes
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);

        // Write and read back
        mapper.write(0x6000, 0x42);
        assert_eq!(mapper.read(0x6000), 0x42);

        mapper.write(0x7FFF, 0xAB);
        assert_eq!(mapper.read(0x7FFF), 0xAB);
    }

    #[test]
    fn test_prg_ram_bank_select() {
        let mut mapper = create_test_mapper();

        // Enable writes
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);

        // Select bank 1
        mapper.write(0x5113, 0x01);

        // Write to bank 1
        mapper.write(0x6000, 0x55);

        // Switch to bank 2
        mapper.write(0x5113, 0x02);

        // $6000 should now be different
        assert_eq!(mapper.read(0x6000), 0);

        // Switch back to bank 1
        mapper.write(0x5113, 0x01);
        assert_eq!(mapper.read(0x6000), 0x55);
    }

    #[test]
    fn test_prg_rom_read_mode3() {
        let mut prg = vec![0u8; 0x40000]; // 256KB
        for i in 0..32 {
            prg[i * 0x2000] = i as u8;
        }
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper5::new(prg, chr);

        // Mode 3: Four 8K banks
        mapper.write(0x5100, 3);

        // Set banks
        mapper.write(0x5114, 0x80 | 0x05); // Bank 5 at $8000 (ROM)
        mapper.write(0x5115, 0x80 | 0x0A); // Bank 10 at $A000 (ROM)
        mapper.write(0x5116, 0x80 | 0x0F); // Bank 15 at $C000 (ROM)
        // $5117 defaults to 0xFF

        assert_eq!(mapper.read(0x8000), 5);
        assert_eq!(mapper.read(0xA000), 10);
        assert_eq!(mapper.read(0xC000), 15);
    }

    #[test]
    fn test_exram_mode() {
        let mut mapper = create_test_mapper();

        // Mode 0: nametable
        mapper.write(0x5104, 0);
        assert_eq!(mapper.exram_mode, 0);

        // Mode 1: extended attributes
        mapper.write(0x5104, 1);
        assert_eq!(mapper.exram_mode, 1);

        // Mode 2: RAM
        mapper.write(0x5104, 2);
        assert_eq!(mapper.exram_mode, 2);

        // Mode 3: read-only
        mapper.write(0x5104, 3);
        assert_eq!(mapper.exram_mode, 3);
    }

    #[test]
    fn test_exram_write_mode2() {
        let mut mapper = create_test_mapper();

        // Set mode 2 (RAM)
        mapper.write(0x5104, 2);

        // Write to ExRAM
        mapper.write(0x5C00, 0x42);
        assert_eq!(mapper.read(0x5C00), 0x42);

        mapper.write(0x5FFF, 0xAB);
        assert_eq!(mapper.read(0x5FFF), 0xAB);
    }

    #[test]
    fn test_exram_write_mode3() {
        let mut mapper = create_test_mapper();

        // Set mode 3 (read-only)
        mapper.write(0x5104, 3);

        // Write should have no effect
        mapper.write(0x5C00, 0x42);
        assert_eq!(mapper.read(0x5C00), 0);
    }

    #[test]
    fn test_multiplier() {
        let mut mapper = create_test_mapper();

        // Write operands
        mapper.write(0x5205, 0x12);
        mapper.write(0x5206, 0x34);

        // Read result: 0x12 * 0x34 = 0x03A8
        assert_eq!(mapper.mult_a, 0x12);
        assert_eq!(mapper.mult_b, 0x34);
        assert_eq!(mapper.read(0x5205), 0xA8); // Low byte
        assert_eq!(mapper.read(0x5206), 0x03); // High byte
    }

    #[test]
    fn test_irq_target() {
        let mut mapper = create_test_mapper();

        mapper.write(0x5203, 100);
        assert_eq!(mapper.irq_target, 100);
    }

    #[test]
    fn test_irq_enable() {
        let mut mapper = create_test_mapper();

        assert!(!mapper.irq_enabled);

        mapper.write(0x5204, 0x80);
        assert!(mapper.irq_enabled);

        mapper.write(0x5204, 0x00);
        assert!(!mapper.irq_enabled);
    }

    #[test]
    fn test_irq_counter() {
        let mut mapper = create_test_mapper();

        // Set target
        mapper.write(0x5203, 5);
        mapper.write(0x5204, 0x80); // Enable IRQ

        // First clock enters frame
        mapper.clock_irq_counter();
        assert!(mapper.in_frame);
        assert_eq!(mapper.irq_counter, 0);

        // Clock up to target
        for _ in 0..5 {
            mapper.clock_irq_counter();
        }

        assert!(mapper.irq_pending());
    }

    #[test]
    fn test_irq_counter_target_zero() {
        let mut mapper = create_test_mapper();

        // Target of 0 never matches
        mapper.write(0x5203, 0);
        mapper.write(0x5204, 0x80);

        mapper.clock_irq_counter();
        for _ in 0..10 {
            mapper.clock_irq_counter();
        }

        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_irq_status_read() {
        let mut mapper = create_test_mapper();

        // Set up IRQ
        mapper.write(0x5203, 1);
        mapper.write(0x5204, 0x80);
        mapper.clock_irq_counter();
        mapper.clock_irq_counter();
        mapper.clock_irq_counter();

        assert!(mapper.irq_pending());

        // Read status
        let status = mapper.read(0x5204);
        assert!(status & 0x80 != 0); // IRQ pending
        assert!(!mapper.irq_pending()); // Should be cleared
    }

    #[test]
    fn test_chr_bank_a_select() {
        let mut mapper = create_test_mapper();

        // Set upper bits
        mapper.write(0x5130, 0x01);

        // Set bank A0
        mapper.write(0x5120, 0x34);
        assert_eq!(mapper.chr_bank_a[0], 0x134);

        // Set bank A7
        mapper.write(0x5127, 0x56);
        assert_eq!(mapper.chr_bank_a[7], 0x156);
    }

    #[test]
    fn test_chr_bank_b_select() {
        let mut mapper = create_test_mapper();

        // Set upper bits
        mapper.write(0x5130, 0x02);

        // Set bank B0
        mapper.write(0x5128, 0x12);
        assert_eq!(mapper.chr_bank_b[0], 0x212);

        // Set bank B3
        mapper.write(0x512B, 0x78);
        assert_eq!(mapper.chr_bank_b[3], 0x278);
    }

    #[test]
    fn test_nametable_mapping() {
        let mut mapper = create_test_mapper();

        // Set mapping: 0, 1, 2, 3 for the 4 nametable slots
        // Value: 0b11_10_01_00 = 0xE4
        mapper.write(0x5105, 0xE4);

        assert_eq!(mapper.nt_mapping[0], 0);
        assert_eq!(mapper.nt_mapping[1], 1);
        assert_eq!(mapper.nt_mapping[2], 2);
        assert_eq!(mapper.nt_mapping[3], 3);
    }

    #[test]
    fn test_fill_mode() {
        let mut mapper = create_test_mapper();

        mapper.write(0x5106, 0x42);
        assert_eq!(mapper.fill_tile, 0x42);

        mapper.write(0x5107, 0x03);
        assert_eq!(mapper.fill_attr, 0x03);
    }

    #[test]
    fn test_on_scanline_counts_and_fires_irq() {
        let mut mapper = create_test_mapper();
        mapper.write(0x5203, 3);
        mapper.write(0x5204, 0x80); // enable IRQ generation

        // Pre-render line starts the frame and resets the counter.
        mapper.on_scanline(261, true);
        assert_eq!(mapper.irq_counter, 0);
        assert!(!mapper.irq_pending());

        mapper.on_scanline(0, true);
        mapper.on_scanline(1, true);
        assert!(!mapper.irq_pending());

        mapper.on_scanline(2, true); // counter reaches 3
        assert!(mapper.irq_pending());

        // VBlank lines do not advance the counter.
        mapper.clear_irq();
        mapper.on_scanline(245, true);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_irq_line_is_gated_by_enable() {
        let mut mapper = create_test_mapper();
        mapper.write(0x5203, 3);

        // Drive the counter to the target while IRQ generation is disabled.
        mapper.on_scanline(261, true); // pre-render resets the counter
        mapper.on_scanline(0, true);
        mapper.on_scanline(1, true);
        mapper.on_scanline(2, true); // counter reaches 3

        // The IRQ line must stay low even though the condition was met.
        assert!(!mapper.irq_pending(), "IRQ must not assert while disabled");

        // Enabling after the flag has latched asserts the IRQ immediately.
        mapper.write(0x5204, 0x80);
        assert!(mapper.irq_pending());

        // $5204 still reports the latched flag, and reading it acknowledges.
        assert_eq!(mapper.read(0x5204) & 0x80, 0x80);
        assert!(!mapper.irq_pending());

        // Disabling again keeps the line low.
        mapper.write(0x5204, 0x00);
        assert!(!mapper.irq_pending());
    }

    fn create_chr_ram_mapper() -> Mapper5 {
        // An empty CHR image means the cartridge has CHR-RAM.
        Mapper5::new(vec![0u8; 0x100000], vec![])
    }

    #[test]
    fn test_5015_read_reports_audio_status() {
        let mut mapper = create_test_mapper();

        // Nothing is playing yet.
        assert_eq!(mapper.read(0x5015), 0);

        // Enable and start pulse A; $5015 must report the channel as active.
        mapper.write(0x5015, 0x01);
        mapper.write(0x5000, 0xBF); // duty 2, constant volume 15
        mapper.write(0x5002, 0x20); // period low
        mapper.write(0x5003, 0x08); // period high + length load
        assert_eq!(mapper.read(0x5015) & 0x01, 0x01);
    }

    #[test]
    fn test_chr_ram_writes_are_visible() {
        let mut mapper = create_chr_ram_mapper();
        assert!(mapper.chr_is_ram);

        // Default (1K) CHR mode with bank 0: a plain write/read round trip.
        mapper.write(0x0010, 0x42);
        assert_eq!(mapper.read(0x0010), 0x42);

        // Bank 1 covers the second 1K page.
        mapper.write(0x5120, 0x01); // slot 0 -> 1K bank 1
        mapper.write(0x0000, 0xAB); // lands in 1K bank 1
        mapper.write(0x5121, 0x01); // slot 1 -> the same 1K bank 1
        assert_eq!(mapper.read(0x0000), 0xAB);
        assert_eq!(mapper.read(0x0400), 0xAB);
    }

    #[test]
    fn test_chr_rom_writes_are_ignored() {
        let mut mapper = create_test_mapper();
        assert!(!mapper.chr_is_ram);

        let before = mapper.read(0x0010);
        mapper.write(0x0010, before ^ 0xFF);
        assert_eq!(mapper.read(0x0010), before);
    }

    #[test]
    fn test_prg_ram_writable_in_prg_space() {
        let mut mapper = create_test_mapper();
        mapper.write(0x5100, 3); // four 8K banks

        // $5114 with bit 7 clear maps $8000-$9FFF to PRG RAM (bank 0).
        mapper.write(0x5114, 0x00);

        // Protected by default: the write is dropped.
        mapper.write(0x8000, 0xAB);
        assert_eq!(mapper.read(0x8000), 0x00);

        // Unlock PRG RAM and write again.
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);
        mapper.write(0x8000, 0xAB);
        assert_eq!(mapper.read(0x8000), 0xAB);

        // The same physical byte is reachable through the $6000 window.
        mapper.write(0x5113, 0x00);
        assert_eq!(mapper.read(0x6000), 0xAB);
    }

    #[test]
    fn test_prg_rom_writes_in_prg_space_are_ignored() {
        let mut prg = vec![0u8; 0x40000];
        prg[0x2000] = 0x11; // 8K bank 1
        let chr = vec![0u8; 8192];
        let mut mapper = Mapper5::new(prg, chr);

        mapper.write(0x5100, 3);
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);

        // $5114 with bit 7 set maps $8000-$9FFF to ROM bank 1.
        mapper.write(0x5114, 0x80 | 0x01);
        assert_eq!(mapper.read(0x8000), 0x11);

        // Writing through a ROM-mapped slot must not corrupt the ROM.
        mapper.write(0x8000, 0x99);
        assert_eq!(mapper.read(0x8000), 0x11);
    }

    #[test]
    fn test_prg_space_ram_survives_battery_snapshot() {
        let mut mapper = create_test_mapper();
        mapper.write(0x5100, 3);
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);
        mapper.write(0x5114, 0x00); // $8000-$9FFF -> PRG RAM bank 0

        mapper.write(0x8000, 0x5A);

        let snapshot = mapper.save_ram().expect("MMC5 exposes work RAM").to_vec();

        // A fresh cartridge restores the snapshot into the same slot.
        let mut restored = create_test_mapper();
        restored.write(0x5100, 3);
        restored.write(0x5102, 0x02);
        restored.write(0x5103, 0x01);
        restored.write(0x5114, 0x00);
        restored.load_save_ram(&snapshot);
        assert_eq!(restored.read(0x8000), 0x5A);
    }

    #[test]
    fn test_save_state_restores_banks_and_ram() {
        use crate::state::{StateReader, StateWriter};

        let mut mapper = create_test_mapper();
        mapper.write(0x5102, 0x02);
        mapper.write(0x5103, 0x01);
        mapper.write(0x5104, 2); // ExRAM as RAM
        mapper.write(0x5113, 0x03); // PRG-RAM bank 3
        mapper.write(0x6000, 0x5A);
        mapper.write(0x5C00, 0xA5);
        mapper.write(0x5100, 2); // PRG mode 2
        mapper.write(0x5115, 0x80 | 0x0A);
        mapper.write(0x5120, 0x11);

        let mut w = StateWriter::new();
        mapper.save_state(&mut w);
        let saved = w.into_bytes();

        // Disturb every piece of state we care about.
        mapper.write(0x6000, 0x00);
        mapper.write(0x5C00, 0x00);
        mapper.write(0x5100, 0);
        mapper.write(0x5115, 0x00);
        mapper.write(0x5120, 0x00);

        let mut r = StateReader::new(&saved);
        mapper.load_state(&mut r);

        assert_eq!(mapper.read(0x6000), 0x5A, "PRG-RAM bank was not restored");
        assert_eq!(mapper.read(0x5C00), 0xA5, "ExRAM was not restored");
        assert_eq!(mapper.prg_mode, 2, "PRG mode was not restored");
        assert_eq!(mapper.prg_bank_reg[2], 0x8A, "$5115 was not restored");
        assert_eq!(mapper.chr_bank_a[0], 0x11, "CHR bank A0 was not restored");
    }
}
