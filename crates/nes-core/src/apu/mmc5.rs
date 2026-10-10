//! MMC5 expansion audio
//!
//! The MMC5 adds two pulse channels and an 8-bit PCM DAC.
//!
//! | register | meaning |
//! |----------|---------|
//! | $5000-$5003 | pulse A: control, unused, period low, period high + length |
//! | $5004-$5007 | pulse B, same layout |
//! | $5011 | raw PCM DAC value (all 8 bits) |
//! | $5015 | channel enable (write) and status (read) |
//!
//! The pulses behave like the 2A03's except that they have no sweep unit and
//! are never silenced by a short period. Their length counters run from a
//! dedicated ~241 Hz clock instead of the APU frame sequencer.
//!
//! PCM read mode ($5010 bit 0) and the PCM IRQ are not emulated.
//!
//! See https://www.nesdev.org/wiki/MMC5_audio

use super::pulse::PulseChannel;
use super::AudioChannel;

/// MMC5 expansion audio.
#[derive(Debug, Clone)]
pub struct Mmc5Audio {
    /// Pulse channel A ($5000-$5003)
    pulse_a: PulseChannel,
    /// Pulse channel B ($5004-$5007)
    pulse_b: PulseChannel,
    /// Raw 8-bit PCM DAC value from $5011
    pcm: u8,
    /// Length-counter clock divider, ~241.08 Hz on NTSC
    length_divider: u32,
}

crate::state::state_fields!(Mmc5Audio {
    pulse_a,
    pulse_b,
    pcm,
    length_divider,
});

/// M2 frequency divided by 7424, the MMC5's length-counter rate.
const LENGTH_COUNTER_PERIOD: u32 = 7424;

impl Mmc5Audio {
    /// Create a silent MMC5 audio unit.
    pub fn new() -> Self {
        Self {
            pulse_a: PulseChannel::new(false, 0x5000),
            pulse_b: PulseChannel::new(false, 0x5004),
            pcm: 0,
            length_divider: 0,
        }
    }

    /// Clock one CPU cycle.
    pub fn tick(&mut self) {
        self.pulse_a.tick();
        self.pulse_b.tick();

        self.length_divider += 1;
        if self.length_divider >= LENGTH_COUNTER_PERIOD {
            self.length_divider = 0;
            self.pulse_a.clock_length_counter();
            self.pulse_b.clock_length_counter();
        }
    }

    /// Handle a write to an MMC5 audio register.
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // Pulse A: $5001 has no counterpart (no sweep unit).
            0x5000 => self.pulse_a.write_register(0x5000, value),
            0x5002 => self.pulse_a.write_register(0x5002, value),
            0x5003 => self.pulse_a.write_register(0x5003, value),
            // Pulse B.
            0x5004 => self.pulse_b.write_register(0x5000, value),
            0x5006 => self.pulse_b.write_register(0x5002, value),
            0x5007 => self.pulse_b.write_register(0x5003, value),
            // Raw PCM DAC.
            0x5011 => self.pcm = value,
            0x5015 => {
                self.pulse_a.set_enabled(value & 0x01 != 0);
                self.pulse_b.set_enabled(value & 0x02 != 0);
            }
            _ => {}
        }
    }

    /// Status bits returned when reading $5015.
    pub fn read_status(&self) -> u8 {
        let mut status = 0;
        if self.pulse_a.is_active() {
            status |= 0x01;
        }
        if self.pulse_b.is_active() {
            status |= 0x02;
        }
        status
    }

    /// Current expansion audio level, on roughly the APU's scale.
    pub fn output(&self) -> f32 {
        let pulses = (self.pulse_a.get_output() + self.pulse_b.get_output()) as f32 / 30.0;
        let pcm = self.pcm as f32 / 255.0;
        pulses * PULSE_GAIN + pcm * PCM_GAIN
    }
}

impl Default for Mmc5Audio {
    fn default() -> Self {
        Self::new()
    }
}

/// How much the two pulse channels contribute at full volume.
const PULSE_GAIN: f32 = 0.30;
/// How much the PCM DAC contributes at full scale.
const PCM_GAIN: f32 = 0.15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_by_default() {
        let mut audio = Mmc5Audio::new();
        for _ in 0..1000 {
            audio.tick();
        }
        assert_eq!(audio.output(), 0.0);
    }

    #[test]
    fn test_pulse_a_becomes_audible() {
        let mut audio = Mmc5Audio::new();

        // Games enable the channel first: the length counter only latches a
        // new value while the channel is enabled.
        audio.write_register(0x5015, 0x01); // enable A
        // Duty 2, constant volume 15.
        audio.write_register(0x5000, 0xBF);
        audio.write_register(0x5002, 0x20); // period low
        audio.write_register(0x5003, 0x08); // period high + length load

        let mut peak = 0.0f32;
        for _ in 0..4000 {
            audio.tick();
            peak = peak.max(audio.output());
        }
        assert!(peak > 0.0, "an enabled MMC5 pulse should be audible");
        assert_eq!(audio.read_status() & 0x01, 0x01);
    }

    #[test]
    fn test_pcm_dac_writes_reach_the_output() {
        let mut audio = Mmc5Audio::new();
        audio.write_register(0x5011, 0x80);
        audio.tick();
        assert!(audio.output() > 0.0);

        audio.write_register(0x5011, 0x00);
        assert_eq!(audio.output(), 0.0);
    }

}
