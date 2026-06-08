//! APU Pulse/Square wave channel implementation
//!
//! The NES APU has two pulse channels (Pulse 1 and Pulse 2) that generate
//! square wave audio with configurable duty cycle, sweep, envelope, and
//! length counter.
//!
//! Register map:
//! - Pulse 1: $4000-$4003
//! - Pulse 2: $4004-$4007
//!
//! Reference: https://www.nesdev.org/wiki/APU_Pulse

use super::AudioChannel;

/// Duty cycle lookup table
///
/// The NES pulse channel has 4 duty cycle modes (12.5%, 25%, 50%, 75%).
/// Each mode defines which of the 8 sequencer positions output high.
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 75%
];

/// Length counter lookup table
///
/// Maps the 5-bit length counter load value to the actual counter value.
const LENGTH_TABLE: [u8; 32] = [
    0x0A, 0xFE, 0x14, 0x02, 0x28, 0x04, 0x50, 0x06, 0xA0, 0x08, 0x3C, 0x0A, 0x0E, 0x0C, 0x1A,
    0x0E, 0x0C, 0x10, 0x18, 0x12, 0x30, 0x14, 0x60, 0x16, 0xC0, 0x18, 0x48, 0x1A, 0x10, 0x1C,
    0x20, 0x1E,
];

/// Envelope unit for pulse channels
///
/// The envelope generates a volume envelope that can either be constant
/// or decay over time. It can also loop to create a repeating envelope.
#[derive(Debug, Clone, Default)]
pub struct Envelope {
    /// Volume/envelope decay rate (4 bits)
    pub volume: u8,
    /// Whether to disable envelope decay (constant volume)
    pub constant_volume: bool,
    /// Whether to loop the envelope (also affects length counter halt)
    pub loop_flag: bool,
    /// Current decay level (4 bits, counts down)
    pub decay_level: u8,
    /// Divider counter for envelope timing
    pub divider: u8,
    /// Flag to reset the envelope on next clock
    pub reset_flag: bool,
}

impl Envelope {
    /// Create a new envelope unit
    pub fn new() -> Self {
        Self {
            volume: 0,
            constant_volume: false,
            loop_flag: false,
            decay_level: 0,
            divider: 0,
            reset_flag: false,
        }
    }

    /// Reset the envelope unit
    pub fn reset(&mut self) {
        self.volume = 0;
        self.constant_volume = false;
        self.loop_flag = false;
        self.decay_level = 0;
        self.divider = 0;
        self.reset_flag = false;
    }

    /// Clock the envelope unit
    ///
    /// Called at specific intervals by the frame counter.
    pub fn clock(&mut self) {
        if self.reset_flag {
            // Reset: reload divider and set decay level to 15
            self.reset_flag = false;
            self.divider = self.volume;
            self.decay_level = 15;
        } else if self.divider == 0 {
            // Divider expired: reload and update decay level
            self.divider = self.volume;
            if self.decay_level > 0 {
                self.decay_level -= 1;
            } else if self.loop_flag {
                self.decay_level = 15;
            }
        } else {
            // Normal operation: decrement divider
            self.divider = self.divider.saturating_sub(1);
        }
    }

    /// Get the current output volume (0-15)
    pub fn output(&self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay_level
        }
    }

    /// Write to the envelope register ($4000/$4004)
    ///
    /// Bits:
    /// - 0-3: Volume/envelope decay rate
    /// - 4:   Constant volume (1) / envelope decay (0)
    /// - 5:   Envelope loop / length counter halt
    /// - 6-7: Duty cycle
    pub fn write(&mut self, value: u8) {
        self.volume = value & 0x0F;
        self.constant_volume = (value & 0x10) != 0;
        self.loop_flag = (value & 0x20) != 0;
    }
}

/// Sweep unit for pulse channels
///
/// The sweep unit can adjust the timer period over time, creating
/// frequency sweep effects. It has separate behavior for Pulse 1
/// (subtract mode) and Pulse 2 (add mode in negate).
#[derive(Debug, Clone, Default)]
pub struct Sweep {
    /// Whether sweep is enabled
    pub enabled: bool,
    /// Divider period (3 bits)
    pub period: u8,
    /// Whether to negate the shift result
    pub negate: bool,
    /// Shift count (3 bits)
    pub shift: u8,
    /// Current divider counter
    pub divider: u8,
    /// Whether to reload the divider on next clock
    pub reload_flag: bool,
    /// Whether the channel is Pulse 1 (affects negate calculation)
    pub is_pulse1: bool,
    /// Shadow copy of timer period for sweep calculations
    pub timer_shadow: u16,
}

impl Sweep {
    /// Create a new sweep unit
    pub fn new(is_pulse1: bool) -> Self {
        Self {
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            divider: 0,
            reload_flag: false,
            is_pulse1,
            timer_shadow: 0,
        }
    }

    /// Reset the sweep unit
    pub fn reset(&mut self) {
        self.enabled = false;
        self.period = 0;
        self.negate = false;
        self.shift = 0;
        self.divider = 0;
        self.reload_flag = false;
        self.timer_shadow = 0;
    }

    /// Calculate the target timer value after sweep
    ///
    /// Returns None if the sweep would silence the channel.
    pub fn calculate_target(&self, timer_period: u16) -> Option<u16> {
        let shift_amount = self.shift;
        if shift_amount == 0 {
            return Some(timer_period);
        }

        let delta = timer_period >> shift_amount;

        let target = if self.negate {
            // Pulse 1 and Pulse 2 have different negate behavior
            // Pulse 1: subtract delta + 1
            // Pulse 2: subtract delta (no +1)
            if self.is_pulse1 {
                timer_period.wrapping_sub(delta).wrapping_sub(1)
            } else {
                timer_period.wrapping_sub(delta)
            }
        } else {
            timer_period + delta
        };

        // Sweep silences channel if target period > 0x7FF (11 bits)
        if target > 0x7FF {
            None
        } else {
            Some(target)
        }
    }

    /// Check if the sweep would silence the channel
    pub fn is_silenced(&self, timer_period: u16) -> bool {
        // Channel is silenced if timer period < 8 or sweep target > 0x7FF
        if timer_period < 8 {
            return true;
        }

        if self.enabled && self.shift > 0 {
            if let Some(target) = self.calculate_target(timer_period) {
                target > 0x7FF
            } else {
                true
            }
        } else {
            false
        }
    }

    /// Clock the sweep unit
    ///
    /// Called at specific intervals by the frame counter.
    /// Returns the new timer period if sweep updated it.
    pub fn clock(&mut self, timer_period: u16) -> Option<u16> {
        let mut result = None;

        if self.divider == 0 {
            // Divider expired
            if self.enabled && self.shift > 0 {
                if let Some(target) = self.calculate_target(timer_period) {
                    if target <= 0x7FF && timer_period >= 8 {
                        result = Some(target);
                    }
                }
            }
            self.divider = self.period;
        } else {
            self.divider -= 1;
        }

        if self.reload_flag {
            self.reload_flag = false;
            self.divider = self.period;
        }

        result
    }

    /// Write to the sweep register ($4001/$4005)
    ///
    /// Bits:
    /// - 0-2: Shift count
    /// - 3:   Negate
    /// - 4-6: Period
    /// - 7:   Enable
    pub fn write(&mut self, value: u8) {
        self.shift = value & 0x07;
        self.negate = (value & 0x08) != 0;
        self.period = (value >> 4) & 0x07;
        self.enabled = (value & 0x80) != 0;
        self.reload_flag = true;
    }
}

/// Length counter unit
///
/// The length counter silences the channel after a specified number
/// of frames, unless the halt flag is set.
#[derive(Debug, Clone, Default)]
pub struct LengthCounter {
    /// Current counter value
    pub counter: u8,
    /// Whether the counter is halted
    pub halt: bool,
    /// Whether the channel is enabled
    pub enabled: bool,
}

impl LengthCounter {
    /// Create a new length counter
    pub fn new() -> Self {
        Self {
            counter: 0,
            halt: false,
            enabled: false,
        }
    }

    /// Reset the length counter
    pub fn reset(&mut self) {
        self.counter = 0;
        self.halt = false;
        self.enabled = false;
    }

    /// Clock the length counter
    ///
    /// Called at specific intervals by the frame counter.
    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    /// Load a value into the length counter
    ///
    /// The lookup value is the upper 5 bits of the register write.
    pub fn load(&mut self, lookup_value: u8) {
        if self.enabled {
            self.counter = LENGTH_TABLE[(lookup_value >> 3) as usize];
        }
    }

    /// Set the enabled state
    ///
    /// When disabled, the counter is cleared to 0.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    /// Check if the channel is active (counter > 0)
    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}

/// Timer unit for pulse channels
///
/// The timer generates the frequency for the pulse wave by counting
/// down from a programmable period.
#[derive(Debug, Clone, Default)]
pub struct Timer {
    /// Timer period (11 bits)
    pub period: u16,
    /// Current counter value
    pub counter: u16,
}

impl Timer {
    /// Create a new timer
    pub fn new() -> Self {
        Self {
            period: 0,
            counter: 0,
        }
    }

    /// Reset the timer
    pub fn reset(&mut self) {
        self.period = 0;
        self.counter = 0;
    }

    /// Clock the timer
    ///
    /// Returns true when the timer expires and should clock the sequencer.
    pub fn clock(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period;
            true
        } else {
            self.counter -= 1;
            false
        }
    }

    /// Write the low 8 bits of the timer period
    pub fn write_low(&mut self, value: u8) {
        self.period = (self.period & 0x0700) | (value as u16);
    }

    /// Write the high 3 bits of the timer period
    pub fn write_high(&mut self, value: u8) {
        self.period = (self.period & 0x00FF) | (((value & 0x07) as u16) << 8);
    }
}

/// Sequencer for pulse channels
///
/// The sequencer steps through the duty cycle pattern to generate
/// the square wave output.
#[derive(Debug, Clone, Default)]
pub struct Sequencer {
    /// Current position in the duty cycle (0-7)
    pub position: u8,
    /// Current duty cycle mode (0-3)
    pub duty_mode: u8,
}

impl Sequencer {
    /// Create a new sequencer
    pub fn new() -> Self {
        Self {
            position: 0,
            duty_mode: 0,
        }
    }

    /// Reset the sequencer
    pub fn reset(&mut self) {
        self.position = 0;
        self.duty_mode = 0;
    }

    /// Clock the sequencer
    ///
    /// Advances to the next position in the duty cycle.
    pub fn clock(&mut self) {
        self.position = (self.position + 1) & 0x07;
    }

    /// Get the current output (0 or 1)
    pub fn output(&self) -> u8 {
        DUTY_TABLE[self.duty_mode as usize][self.position as usize]
    }

    /// Set the duty mode
    pub fn set_duty(&mut self, mode: u8) {
        self.duty_mode = mode & 0x03;
    }
}

/// Pulse channel implementation
///
/// Combines all the units (envelope, sweep, length counter, timer, sequencer)
/// to create a complete pulse channel.
#[derive(Debug, Clone)]
pub struct PulseChannel {
    /// Envelope unit
    pub envelope: Envelope,
    /// Sweep unit
    pub sweep: Sweep,
    /// Length counter unit
    pub length_counter: LengthCounter,
    /// Timer unit
    pub timer: Timer,
    /// Sequencer unit
    pub sequencer: Sequencer,
    /// Base register address ($4000 for Pulse 1, $4004 for Pulse 2)
    pub base_addr: u16,
}

impl PulseChannel {
    /// Create a new pulse channel
    ///
    /// # Arguments
    /// * `is_pulse1` - true for Pulse 1, false for Pulse 2 (affects sweep negate)
    /// * `base_addr` - Base register address ($4000 or $4004)
    pub fn new(is_pulse1: bool, base_addr: u16) -> Self {
        Self {
            envelope: Envelope::new(),
            sweep: Sweep::new(is_pulse1),
            length_counter: LengthCounter::new(),
            timer: Timer::new(),
            sequencer: Sequencer::new(),
            base_addr,
        }
    }

    /// Reset the pulse channel
    pub fn reset(&mut self) {
        self.envelope.reset();
        self.sweep.reset();
        self.length_counter.reset();
        self.timer.reset();
        self.sequencer.reset();
    }

    /// Clock the envelope unit
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    /// Clock the sweep unit
    pub fn clock_sweep(&mut self) {
        if let Some(new_period) = self.sweep.clock(self.timer.period) {
            self.timer.period = new_period;
        }
    }

    /// Clock the length counter
    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }

    /// Set the channel enabled state
    pub fn set_enabled(&mut self, enabled: bool) {
        self.length_counter.set_enabled(enabled);
    }

    /// Check if the channel is currently outputting sound
    pub fn is_active(&self) -> bool {
        self.length_counter.is_active()
    }

    /// Get the current output value (0-15)
    ///
    /// Returns 0 if the channel is silenced.
    fn get_output_internal(&self) -> u8 {
        // Channel is silenced if:
        // 1. Length counter is 0
        // 2. Timer period < 8
        // 3. Sweep would silence the channel
        if !self.length_counter.is_active() {
            return 0;
        }

        if self.timer.period < 8 {
            return 0;
        }

        if self.sweep.is_silenced(self.timer.period) {
            return 0;
        }

        // Output is volume * duty output
        let duty_output = self.sequencer.output();
        if duty_output == 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}

impl AudioChannel for PulseChannel {
    fn tick(&mut self) {
        // Clock the timer, and when it expires, clock the sequencer
        if self.timer.clock() {
            self.sequencer.clock();
        }
    }

    fn get_output(&self) -> u8 {
        self.get_output_internal()
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        let offset = addr - self.base_addr;
        match offset {
            0 => {
                // $4000/$4004: Duty, envelope, length counter halt
                // Bits:
                // - 0-3: Volume/envelope decay rate
                // - 4:   Constant volume
                // - 5:   Length counter halt / envelope loop
                // - 6-7: Duty cycle
                self.sequencer.set_duty(value >> 6);
                self.envelope.write(value);
                self.length_counter.halt = (value & 0x20) != 0;
            }
            1 => {
                // $4001/$4005: Sweep
                self.sweep.write(value);
            }
            2 => {
                // $4002/$4006: Timer low
                self.timer.write_low(value);
            }
            3 => {
                // $4003/$4007: Length counter load, timer high, envelope reset
                // Bits:
                // - 0-2: Timer high bits
                // - 3-7: Length counter load
                self.timer.write_high(value);
                self.length_counter.load(value & 0xF8);
                self.envelope.reset_flag = true;
                // Reset sequencer position
                self.sequencer.position = 0;
            }
            _ => {
                // Invalid offset, ignore
            }
        }
    }

    fn read_register(&self, addr: u16) -> u8 {
        let offset = addr - self.base_addr;
        match offset {
            0 => {
                // Read back duty/envelope settings
                let duty = (self.sequencer.duty_mode & 0x03) << 6;
                let loop_flag = if self.envelope.loop_flag { 0x20 } else { 0 };
                let constant = if self.envelope.constant_volume { 0x10 } else { 0 };
                duty | loop_flag | constant | (self.envelope.volume & 0x0F)
            }
            1 => {
                // Read back sweep settings
                let enabled = if self.sweep.enabled { 0x80 } else { 0 };
                let period = (self.sweep.period & 0x07) << 4;
                let negate = if self.sweep.negate { 0x08 } else { 0 };
                enabled | period | negate | (self.sweep.shift & 0x07)
            }
            2 => {
                // Timer low byte
                (self.timer.period & 0xFF) as u8
            }
            3 => {
                // Timer high bits
                ((self.timer.period >> 8) & 0x07) as u8
            }
            _ => 0,
        }
    }
}

/// Pulse 1 channel ($4000-$4003)
pub type Pulse1 = PulseChannel;

/// Pulse 2 channel ($4004-$4007)
pub type Pulse2 = PulseChannel;

impl Pulse1 {
    /// Create a new Pulse 1 channel
    pub fn new_pulse1() -> Self {
        Self::new(true, 0x4000)
    }
}

impl Pulse2 {
    /// Create a new Pulse 2 channel
    pub fn new_pulse2() -> Self {
        Self::new(false, 0x4004)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Envelope Tests
    // =========================================================================

    #[test]
    fn test_envelope_new() {
        let env = Envelope::new();
        assert_eq!(env.volume, 0);
        assert!(!env.constant_volume);
        assert!(!env.loop_flag);
        assert_eq!(env.decay_level, 0);
        assert_eq!(env.divider, 0);
        assert!(!env.reset_flag);
    }

    #[test]
    fn test_envelope_write() {
        let mut env = Envelope::new();

        // Write volume = 5, constant volume = true, loop = true
        env.write(0x35);
        assert_eq!(env.volume, 5);
        assert!(env.constant_volume);
        assert!(env.loop_flag);

        // Write volume = 10, constant volume = false, loop = false
        env.write(0x0A);
        assert_eq!(env.volume, 10);
        assert!(!env.constant_volume);
        assert!(!env.loop_flag);
    }

    #[test]
    fn test_envelope_reset() {
        let mut env = Envelope::new();
        env.write(0x0A); // Volume = 10

        // Trigger reset
        env.reset_flag = true;
        env.clock();

        assert!(!env.reset_flag);
        assert_eq!(env.divider, 10);
        assert_eq!(env.decay_level, 15);
    }

    #[test]
    fn test_envelope_decay() {
        let mut env = Envelope::new();
        env.write(0x01);
        env.loop_flag = false;

        env.reset_flag = true;
        env.clock();
        assert_eq!(env.decay_level, 15);
        assert_eq!(env.divider, 1);

        env.clock();
        assert_eq!(env.decay_level, 15);
        assert_eq!(env.divider, 0);

        env.clock();
        assert_eq!(env.decay_level, 14);
        assert_eq!(env.divider, 1);

        env.clock();
        assert_eq!(env.decay_level, 14);

        env.clock();
        assert_eq!(env.decay_level, 13);
    }

    #[test]
    fn test_envelope_loop() {
        let mut env = Envelope::new();
        env.write(0x01);
        env.loop_flag = true;

        env.reset_flag = true;
        env.clock();

        for _ in 0..32 {
            env.clock();
        }

        assert_eq!(env.decay_level, 15);
    }

    #[test]
    fn test_envelope_constant_volume() {
        let mut env = Envelope::new();
        env.write(0x1A); // Volume = 10, constant volume = true

        assert_eq!(env.output(), 10);

        // Clocking shouldn't change output
        env.clock();
        assert_eq!(env.output(), 10);
    }

    // =========================================================================
    // Sweep Tests
    // =========================================================================

    #[test]
    fn test_sweep_new() {
        let sweep1 = Sweep::new(true);
        assert!(sweep1.is_pulse1);
        assert!(!sweep1.enabled);

        let sweep2 = Sweep::new(false);
        assert!(!sweep2.is_pulse1);
    }

    #[test]
    fn test_sweep_write() {
        let mut sweep = Sweep::new(true);

        sweep.write(0xDB);
        assert!(sweep.enabled);
        assert_eq!(sweep.period, 5);
        assert!(sweep.negate);
        assert_eq!(sweep.shift, 3);
        assert!(sweep.reload_flag);
    }

    #[test]
    fn test_sweep_calculate_target_add() {
        let mut sweep = Sweep::new(true);
        sweep.shift = 2;
        sweep.negate = false;

        // Period = 100, shift = 2, delta = 25
        // Target = 100 + 25 = 125
        let target = sweep.calculate_target(100);
        assert_eq!(target, Some(125));
    }

    #[test]
    fn test_sweep_calculate_target_negate_pulse1() {
        let mut sweep = Sweep::new(true);
        sweep.shift = 2;
        sweep.negate = true;

        // Period = 100, shift = 2, delta = 25
        // Pulse 1: target = 100 - 25 - 1 = 74
        let target = sweep.calculate_target(100);
        assert_eq!(target, Some(74));
    }

    #[test]
    fn test_sweep_calculate_target_negate_pulse2() {
        let mut sweep = Sweep::new(false);
        sweep.shift = 2;
        sweep.negate = true;

        // Period = 100, shift = 2, delta = 25
        // Pulse 2: target = 100 - 25 = 75
        let target = sweep.calculate_target(100);
        assert_eq!(target, Some(75));
    }

    #[test]
    fn test_sweep_silenced_by_overflow() {
        let mut sweep = Sweep::new(true);
        sweep.enabled = true;
        sweep.shift = 1;
        sweep.negate = false;

        // Period = 0x600, shift = 1, delta = 0x300
        // Target = 0x900 > 0x7FF, should be silenced
        assert!(sweep.is_silenced(0x600));
    }

    #[test]
    fn test_sweep_silenced_by_low_period() {
        let mut sweep = Sweep::new(true);
        sweep.enabled = true;
        sweep.shift = 1;

        // Period < 8 should silence
        assert!(sweep.is_silenced(7));
        assert!(!sweep.is_silenced(8));
    }

    #[test]
    fn test_sweep_clock_updates_period() {
        let mut sweep = Sweep::new(true);
        sweep.enabled = true;
        sweep.period = 0;
        sweep.shift = 1;
        sweep.negate = false;
        sweep.divider = 0;

        // Period = 100, shift = 1, target = 150
        let result = sweep.clock(100);
        assert_eq!(result, Some(150));
    }

    // =========================================================================
    // Length Counter Tests
    // =========================================================================

    #[test]
    fn test_length_counter_new() {
        let lc = LengthCounter::new();
        assert_eq!(lc.counter, 0);
        assert!(!lc.halt);
        assert!(!lc.enabled);
    }

    #[test]
    fn test_length_counter_load() {
        let mut lc = LengthCounter::new();
        lc.enabled = true;

        // Load value 0x08 (index 1 in table)
        lc.load(0x08);
        assert_eq!(lc.counter, LENGTH_TABLE[1]);

        // Load value 0xF8 (index 31 in table)
        lc.load(0xF8);
        assert_eq!(lc.counter, LENGTH_TABLE[31]);
    }

    #[test]
    fn test_length_counter_clock() {
        let mut lc = LengthCounter::new();
        lc.enabled = true;
        lc.load(0x08); // Load some value
        lc.halt = false;

        let initial = lc.counter;
        lc.clock();
        assert_eq!(lc.counter, initial - 1);
    }

    #[test]
    fn test_length_counter_halt() {
        let mut lc = LengthCounter::new();
        lc.enabled = true;
        lc.load(0x08);
        lc.halt = true;

        let initial = lc.counter;
        lc.clock();
        assert_eq!(lc.counter, initial); // Should not decrement
    }

    #[test]
    fn test_length_counter_disable() {
        let mut lc = LengthCounter::new();
        lc.enabled = true;
        lc.load(0x08);
        assert!(lc.is_active());

        lc.set_enabled(false);
        assert_eq!(lc.counter, 0);
        assert!(!lc.is_active());
    }

    // =========================================================================
    // Timer Tests
    // =========================================================================

    #[test]
    fn test_timer_new() {
        let timer = Timer::new();
        assert_eq!(timer.period, 0);
        assert_eq!(timer.counter, 0);
    }

    #[test]
    fn test_timer_clock() {
        let mut timer = Timer::new();
        timer.period = 5;
        timer.counter = 2;

        // Clock 3 times
        assert!(!timer.clock()); // counter: 2 -> 1
        assert!(!timer.clock()); // counter: 1 -> 0, reloads to 5
        assert!(timer.clock()); // counter was 5, now 4, but returned true because it was 0

        // Actually, let me trace through:
        // Initial: counter = 2
        // clock(): counter != 0, so counter-- (2->1), return false
        // clock(): counter != 0, so counter-- (1->0), return false
        // clock(): counter == 0, so counter = period (5), return true

        // Wait, that's wrong. Let me re-read the code.
        // When counter == 0, we set counter = period and return true
        // So after the second clock, counter is 0, but we returned false
        // Third clock: counter == 0, set to 5, return true

        // Let me redo this test
        timer.counter = 2;
        assert!(!timer.clock()); // 2 -> 1
        assert!(!timer.clock()); // 1 -> 0
        assert!(timer.clock()); // 0 -> 5, return true
        assert_eq!(timer.counter, 5);
    }

    #[test]
    fn test_timer_write_low() {
        let mut timer = Timer::new();
        timer.period = 0x700;
        timer.write_low(0xAB);
        assert_eq!(timer.period, 0x7AB);
    }

    #[test]
    fn test_timer_write_high() {
        let mut timer = Timer::new();
        timer.period = 0x0FF;
        timer.write_high(0x05);
        assert_eq!(timer.period, 0x5FF);
    }

    // =========================================================================
    // Sequencer Tests
    // =========================================================================

    #[test]
    fn test_sequencer_new() {
        let seq = Sequencer::new();
        assert_eq!(seq.position, 0);
        assert_eq!(seq.duty_mode, 0);
    }

    #[test]
    fn test_sequencer_clock() {
        let mut seq = Sequencer::new();

        // Clock through all 8 positions
        for i in 0..8 {
            assert_eq!(seq.position, i);
            seq.clock();
        }
        // Should wrap back to 0
        assert_eq!(seq.position, 0);
    }

    #[test]
    fn test_sequencer_output() {
        // Test 12.5% duty (mode 0)
        let mut seq = Sequencer::new();
        seq.set_duty(0);
        assert_eq!(seq.output(), 0);
        seq.clock();
        assert_eq!(seq.output(), 1);
        seq.clock();
        assert_eq!(seq.output(), 0);

        // Test 50% duty (mode 2)
        seq.set_duty(2);
        seq.position = 0;
        assert_eq!(seq.output(), 0);
        seq.clock();
        assert_eq!(seq.output(), 1);
        seq.clock();
        assert_eq!(seq.output(), 1);
        seq.clock();
        assert_eq!(seq.output(), 1);
        seq.clock();
        assert_eq!(seq.output(), 1);
        seq.clock();
        assert_eq!(seq.output(), 0);
    }

    #[test]
    fn test_duty_table_values() {
        // Verify the duty table matches expected patterns
        assert_eq!(DUTY_TABLE[0], [0, 1, 0, 0, 0, 0, 0, 0]); // 12.5%
        assert_eq!(DUTY_TABLE[1], [0, 1, 1, 0, 0, 0, 0, 0]); // 25%
        assert_eq!(DUTY_TABLE[2], [0, 1, 1, 1, 1, 0, 0, 0]); // 50%
        assert_eq!(DUTY_TABLE[3], [1, 0, 0, 1, 1, 1, 1, 1]); // 75%
    }

    // =========================================================================
    // PulseChannel Tests
    // =========================================================================

    #[test]
    fn test_pulse_channel_new() {
        let ch = PulseChannel::new(true, 0x4000);
        assert_eq!(ch.base_addr, 0x4000);
        assert!(ch.sweep.is_pulse1);

        let ch2 = PulseChannel::new(false, 0x4004);
        assert_eq!(ch2.base_addr, 0x4004);
        assert!(!ch2.sweep.is_pulse1);
    }

    #[test]
    fn test_pulse_channel_write_reg0() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.write_register(0x4000, 0xBA);

        assert_eq!(ch.sequencer.duty_mode, 2);
        assert!(ch.envelope.loop_flag);
        assert!(ch.envelope.constant_volume);
        assert_eq!(ch.envelope.volume, 10);
        assert!(ch.length_counter.halt);
    }

    #[test]
    fn test_pulse_channel_write_reg1() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.write_register(0x4001, 0xDB);

        assert!(ch.sweep.enabled);
        assert_eq!(ch.sweep.period, 5);
        assert!(ch.sweep.negate);
        assert_eq!(ch.sweep.shift, 3);
    }

    #[test]
    fn test_pulse_channel_write_reg2() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.timer.period = 0x700;
        ch.write_register(0x4002, 0xAB);

        assert_eq!(ch.timer.period, 0x7AB);
    }

    #[test]
    fn test_pulse_channel_write_reg3() {
        let mut ch = PulseChannel::new(true, 0x4000);
        ch.length_counter.enabled = true;

        ch.timer.period = 0x0FF;
        ch.write_register(0x4003, 0x07);

        assert_eq!(ch.timer.period, 0x7FF);
        assert_eq!(ch.length_counter.counter, LENGTH_TABLE[0]);
        assert!(ch.envelope.reset_flag);
        assert_eq!(ch.sequencer.position, 0);
    }

    #[test]
    fn test_pulse_channel_tick() {
        let mut ch = PulseChannel::new(true, 0x4000);
        ch.timer.period = 2;
        ch.timer.counter = 0;

        // First tick should reload counter and clock sequencer
        ch.tick();
        assert_eq!(ch.timer.counter, 2);
        assert_eq!(ch.sequencer.position, 1);

        // Next two ticks should just decrement
        ch.tick();
        assert_eq!(ch.timer.counter, 1);
        ch.tick();
        assert_eq!(ch.timer.counter, 0);

        // This tick should clock sequencer again
        ch.tick();
        assert_eq!(ch.timer.counter, 2);
        assert_eq!(ch.sequencer.position, 2);
    }

    #[test]
    fn test_pulse_channel_output_silenced_by_length() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.length_counter.enabled = true;
        ch.write_register(0x4000, 0x1F);
        ch.write_register(0x4003, 0x08);

        assert!(ch.length_counter.is_active());

        ch.length_counter.counter = 0;
        assert_eq!(ch.get_output(), 0);
    }

    #[test]
    fn test_pulse_channel_output_silenced_by_low_timer() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.write_register(0x4000, 0x1F); // Duty 0, constant volume 15
        ch.write_register(0x4002, 0x05); // Timer low = 5 (period < 8)
        ch.length_counter.enabled = true;
        ch.length_counter.counter = 10;

        // Timer period is now 5, which is < 8
        assert_eq!(ch.get_output(), 0);
    }

    #[test]
    fn test_pulse_channel_output_with_duty() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.write_register(0x4000, 0x9F);
        ch.write_register(0x4001, 0x00);
        ch.length_counter.enabled = true;
        ch.length_counter.counter = 10;
        ch.timer.period = 10;
        ch.sweep.enabled = false;

        ch.sequencer.position = 0;
        assert_eq!(ch.get_output(), 0);

        ch.sequencer.position = 1;
        assert_eq!(ch.get_output(), 15);

        ch.sequencer.position = 4;
        assert_eq!(ch.get_output(), 15);

        ch.sequencer.position = 5;
        assert_eq!(ch.get_output(), 0);
    }

    #[test]
    fn test_pulse_channel_read_registers() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.write_register(0x4000, 0xBA);
        ch.write_register(0x4001, 0xDB);
        ch.write_register(0x4002, 0xAB);
        ch.write_register(0x4003, 0x75);

        assert_eq!(ch.read_register(0x4000), 0xBA);
        assert_eq!(ch.read_register(0x4001), 0xDB);
        assert_eq!(ch.read_register(0x4002), 0xAB);
        assert_eq!(ch.read_register(0x4003), 0x05);
    }

    #[test]
    fn test_pulse_channel_reset() {
        let mut ch = PulseChannel::new(true, 0x4000);

        // Set up some state
        ch.write_register(0x4000, 0xFF);
        ch.write_register(0x4001, 0xFF);
        ch.write_register(0x4002, 0xFF);
        ch.write_register(0x4003, 0xFF);

        ch.reset();

        // Everything should be reset
        assert_eq!(ch.envelope.volume, 0);
        assert!(!ch.sweep.enabled);
        assert_eq!(ch.length_counter.counter, 0);
        assert_eq!(ch.timer.period, 0);
        assert_eq!(ch.sequencer.position, 0);
    }

    #[test]
    fn test_pulse_channel_clock_units() {
        let mut ch = PulseChannel::new(true, 0x4000);

        // Set up envelope
        ch.write_register(0x4000, 0x01); // Volume = 1
        ch.envelope.reset_flag = true;
        ch.envelope.clock(); // Reset it

        // Clock envelope
        let old_level = ch.envelope.decay_level;
        ch.clock_envelope();
        assert!(ch.envelope.decay_level <= old_level);

        // Set up length counter
        ch.length_counter.enabled = true;
        ch.write_register(0x4003, 0x08);
        let old_length = ch.length_counter.counter;
        ch.clock_length_counter();
        assert_eq!(ch.length_counter.counter, old_length - 1);

        // Set up sweep
        ch.write_register(0x4001, 0x81); // Enable, period 0, shift 1
        ch.timer.period = 100;
        ch.sweep.divider = 0;
        ch.clock_sweep();
        // Period should have been updated
    }

    // =========================================================================
    // Pulse1 and Pulse2 Type Tests
    // =========================================================================

    #[test]
    fn test_pulse1_new() {
        let p1 = Pulse1::new_pulse1();
        assert_eq!(p1.base_addr, 0x4000);
        assert!(p1.sweep.is_pulse1);
    }

    #[test]
    fn test_pulse2_new() {
        let p2 = Pulse2::new_pulse2();
        assert_eq!(p2.base_addr, 0x4004);
        assert!(!p2.sweep.is_pulse1);
    }

    #[test]
    fn test_pulse1_sweep_negate() {
        let mut p1 = Pulse1::new_pulse1();
        p1.sweep.shift = 1;
        p1.sweep.negate = true;

        // Pulse 1: 100 - 50 - 1 = 49
        let target = p1.sweep.calculate_target(100);
        assert_eq!(target, Some(49));
    }

    #[test]
    fn test_pulse2_sweep_negate() {
        let mut p2 = Pulse2::new_pulse2();
        p2.sweep.shift = 1;
        p2.sweep.negate = true;

        // Pulse 2: 100 - 50 = 50
        let target = p2.sweep.calculate_target(100);
        assert_eq!(target, Some(50));
    }

    #[test]
    fn test_length_table_values() {
        // Verify some known values from the length table
        assert_eq!(LENGTH_TABLE[0], 0x0A); // 10
        assert_eq!(LENGTH_TABLE[1], 0xFE); // 254
        assert_eq!(LENGTH_TABLE[2], 0x14); // 20
        assert_eq!(LENGTH_TABLE[31], 0x1E); // 30
    }

    #[test]
    fn test_pulse_channel_integration() {
        let mut ch = PulseChannel::new(true, 0x4000);

        ch.length_counter.enabled = true;
        ch.write_register(0x4000, 0x9F);
        ch.write_register(0x4001, 0x00);
        ch.write_register(0x4002, 0x0A);
        ch.write_register(0x4003, 0x08);

        let mut outputs = Vec::new();
        for _ in 0..16 {
            ch.tick();
            outputs.push(ch.get_output());
        }

        let non_zero_count = outputs.iter().filter(|&&x| x > 0).count();
        assert!(non_zero_count > 0, "Channel should produce some output");
    }
}
