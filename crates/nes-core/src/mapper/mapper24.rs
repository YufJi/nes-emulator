//! Mapper 24 / 26 - Konami VRC6
//!
//! Used by Akumajou Densetsu (Castlevania III, Japan), Madara and Esper Dream 2.
//!
//! Mapper 26 is the same chip with address lines A0 and A1 swapped, so the two
//! share one implementation.
//!
//! Banks:
//! - $6000-$7FFF: 8KB PRG RAM
//! - $8000-$BFFF: 16KB switchable PRG ROM
//! - $C000-$DFFF: 8KB switchable PRG ROM
//! - $E000-$FFFF: last 8KB PRG ROM, fixed
//! - $0000-$1FFF: eight 1KB switchable CHR ROM banks
//!
//! Registers (address AND $F003; mapper 26 swaps bits 0 and 1):
//! - $8000: 16KB PRG bank
//! - $B003: PPU banking style (bits 0-1 mode, bits 2-3 mirroring, bit 5 A10 pass-through)
//! - $D000-$D003 / $E000-$E003: CHR banks R0-R7
//! - $F000: IRQ latch, $F001: IRQ control, $F002: IRQ acknowledge
//!
//! Only PPU banking mode 0 is emulated, which is what every commercial VRC6
//! game uses: 1KB CHR banks with vertical / horizontal / one-screen mirroring
//! selected by $B003 bits 2-3. ROM nametables ($B003 bit 4) and the VRC6

//!
//! See https://www.nesdev.org/wiki/INES_Mapper_024

use super::Mapper;
use crate::apu::vrc6::Vrc6Audio;
use crate::ppu::rendering::Mirroring;

/// Konami VRC6 mapper (Mappers 24 and 26)
#[derive(Debug)]
pub struct Mapper24 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_is_ram: bool,
    /// 8KB work RAM at $6000-$7FFF
    prg_ram: [u8; 0x2000],
    /// 16KB PRG bank at $8000-$BFFF
    prg_bank_16k: u8,
    /// 8KB PRG bank at $C000-$DFFF
    prg_bank_8k: u8,
    /// CHR banks R0-R7 (1KB each)
    chr_banks: [u8; 8],
    /// Value written to $B003
    banking_style: u8,
    /// IRQ counter
    irq_counter: u8,
    /// IRQ reload value
    irq_latch: u8,
    /// Cycle mode (as opposed to scanline mode)
    irq_cycle_mode: bool,
    irq_enabled: bool,
    irq_pending: bool,
    /// Cycles accumulated while in scanline mode
    irq_scanline_accumulator: u16,
    /// The chip's expansion audio
    audio: Vrc6Audio,
    /// Whether this is the VRC6b revision (mapper 26)
    swap_address_bits: bool,
    prg_16k_bank_count: usize,
    prg_8k_bank_count: usize,
    chr_bank_count: usize,
}

impl Mapper24 {
    /// Create a VRC6a instance (mapper 24)
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, false)
    }

    /// Create a VRC6b instance (mapper 26), which swaps address bits 0 and 1
    pub fn new_swapped(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self::new_variant(prg_rom, chr_rom, true)
    }

    fn new_variant(prg_rom: Vec<u8>, chr_rom: Vec<u8>, swap_address_bits: bool) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };

        Self {
            prg_16k_bank_count: (prg_rom.len() / 0x4000).max(1),
            prg_8k_bank_count: (prg_rom.len() / 0x2000).max(1),
            chr_bank_count: (chr_rom.len() / 0x400).max(1),
            prg_rom,
            chr_rom,
            chr_is_ram,
            prg_ram: [0; 0x2000],
            prg_bank_16k: 0,
            prg_bank_8k: 0,
            chr_banks: [0; 8],
            banking_style: 0,
            irq_counter: 0,
            irq_latch: 0,
            irq_cycle_mode: false,
            irq_enabled: false,
            irq_pending: false,
            irq_scanline_accumulator: 0,
            audio: Vrc6Audio::new(),
            swap_address_bits,
            }
    }

    fn prg_16k_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_16k_bank_count) * 0x4000
    }

    fn prg_8k_offset(&self, bank: u8) -> usize {
        (bank as usize % self.prg_8k_bank_count) * 0x2000
    }

    fn chr_offset(&self, bank: u8) -> usize {
        (bank as usize % self.chr_bank_count) * 0x400
    }

    /// Register index for a write, honouring the VRC6b address swap.
    fn register_index(&self, addr: u16) -> u16 {
        let masked = addr & 0xF003;
        if self.swap_address_bits {
            let low = masked & 0x03;
            let swapped = ((low & 0x01) << 1) | ((low & 0x02) >> 1);
            (masked & !0x03) | swapped
        } else {
            masked
        }
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

impl Mapper for Mapper24 {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                self.chr_rom.get(offset + (addr as usize & 0x3FF)).copied().unwrap_or(0)
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let offset = self.prg_16k_offset(self.prg_bank_16k);
                self.prg_rom.get(offset + (addr as usize & 0x3FFF)).copied().unwrap_or(0)
            }
            0xC000..=0xDFFF => {
                let offset = self.prg_8k_offset(self.prg_bank_8k);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            0xE000..=0xFFFF => {
                let offset = self.prg_8k_offset((self.prg_8k_bank_count - 1) as u8);
                self.prg_rom.get(offset + (addr as usize & 0x1FFF)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let slot = (addr as usize) >> 10;
                let offset = self.chr_offset(self.chr_banks[slot]);
                let index = offset + (addr as usize & 0x3FF);
                if index < self.chr_rom.len() {
                    self.chr_rom[index] = value;
                }
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            0x8000..=0xFFFF => match self.register_index(addr) {
                0x8000 => self.prg_bank_16k = value & 0x0F,
                // $9000-$9003, $A000-$A002 and $B000-$B002 are the chip's
                // expansion audio registers.
                0x9000..=0x9003 | 0xA000..=0xA002 | 0xB000..=0xB002 => {
                    self.audio.write_register(self.register_index(addr), value)
                }
                0xB003 => self.banking_style = value,
                0xC000 => self.prg_bank_8k = value & 0x1F,
                0xD000..=0xD003 => self.chr_banks[(self.register_index(addr) - 0xD000) as usize] = value,
                0xE000..=0xE003 => {
                    self.chr_banks[(self.register_index(addr) - 0xE000 + 4) as usize] = value
                }
                0xF000 => {
                    self.irq_latch = value;
                    self.irq_counter = value;
                }
                0xF001 => {
                    self.irq_cycle_mode = value & 0x02 != 0;
                    self.irq_enabled = value & 0x01 != 0;
                    if value & 0x01 != 0 {
                        self.irq_counter = self.irq_latch;
                    }
                    if value & 0x04 != 0 {
                        self.irq_pending = false;
                    }
                }
                0xF002 => self.irq_pending = false,
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

    fn mirroring(&self) -> Option<Mirroring> {
        // PPU banking mode 0: bits 2-3 of $B003 pick the arrangement.
        Some(match self.banking_style & 0x0C {
            0x00 => Mirroring::Vertical,
            0x04 => Mirroring::Horizontal,
            0x08 => Mirroring::SingleScreen,
            _ => Mirroring::SingleScreenUpper,
        })
    }

    fn save_ram(&self) -> Option<&[u8]> {
        Some(&self.prg_ram)
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.prg_ram.len());
        self.prg_ram[..n].copy_from_slice(&data[..n]);
    }

    /// VRC6 can count CPU cycles or "scanlines", which the hardware measures as
    /// a fixed number of CPU cycles. The latter is approximated here.
    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    /// Clock the IRQ counter and the expansion audio.
    fn on_cpu_cycles(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.audio.tick();
        }

        if !self.irq_enabled || self.irq_cycle_mode {
            if self.irq_enabled {
                for _ in 0..cycles {
                    self.clock_irq();
                }
            }
            return;
        }

        // Scanline mode: 341 PPU dots / 3 = ~113.67 CPU cycles per scanline.
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

    fn make_mapper() -> Mapper24 {
        let mut prg = vec![0u8; 8 * 0x2000];
        for bank in 0..8 {
            prg[bank * 0x2000] = bank as u8;
        }
        let mut chr = vec![0u8; 8 * 0x400];
        for bank in 0..8 {
            chr[bank * 0x400] = 0x50 + bank as u8;
        }
        Mapper24::new(prg, chr)
    }

    #[test]
    fn test_prg_banks() {
        let mut mapper = make_mapper();

        mapper.write(0x8000, 1); // 16KB bank 1 -> $8000-$BFFF
        mapper.write(0xC000, 5); // 8KB bank 5 -> $C000-$DFFF
        assert_eq!(mapper.read(0x8000), 2);
        assert_eq!(mapper.read(0xC000), 5);
        assert_eq!(mapper.read(0xE000), 7, "$E000 is fixed to the last bank");
    }

    #[test]
    fn test_chr_banks_and_mirroring() {
        let mut mapper = make_mapper();

        mapper.write(0xD001, 3); // R1
        mapper.write(0xE002, 6); // R6
        assert_eq!(mapper.read(0x0400), 0x53);
        assert_eq!(mapper.read(0x1800), 0x56);

        assert_eq!(mapper.mirroring(), Some(Mirroring::Vertical));
        mapper.write(0xB003, 0x04);
        assert_eq!(mapper.mirroring(), Some(Mirroring::Horizontal));
        mapper.write(0xB003, 0x08);
        assert_eq!(mapper.mirroring(), Some(Mirroring::SingleScreen));
        mapper.write(0xB003, 0x0C);
        assert_eq!(mapper.mirroring(), Some(Mirroring::SingleScreenUpper));
    }

    #[test]
    fn test_irq_cycle_mode_counts_cycles() {
        let mut mapper = make_mapper();

        mapper.write(0xF000, 3); // latch = 3, counter = 3
        mapper.write(0xF001, 0x03); // cycle mode + enable (reloads)

        mapper.on_cpu_cycles(1); // 3 -> 2
        mapper.on_cpu_cycles(1); // 2 -> 1
        assert!(!mapper.irq_pending());
        mapper.on_cpu_cycles(1); // 1 -> 0
        mapper.on_cpu_cycles(1); // 0 -> reload + IRQ
        assert!(mapper.irq_pending());

        mapper.write(0xF002, 0);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_vrc6b_swaps_register_bits() {
        let mut mapper = Mapper24::new_swapped(vec![0u8; 8 * 0x2000], vec![0u8; 8 * 0x400]);

        // On VRC6b $D002 addresses R1, not R2.
        mapper.write(0xD002, 5);
        assert_eq!(mapper.chr_banks[1], 5);
    }

}
