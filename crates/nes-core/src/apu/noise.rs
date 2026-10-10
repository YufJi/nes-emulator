//! APU Noise Channel
//!
//! The noise channel produces pseudo-random noise using a Linear Feedback Shift Register (LFSR).
//! It supports envelope control and length counting like the pulse channels.
//!
//! Reference: https://www.nesdev.org/wiki/APU_Noise

use super::AudioChannel;

/// Noise timer period lookup table (NTSC)
/// These are the timer reload values for each of the 16 period settings.
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// Length counter lookup table
/// Maps the 5-bit length index to actual length counter values.
const LENGTH_LOOKUP: [u8; 32] = [
    0x0A, 0xFE, 0x14, 0x02, 0x28, 0x04, 0x50, 0x06, 0xA0, 0x08, 0x3C, 0x0A, 0x0E, 0x0C, 0x1A,
    0x0E, 0x0C, 0x10, 0x18, 0x12, 0x30, 0x14, 0x60, 0x16, 0xC0, 0x18, 0x48, 0x1A, 0x10, 0x1C,
    0x20, 0x1E,
];

/// Noise channel state
///
/// The noise channel uses a 15-bit Linear Feedback Shift Register (LFSR) to generate
/// pseudo-random noise. The LFSR shifts right on each timer tick, with the feedback
/// bit computed from specific taps based on the mode flag.
pub struct NoiseChannel {
    /// Whether the channel is enabled (controlled via $4015)
    enabled: bool,

    /// Programmable timer current count
    timer_count: u16,
    /// Programmable timer maximum value (from period table)
    timer_max: u16,

    /// 15-bit Linear Feedback Shift Register
    /// Initialized to 1 (must be non-zero for proper operation)
    shift_reg: u16,

    /// Random mode flag (bit 7 of $400E)
    /// false = Mode 0: feedback = bit0 XOR bit1
    /// true = Mode 1: feedback = bit0 XOR bit6
    mode_flag: bool,

    /// Length counter value (0-254)
    length_counter: u8,
    /// Whether length counter is enabled (inverted bit 5 of $400C)
    length_counter_enabled: bool,

    // Envelope generator state
    /// Envelope decay rate (4 bits from $400C)
    env_decay_rate: u8,
    /// Envelope decay counter (counts down to trigger volume change)
    env_decay_counter: u8,
    /// Current envelope volume (4 bits, 0-15)
    env_volume: u8,
    /// Whether envelope is disabled (bit 4 of $400C)
    /// When true, use env_decay_rate directly as volume
    env_decay_disable: bool,
    /// Whether envelope loops (bit 5 of $400C)
    env_loop_enable: bool,
    /// Flag to reset envelope on next clock
    env_reset: bool,

    /// Master volume output (0-15)
    /// Either env_volume or env_decay_rate depending on env_decay_disable
    master_volume: u8,

    /// Current output value (0 or master_volume, depending on LFSR bit 0)
    output: u8,
}

crate::state::state_fields!(NoiseChannel {
    enabled,
    timer_count,
    timer_max,
    shift_reg,
    mode_flag,
    length_counter,
    length_counter_enabled,
    env_decay_rate,
    env_decay_counter,
    env_volume,
    env_decay_disable,
    env_loop_enable,
    env_reset,
    master_volume,
    output,
});

impl NoiseChannel {
    /// Create a new noise channel with default state
    pub fn new() -> Self {
        Self {
            enabled: false,
            timer_count: 0,
            timer_max: 0,
            shift_reg: 1, // Must be non-zero
            mode_flag: false,
            length_counter: 0,
            length_counter_enabled: false,
            env_decay_rate: 0,
            env_decay_counter: 0,
            env_volume: 0,
            env_decay_disable: false,
            env_loop_enable: false,
            env_reset: false,
            master_volume: 0,
            output: 0,
        }
    }

    /// Clock the length counter
    ///
    /// Called by the frame counter at specific intervals.
    /// Decrements the length counter if enabled and non-zero.
    /// Reset the channel to its power-on state.
    pub fn reset(&mut self) {
        self.enabled = false;
        self.timer_count = 0;
        self.timer_max = 0;
        self.shift_reg = 1;
        self.mode_flag = false;
        self.length_counter = 0;
        self.length_counter_enabled = false;
        self.env_decay_rate = 0;
        self.env_decay_counter = 0;
        self.env_volume = 0;
        self.env_decay_disable = false;
        self.env_loop_enable = false;
        self.env_reset = false;
        self.master_volume = 0;
        self.output = 0;
    }

    pub fn clock_length_counter(&mut self) {
        if self.length_counter_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            self.update_output();
        }
    }

    /// Clock the envelope generator
    ///
    /// Called by the frame counter at specific intervals.
    /// Updates the envelope volume based on decay settings.
    pub fn clock_envelope(&mut self) {
        if self.env_reset {
            // Reset envelope
            self.env_reset = false;
            self.env_decay_counter = self.env_decay_rate + 1;
            self.env_volume = 15;
        } else if self.env_decay_counter > 0 {
            // Decrement counter and check if it reached zero
            self.env_decay_counter -= 1;
            if self.env_decay_counter == 0 {
                // Decay counter reached zero, reload and update volume
                self.env_decay_counter = self.env_decay_rate + 1;
                if self.env_volume > 0 {
                    self.env_volume -= 1;
                } else if self.env_loop_enable {
                    self.env_volume = 15;
                }
            }
        } else {
            // Counter was 0 (shouldn't happen in normal operation), reload it
            self.env_decay_counter = self.env_decay_rate + 1;
        }

        // Update master volume based on envelope settings
        if self.env_decay_disable {
            self.master_volume = self.env_decay_rate;
        } else {
            self.master_volume = self.env_volume;
        }

        self.update_output();
    }

    /// Update the output value based on current state
    ///
    /// Output is master_volume if channel is enabled, length counter is non-zero,
    /// and LFSR bit 0 is 0 (inverted). Otherwise output is 0.
    fn update_output(&mut self) {
        if self.enabled && self.length_counter > 0 {
            // LFSR bit 0 is inverted: 0 becomes output, 1 becomes silence
            let lfsr_bit0 = (self.shift_reg & 1) as u8;
            if lfsr_bit0 == 0 {
                self.output = self.master_volume;
            } else {
                self.output = 0;
            }
        } else {
            self.output = 0;
        }
    }

    /// Set whether the channel is enabled
    ///
    /// Controlled by bit 3 of $4015. When disabled, the length counter is cleared.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
        self.update_output();
    }

    /// Check if the channel is active (for status register)
    ///
    /// Returns true if the channel is enabled and the length counter is non-zero.
    pub fn is_active(&self) -> bool {
        self.enabled && self.length_counter > 0
    }

    /// Get the length counter status bit
    ///
    /// Returns 1 if the length counter is non-zero and channel is enabled, 0 otherwise.
    pub fn get_length_status(&self) -> u8 {
        if self.is_active() {
            1
        } else {
            0
        }
    }

    /// Step the LFSR once
    ///
    /// Shifts the LFSR right and computes the feedback bit.
    /// Feedback is bit0 XOR bit1 (mode 0) or bit0 XOR bit6 (mode 1).
    fn step_lfsr(&mut self) {
        // Get bit 0 (will be shifted out)
        let bit0 = self.shift_reg & 1;

        // Compute feedback bit
        let feedback_bit = if self.mode_flag {
            // Mode 1: feedback = bit0 XOR bit6
            let bit6 = (self.shift_reg >> 6) & 1;
            bit0 ^ bit6
        } else {
            // Mode 0: feedback = bit0 XOR bit1
            let bit1 = (self.shift_reg >> 1) & 1;
            bit0 ^ bit1
        };

        // Shift right and insert feedback at bit 14
        self.shift_reg >>= 1;
        self.shift_reg &= 0x3FFF; // Clear bit 14
        self.shift_reg |= (feedback_bit as u16) << 14;
    }
}

impl AudioChannel for NoiseChannel {
    fn tick(&mut self) {
        // Timer counts down
        if self.timer_count > 0 {
            self.timer_count -= 1;
        } else {
            // Timer reached zero, reload and step LFSR
            self.timer_count = self.timer_max;
            self.step_lfsr();
            self.update_output();
        }
    }

    fn get_output(&self) -> u8 {
        self.output
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x400C => {
                // Volume/Envelope decay ($400C)
                // Bit 0-3: Envelope decay rate / constant volume
                // Bit 4:   Envelope decay disable (0 = use envelope, 1 = constant volume)
                // Bit 5:   Envelope loop / length counter disable
                // Bit 6-7: Unused

                self.env_decay_disable = (value & 0x10) != 0;
                self.env_decay_rate = value & 0x0F;
                self.env_loop_enable = (value & 0x20) != 0;
                self.length_counter_enabled = (value & 0x20) == 0;

                // Update master volume immediately
                if self.env_decay_disable {
                    self.master_volume = self.env_decay_rate;
                } else {
                    self.master_volume = self.env_volume;
                }

                self.update_output();
            }
            0x400E => {
                // Programmable timer / mode ($400E)
                // Bit 0-3: Timer period index (lookup in NOISE_PERIOD_TABLE)
                // Bit 6-7: Unused
                // Bit 7:   Mode flag (0 = mode 0, 1 = mode 1)

                let period_index = (value & 0x0F) as usize;
                self.timer_max = NOISE_PERIOD_TABLE[period_index];
                self.mode_flag = (value & 0x80) != 0;
            }
            0x400F => {
                // Length counter load / envelope reset ($400F)
                // Bit 3-7: Length counter load value (lookup in LENGTH_LOOKUP)
                // Bit 0-2: Unused
                // Writing this register resets the envelope

                // Length counter is only loaded if the channel is enabled
                // See https://www.nesdev.org/wiki/APU#Status_($4015)
                if self.enabled {
                    let length_index = ((value & 0xF8) >> 3) as usize;
                    self.length_counter = LENGTH_LOOKUP[length_index];
                }

                self.env_reset = true;
                self.update_output();
            }
            _ => {
                // Unknown register, ignore
            }
        }
    }

    fn read_register(&self, addr: u16) -> u8 {
        // Noise channel registers are write-only
        // Return open bus (0 for now)
        0
    }
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_channel_new() {
        let channel = NoiseChannel::new();
        assert_eq!(channel.get_output(), 0);
        assert!(!channel.is_active());
    }

    #[test]
    fn test_lfsr_mode_0() {
        // Mode 0: feedback = bit0 XOR bit1
        let mut channel = NoiseChannel::new();
        channel.mode_flag = false;
        channel.shift_reg = 0x0001; // bit0=1, bit1=0, feedback should be 1

        channel.step_lfsr();

        // After shift: bit14 should be 1 (feedback), rest shifted right
        // Original: 0b0000_0000_0000_0001
        // bit0=1, bit1=0, feedback=1
        // Result: 0b0100_0000_0000_0000 = 0x4000
        assert_eq!(channel.shift_reg, 0x4000);
    }

    #[test]
    fn test_lfsr_mode_1() {
        // Mode 1: feedback = bit0 XOR bit6
        let mut channel = NoiseChannel::new();
        channel.mode_flag = true;
        channel.shift_reg = 0x0041; // bit0=1, bit6=1, feedback should be 0

        channel.step_lfsr();

        // After shift: bit14 should be 0 (feedback), rest shifted right
        // Original: 0b0000_0000_0100_0001
        // bit0=1, bit6=1, feedback=0
        // Result: 0b0000_0000_0010_0000 = 0x0020
        assert_eq!(channel.shift_reg, 0x0020);
    }

    #[test]
    fn test_lfsr_output_inversion() {
        let mut channel = NoiseChannel::new();
        channel.enabled = true;
        channel.length_counter = 1;
        channel.master_volume = 15;

        // When LFSR bit0 is 0, output should be master_volume
        channel.shift_reg = 0x0000;
        channel.update_output();
        assert_eq!(channel.get_output(), 15);

        // When LFSR bit0 is 1, output should be 0
        channel.shift_reg = 0x0001;
        channel.update_output();
        assert_eq!(channel.get_output(), 0);
    }

    #[test]
    fn test_timer_period_table() {
        // Verify the period table values match expected NTSC values
        assert_eq!(NOISE_PERIOD_TABLE[0], 4);
        assert_eq!(NOISE_PERIOD_TABLE[1], 8);
        assert_eq!(NOISE_PERIOD_TABLE[15], 4068);
    }

    #[test]
    fn test_write_400c_envelope() {
        let mut channel = NoiseChannel::new();

        // Write to $400C: constant volume mode, volume = 10
        channel.write_register(0x400C, 0x1A); // bit 4 set (constant), volume = 10

        assert!(channel.env_decay_disable);
        assert_eq!(channel.env_decay_rate, 10);
        assert_eq!(channel.master_volume, 10);
    }

    #[test]
    fn test_write_400e_timer() {
        let mut channel = NoiseChannel::new();

        // Write to $400E: period index 5, mode 1
        channel.write_register(0x400E, 0x85); // bit 7 set (mode 1), index 5

        assert!(channel.mode_flag);
        assert_eq!(channel.timer_max, NOISE_PERIOD_TABLE[5]); // Should be 96
    }

    #[test]
    fn test_write_400f_length_counter() {
        let mut channel = NoiseChannel::new();
        channel.enabled = true;

        // Write to $400F: length index from upper 5 bits
        // value = 0xF8 -> upper 5 bits = 0x1F (31)
        channel.write_register(0x400F, 0xF8);

        assert_eq!(channel.length_counter, LENGTH_LOOKUP[31]); // Should be 0x1E (30)
        assert!(channel.env_reset);
    }

    #[test]
    fn test_write_400f_when_disabled() {
        let mut channel = NoiseChannel::new();
        channel.enabled = false;
        channel.length_counter = 0;

        // Writing $400F when disabled should not load length counter
        channel.write_register(0x400F, 0xF8);

        assert_eq!(channel.length_counter, 0); // Should remain 0
    }

    #[test]
    fn test_length_counter_clock() {
        let mut channel = NoiseChannel::new();
        channel.enabled = true;
        channel.length_counter_enabled = true;
        channel.length_counter = 5;
        channel.master_volume = 15;
        channel.shift_reg = 0; // Ensure output is non-zero

        // Clock length counter 5 times
        for _ in 0..5 {
            channel.clock_length_counter();
        }

        assert_eq!(channel.length_counter, 0);
        assert_eq!(channel.get_output(), 0); // Output should be silent now
    }

    #[test]
    fn test_envelope_clock() {
        let mut channel = NoiseChannel::new();
        channel.env_decay_rate = 2; // Decay every 2 clocks
        channel.env_decay_disable = false;
        channel.env_loop_enable = false;
        channel.env_reset = true;

        // First clock should reset envelope
        channel.clock_envelope();
        assert_eq!(channel.env_volume, 15);
        assert_eq!(channel.env_decay_counter, 3); // env_decay_rate + 1

        // Clock 3 times to trigger decay (rate=2 means 3 clocks: 3->2->1->0)
        channel.clock_envelope(); // 3 -> 2
        channel.clock_envelope(); // 2 -> 1
        channel.clock_envelope(); // 1 -> 0, triggers decay
        assert_eq!(channel.env_volume, 14);
    }

    #[test]
    fn test_envelope_loop() {
        let mut channel = NoiseChannel::new();
        channel.env_decay_rate = 0; // Decay every clock
        channel.env_decay_disable = false;
        channel.env_loop_enable = true;
        channel.env_volume = 0;
        channel.env_decay_counter = 1; // Counter at 1, will expire on next clock

        // Clock envelope - counter goes 1->0, triggers reload and volume wrap
        channel.clock_envelope();
        assert_eq!(channel.env_volume, 15);
    }

    #[test]
    fn test_tick_timer() {
        let mut channel = NoiseChannel::new();
        channel.timer_max = 3;
        channel.timer_count = 3;
        channel.shift_reg = 0x0001;

        // Tick 3 times - timer should count down
        channel.tick();
        assert_eq!(channel.timer_count, 2);
        channel.tick();
        assert_eq!(channel.timer_count, 1);
        channel.tick();
        assert_eq!(channel.timer_count, 0);

        // Next tick should reload timer and step LFSR
        let old_shift_reg = channel.shift_reg;
        channel.tick();
        assert_eq!(channel.timer_count, 3); // Reloaded
        assert_ne!(channel.shift_reg, old_shift_reg); // LFSR stepped
    }

    #[test]
    fn test_set_enabled() {
        let mut channel = NoiseChannel::new();
        channel.enabled = true;
        channel.length_counter = 10;

        // Disable channel
        channel.set_enabled(false);
        assert!(!channel.enabled);
        assert_eq!(channel.length_counter, 0);

        // Enable channel
        channel.set_enabled(true);
        assert!(channel.enabled);
        // Length counter should remain 0 until $400F is written
        assert_eq!(channel.length_counter, 0);
    }

    #[test]
    fn test_get_length_status() {
        let mut channel = NoiseChannel::new();

        // Disabled channel
        assert_eq!(channel.get_length_status(), 0);

        // Enabled but length counter is 0
        channel.enabled = true;
        assert_eq!(channel.get_length_status(), 0);

        // Enabled with non-zero length counter
        channel.length_counter = 5;
        assert_eq!(channel.get_length_status(), 1);
    }

    #[test]
    fn test_audio_channel_trait() {
        let mut channel = NoiseChannel::new();

        // Test that NoiseChannel implements AudioChannel
        channel.write_register(0x400C, 0x0F);
        channel.write_register(0x400E, 0x00);
        channel.write_register(0x400F, 0x00);

        // Enable and set up for output
        channel.set_enabled(true);
        channel.length_counter = 1;

        // Tick should not panic
        channel.tick();

        // Output should be in valid range
        let output = channel.get_output();
        assert!(output <= 15);
    }

    #[test]
    fn test_lfsr_periodicity() {
        // Test that LFSR cycles through all 32767 non-zero values
        let mut channel = NoiseChannel::new();
        channel.mode_flag = false;
        channel.shift_reg = 1;

        let initial = channel.shift_reg;
        let mut steps = 0;
        const MAX_STEPS: u32 = 40000; // Should cycle after 32767 steps

        loop {
            channel.step_lfsr();
            steps += 1;

            if channel.shift_reg == initial {
                break;
            }

            assert!(
                steps < MAX_STEPS,
                "LFSR did not cycle within expected period"
            );
            assert!(channel.shift_reg != 0, "LFSR should never become zero");
        }

        // Mode 0 LFSR has period of 32767 (2^15 - 1)
        assert_eq!(steps, 32767);
    }

    #[test]
    fn test_lfsr_never_zero() {
        // Verify LFSR never becomes zero (would lock up)
        let mut channel = NoiseChannel::new();
        channel.mode_flag = false;
        channel.shift_reg = 1;

        for _ in 0..40000 {
            channel.step_lfsr();
            assert!(channel.shift_reg != 0, "LFSR became zero!");
        }
    }

    #[test]
    fn test_read_register_returns_zero() {
        let channel = NoiseChannel::new();

        // Noise channel registers are write-only
        assert_eq!(channel.read_register(0x400C), 0);
        assert_eq!(channel.read_register(0x400E), 0);
        assert_eq!(channel.read_register(0x400F), 0);
    }
}
