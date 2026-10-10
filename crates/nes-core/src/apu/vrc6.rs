//! VRC6 expansion audio
//!
//! The VRC6 adds two pulse channels and a sawtooth to the cartridge's output.
//! All three run at the CPU clock rate, so the cartridge clocks them from
//! [`Mapper::on_cpu_cycles`](crate::mapper::Mapper::on_cpu_cycles).
//!
//! Registers (all on the cartridge, see mapper 24):
//! - $x000: `M DDD VVVV` mode, duty and volume (sawtooth: accumulator rate)
//! - $x001: low 8 bits of the 12-bit period
//! - $x002: `E FFFF` enable and high 4 bits of the period
//! - $9003: halt / 16x / 256x frequency scaling
//!
//! See https://www.nesdev.org/wiki/VRC6_audio

/// VRC6 expansion audio: two pulses plus a sawtooth.
#[derive(Debug, Clone)]
pub struct Vrc6Audio {
    pulses: [Vrc6Pulse; 2],
    saw: Vrc6Saw,
    /// $9003 bit 0: halt every oscillator in place
    halt: bool,
    /// $9003 bit 1: shift every period right by 4
    shift_16x: bool,
    /// $9003 bit 2: shift every period right by 8
    shift_256x: bool,
}

crate::state::state_fields!(Vrc6Audio {
    pulses,
    saw,
    halt,
    shift_16x,
    shift_256x,
});

impl Vrc6Audio {
    /// Create a silent VRC6 audio unit.
    pub fn new() -> Self {
        Self {
            pulses: [Vrc6Pulse::new(), Vrc6Pulse::new()],
            saw: Vrc6Saw::new(),
            halt: false,
            shift_16x: false,
            shift_256x: false,
        }
    }

    /// Clock one CPU cycle.
    pub fn tick(&mut self) {
        if self.halt {
            return;
        }
        let shift = if self.shift_256x {
            8
        } else if self.shift_16x {
            4
        } else {
            0
        };

        self.pulses[0].tick(shift);
        self.pulses[1].tick(shift);
        self.saw.tick(shift);
    }

    /// Handle a write to a VRC6 audio register.
    ///
    /// `addr` is the register address as seen by the CPU ($9000-$B003).
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0xF003 {
            0x9000 => self.pulses[0].write_control(value),
            0x9001 => self.pulses[0].write_period_low(value),
            0x9002 => self.pulses[0].write_period_high(value),
            0x9003 => {
                self.halt = value & 0x01 != 0;
                self.shift_16x = value & 0x02 != 0;
                self.shift_256x = value & 0x04 != 0;
            }
            0xA000 => self.pulses[1].write_control(value),
            0xA001 => self.pulses[1].write_period_low(value),
            0xA002 => self.pulses[1].write_period_high(value),
            0xB000 => self.saw.write_rate(value),
            0xB001 => self.saw.write_period_low(value),
            0xB002 => self.saw.write_period_high(value),
            _ => {}
        }
    }

    /// Current expansion audio level, on roughly the same scale as the APU.
    ///
    /// The chip sums the two 4-bit pulses and the top 5 bits of the saw
    /// accumulator into a 6-bit DAC; the gain below puts that range on a par
    /// with the 2A03's own channels without clipping the mix.
    pub fn output(&self) -> f32 {
        let sum = self.pulses[0].output() + self.pulses[1].output() + self.saw.output();
        (sum as f32 / 61.0) * EXPANSION_GAIN
    }
}

impl Default for Vrc6Audio {
    fn default() -> Self {
        Self::new()
    }
}

/// How much of the 2A03 output a fully-driven VRC6 adds.
const EXPANSION_GAIN: f32 = 0.35;

/// One VRC6 pulse channel.
#[derive(Debug, Clone)]
struct Vrc6Pulse {
    volume: u8,
    duty: u8,
    /// Ignore the duty generator and output the volume continuously
    ignore_duty: bool,
    enabled: bool,
    period: u16,
    divider: u16,
    step: u8,
}

crate::state::state_fields!(Vrc6Pulse {
    volume,
    duty,
    ignore_duty,
    enabled,
    period,
    divider,
    step,
});

impl Vrc6Pulse {
    fn new() -> Self {
        Self {
            volume: 0,
            duty: 0,
            ignore_duty: false,
            enabled: false,
            period: 0,
            divider: 0,
            step: 0,
        }
    }

    fn write_control(&mut self, value: u8) {
        self.ignore_duty = value & 0x80 != 0;
        self.duty = (value >> 4) & 0x07;
        self.volume = value & 0x0F;
    }

    fn write_period_low(&mut self, value: u8) {
        self.period = (self.period & 0x0F00) | value as u16;
    }

    fn write_period_high(&mut self, value: u8) {
        self.enabled = value & 0x80 != 0;
        self.period = (self.period & 0x00FF) | (((value & 0x0F) as u16) << 8);
    }

    fn tick(&mut self, shift: u8) {
        if self.divider == 0 {
            self.divider = self.period >> shift;
            // The duty generator counts 15 down to 0 and wraps.
            self.step = self.step.wrapping_sub(1) & 0x0F;
        } else {
            self.divider -= 1;
        }
    }

    /// 0-15 output, zero when the channel is disabled.
    fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.ignore_duty || self.step <= self.duty {
            self.volume
        } else {
            0
        }
    }
}

/// The VRC6 sawtooth generator.
#[derive(Debug, Clone)]
struct Vrc6Saw {
    rate: u8,
    enabled: bool,
    period: u16,
    divider: u16,
    accumulator: u8,
    /// Position in the 14-step cycle
    step: u8,
}

crate::state::state_fields!(Vrc6Saw {
    rate,
    enabled,
    period,
    divider,
    accumulator,
    step,
});

impl Vrc6Saw {
    fn new() -> Self {
        Self {
            rate: 0,
            enabled: false,
            period: 0,
            divider: 0,
            accumulator: 0,
            step: 0,
        }
    }

    fn write_rate(&mut self, value: u8) {
        self.rate = value & 0x3F;
    }

    fn write_period_low(&mut self, value: u8) {
        self.period = (self.period & 0x0F00) | value as u16;
    }

    fn write_period_high(&mut self, value: u8) {
        self.enabled = value & 0x80 != 0;
        self.period = (self.period & 0x00FF) | (((value & 0x0F) as u16) << 8);
    }

    fn tick(&mut self, shift: u8) {
        if !self.enabled {
            // Disabling the channel parks the accumulator at zero.
            self.accumulator = 0;
            self.step = 0;
            return;
        }

        if self.divider == 0 {
            self.divider = self.period >> shift;

            // The accumulator only reacts on every second clock and restarts
            // after six additions.
            self.step = (self.step + 1) % 14;
            if self.step == 0 {
                self.accumulator = 0;
            } else if self.step % 2 == 0 {
                self.accumulator = self.accumulator.wrapping_add(self.rate);
            }
        } else {
            self.divider -= 1;
        }
    }

    /// Top five bits of the accumulator (0-31).
    fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        self.accumulator >> 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_until_programmed() {
        let mut audio = Vrc6Audio::new();
        for _ in 0..1000 {
            audio.tick();
        }
        assert_eq!(audio.output(), 0.0);
    }

    #[test]
    fn test_pulse_outputs_volume_while_enabled() {
        let mut audio = Vrc6Audio::new();

        // Duty 7 with the mode bit set: always output the volume.
        audio.write_register(0x9000, 0xF0 | 0x0F);
        audio.write_register(0x9001, 0x00);
        audio.write_register(0x9002, 0x80); // enabled, period high 0
        audio.write_register(0x9003, 0x00);

        for _ in 0..64 {
            audio.tick();
        }
        assert!(audio.output() > 0.0, "enabled pulse should be audible");
    }

    #[test]
    fn test_disabling_the_enable_bit_silences_a_pulse() {
        let mut audio = Vrc6Audio::new();
        audio.write_register(0x9000, 0xF0 | 0x0F);
        audio.write_register(0x9002, 0x80);
        for _ in 0..64 {
            audio.tick();
        }
        assert!(audio.output() > 0.0);

        audio.write_register(0x9002, 0x00); // clear E
        assert_eq!(audio.output(), 0.0);
    }

    #[test]
    fn test_sawtooth_ramps_and_resets() {
        let mut audio = Vrc6Audio::new();
        audio.write_register(0xB000, 0x08); // rate
        audio.write_register(0xB001, 0x00);
        audio.write_register(0xB002, 0x80); // enabled, short period

        let mut peak = 0u8;
        for _ in 0..400 {
            audio.tick();
            peak = peak.max(audio.saw.output());
        }
        assert!(peak > 0, "the sawtooth should ramp up from zero");
        assert!(peak <= 31);
    }
}
