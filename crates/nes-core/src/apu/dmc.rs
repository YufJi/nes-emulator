//! DMC (Delta Modulation Channel) for the NES APU
//!
//! The DMC channel plays delta-encoded samples from CPU memory ($C000-$FFFF).
//! It uses DMA to fetch sample bytes and outputs a 7-bit DAC value.
//!
//! Reference: https://www.nesdev.org/wiki/APU_DMC

use crate::apu::AudioChannel;
use crate::prelude::*;
use crate::Region;

/// DMC timer period table for NTSC (16 preset values)
/// These values represent the number of CPU cycles between output unit clocks.
const DMC_PERIOD_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// The same table for PAL, where the APU runs on the other crystal: Mesen keeps
/// a second table (`_dmcPeriodLookupTablePal`) and picks it from the console's
/// region.
const DMC_PERIOD_TABLE_PAL: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

/// The sixteen output-clock periods for a console's region.
fn dmc_period_table(region: Region) -> &'static [u16; 16] {
    match region {
        Region::Ntsc => &DMC_PERIOD_TABLE,
        Region::Pal => &DMC_PERIOD_TABLE_PAL,
    }
}

/// DMC channel play modes
#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayMode {
    /// Normal mode - play once, no IRQ
    Normal,
    /// Loop mode - restart sample when it ends
    Loop,
    /// IRQ mode - generate IRQ when sample ends
    Irq,
}

impl crate::state::StateField for PlayMode {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(match self {
            PlayMode::Normal => 0,
            PlayMode::Loop => 1,
            PlayMode::Irq => 2,
        });
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = match r.u8() {
            1 => PlayMode::Loop,
            2 => PlayMode::Irq,
            _ => PlayMode::Normal,
        };
    }
}

/// DMC (Delta Modulation Channel) for sample playback
///
/// The DMC channel plays 1-bit delta-encoded samples from CPU memory.
/// Each sample byte contains 8 delta values (1 bit each). When a bit is 0,
/// the output level decreases by 2 (clamped at 0). When a bit is 1, the
/// output level increases by 2 (clamped at 126).
pub struct DmcChannel {
    /// Whether the channel is enabled (via $4015)
    is_enabled: bool,
    /// Whether the sample buffer has data
    has_sample: bool,
    /// IRQ flag - set when sample ends in IRQ mode
    irq_generated: bool,
    /// Current play mode (normal, loop, or IRQ)
    play_mode: PlayMode,
    /// Timer period (from the region's period table)
    timer_period: u16,
    /// The region's sixteen output-clock periods, fixed when the channel is
    /// built: PAL's APU runs on a slower crystal and has its own table.
    period_table: &'static [u16; 16],
    /// Timer counter (counts down to 0, then clocks output unit)
    timer_counter: u16,
    /// Output level (0-126, stored as 7-bit value)
    output_level: u8,
    /// Sample start address ($C000 + reg_value * 64)
    sample_start_address: u16,
    /// Current sample address (increments during playback)
    sample_address: u16,
    /// Sample length in bytes (reg_value * 16 + 1)
    sample_length: u16,
    /// Bytes remaining counter
    bytes_remaining: u16,
    /// Sample buffer (holds byte fetched via DMA)
    sample_buffer: u8,
    /// Shift register (holds current byte being processed)
    shift_register: u8,
    /// Bits remaining in shift register (0-8), counted down to the byte
    /// boundary the output unit reloads on.
    bits_remaining: u8,
    /// Whether the register is holding its level rather than playing a byte
    /// (Mesen's `_silenceFlag`). The output unit is free-running: a byte that
    /// arrives while it is silent starts at the next boundary, not on the spot.
    silence_flag: bool,
    /// Raw $4012 register value (for save states)
    reg_4012: u8,
    /// Raw $4013 register value (for save states)
    reg_4013: u8,
    /// Last byte fetched via DMA (for bus hijacking detection)
    last_fetched_byte: u8,
    /// Address of a byte the channel is waiting for the bus to supply.
    ///
    /// When no `dma_callback` is installed the channel cannot read CPU memory
    /// itself; it publishes the address here and the bus replies with
    /// [`DmcChannel::supply_dma_byte`].
    dma_request: Option<u16>,
    /// Callback for DMA reads from CPU memory
    dma_callback: Option<Box<dyn FnMut(u16) -> u8 + Send>>,
    /// Callback for IRQ generation
    irq_callback: Option<Box<dyn FnMut() + Send>>,
}

impl crate::state::StateField for DmcChannel {
    fn write(&self, w: &mut crate::state::StateWriter) {
        self.is_enabled.write(w);
        self.has_sample.write(w);
        self.irq_generated.write(w);
        self.play_mode.write(w);
        self.timer_period.write(w);
        self.timer_counter.write(w);
        self.output_level.write(w);
        self.sample_start_address.write(w);
        self.sample_address.write(w);
        self.sample_length.write(w);
        self.bytes_remaining.write(w);
        self.sample_buffer.write(w);
        self.shift_register.write(w);
        self.bits_remaining.write(w);
        self.silence_flag.write(w);
        self.reg_4012.write(w);
        self.reg_4013.write(w);
        self.last_fetched_byte.write(w);
        self.dma_request.write(w);
        // The DMA and IRQ callbacks are owned by the bus and survive a load.
    }

    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        self.is_enabled.read(r);
        self.has_sample.read(r);
        self.irq_generated.read(r);
        self.play_mode.read(r);
        self.timer_period.read(r);
        self.timer_counter.read(r);
        self.output_level.read(r);
        self.sample_start_address.read(r);
        self.sample_address.read(r);
        self.sample_length.read(r);
        self.bytes_remaining.read(r);
        self.sample_buffer.read(r);
        self.shift_register.read(r);
        self.bits_remaining.read(r);
        self.silence_flag.read(r);
        self.reg_4012.read(r);
        self.reg_4013.read(r);
        self.last_fetched_byte.read(r);
        self.dma_request.read(r);
    }
}

impl DmcChannel {
    /// Create a new DMC channel
    pub fn new() -> Self {
        Self {
            is_enabled: false,
            has_sample: false,
            irq_generated: false,
            play_mode: PlayMode::Normal,
            timer_period: DMC_PERIOD_TABLE[0],
            period_table: dmc_period_table(Region::Ntsc),
            timer_counter: 0,
            output_level: 0,
            sample_start_address: 0xC000,
            sample_address: 0xC000,
            sample_length: 1,
            bytes_remaining: 0,
            sample_buffer: 0,
            shift_register: 0,
            bits_remaining: 8,
            silence_flag: true,
            reg_4012: 0,
            reg_4013: 0,
            last_fetched_byte: 0,
            dma_request: None,
            dma_callback: None,
            irq_callback: None,
        }
    }

    /// Set the DMA callback for reading from CPU memory
    pub fn set_dma_callback<F>(&mut self, callback: F)
    where
        F: FnMut(u16) -> u8 + Send + 'static,
    {
        self.dma_callback = Some(Box::new(callback));
    }

    /// Set the IRQ callback for generating interrupts
    pub fn set_irq_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + Send + 'static,
    {
        self.irq_callback = Some(Box::new(callback));
    }

    /// Clock the DMC channel timer
    ///
    /// This should be called at CPU clock rate. When the timer expires,
    /// the output unit is clocked.
    pub fn clock_timer(&mut self) {
        if self.timer_counter > 0 {
            self.timer_counter -= 1;
        }
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.clock_output_unit();
        }
    }

    /// Clock the output unit (called when timer expires)
    ///
    /// One bit of the byte being played leaves the shift register and moves the
    /// DAC. When the register runs out of bits the byte the reader put in the
    /// buffer takes its place, and the reader is fed again from there — Mesen's
    /// `Run`: a transfer starts at the byte boundary, and the byte it brings
    /// back waits in the buffer until the register runs dry. With nothing
    /// buffered the DAC holds its level (Mesen's silence flag) rather than
    /// decaying over a register full of zeroes.
    fn clock_output_unit(&mut self) {
        if !self.silence_flag {
            let bit = self.shift_register & 1;
            if bit == 0 {
                // Decrement by 2, clamped at 0
                if self.output_level >= 2 {
                    self.output_level -= 2;
                } else {
                    self.output_level = 0;
                }
            } else {
                // Increment by 2, clamped at 126
                if self.output_level <= 124 {
                    self.output_level += 2;
                } else {
                    self.output_level = 126;
                }
            }
            self.shift_register >>= 1;
        }

        self.bits_remaining = self.bits_remaining.saturating_sub(1);
        if self.bits_remaining == 0 {
            // The byte is over. The register reloads from the buffer; with
            // nothing waiting it holds its level and the reader is asked for a
            // byte — a transfer starts at the byte boundary, and what it brings
            // back waits in the buffer until the boundary after it.
            self.bits_remaining = 8;
            if self.has_sample {
                self.shift_register = self.sample_buffer;
                self.has_sample = false;
                self.silence_flag = false;
                self.fetch_next_sample();
            } else {
                self.silence_flag = true;
            }
        }

        // Trigger IRQ if flag is set
        if self.irq_generated {
            if let Some(ref mut callback) = self.irq_callback {
                callback();
            }
        }
    }

    /// Fetch the next sample byte via DMA
    ///
    /// This reads a byte from CPU memory and loads it into the sample buffer.
    fn fetch_next_sample(&mut self) {
        if self.bytes_remaining == 0 || self.dma_request.is_some() {
            return;
        }
        if let Some(ref mut callback) = self.dma_callback {
            // Read from current sample address
            let byte = callback(self.sample_address);
            self.load_sample_byte(byte);
            return;
        }

        // No callback: ask the bus for the byte. It replies through
        // `supply_dma_byte` before the next timer clock.
        self.dma_request = Some(self.sample_address);
    }

    /// Load a fetched byte into the sample buffer and advance the address.
    ///
    /// This is where the length counter moves — Mesen's `SetDmcReadBuffer`,
    /// called when the byte is *read out of memory* rather than when it
    /// finishes playing — so `$4015`'s bit 4 turns off while the last byte is
    /// still on its way out of the register, and the IRQ flag goes up at that
    /// same fetch.
    fn load_sample_byte(&mut self, byte: u8) {
        self.sample_buffer = byte;
        self.last_fetched_byte = byte;
        self.has_sample = true;

        self.sample_address = self.sample_address.wrapping_add(1);

        // Wrap address from $FFFF to $8000
        if self.sample_address == 0x0000 {
            self.sample_address = 0x8000;
        }

        if self.bytes_remaining > 0 {
            self.bytes_remaining -= 1;
        }

        if self.bytes_remaining == 0 {
            match self.play_mode {
                // A looped sample never ends: the next pass starts here.
                PlayMode::Loop => {
                    self.sample_address = self.sample_start_address;
                    self.bytes_remaining = self.sample_length;
                }
                PlayMode::Irq => self.irq_generated = true,
                PlayMode::Normal => {}
            }
        }

        // The byte waits in the buffer: the output unit takes it from there at
        // the next byte boundary (Mesen's `Run` does the move, not the fetch).
    }

    /// Address of the byte the channel is waiting for, if any.
    pub fn dma_request(&self) -> Option<u16> {
        self.dma_request
    }

    /// Take the pending fetch request, leaving the channel waiting for the
    /// byte the bus owes it.
    ///
    /// Mesen's halt latch is one-shot: the CPU clears `_needHalt` when it
    /// starts the transfer, so one request is answered by exactly one transfer.
    /// Taking the request here is what keeps the cycles the transfer itself
    /// takes — during which the APU keeps ticking — from arming it a second
    /// time, which would fetch (and charge the CPU for) the same byte twice.
    pub fn take_dma_request(&mut self) -> Option<u16> {
        self.dma_request.take()
    }

    /// Follow the console's TV system, whose APU has its own set of periods.
    pub fn set_region(&mut self, region: Region) {
        self.period_table = dmc_period_table(region);
        self.timer_period = self.period_table[0];
    }

    /// Reset the channel to its power-on state.
    pub fn reset(&mut self) {
        self.is_enabled = false;
        self.has_sample = false;
        self.irq_generated = false;
        self.play_mode = PlayMode::Normal;
        self.timer_period = self.period_table[0];
        self.timer_counter = 0;
        self.output_level = 0;
        self.sample_start_address = 0xC000;
        self.sample_address = 0xC000;
        self.sample_length = 1;
        self.bytes_remaining = 0;
        self.sample_buffer = 0;
        self.shift_register = 0;
        self.bits_remaining = 8;
        self.silence_flag = true;
        self.reg_4012 = 0;
        self.reg_4013 = 0;
        self.last_fetched_byte = 0;
        self.dma_request = None;
    }

    /// A soft reset: the sample's address and its length survive, since
    /// hardware only sets those at power-on (`NesApu::Reset(true)` and Mesen's
    /// `DeltaModulationChannel::Reset`, which keeps `_sampleAddr` and
    /// `_sampleLength`). Everything else — the reader, the buffer, the DAC, the
    /// IRQ and loop enables — restarts.
    pub fn soft_reset(&mut self) {
        let sample_start_address = self.sample_start_address;
        let sample_length = self.sample_length;
        let reg_4012 = self.reg_4012;
        let reg_4013 = self.reg_4013;

        self.reset();

        self.sample_start_address = sample_start_address;
        self.sample_length = sample_length;
        self.reg_4012 = reg_4012;
        self.reg_4013 = reg_4013;
    }

    /// Supply the byte requested through [`DmcChannel::dma_request`].
    pub fn supply_dma_byte(&mut self, value: u8) {
        self.dma_request = None;
        self.load_sample_byte(value);
    }

    /// Clear the pending IRQ flag.
    pub fn clear_irq(&mut self) {
        self.irq_generated = false;
    }

    /// Get the IRQ status
    pub fn irq_active(&self) -> bool {
        self.irq_generated
    }

    /// Get the bytes remaining status (for $4015 read)
    pub fn is_active(&self) -> bool {
        self.bytes_remaining > 0 && self.is_enabled
    }

    /// Enable or disable the channel
    pub fn set_enabled(&mut self, enabled: bool) {
        self.is_enabled = enabled;
    }

    /// Get the last fetched byte (for bus hijacking detection)
    pub fn last_fetched_byte(&self) -> u8 {
        self.last_fetched_byte
    }

    /// Write to $4010 - IRQ enable, loop, frequency
    fn write_4010(&mut self, value: u8) {
        // Bits 7-6: IRQ enable and loop flags
        let mode_bits = (value >> 6) & 0x03;
        self.play_mode = match mode_bits {
            0 => PlayMode::Normal,
            1 => PlayMode::Loop,
            2 => PlayMode::Irq,
            3 => PlayMode::Loop, // Both bits set = loop mode
            _ => unreachable!(),
        };

        // Bit 7 clear: clear IRQ flag
        if (value & 0x80) == 0 {
            self.irq_generated = false;
        }

        // Bits 3-0: frequency index
        let freq_index = (value & 0x0F) as usize;
        self.timer_period = self.period_table[freq_index];
    }

    /// Write to $4011 - DAC value
    fn write_4011(&mut self, value: u8) {
        // Bits 6-0: DAC output level (0-127)
        self.output_level = value & 0x7F;
    }

    /// Write to $4012 - sample address
    fn write_4012(&mut self, value: u8) {
        // Store raw register value
        self.reg_4012 = value;

        // Sample address = $C000 + (value * 64)
        self.sample_start_address = 0xC000 + ((value as u16) << 6);
    }

    /// Write to $4013 - sample length
    fn write_4013(&mut self, value: u8) {
        // Store raw register value
        self.reg_4013 = value;

        // Sample length = (value * 16) + 1 bytes
        self.sample_length = ((value as u16) << 4) + 1;
    }

    /// Write to $4015 - channel enable
    fn write_4015(&mut self, value: u8) {
        // Writing $4015 always clears the DMC IRQ flag first
        self.irq_generated = false;

        // Bit 4: DMC enable. The status register reports the channel as active
        // only while it is enabled *and* has bytes left, so this flag is what
        // `$4015`'s bit 4 follows.
        let dmc_enabled = (value & 0x10) != 0;
        self.is_enabled = dmc_enabled;

        if !dmc_enabled {
            // Disable: clear bytes remaining
            self.bytes_remaining = 0;
        } else {
            // Enable: the sample restarts when the counter is empty, and the
            // first byte is asked for right away — Mesen's `SetEnabled` and the
            // transfer delay that follows it. A byte already in the buffer is
            // not replaced (`StartDmcTransfer` refuses), which is what keeps a
            // restart from queueing two bytes behind a sample that is still
            // playing.
            if self.bytes_remaining == 0 {
                self.sample_address = self.sample_start_address;
                self.bytes_remaining = self.sample_length;

                if !self.has_sample && self.bytes_remaining > 0 {
                    self.fetch_next_sample();
                }
            }
        }
    }
}

impl AudioChannel for DmcChannel {
    fn tick(&mut self) {
        // The DMC channel ticks at CPU rate, not APU rate
        // This method is called for APU cycles, so we don't use it
        // Instead, clock_timer() should be called at CPU rate
    }

    fn get_output(&self) -> u8 {
        // Return the 7-bit DAC output (0-127)
        // This gets scaled appropriately by the mixer
        self.output_level
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4010 => self.write_4010(value),
            0x4011 => self.write_4011(value),
            0x4012 => self.write_4012(value),
            0x4013 => self.write_4013(value),
            0x4015 => self.write_4015(value),
            _ => {
                // Unknown register, ignore
            }
        }
    }

    fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0x4010 => {
                // Read $4010: return IRQ flag in bit 7
                if self.irq_generated {
                    0x80
                } else {
                    0
                }
            }
            0x4011 => {
                // Read $4011: return current DAC value
                self.output_level
            }
            0x4012 => {
                // Read $4012: return raw register value
                self.reg_4012
            }
            0x4013 => {
                // Read $4013: return raw register value
                self.reg_4013
            }
            _ => {
                // Unknown register, return 0
                0
            }
        }
    }
}

impl Default for DmcChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmc_channel_creation() {
        let dmc = DmcChannel::new();

        // Verify initial state
        assert!(!dmc.is_enabled);
        assert!(!dmc.has_sample);
        assert!(!dmc.irq_generated);
        assert_eq!(dmc.output_level, 0);
        assert_eq!(dmc.bytes_remaining, 0);
    }

    #[test]
    fn test_4010_frequency_write() {
        let mut dmc = DmcChannel::new();

        // Write frequency index 0
        dmc.write_register(0x4010, 0x00);
        assert_eq!(dmc.timer_period, DMC_PERIOD_TABLE[0]);

        // Write frequency index 15
        dmc.write_register(0x4010, 0x0F);
        assert_eq!(dmc.timer_period, DMC_PERIOD_TABLE[15]);

        // Write with loop flag set
        dmc.write_register(0x4010, 0x40);
        assert_eq!(dmc.play_mode, PlayMode::Loop);

        // Write with IRQ flag set
        dmc.write_register(0x4010, 0x80);
        assert_eq!(dmc.play_mode, PlayMode::Irq);
    }

    #[test]
    fn test_4010_irq_clear() {
        let mut dmc = DmcChannel::new();

        // Set IRQ flag
        dmc.irq_generated = true;

        // Write $4010 with bit 7 clear - should clear IRQ
        dmc.write_register(0x4010, 0x00);
        assert!(!dmc.irq_generated);
    }

    #[test]
    fn test_4011_dac_write() {
        let mut dmc = DmcChannel::new();

        // Write DAC value
        dmc.write_register(0x4011, 0x7F);
        assert_eq!(dmc.output_level, 0x7F);

        // Write with bit 7 set (should be masked)
        dmc.write_register(0x4011, 0xFF);
        assert_eq!(dmc.output_level, 0x7F);
    }

    #[test]
    fn test_4012_address_write() {
        let mut dmc = DmcChannel::new();

        // Write address register
        dmc.write_register(0x4012, 0x00);
        assert_eq!(dmc.sample_start_address, 0xC000);

        dmc.write_register(0x4012, 0x01);
        assert_eq!(dmc.sample_start_address, 0xC040);

        dmc.write_register(0x4012, 0xFF);
        assert_eq!(dmc.sample_start_address, 0xFFC0);
    }

    #[test]
    fn test_4013_length_write() {
        let mut dmc = DmcChannel::new();

        // Write length register
        dmc.write_register(0x4013, 0x00);
        assert_eq!(dmc.sample_length, 1);

        dmc.write_register(0x4013, 0x01);
        assert_eq!(dmc.sample_length, 17);

        dmc.write_register(0x4013, 0xFF);
        assert_eq!(dmc.sample_length, 4081);
    }

    #[test]
    fn test_4015_enable_disable() {
        let mut dmc = DmcChannel::new();

        // Set up sample parameters
        dmc.write_register(0x4012, 0x00); // Address = $C000
        dmc.write_register(0x4013, 0x01); // Length = 17 bytes

        // Enable DMC
        dmc.write_register(0x4015, 0x10);
        assert_eq!(dmc.bytes_remaining, 17);
        assert_eq!(dmc.sample_address, 0xC000);

        // Disable DMC
        dmc.write_register(0x4015, 0x00);
        assert_eq!(dmc.bytes_remaining, 0);
    }

    #[test]
    fn test_4015_irq_clear() {
        let mut dmc = DmcChannel::new();

        // Set IRQ flag
        dmc.irq_generated = true;

        // Write $4015 - should clear IRQ
        dmc.write_register(0x4015, 0x00);
        assert!(!dmc.irq_generated);
    }

    #[test]
    fn test_output_level_increase() {
        let mut dmc = DmcChannel::new();

        // Set up for testing output changes: the register is playing a byte,
        // which is what moves the level (a silent register holds it).
        dmc.output_level = 64;
        dmc.shift_register = 0xFF; // All 1s
        dmc.silence_flag = false;
        dmc.has_sample = true;
        dmc.bits_remaining = 8;

        // Clock the output unit (should increase by 2)
        dmc.clock_output_unit();
        assert_eq!(dmc.output_level, 66);
    }

    #[test]
    fn test_output_level_decrease() {
        let mut dmc = DmcChannel::new();

        // Set up for testing output changes: the register is playing a byte,
        // which is what moves the level (a silent register holds it).
        dmc.output_level = 64;
        dmc.shift_register = 0x00; // All 0s
        dmc.silence_flag = false;
        dmc.has_sample = true;
        dmc.bits_remaining = 8;

        // Clock the output unit (should decrease by 2)
        dmc.clock_output_unit();
        assert_eq!(dmc.output_level, 62);
    }

    #[test]
    fn test_output_level_clamp_high() {
        let mut dmc = DmcChannel::new();

        // Set output at maximum
        dmc.output_level = 126;
        dmc.shift_register = 0xFF; // All 1s
        dmc.silence_flag = false;
        dmc.has_sample = true;
        dmc.bits_remaining = 8;

        // Clock the output unit (should stay at 126)
        dmc.clock_output_unit();
        assert_eq!(dmc.output_level, 126);
    }

    #[test]
    fn test_output_level_clamp_low() {
        let mut dmc = DmcChannel::new();

        // Set output at minimum
        dmc.output_level = 0;
        dmc.shift_register = 0x00; // All 0s
        dmc.silence_flag = false;
        dmc.has_sample = true;
        dmc.bits_remaining = 8;

        // Clock the output unit (should stay at 0)
        dmc.clock_output_unit();
        assert_eq!(dmc.output_level, 0);
    }

    #[test]
    fn test_irq_active() {
        let mut dmc = DmcChannel::new();

        // Initially no IRQ
        assert!(!dmc.irq_active());

        // Set IRQ flag
        dmc.irq_generated = true;
        assert!(dmc.irq_active());
    }

    #[test]
    fn test_is_active() {
        let mut dmc = DmcChannel::new();

        // Not active when disabled
        dmc.bytes_remaining = 10;
        dmc.is_enabled = false;
        assert!(!dmc.is_active());

        // Not active when no bytes remaining
        dmc.bytes_remaining = 0;
        dmc.is_enabled = true;
        assert!(!dmc.is_active());

        // Active when enabled and bytes remaining
        dmc.bytes_remaining = 10;
        dmc.is_enabled = true;
        assert!(dmc.is_active());
    }

    #[test]
    fn test_read_registers() {
        let mut dmc = DmcChannel::new();

        // Set up some values
        dmc.irq_generated = true;
        dmc.output_level = 0x42;
        dmc.reg_4012 = 0x12;
        dmc.reg_4013 = 0x34;

        // Read registers
        assert_eq!(dmc.read_register(0x4010), 0x80); // IRQ flag in bit 7
        assert_eq!(dmc.read_register(0x4011), 0x42); // DAC value
        assert_eq!(dmc.read_register(0x4012), 0x12); // Raw $4012
        assert_eq!(dmc.read_register(0x4013), 0x34); // Raw $4013
    }

    #[test]
    fn test_dma_callback() {
        let mut dmc = DmcChannel::new();

        // Set up DMA callback
        dmc.set_dma_callback(|addr| {
            assert_eq!(addr, 0xC000);
            0xAB // Return test byte
        });

        // Set up for DMA
        dmc.sample_address = 0xC000;
        dmc.bytes_remaining = 1;

        // Fetch sample
        dmc.fetch_next_sample();

        // The fetched byte is what the channel will play, and it waits in the
        // buffer: the output unit takes it from there at the next byte
        // boundary, which is what lets `StartDmcTransfer` refuse to fetch while
        // a byte is already waiting (Mesen's `_bufferEmpty`).
        assert_eq!(dmc.sample_buffer, 0xAB);
        assert_eq!(dmc.last_fetched_byte, 0xAB);
        assert!(dmc.has_sample, "the byte waits in the buffer");
        assert_eq!(dmc.shift_register, 0, "and the register is untouched");
        assert_eq!(dmc.bytes_remaining, 0, "the fetch is what moves the count");

        // The output unit is what moves it into the register, at the byte
        // boundary.
        dmc.bits_remaining = 1;
        dmc.clock_output_unit();
        assert_eq!(dmc.shift_register, 0xAB);
        assert_eq!(dmc.bits_remaining, 8);
        assert!(!dmc.has_sample);
    }

    #[test]
    fn test_irq_callback() {
        let mut dmc = DmcChannel::new();

        // Set up IRQ callback
        dmc.set_irq_callback(|| {});

        // Set up for IRQ
        dmc.irq_generated = true;
        dmc.output_level = 64;
        dmc.shift_register = 0xFF;
        dmc.has_sample = true;
        dmc.bits_remaining = 1;

        // Clock output unit (should trigger IRQ callback)
        dmc.clock_output_unit();

        // Note: Due to closure capture limitations in tests,
        // we can't directly verify the callback was called,
        // but we can verify the IRQ flag is still set
        assert!(dmc.irq_generated);
    }

    #[test]
    fn test_address_wrap() {
        let mut dmc = DmcChannel::new();

        // Set address to $FFFF
        dmc.sample_address = 0xFFFF;

        // Set up DMA callback
        dmc.set_dma_callback(|_| 0x00);

        // Set up for fetch
        dmc.bytes_remaining = 1;

        // Fetch sample
        dmc.fetch_next_sample();

        // Address should wrap to $8000
        assert_eq!(dmc.sample_address, 0x8000);
    }

    /// The status bit follows the *fetches*, not the playback: blargg's
    /// `7-dmc_basics` check 19 reads `$4015` right after starting a one-byte
    /// sample and expects it stopped, and `8-dmc_rates` reads it where a
    /// 17-byte sample's own period puts the transition.
    #[test]
    fn test_the_status_bit_follows_the_fetches_not_the_playback() {
        let mut dmc = DmcChannel::new();
        dmc.set_dma_callback(|_| 0x00);
        dmc.write_register(0x4010, 0x0F); // fastest rate, no loop
        dmc.write_register(0x4013, 1); // 17 bytes
        dmc.timer_period = 10; // one output clock every 10 cycles
        dmc.timer_counter = 10;
        dmc.write_register(0x4015, 0x10); // start

        assert!(dmc.is_active(), "16 bytes are still to be fetched");

        // Sixteen byte boundaries: the last one fetches the 17th byte, and the
        // sample is over from there — while that byte is still playing.
        for _ in 0..16 {
            for _ in 0..8 * 10 {
                dmc.clock_timer();
            }
        }
        assert!(!dmc.is_active(), "the 17th fetch is what ends the sample");
        assert!(
            dmc.bits_remaining > 0,
            "and the last byte is still on its way out of the register"
        );
    }

    #[test]
    fn test_timer_clocking() {
        let mut dmc = DmcChannel::new();

        // Set timer period
        dmc.timer_period = 10;
        dmc.timer_counter = 10;

        // Set up output unit: a byte is playing, so the clocks move the level.
        dmc.output_level = 64;
        dmc.shift_register = 0xFF;
        dmc.silence_flag = false;
        dmc.has_sample = true;
        dmc.bits_remaining = 8;

        // A period is `period` CPU cycles, not `period + 1`: blargg's
        // `delay_dmc` counts 54*8 per byte at rate $F, and `8-dmc_rates` reads
        // the status bit where a 17-byte sample's own period puts it. The
        // counter runs 10 -> 0 and the transfer fires on the tenth clock.
        for _ in 0..9 {
            dmc.clock_timer();
        }
        assert_eq!(dmc.output_level, 64); // No change yet

        // The tenth clock triggers the output unit and reloads the counter.
        dmc.clock_timer();
        assert_eq!(dmc.output_level, 66); // Increased by 2
    }

    #[test]
    fn test_get_output() {
        let mut dmc = DmcChannel::new();

        // Initial output should be 0
        assert_eq!(dmc.get_output(), 0);

        // Set DAC value
        dmc.output_level = 0x42;
        assert_eq!(dmc.get_output(), 0x42);
    }

    #[test]
    fn test_audio_channel_trait() {
        let mut dmc = DmcChannel::new();

        // Test that DmcChannel implements AudioChannel
        dmc.write_register(0x4011, 0x42);
        assert_eq!(dmc.read_register(0x4011), 0x42);
        assert_eq!(dmc.get_output(), 0x42);
    }

    #[test]
    fn test_play_mode_transitions() {
        let mut dmc = DmcChannel::new();

        // Normal mode (bits 7-6 = 00)
        dmc.write_register(0x4010, 0x00);
        assert_eq!(dmc.play_mode, PlayMode::Normal);

        // Loop mode (bits 7-6 = 01)
        dmc.write_register(0x4010, 0x40);
        assert_eq!(dmc.play_mode, PlayMode::Loop);

        // IRQ mode (bits 7-6 = 10)
        dmc.write_register(0x4010, 0x80);
        assert_eq!(dmc.play_mode, PlayMode::Irq);

        // Both set (bits 7-6 = 11) = Loop mode
        dmc.write_register(0x4010, 0xC0);
        assert_eq!(dmc.play_mode, PlayMode::Loop);
    }

    /// PAL's APU runs on the other crystal, so its DMC has its own set of
    /// periods (Mesen's `_dmcPeriodLookupTablePal`).
    #[test]
    fn test_dmc_periods_follow_the_region() {
        let mut dmc = DmcChannel::new();
        dmc.set_region(Region::Ntsc);
        dmc.write_register(0x4010, 0x00);
        assert_eq!(dmc.timer_period, 428);
        dmc.write_register(0x4010, 0x0F);
        assert_eq!(dmc.timer_period, 54);

        let mut dmc = DmcChannel::new();
        dmc.set_region(Region::Pal);
        dmc.write_register(0x4010, 0x00);
        assert_eq!(dmc.timer_period, 398);
        dmc.write_register(0x4010, 0x0F);
        assert_eq!(dmc.timer_period, 50);
    }

    #[test]
    fn test_period_table_values() {
        // Verify the period table has correct NTSC values
        assert_eq!(DMC_PERIOD_TABLE[0], 428);
        assert_eq!(DMC_PERIOD_TABLE[7], 214);
        assert_eq!(DMC_PERIOD_TABLE[15], 54);
    }

    #[test]
    fn test_default_implementation() {
        let dmc: DmcChannel = Default::default();

        assert!(!dmc.is_enabled);
        assert_eq!(dmc.output_level, 0);
    }
}
