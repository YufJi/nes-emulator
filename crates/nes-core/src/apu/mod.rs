//! APU (Audio Processing Unit) module
//!
//! The APU generates audio for the NES using 5 channels:
//! - 2 Pulse/Square wave channels
//! - 1 Triangle wave channel
//! - 1 Noise channel
//! - 1 DMC (Delta Modulation Channel) for sample playback
//!
//! Reference: https://www.nesdev.org/wiki/APU

pub mod dmc;
pub mod mmc5;
pub mod noise;
pub mod pulse;
pub mod sunsoft5b;
pub mod triangle;
pub mod vrc6;

pub use dmc::DmcChannel;
pub use mmc5::Mmc5Audio;
pub use noise::NoiseChannel;
pub use pulse::PulseChannel;
pub use sunsoft5b::Sunsoft5bAudio;
pub use triangle::TriangleChannel;
pub use vrc6::Vrc6Audio;

/// AudioChannel trait defines the interface for all APU channels
pub trait AudioChannel {
    /// Tick the channel for one APU cycle
    fn tick(&mut self);

    /// Get the current output value (0-15 for most channels)
    fn get_output(&self) -> u8;

    /// Write to a channel register
    fn write_register(&mut self, addr: u16, value: u8);

    /// Read from a channel register
    fn read_register(&self, addr: u16) -> u8;
}

/// Frame counter mode - determines the sequence of clock events
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FrameCounterMode {
    /// 4-step sequence (fires IRQ on step 4)
    #[default]
    FourStep,
    /// 5-step sequence (no IRQ)
    FiveStep,
}

impl crate::state::StateField for FrameCounterMode {
    fn write(&self, w: &mut crate::state::StateWriter) {
        w.u8(match self {
            FrameCounterMode::FourStep => 0,
            FrameCounterMode::FiveStep => 1,
        });
    }
    fn read(&mut self, r: &mut crate::state::StateReader<'_>) {
        *self = match r.u8() {
            1 => FrameCounterMode::FiveStep,
            _ => FrameCounterMode::FourStep,
        };
    }
}

/// Frame counter state - manages the APU frame sequencer
///
/// The frame counter generates clock signals at specific intervals
/// to update envelope, sweep, and length counters.
#[derive(Debug, Clone)]
pub struct FrameCounter {
    /// Current cycle within the frame sequence
    pub cycle: u32,
    /// Current step in the sequence (0-4)
    pub step: u8,
    /// Cycles the counter is held for after a `$4017` restart.
    ///
    /// The write's effect on the *sequence* is late: the mode and the IRQ
    /// inhibit take effect at once, but the counter passes its next boundary
    /// three cycles later — four when the write lands on the APU's other clock
    /// phase. That jitter is what blargg's `4-jitter` measures, and it moves
    /// every later boundary with it, which is what `5-len_timing` measures.
    hold: u8,
}

crate::state::state_fields!(FrameCounter { cycle, step, hold });

impl FrameCounter {
    /// Create a new frame counter
    pub fn new() -> Self {
        Self {
            cycle: 0,
            step: 0,
            hold: 0,
        }
    }

    /// Hold the counter for `cycles` before it advances again.
    pub fn hold(&mut self, cycles: u8) {
        self.hold = cycles;
    }

    /// Reset the frame counter
    pub fn reset(&mut self) {
        self.cycle = 0;
        self.step = 0;
    }

    /// Tick the frame counter by one CPU cycle
    /// Returns true if a step boundary was crossed
    pub fn tick(&mut self, mode: FrameCounterMode) -> bool {
        if self.hold > 0 {
            self.hold -= 1;
            return false;
        }
        self.cycle += 1;

        let step_boundaries = match mode {
            FrameCounterMode::FourStep => [7457, 14913, 22371, 29828, 29829],
            FrameCounterMode::FiveStep => [7457, 14913, 22371, 29829, 37281],
        };

        let period = match mode {
            FrameCounterMode::FourStep => 29830,
            FrameCounterMode::FiveStep => 37282,
        };

        // Check if we crossed a step boundary
        let old_step = self.step;

        for (i, &boundary) in step_boundaries.iter().enumerate() {
            if self.cycle == boundary {
                self.step = i as u8 + 1;
                break;
            }
        }

        // Wrap around at end of period
        if self.cycle >= period {
            self.cycle = 0;
            self.step = 0;
        }

        self.step != old_step
    }
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Main APU structure
pub struct Apu {
    /// Frame counter state
    pub frame_counter: FrameCounter,
    /// Frame counter mode (4-step or 5-step)
    pub frame_counter_mode: FrameCounterMode,
    /// Frame counter IRQ flag
    pub frame_counter_irq: bool,
    /// Cycles the frame IRQ flag still has to be set for.
    ///
    /// The flag is not set once but three cycles in a row, starting 29831
    /// clocks after a `$4017` restart — blargg's `6-irq_flag_timing` reads it
    /// three times in a row and expects it to come back after each read.
    frame_counter_irq_window: u8,
    /// Frame counter IRQ inhibit flag
    pub frame_counter_irq_inhibit: bool,

    /// Pulse channel 1 ($4000-$4003)
    pub pulse1: PulseChannel,
    /// Pulse channel 2 ($4004-$4007)
    pub pulse2: PulseChannel,
    /// Triangle channel ($4008-$400B)
    pub triangle: TriangleChannel,
    /// Noise channel ($400C-$400F)
    pub noise: NoiseChannel,
    /// Delta modulation channel ($4010-$4013)
    pub dmc: DmcChannel,

    /// Lookup table for pulse channel mixing (0-30)
    pulse_table: [f32; 31],
    /// Lookup table for triangle/noise/DMC mixing (0-202)
    tnd_table: [f32; 203],
}

crate::state::state_fields!(Apu {
    frame_counter,
    frame_counter_mode,
    frame_counter_irq_window,
    frame_counter_irq,
    frame_counter_irq_inhibit,
    pulse1,
    pulse2,
    triangle,
    noise,
    dmc,
});

impl Apu {
    /// Create a new APU instance
    pub fn new() -> Self {
        let mut apu = Self {
            frame_counter: FrameCounter::new(),
            frame_counter_mode: FrameCounterMode::FourStep,
            frame_counter_irq_window: 0,
            frame_counter_irq: false,
            frame_counter_irq_inhibit: false,
            pulse1: PulseChannel::new(true, 0x4000),
            pulse2: PulseChannel::new(false, 0x4004),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
            pulse_table: [0.0; 31],
            tnd_table: [0.0; 203],
        };

        apu.init_lookup_tables();
        apu
    }

    /// Initialize the DAC lookup tables
    ///
    /// These tables are used to mix the output of all channels
    /// according to the NES APU hardware formula.
    fn init_lookup_tables(&mut self) {
        // Pulse channel lookup table
        // Formula: 95.52 / (8128.0 / pulse_output + 100.0)
        for i in 0..31 {
            self.pulse_table[i] = 95.52 / (8128.0 / (i as f32) + 100.0);
        }
        // Handle the 0 case (avoid division by zero)
        self.pulse_table[0] = 0.0;

        // Triangle/Noise/DMC lookup table
        // Formula: 163.67 / (24329.0 / tnd_output + 100.0)
        for i in 0..203 {
            self.tnd_table[i] = 163.67 / (24329.0 / (i as f32) + 100.0);
        }
        // Handle the 0 case
        self.tnd_table[0] = 0.0;
    }

    /// Read from an APU register
    ///
    /// # Arguments
    /// * `addr` - Register address ($4000-$4017)
    ///
    /// # Returns
    /// The value read from the register
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                // Status register
                // Bits: DMC IRQ | Frame IRQ | DMC active | Noise active | Triangle active | Pulse 2 active | Pulse 1 active
                let mut status = 0u8;

                if self.pulse1.is_active() {
                    status |= 0x01;
                }
                if self.pulse2.is_active() {
                    status |= 0x02;
                }
                if self.triangle.is_active() {
                    status |= 0x04;
                }
                if self.noise.is_active() {
                    status |= 0x08;
                }
                if self.dmc.is_active() {
                    status |= 0x10;
                }
                if self.dmc.irq_active() {
                    status |= 0x80;
                }

                // Frame IRQ flag (bit 6). A read clears the *frame* IRQ flag,
                // and only that one: the DMC's own flag is cleared by a write
                // to $4015, which is what blargg's `7-dmc_basics` checks.
                if self.frame_counter_irq {
                    status |= 0x40;
                }
                self.frame_counter_irq = false;

                status
            }
            _ => {
                // Most APU registers are write-only
                0
            }
        }
    }

    /// Write to an APU register
    ///
    /// # Arguments
    /// * `addr` - Register address ($4000-$4017)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.write_register_at(addr, value, false)
    }

    /// Write a register, telling the frame counter which APU clock phase the
    /// write landed on: a `$4017` restart is three cycles late, or four when it
    /// lands on the odd one.
    pub fn write_register_at(&mut self, addr: u16, value: u8, odd_apu_cycle: bool) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write_register(addr, value),
            0x4004..=0x4007 => self.pulse2.write_register(addr, value),
            0x4008..=0x400B => self.triangle.write_register(addr, value),
            0x400C..=0x400F => self.noise.write_register(addr, value),
            0x4010..=0x4013 => self.dmc.write_register(addr, value),
            0x4015 => {
                // Channel enable register.
                self.pulse1.set_enabled(value & 0x01 != 0);
                self.pulse2.set_enabled(value & 0x02 != 0);
                self.triangle.set_enabled(value & 0x04 != 0);
                self.noise.set_enabled(value & 0x08 != 0);
                // The DMC handles its own enable/disable and IRQ clearing.
                self.dmc.write_register(0x4015, value);
            }
            0x4017 => {
                // Frame counter control:
                //   bit 7 - mode (0 = 4-step, 1 = 5-step)
                //   bit 6 - IRQ inhibit
                let new_mode = if (value & 0x80) != 0 {
                    FrameCounterMode::FiveStep
                } else {
                    FrameCounterMode::FourStep
                };

                self.frame_counter.reset();
                self.frame_counter.hold(if odd_apu_cycle { 4 } else { 3 });
                self.frame_counter_mode = new_mode;
                self.frame_counter_irq_inhibit = (value & 0x40) != 0;

                if self.frame_counter_irq_inhibit {
                    self.frame_counter_irq = false;
                    self.frame_counter_irq_window = 0;
                }

                // Selecting 5-step mode immediately clocks the quarter and
                // half frame units.
                if new_mode == FrameCounterMode::FiveStep {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            _ => {
                // Unknown register, ignore
            }
        }
    }

    /// Tick the APU for one CPU cycle.
    ///
    /// Every channel is clocked at the CPU rate; the frame counter runs
    /// alongside it and drives the envelope, sweep and length units.
    ///
    /// If the DMC needs a sample byte it publishes the address through
    /// [`Apu::dmc_dma_request`]; the caller must answer with
    /// [`Apu::supply_dmc_byte`].
    pub fn tick(&mut self) {
        self.pulse1.tick();
        self.pulse2.tick();
        self.triangle.tick();
        self.noise.tick();
        self.dmc.clock_timer();

        // The window runs first, so the cycle the sequencer opens it on is the
        // first of the three the flag is set for.
        if self.frame_counter_irq_window > 0 {
            self.frame_counter_irq_window -= 1;
            if !self.frame_counter_irq_inhibit {
                self.frame_counter_irq = true;
            }
        }

        if self.frame_counter.tick(self.frame_counter_mode) {
            self.clock_frame_sequencer();
        }
    }

    /// Answer the DMC's outstanding DMA request with a byte read from CPU
    /// memory. Does nothing when no request is pending.
    pub fn supply_dmc_byte(&mut self, value: u8) {
        self.dmc.supply_dma_byte(value);
    }

    /// Address of the byte the DMC channel is waiting for, if any.
    pub fn dmc_dma_request(&self) -> Option<u16> {
        self.dmc.dma_request()
    }

    /// Clock the units driven by the current frame counter step.
    fn clock_frame_sequencer(&mut self) {
        match self.frame_counter.step {
            // 7457 (both modes): quarter frame
            1 => self.clock_quarter_frame(),
            // 14913 (both modes): quarter + half frame
            2 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            // 22371 (both modes): quarter frame
            3 => self.clock_quarter_frame(),
            // 29828 (4-step only): frame IRQ. The flag goes up here and stays
            // up for two more cycles, which is the "three times in a row" the
            // tests read back.
            4 => {
                if self.frame_counter_mode == FrameCounterMode::FourStep
                    && !self.frame_counter_irq_inhibit
                {
                    self.frame_counter_irq = true;
                    self.frame_counter_irq_window = 2;
                }
            }
            // 29829 (4-step) / 37281 (5-step): quarter + half frame
            5 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            _ => {}
        }
    }

    /// Clock the envelope units and the triangle's linear counter.
    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.noise.clock_envelope();
        self.triangle.clock_linear_counter();
    }

    /// Clock the length counters and sweep units.
    fn clock_half_frame(&mut self) {
        self.pulse1.clock_length_counter();
        self.pulse2.clock_length_counter();
        self.triangle.clock_length_counter();
        self.noise.clock_length_counter();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    /// Mix the channel outputs into one analogue sample.
    ///
    /// Uses the non-linear mixing formula from the NES documentation: silence
    /// is 0.0 and full scale is roughly 1.0.
    pub fn output(&self) -> f32 {
        let pulse_index = (self.pulse1.get_output() + self.pulse2.get_output()) as usize;
        let tnd_index = 3 * self.triangle.get_output() as usize
            + 2 * self.noise.get_output() as usize
            + self.dmc.get_output() as usize;

        self.pulse_table[pulse_index.min(self.pulse_table.len() - 1)]
            + self.tnd_table[tnd_index.min(self.tnd_table.len() - 1)]
    }

    /// Reset the APU to its power-on state.
    pub fn reset(&mut self) {
        self.frame_counter.reset();
        self.frame_counter_mode = FrameCounterMode::FourStep;
        self.frame_counter_irq = false;
        self.frame_counter_irq_inhibit = false;

        self.pulse1.reset();
        self.pulse2.reset();
        self.triangle.reset();
        self.noise.reset();
        self.dmc.reset();
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structure() {
        // Test that Apu can be instantiated
        let apu = Apu::new();

        // Verify initial state
        assert_eq!(apu.frame_counter.cycle, 0);
        assert_eq!(apu.frame_counter.step, 0);
        assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);
        assert!(!apu.frame_counter_irq);
        assert!(!apu.frame_counter_irq_inhibit);
    }

    #[test]
    fn test_frame_counter() {
        let mut apu = Apu::new();

        // Test frame counter ticking
        for _ in 0..100 {
            apu.tick();
        }

        // Frame counter should have advanced
        assert_eq!(apu.frame_counter.cycle, 100);

        // Test reset
        apu.reset();
        assert_eq!(apu.frame_counter.cycle, 0);
        assert_eq!(apu.frame_counter.step, 0);
    }

    #[test]
    fn test_frame_counter_mode_switch() {
        let mut apu = Apu::new();

        // Default is 4-step mode
        assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);

        // Switch to 5-step mode via register write
        apu.write_register(0x4017, 0x80);
        assert_eq!(apu.frame_counter_mode, FrameCounterMode::FiveStep);

        // Switch back to 4-step mode
        apu.write_register(0x4017, 0x00);
        assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);
    }

    #[test]
    fn test_frame_counter_irq_inhibit() {
        let mut apu = Apu::new();

        // Enable IRQ inhibit
        apu.write_register(0x4017, 0x40);
        assert!(apu.frame_counter_irq_inhibit);

        // Set IRQ flag manually
        apu.frame_counter_irq = true;

        // Writing with inhibit flag should clear IRQ
        apu.write_register(0x4017, 0x40);
        assert!(!apu.frame_counter_irq);
    }

    #[test]
    fn test_read_status_register() {
        let mut apu = Apu::new();

        // Read status register ($4015)
        let status = apu.read_register(0x4015);

        // Initially should be 0 (no channels active, no IRQ)
        assert_eq!(status, 0);

        // Set IRQ flag and read again
        apu.frame_counter_irq = true;
        let status_with_irq = apu.read_register(0x4015);

        // Bit 6 should be set
        assert_eq!(status_with_irq & 0x40, 0x40);

        // IRQ flag should be cleared after read
        assert!(!apu.frame_counter_irq);
    }

    #[test]
    fn test_lookup_tables_initialized() {
        let apu = Apu::new();

        // Verify lookup tables are initialized
        // Index 0 should be 0.0 (special case to avoid division by zero)
        assert_eq!(apu.pulse_table[0], 0.0);
        assert_eq!(apu.tnd_table[0], 0.0);

        // Non-zero indices should have calculated values
        assert!(apu.pulse_table[1] > 0.0);
        assert!(apu.tnd_table[1] > 0.0);
    }

    #[test]
    fn test_audio_channel_trait_mock() {
        // Test that we can create a mock AudioChannel
        struct MockChannel {
            output: u8,
        }

        impl AudioChannel for MockChannel {
            fn tick(&mut self) {
                self.output = (self.output + 1) % 16;
            }

            fn get_output(&self) -> u8 {
                self.output
            }

            fn write_register(&mut self, _addr: u16, value: u8) {
                self.output = value & 0x0F;
            }

            fn read_register(&self, _addr: u16) -> u8 {
                self.output
            }
        }

        let mut channel = MockChannel { output: 0 };

        // Test tick
        channel.tick();
        assert_eq!(channel.get_output(), 1);

        // Test write/read
        channel.write_register(0x4000, 0x0A);
        assert_eq!(channel.read_register(0x4000), 0x0A);
    }
}
