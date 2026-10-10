//! Mappers 21 / 22 / 23 / 25 - Konami VRC2 and VRC4
//!
//! These are the same chip on different boards; the boards differ only in which
//! two CPU address lines select the sub-register within a $1000 block:
//!
//! | iNES | variant | lines  | registers                          |
//! |------|---------|--------|------------------------------------|
//! | 22   | VRC2a   | A1, A0 | $x000 $x002 $x001 $x003            |
//! | 23   | VRC4f   | A0, A1 | $x000 $x001 $x002 $x003            |
//! | 21   | VRC4a   | A1, A2 | $x000 $x002 $x004 $x006            |
//! | 25   | VRC4b   | A1, A0 | $x000 $x002 $x001 $x003            |
//!
//! Only the primary variant of each mapper number is emulated; the rarer
//! PCB revisions that share a mapper number (VRC4c on 21, VRC4d on 25) use
//! different address lines and are not distinguished.
//!
//! Registers (sub-register index in brackets):
//! - $8000[0-3]: PRG bank for $8000 (or $C000 when swap mode is on)
//! - $9000[0]: nametable arrangement, [2]: PRG swap / WRAM enable (VRC4)
//! - $A000[0-3]: PRG bank for $A000
//! - $B000 / $C000 / $D000 / $E000: CHR banks 0/1, 2/3, 4/5, 6/7 (low, high)
//! - $F000: IRQ latch, $F001: IRQ latch high, $F002: IRQ control,
//!   $F003: IRQ acknowledge
//!
//! VRC2 (mapper 22) has no $C000 PRG bank, no swap mode and only
//! vertical/horizontal mirroring.
//!
//! See https://www.nesdev.org/wiki/INES_Mapper_021

use super::Mapper;
use crate::ppu::rendering::Mirroring;

/// Konami VRC2/VRC4 mapper
#[derive(Debug)]
pub struct Mapper21 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    /// Optional 8KB work RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// Address bit that selects sub-register bit 0
    addr_bit0: u8,
    /// Address bit that selects sub-register bit 1
    addr_bit1: u8,
    /// VRC2 lacks the $C000 PRG bank, swap mode and one-screen mirroring
    is_vrc2: bool,
    /// 8KB PRG bank for $8000 (or $C000 when swap mode is set)
    prg_bank_0: u8,
    /// 8KB PRG bank for $A000
    prg_bank_1: u8,
    swap_mode: bool,
    wram_enable: bool,
    /// Low nibble of each 1KB CHR bank
    chr_low: [u8; 8],
    /// High bits (4-8) of each 1KB CHR bank
    chr_high: [u8; 8],
    mirroring: Mirroring,
    irq_latch: u8,
    irq_counter: u8,
    irq_cycle_mode: bool,
    irq_enabled: bool,
    irq_pending: bool,
    /// Cycles accumulated while in scanline mode
    irq_scanline_accumulator: u16,
    prg_8k_bank_count: usize,
    chr_1k_bank_count: usize,
}

impl Mapper21 {
    /// Create a mapper with the given sub-register address lines.
    fn new_variant(prg_rom: Vec<u8>, chr_rom: Vec<u8>, addr_bit0: u8, addr_bit1: u8, is_vrc2: bool) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_8k_bank_count: (prg_rom.len() / 0x2000).max(1),
            chr_1k_bank_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_ram: [0; 0x2000],
            addr_bit0,
            addr_bit1,
            is_vrc2,
            prg_bank_0: 0,
            prg_bank_1: 1,
            swap_mode: false,
            wram_enable: false,
            chr_low: [0; 8],
            chr_high: [0; 8],
            mirroring: Mirroring::Vertical,
            irq_latch: 0,
            irq_counter: 0,
            irq_cycle_mode: false,
            irq_enabled: false,
            irq_pending: false,
            irq_scanline_accumulator: 0,
        }
    }

    /// VRC2a (mapper 22)
    pub fn new_vrc2a(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, 1, 0, true)
    }

    /// VRC4f (mapper 23)
    pub fn new_vrc4f(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, 0, 1, false)
    }

    /// VRC4a (mapper 21)
    pub fn new_vrc4a(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, 1, 2, false)
    }

    /// VRC4b (mapper 25)
    pub fn new_vrc4b(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, 1, 0, false)
    }

    fn prg_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_8k_bank_count) * 0x2000
    }

    fn chr_offset(&self, bank: u16) -> usize {
        (bank as usize % self.chr_1k_bank_count) * 0x400
    }

    /// Sub-register index selected by the configured address lines.
    fn sub_register(&self, addr: u16) -> u8 {
        let bit0 = (addr >> self.addr_bit0) & 0x01;
        let bit1 = (addr >> self.addr_bit1) & 0x01;
        ((bit1 << 1) | bit0) as u8
    }

    /// 1KB CHR bank built from the low and high registers.
    fn chr_bank(&self, slot: usize) -> u16 {
        self.chr_low[slot] as u16 | ((self.chr_high[slot] as u16) << 4)
    }

    fn clock_irq(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter -= 1;
        }
    }
}

impl Mapper for Mapper21 {
    crate::state::mapper_state!(chr_if ram: chr_is_ram, chr_rom, prg_ram, addr_bit0, addr_bit1,
        is_vrc2, prg_bank_0, prg_bank_1, swap_mode, wram_enable, chr_low, chr_high, mirroring,
        irq_latch, irq_counter, irq_cycle_mode, irq_enabled, irq_pending,
        irq_scanline_accumulator, prg_8k_bank_count, chr_1k_bank_count);

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_bank(slot));
                self.chr_rom.get(offset + (addr as usize & 0x3FF)).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => {
                if self.wram_enable || self.is_vrc2 {
                    self.prg_ram[(addr - 0x6000) as usize]
                } else {
                    0
                }
            }
            0x8000..=0x9FFF if self.swap_mode => {
                // Swap mode: $8000 shows the second-to-last bank.
                let offset = self.prg_offset((self.prg_8k_bank_count - 2) as u8);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0x8000..=0x9FFF => {
                let offset = self.prg_offset(self.prg_bank_0);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_offset(self.prg_bank_1);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xC000..=0xDFFF if !self.is_vrc2 => {
                if self.swap_mode {
                    let offset = self.prg_offset(self.prg_bank_0);
                    self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
                } else {
                    let offset = self.prg_offset((self.prg_8k_bank_count - 2) as u8);
                    self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
                }
            }
            // $C000-$FFFF on VRC2, and $E000-$FFFF everywhere, is the last
            // 16KB / 8KB of the ROM.
            0xC000..=0xFFFF => {
                let (bank, mask) = if self.is_vrc2 {
                    ((self.prg_8k_bank_count - 2) as u8, 0x3FFF)
                } else {
                    ((self.prg_8k_bank_count - 1) as u8, 0x1FFF)
                };
                let offset = self.prg_offset(bank);
                self.prg_rom.get(offset + (addr as usize & mask)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_bank(slot));
                let index = offset + (addr as usize & 0x3FF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            // VRC2 chips pass the value straight through; VRC4 uses the
            // address to pick the sub-register.
            0x8000..=0xFFFF => {
                let sub = self.sub_register(addr);
                match addr & 0xF000 {
                    0x8000 => self.prg_bank_0 = value & 0x1F,
                    0x9000 => {
                        if self.is_vrc2 {
                            // Every $9000-$9003 sub-register sets mirroring and
                            // only vertical/horizontal exist.
                            self.mirroring = if value & 0x01 != 0 {
                                Mirroring::Horizontal
                            } else {
                                Mirroring::Vertical
                            };
                        } else {
                            match sub {
                                0 => {
                                    self.mirroring = match value & 0x03 {
                                        0 => Mirroring::Vertical,
                                        1 => Mirroring::Horizontal,
                                        2 => Mirroring::SingleScreen,
                                        _ => Mirroring::SingleScreenUpper,
                                    };
                                }
                                2 => {
                                    self.swap_mode = value & 0x02 != 0;
                                    self.wram_enable = value & 0x01 != 0;
                                }
                                _ => {}
                            }
                        }
                    }
                    0xA000 => self.prg_bank_1 = value & 0x1F,
                    0xB000 => {
                        // $B000/$B001: CHR 0 low/high, $B002/$B003: CHR 1
                        let slot = (sub >> 1) as usize;
                        if sub & 1 == 0 {
                            self.chr_low[slot] = value & 0x0F;
                        } else {
                            self.chr_high[slot] = value & 0x1F;
                        }
                    }
                    0xC000 => {
                        let slot = 2 + (sub >> 1) as usize;
                        if sub & 1 == 0 {
                            self.chr_low[slot] = value & 0x0F;
                        } else {
                            self.chr_high[slot] = value & 0x1F;
                        }
                    }
                    0xD000 => {
                        let slot = 4 + (sub >> 1) as usize;
                        if sub & 1 == 0 {
                            self.chr_low[slot] = value & 0x0F;
                        } else {
                            self.chr_high[slot] = value & 0x1F;
                        }
                    }
                    0xE000 => {
                        let slot = 6 + (sub >> 1) as usize;
                        if sub & 1 == 0 {
                            self.chr_low[slot] = value & 0x0F;
                        } else {
                            self.chr_high[slot] = value & 0x1F;
                        }
                    }
                    0xF000 => match sub {
                        0 => {
                            self.irq_latch = value;
                            self.irq_counter = value;
                        }
                        2 => {
                            self.irq_cycle_mode = value & 0x02 != 0;
                            self.irq_enabled = value & 0x01 != 0;
                            if value & 0x01 != 0 {
                                self.irq_counter = self.irq_latch;
                            }
                            if value & 0x04 != 0 {
                                self.irq_pending = false;
                            }
                        }
                        3 => self.irq_pending = false,
                        _ => {}
                    },
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

    fn mirroring(&self) -> Option<Mirroring> {
        Some(self.mirroring)
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }

    fn on_cpu_cycles(&mut self, cycles: u8) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_cycle_mode {
            for _ in 0..cycles {
                self.clock_irq();
            }
            return;
        }
        // Scanline mode: ~113.67 CPU cycles per scanline.
        self.irq_scanline_accumulator += cycles as u16;
        while self.irq_scanline_accumulator >= 114 {
            self.irq_scanline_accumulator -= 114;
            self.clock_irq();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vrc4f() -> Mapper21 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x70 + bank as u8;
        }
        Mapper21::new_vrc4f(prg, chr)
    }

    #[test]
    fn test_prg_banks_and_swap_mode() {
        let mut mapper = make_vrc4f();

        mapper.write(0x8000, 3);
        mapper.write(0xA000, 4);
        assert_eq!(mapper.read(0x8000), 3);
        assert_eq!(mapper.read(0xA000), 4);
        assert_eq!(mapper.read(0xC000), 6, "$C000 is the second-to-last bank");
        assert_eq!(mapper.read(0xE000), 7, "$E000 is the last bank");

        // Swap mode moves the switchable bank down to $C000.
        mapper.write(0x9002, 0x02);
        assert_eq!(mapper.read(0x8000), 6);
        assert_eq!(mapper.read(0xC000), 3);
    }

    #[test]
    fn test_chr_banking_includes_high_bits() {
        let mut mapper = Mapper21::new_vrc4f(vec![0u8; 8 * 0x2000], vec![0u8; 64 * 0x400]);
        for bank in 0..64usize {
            mapper.chr_rom[bank * 0x400] = bank as u8;
        }

        // Slot 0: low nibble from $B000, high bits from $B001.
        mapper.write(0xB000, 0x0A);
        mapper.write(0xB001, 0x01); // bit 4
        assert_eq!(mapper.read(0x0000), 0x1A);
    }

    #[test]
    fn test_vrc2a_register_lines() {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut mapper = Mapper21::new_vrc2a(prg, vec![0u8; 8 * 0x400]);

        // On VRC2a every $9000-$9003 sub-register sets mirroring and only
        // vertical/horizontal are supported.
        mapper.write(0x9002, 0x01);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));

        mapper.write(0x9001, 0x00);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));

        // VRC2 fixes $C000-$FFFF to the last 16KB and has no swap mode.
        assert_eq!(mapper.read(0xC000), 6);
    }

    #[test]
    fn test_mirroring_modes() {
        let mut mapper = make_vrc4f();
        mapper.write(0x9000, 0x00);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));
        mapper.write(0x9000, 0x01);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));
        mapper.write(0x9000, 0x02);
        assert_eq!(mapper.mirroring(), Some(Mirroring::SingleScreen));
        mapper.write(0x9000, 0x03);
        assert_eq!(mapper.mirroring(), Some(Mirroring::SingleScreenUpper));
    }

    #[test]
    fn test_irq() {
        let mut mapper = make_vrc4f();
        mapper.write(0xF000, 2); // latch
        mapper.write(0xF002, 0x03); // cycle mode + enable

        mapper.on_cpu_cycles(1); // reload to 2
        mapper.on_cpu_cycles(1); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.on_cpu_cycles(1); // 1 -> 0
        mapper.on_cpu_cycles(1); // reload + IRQ
        assert!(mapper.irq_pending());

        mapper.write(0xF003, 0);
        assert!(!mapper.irq_pending());
    }
}
