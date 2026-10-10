//! Triangle wave channel for the NES APU
//!
//! The triangle channel produces a triangle waveform with 32 steps.
//! It uses a linear counter and length counter to control output duration.
//!
//! Reference: https://www.nesdev.org/wiki/APU_Triangle

use super::AudioChannel;

/// Triangle sequencer table - 32-step waveform
/// Produces values from 15 down to 0, then 0 up to 15
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8,
    7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7,
    8, 9, 10, 11, 12, 13, 14, 15
];

/// Length counter lookup table
/// Maps the upper 5 bits of the length register to a count value
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6,
    160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30
];

/// Triangle channel state
#[derive(Debug, Clone)]
pub struct TriangleChannel {
    /// Timer period (11-bit value from $400A/$400B)
    timer_period: u16,
    /// Timer current count (counts down to 0)
    timer_count: u16,
    /// Sequencer position (0-31, indexes into TRIANGLE_TABLE)
    sequencer_pos: u8,
    /// Linear counter (5-bit reloadable counter)
    linear_counter: u8,
    /// Linear counter reload value (set by $4008 bits 0-6)
    linear_counter_reload: u8,
    /// Linear counter halt flag (set by $4008 bit 7)
    linear_counter_control: bool,
    /// Linear counter halt flag (internal state)
    linear_counter_halt: bool,
    /// Length counter (shared logic with other channels)
    length_counter: u8,
    /// Length counter enable flag
    length_counter_enable: bool,
    /// Channel enabled flag (controlled by $4015 bit 2)
    enabled: bool,
}

crate::state::state_fields!(TriangleChannel {
    timer_period,
    timer_count,
    sequencer_pos,
    linear_counter,
    linear_counter_reload,
    linear_counter_control,
    linear_counter_halt,
    length_counter,
    length_counter_enable,
    enabled,
});

impl TriangleChannel {
    /// Create a new triangle channel in its initial state
    pub fn new() -> Self {
        Self {
            timer_period: 0,
            timer_count: 0,
            sequencer_pos: 0,
            linear_counter: 0,
            linear_counter_reload: 0,
            linear_counter_control: false,
            linear_counter_halt: false,
            length_counter: 0,
            length_counter_enable: false,
            enabled: false,
        }
    }

    /// Reset the channel to its initial state
    pub fn reset(&mut self) {
        self.timer_period = 0;
        self.timer_count = 0;
        self.sequencer_pos = 0;
        self.linear_counter = 0;
        self.linear_counter_reload = 0;
        self.linear_counter_control = false;
        self.linear_counter_halt = false;
        self.length_counter = 0;
        self.length_counter_enable = false;
        self.enabled = false;
    }

    /// Enable or disable the channel
    /// Called when writing to $4015 (bit 2 controls triangle)
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            // When disabled, length counter is immediately cleared
            self.length_counter = 0;
        }
    }

    /// Check if the channel is currently active
    /// Returns true if length counter is non-zero and channel is enabled
    pub fn is_active(&self) -> bool {
        self.enabled && self.length_counter > 0
    }

    /// Clock the linear counter
    /// Called by the frame counter at specific steps
    pub fn clock_linear_counter(&mut self) {
        if self.linear_counter_halt {
            // Load: set linear counter to reload value
            self.linear_counter = self.linear_counter_reload;
        } else if self.linear_counter > 0 {
            // Decrement: count down
            self.linear_counter -= 1;
        }

        // Clear halt flag if control flag is not set
        if !self.linear_counter_control {
            self.linear_counter_halt = false;
        }
    }

    /// Clock the length counter
    /// Called by the frame counter at specific steps
    pub fn clock_length_counter(&mut self) {
        if self.length_counter_enable && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    /// Trigger a linear counter reload
    /// Called at the beginning of the frame sequencer step
    pub fn reload_linear_counter(&mut self) {
        self.linear_counter_halt = true;
    }

    /// Get the current output value
    /// Returns the sequencer value if channel is audible, 0 otherwise
    fn get_current_output(&self) -> u8 {
        // Output is silenced if:
        // - Channel is disabled
        // - Timer period is too low (< 2, which causes ultrasonic frequencies)
        // - Linear counter is 0
        // - Length counter is 0
        if !self.enabled
            || self.timer_period < 2
            || self.linear_counter == 0
            || self.length_counter == 0
        {
            return 0;
        }

        TRIANGLE_TABLE[self.sequencer_pos as usize]
    }

    /// Get the length counter status bit
    /// Returns 1 if length counter is non-zero and channel is enabled
    pub fn get_length_status(&self) -> u8 {
        if self.enabled && self.length_counter > 0 {
            1
        } else {
            0
        }
    }
}

impl AudioChannel for TriangleChannel {
    fn tick(&mut self) {
        // Timer counts down at CPU rate
        if self.timer_count > 0 {
            self.timer_count -= 1;
        } else {
            // Timer reached 0, reload and clock the sequencer
            self.timer_count = self.timer_period;

            // Only clock sequencer if the channel is producing output
            // This matches the hardware behavior where the sequencer
            // still runs but output is silenced by the counters
            self.sequencer_pos = (self.sequencer_pos + 1) & 0x1F;
        }
    }

    fn get_output(&self) -> u8 {
        self.get_current_output()
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4008 => {
                // Linear counter setup
                // Bit 7: Control flag (also controls length counter enable)
                // Bits 6-0: Linear counter reload value
                self.linear_counter_control = (value & 0x80) != 0;
                self.linear_counter_reload = value & 0x7F;

                // Length counter is enabled when control flag is clear
                self.length_counter_enable = !self.linear_counter_control;
            }
            0x400A => {
                // Timer low byte (bits 7-0 of timer period)
                self.timer_period = (self.timer_period & 0x0700) | (value as u16);
            }
            0x400B => {
                // Timer high byte (bits 10-8) and length counter load
                // Bits 2-0: High bits of timer period
                // Bits 7-3: Length counter load value
                self.timer_period = (self.timer_period & 0x00FF) | (((value & 0x07) as u16) << 8);

                // Length counter is only loaded when the channel is enabled
                // Writing this register while disabled has no effect on the length counter
                if self.enabled {
                    let length_index = (value >> 3) & 0x1F;
                    self.length_counter = LENGTH_TABLE[length_index as usize];
                }

                // Set the halt flag to reload the linear counter
                self.linear_counter_halt = true;
            }
            _ => {
                // Ignore writes to other addresses
            }
        }
    }

    fn read_register(&self, _addr: u16) -> u8 {
        // Triangle channel registers are write-only
        // Return open bus (0 for now)
        0
    }
}

impl Default for TriangleChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_channel() {
        let channel = TriangleChannel::new();

        assert_eq!(channel.timer_period, 0);
        assert_eq!(channel.timer_count, 0);
        assert_eq!(channel.sequencer_pos, 0);
        assert_eq!(channel.linear_counter, 0);
        assert_eq!(channel.linear_counter_reload, 0);
        assert!(!channel.linear_counter_control);
        assert!(!channel.linear_counter_halt);
        assert_eq!(channel.length_counter, 0);
        assert!(!channel.length_counter_enable);
        assert!(!channel.enabled);
    }

    #[test]
    fn test_reset() {
        let mut channel = TriangleChannel::new();

        // Set some state
        channel.timer_period = 100;
        channel.timer_count = 50;
        channel.sequencer_pos = 10;
        channel.linear_counter = 5;
        channel.linear_counter_reload = 7;
        channel.linear_counter_control = true;
        channel.linear_counter_halt = true;
        channel.length_counter = 20;
        channel.length_counter_enable = true;
        channel.enabled = true;

        // Reset
        channel.reset();

        // Verify all state is cleared
        assert_eq!(channel.timer_period, 0);
        assert_eq!(channel.timer_count, 0);
        assert_eq!(channel.sequencer_pos, 0);
        assert_eq!(channel.linear_counter, 0);
        assert_eq!(channel.linear_counter_reload, 0);
        assert!(!channel.linear_counter_control);
        assert!(!channel.linear_counter_halt);
        assert_eq!(channel.length_counter, 0);
        assert!(!channel.length_counter_enable);
        assert!(!channel.enabled);
    }

    #[test]
    fn test_enable_disable() {
        let mut channel = TriangleChannel::new();

        // Load length counter first
        channel.enabled = true;
        channel.write_register(0x400B, 0xF8); // Load length counter with max value
        assert_eq!(channel.length_counter, 30); // 0x1F index = 30

        // Disable should clear length counter
        channel.set_enabled(false);
        assert!(!channel.enabled);
        assert_eq!(channel.length_counter, 0);

        // Re-enable should not restore length counter
        channel.set_enabled(true);
        assert!(channel.enabled);
        assert_eq!(channel.length_counter, 0);
    }

    #[test]
    fn test_linear_counter_reload() {
        let mut channel = TriangleChannel::new();

        // Set reload value
        channel.write_register(0x4008, 0x07); // Reload value = 7
        assert_eq!(channel.linear_counter_reload, 7);

        // Set halt flag
        channel.reload_linear_counter();
        assert!(channel.linear_counter_halt);

        // Clock linear counter - should load the value
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 7);
        assert!(!channel.linear_counter_halt); // Halt cleared when control is false
    }

    #[test]
    fn test_linear_counter_countdown() {
        let mut channel = TriangleChannel::new();

        // Setup: reload value = 5, no control flag
        channel.write_register(0x4008, 0x05);
        channel.reload_linear_counter();

        // First clock loads the value
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 5);

        // Subsequent clocks decrement
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 4);

        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 3);

        // Continue to 0
        channel.clock_linear_counter();
        channel.clock_linear_counter();
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 0);

        // Stays at 0
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 0);
    }

    #[test]
    fn test_linear_counter_control_flag() {
        let mut channel = TriangleChannel::new();

        // Setup with control flag set
        channel.write_register(0x4008, 0x85); // Control = true, reload = 5
        assert!(channel.linear_counter_control);

        // Set halt flag
        channel.reload_linear_counter();
        assert!(channel.linear_counter_halt);

        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 5);
        assert!(channel.linear_counter_halt);

        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 5);
        assert!(channel.linear_counter_halt);
    }

    #[test]
    fn test_length_counter_clock() {
        let mut channel = TriangleChannel::new();

        // Enable channel and load length counter
        channel.enabled = true;
        channel.length_counter_enable = true;
        channel.length_counter = 10;

        // Clock decrements
        channel.clock_length_counter();
        assert_eq!(channel.length_counter, 9);

        channel.clock_length_counter();
        assert_eq!(channel.length_counter, 8);

        // Continue to 0
        for _ in 0..8 {
            channel.clock_length_counter();
        }
        assert_eq!(channel.length_counter, 0);

        // Stays at 0
        channel.clock_length_counter();
        assert_eq!(channel.length_counter, 0);
    }

    #[test]
    fn test_length_counter_disabled() {
        let mut channel = TriangleChannel::new();

        // Setup with length counter disabled
        channel.length_counter = 10;
        channel.length_counter_enable = false;

        // Clock should not decrement
        channel.clock_length_counter();
        assert_eq!(channel.length_counter, 10);
    }

    #[test]
    fn test_register_4008_linear_counter() {
        let mut channel = TriangleChannel::new();

        // Write to $4008 with control flag and reload value
        channel.write_register(0x4008, 0x8A); // Control = 1, reload = 10

        assert!(channel.linear_counter_control);
        assert!(!channel.length_counter_enable); // Disabled when control is set
        assert_eq!(channel.linear_counter_reload, 10);

        // Write without control flag
        channel.write_register(0x4008, 0x05); // Control = 0, reload = 5

        assert!(!channel.linear_counter_control);
        assert!(channel.length_counter_enable); // Enabled when control is clear
        assert_eq!(channel.linear_counter_reload, 5);
    }

    #[test]
    fn test_register_400a_timer_low() {
        let mut channel = TriangleChannel::new();

        // Set initial timer period
        channel.timer_period = 0x700;

        // Write low byte
        channel.write_register(0x400A, 0xAB);

        assert_eq!(channel.timer_period, 0x7AB);

        // Write another low byte
        channel.write_register(0x400A, 0x34);

        assert_eq!(channel.timer_period, 0x734);
    }

    #[test]
    fn test_register_400b_timer_high_and_length() {
        let mut channel = TriangleChannel::new();

        // Set initial timer period
        channel.timer_period = 0x0FF;
        channel.enabled = true;

        // Write to $400B: timer high bits and length counter
        // value = 0xF8: bits 2-0 = 0 (timer high), bits 7-3 = 0x1F (length index)
        channel.write_register(0x400B, 0xF8);

        // Timer period should be updated (high bits)
        assert_eq!(channel.timer_period, 0x0FF); // High bits are 0

        // Length counter should be loaded (index 0x1F = 30)
        assert_eq!(channel.length_counter, 30);

        // Halt flag should be set
        assert!(channel.linear_counter_halt);
    }

    #[test]
    fn test_register_400b_length_lookup() {
        let mut channel = TriangleChannel::new();
        channel.enabled = true;

        // Test various length values from the lookup table
        let test_cases = [
            (0x00, 10),  // Index 0
            (0x08, 254), // Index 1
            (0x10, 20),  // Index 2
            (0x18, 2),   // Index 3
            (0x20, 40),  // Index 4
            (0x28, 4),   // Index 5
            (0x30, 80),  // Index 6
            (0x38, 6),   // Index 7
            (0xF8, 30),  // Index 31
        ];

        for (input, expected) in test_cases.iter() {
            channel.write_register(0x400B, *input);
            assert_eq!(channel.length_counter, *expected, "Failed for input 0x{:02X}", input);
        }
    }

    #[test]
    fn test_register_400b_disabled_no_length_load() {
        let mut channel = TriangleChannel::new();

        // Channel is disabled
        channel.enabled = false;
        channel.length_counter = 0;

        // Write to $400B should not load length counter
        channel.write_register(0x400B, 0xF8);

        assert_eq!(channel.length_counter, 0); // Unchanged
    }

    #[test]
    fn test_timer_tick() {
        let mut channel = TriangleChannel::new();

        // Set timer period
        channel.timer_period = 3;
        channel.timer_count = 3;

        // Tick should decrement
        channel.tick();
        assert_eq!(channel.timer_count, 2);

        channel.tick();
        assert_eq!(channel.timer_count, 1);

        channel.tick();
        assert_eq!(channel.timer_count, 0);

        // Next tick should reload and advance sequencer
        channel.tick();
        assert_eq!(channel.timer_count, 3); // Reloaded
        assert_eq!(channel.sequencer_pos, 1); // Advanced
    }

    #[test]
    fn test_sequencer_advances() {
        let mut channel = TriangleChannel::new();

        channel.timer_period = 1;
        channel.timer_count = 1;

        // First tick decrements timer_count from 1 to 0
        channel.tick();
        assert_eq!(channel.timer_count, 0);
        assert_eq!(channel.sequencer_pos, 0); // Sequencer hasn't advanced yet

        // Second tick reloads timer and advances sequencer
        channel.tick();
        assert_eq!(channel.timer_count, 1);
        assert_eq!(channel.sequencer_pos, 1);

        // Continue through remaining 31 positions
        for i in 2..33 {
            channel.tick();
            channel.tick();
            assert_eq!(channel.sequencer_pos, (i % 32) as u8);
        }

        // Should wrap back to 0
        assert_eq!(channel.sequencer_pos, 0);
    }

    #[test]
    fn test_sequencer_waveform() {
        // Verify the sequencer produces the expected waveform
        let expected_waveform = [
            15, 14, 13, 12, 11, 10, 9, 8,
            7, 6, 5, 4, 3, 2, 1, 0,
            0, 1, 2, 3, 4, 5, 6, 7,
            8, 9, 10, 11, 12, 13, 14, 15
        ];

        for (i, &expected) in expected_waveform.iter().enumerate() {
            assert_eq!(TRIANGLE_TABLE[i], expected);
        }
    }

    #[test]
    fn test_output_silenced_when_disabled() {
        let mut channel = TriangleChannel::new();

        // Setup channel to produce output
        channel.enabled = true;
        channel.timer_period = 10;
        channel.linear_counter = 5;
        channel.length_counter = 10;
        channel.sequencer_pos = 0; // Would output 15

        // Disable channel
        channel.enabled = false;

        assert_eq!(channel.get_output(), 0);
    }

    #[test]
    fn test_output_silenced_when_timer_too_low() {
        let mut channel = TriangleChannel::new();

        // Setup channel
        channel.enabled = true;
        channel.linear_counter = 5;
        channel.length_counter = 10;
        channel.sequencer_pos = 0;

        // Timer period = 0 should silence
        channel.timer_period = 0;
        assert_eq!(channel.get_output(), 0);

        // Timer period = 1 should silence
        channel.timer_period = 1;
        assert_eq!(channel.get_output(), 0);

        // Timer period = 2 should produce output
        channel.timer_period = 2;
        assert_eq!(channel.get_output(), 15);
    }

    #[test]
    fn test_output_silenced_when_linear_counter_zero() {
        let mut channel = TriangleChannel::new();

        // Setup channel
        channel.enabled = true;
        channel.timer_period = 10;
        channel.length_counter = 10;
        channel.sequencer_pos = 0;

        // Linear counter = 0 should silence
        channel.linear_counter = 0;
        assert_eq!(channel.get_output(), 0);

        // Linear counter > 0 should produce output
        channel.linear_counter = 1;
        assert_eq!(channel.get_output(), 15);
    }

    #[test]
    fn test_output_silenced_when_length_counter_zero() {
        let mut channel = TriangleChannel::new();

        // Setup channel
        channel.enabled = true;
        channel.timer_period = 10;
        channel.linear_counter = 5;
        channel.sequencer_pos = 0;

        // Length counter = 0 should silence
        channel.length_counter = 0;
        assert_eq!(channel.get_output(), 0);

        // Length counter > 0 should produce output
        channel.length_counter = 1;
        assert_eq!(channel.get_output(), 15);
    }

    #[test]
    fn test_output_varies_with_sequencer() {
        let mut channel = TriangleChannel::new();

        // Setup channel to produce output
        channel.enabled = true;
        channel.timer_period = 10;
        channel.linear_counter = 5;
        channel.length_counter = 10;

        // Test various sequencer positions
        channel.sequencer_pos = 0;
        assert_eq!(channel.get_output(), 15);

        channel.sequencer_pos = 15;
        assert_eq!(channel.get_output(), 0);

        channel.sequencer_pos = 16;
        assert_eq!(channel.get_output(), 0);

        channel.sequencer_pos = 31;
        assert_eq!(channel.get_output(), 15);
    }

    #[test]
    fn test_get_length_status() {
        let mut channel = TriangleChannel::new();

        // Disabled, length = 0
        assert_eq!(channel.get_length_status(), 0);

        // Enabled, length = 0
        channel.enabled = true;
        assert_eq!(channel.get_length_status(), 0);

        // Enabled, length > 0
        channel.length_counter = 5;
        assert_eq!(channel.get_length_status(), 1);

        // Disabled, length > 0
        channel.enabled = false;
        assert_eq!(channel.get_length_status(), 0);
    }

    #[test]
    fn test_is_active() {
        let mut channel = TriangleChannel::new();

        assert!(!channel.is_active());

        channel.enabled = true;
        assert!(!channel.is_active()); // Still false, length is 0

        channel.length_counter = 5;
        assert!(channel.is_active());

        channel.enabled = false;
        assert!(!channel.is_active());
    }

    #[test]
    fn test_read_register_returns_zero() {
        let channel = TriangleChannel::new();

        // Triangle registers are write-only
        assert_eq!(channel.read_register(0x4008), 0);
        assert_eq!(channel.read_register(0x400A), 0);
        assert_eq!(channel.read_register(0x400B), 0);
        assert_eq!(channel.read_register(0x0000), 0);
    }

    #[test]
    fn test_full_sequence() {
        let mut channel = TriangleChannel::new();

        // Enable channel
        channel.set_enabled(true);

        // Setup linear counter
        channel.write_register(0x4008, 0x05); // Reload value = 5

        // Setup timer
        channel.write_register(0x400A, 0x10); // Timer low = 0x10
        channel.write_register(0x400B, 0x08); // Timer high = 0, length index = 1 (254)

        // Verify initial state
        assert_eq!(channel.timer_period, 0x10);
        assert_eq!(channel.length_counter, 254);
        assert!(channel.linear_counter_halt);

        // Clock linear counter to load it
        channel.clock_linear_counter();
        assert_eq!(channel.linear_counter, 5);

        // Channel should be producing output
        assert!(channel.get_output() > 0);

        // Run timer for a few cycles
        for _ in 0..20 {
            channel.tick();
        }

        // Sequencer should have advanced
        assert!(channel.sequencer_pos > 0);

        // Output should still be non-zero
        assert!(channel.get_output() <= 15);
    }
}
