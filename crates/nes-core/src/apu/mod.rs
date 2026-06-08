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
pub mod noise;
pub mod pulse;
pub mod triangle;

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameCounterMode {
    /// 4-step sequence (fires IRQ on step 4)
    FourStep,
    /// 5-step sequence (no IRQ)
    FiveStep,
}

impl Default for FrameCounterMode {
    fn default() -> Self {
        FrameCounterMode::FourStep
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
}

impl FrameCounter {
    /// Create a new frame counter
    pub fn new() -> Self {
        Self {
            cycle: 0,
            step: 0,
        }
    }
    
    /// Reset the frame counter
    pub fn reset(&mut self) {
        self.cycle = 0;
        self.step = 0;
    }
    
    /// Tick the frame counter by one CPU cycle
    /// Returns true if a step boundary was crossed
    pub fn tick(&mut self, mode: FrameCounterMode) -> bool {
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
    /// Frame counter IRQ inhibit flag
    pub frame_counter_irq_inhibit: bool,
    
    // Channels (placeholder for future implementation)
    // pub pulse1: PulseChannel,
    // pub pulse2: PulseChannel,
    // pub triangle: TriangleChannel,
    // pub noise: NoiseChannel,
    // pub dmc: DmcChannel,
    
    /// Lookup table for pulse channel mixing (0-30)
    pulse_table: [f32; 31],
    /// Lookup table for triangle/noise/DMC mixing (0-202)
    tnd_table: [f32; 203],
}

impl Apu {
    /// Create a new APU instance
    pub fn new() -> Self {
        let mut apu = Self {
            frame_counter: FrameCounter::new(),
            frame_counter_mode: FrameCounterMode::FourStep,
            frame_counter_irq: false,
            frame_counter_irq_inhibit: false,
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
                
                // Frame IRQ flag (bit 6)
                if self.frame_counter_irq {
                    status |= 0x40;
                    // Reading $4015 clears the frame IRQ flag
                    self.frame_counter_irq = false;
                }
                
                // TODO: Add channel status bits when channels are implemented
                // status |= if self.pulse1.is_active() { 0x01 } else { 0 };
                // status |= if self.pulse2.is_active() { 0x02 } else { 0 };
                // status |= if self.triangle.is_active() { 0x04 } else { 0 };
                // status |= if self.noise.is_active() { 0x08 } else { 0 };
                // status |= if self.dmc.is_active() { 0x10 } else { 0 };
                // status |= if self.dmc.irq_active() { 0x80 } else { 0 };
                
                status
            }
            _ => {
                // Most APU registers are write-only
                // Return open bus (0 for now)
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
        match addr {
            0x4000..=0x4003 => {
                // Pulse 1 registers
                // TODO: Route to pulse1 channel when implemented
            }
            0x4004..=0x4007 => {
                // Pulse 2 registers
                // TODO: Route to pulse2 channel when implemented
            }
            0x4008..=0x400B => {
                // Triangle registers
                // TODO: Route to triangle channel when implemented
            }
            0x400C..=0x400F => {
                // Noise registers
                // TODO: Route to noise channel when implemented
            }
            0x4010..=0x4013 => {
                // DMC registers
                // TODO: Route to DMC channel when implemented
            }
            0x4015 => {
                // Channel enable/status register
                // TODO: Enable/disable channels when implemented
                // Bit 0: Pulse 1
                // Bit 1: Pulse 2
                // Bit 2: Triangle
                // Bit 3: Noise
                // Bit 4: DMC
            }
            0x4017 => {
                // Frame counter control
                // Bit 7: Mode (0 = 4-step, 1 = 5-step)
                // Bit 6: IRQ inhibit flag
                
                self.frame_counter_mode = if (value & 0x80) != 0 {
                    FrameCounterMode::FiveStep
                } else {
                    FrameCounterMode::FourStep
                };
                
                self.frame_counter_irq_inhibit = (value & 0x40) != 0;
                
                // If IRQ inhibit is set, clear the frame IRQ flag
                if self.frame_counter_irq_inhibit {
                    self.frame_counter_irq = false;
                }
                
                // Writing to $4017 resets the frame counter
                // If mode 1 (5-step), the frame counter immediately clocks all units
                self.frame_counter.reset();
                
                if self.frame_counter_mode == FrameCounterMode::FiveStep {
                    // In 5-step mode, immediately clock all units
                    // TODO: Clock envelope, sweep, and length counters
                }
            }
            _ => {
                // Unknown register, ignore
            }
        }
    }
    
    /// Tick the APU for one CPU cycle
    /// 
    /// This advances the frame counter and handles IRQ generation.
    pub fn tick(&mut self) {
        // Tick the frame counter
        let step_changed = self.frame_counter.tick(self.frame_counter_mode);
        
        if step_changed {
            match self.frame_counter_mode {
                FrameCounterMode::FourStep => {
                    // 4-step mode: Steps 0-3 clock various units
                    // Step 3 (index 3 at cycle 29828) sets IRQ flag
                    if self.frame_counter.step == 4 {
                        // This is the end of step 3 / beginning of step 0
                        // IRQ is set at cycle 29828 in 4-step mode
                        if !self.frame_counter_irq_inhibit {
                            self.frame_counter_irq = true;
                        }
                    }
                }
                FrameCounterMode::FiveStep => {
                    // 5-step mode: No IRQ
                    // Steps 0-4 clock various units
                }
            }
            
            // TODO: Clock envelope, sweep, and length counters based on step
        }
    }
    
    /// Reset the APU to its initial state
    pub fn reset(&mut self) {
        self.frame_counter.reset();
        self.frame_counter_mode = FrameCounterMode::FourStep;
        self.frame_counter_irq = false;
        self.frame_counter_irq_inhibit = false;
        
        // TODO: Reset all channels when implemented
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
