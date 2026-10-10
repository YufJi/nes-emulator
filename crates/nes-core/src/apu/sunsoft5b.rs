//! Sunsoft 5B expansion audio
//!
//! The 5B is a Yamaha YM2149F with its clock-select pin low, so the tone, noise
//! and envelope generators all run at half the YM2149's usual rate: a tone
//! half-cycle is 16 CPU clocks and a new noise bit appears every 32.
//!
//! The mapper selects an internal register through $C000 and writes it through
//! $E000:
//!
//! | register | meaning |
//! |----------|---------|
//! | $00-$05  | 12-bit tone periods for channels A, B, C |
//! | $06      | 5-bit noise period |
//! | $07      | `--CB Acba` mixer; a 0 bit *enables* tone/noise |
//! | $08-$0A  | `---E VVVV` envelope mode and volume per channel |
//! | $0B-$0C  | 16-bit envelope period |
//! | $0D      | `---- CAaH` envelope shape |
//!
//! Only Gimmick! shipped with the 5B and it does not use the envelope
//! generator, so the envelope shapes here are a straightforward AY-style
//! approximation rather than a cycle-exact model.
//!
//! See https://www.nesdev.org/wiki/Sunsoft_5B_audio

/// Sunsoft 5B expansion audio.
#[derive(Debug, Clone)]
pub struct Sunsoft5bAudio {
    /// 12-bit tone periods for the three channels
    tone_period: [u16; 3],
    /// Tone phase counters
    tone_counter: [u16; 3],
    /// Square-wave state of each channel
    tone_output: [bool; 3],
    /// 5-bit noise period
    noise_period: u8,
    /// 17-bit noise shift register
    noise_lfsr: u32,
    noise_output: bool,
    /// Mixer register $07
    mixer: u8,
    /// Volume/envelope registers $08-$0A
    volume: [u8; 3],
    envelope_period: u16,
    envelope_counter: u16,
    envelope_level: u8,
    /// Shape register $0D
    envelope_shape: u8,
    envelope_attack: bool,
    envelope_hold: bool,
    /// Register selected through $C000
    selected_register: u8,
    /// Noise advances one step every second sixteenth
    noise_phase: bool,
    noise_counter: u16,
    /// Divides the CPU clock down to the 5B's internal rate
    clock_divider: u8,
}

impl Sunsoft5bAudio {
    /// Create a silent 5B.
    pub fn new() -> Self {
        Self {
            tone_period: [1; 3],
            tone_counter: [0; 3],
            tone_output: [false; 3],
            noise_period: 1,
            noise_lfsr: 1,
            noise_output: false,
            mixer: 0xFF,
            volume: [0; 3],
            envelope_period: 1,
            envelope_counter: 0,
            envelope_level: 0,
            envelope_shape: 0,
            envelope_attack: false,
            envelope_hold: false,
            selected_register: 0,
            noise_phase: false,
            noise_counter: 0,
            clock_divider: 0,
        }
    }

    /// Handle a write to $C000 (register select).
    pub fn write_select(&mut self, value: u8) {
        // Bits 4-7 disable further writes on real hardware; the games only use
        // the low nibble.
        self.selected_register = value & 0x0F;
    }

    /// Handle a write to $E000 (register data).
    pub fn write_data(&mut self, value: u8) {
        match self.selected_register {
            0x00 => self.tone_period[0] = (self.tone_period[0] & 0x0F00) | value as u16,
            0x01 => self.tone_period[0] = (self.tone_period[0] & 0x00FF) | (((value & 0x0F) as u16) << 8),
            0x02 => self.tone_period[1] = (self.tone_period[1] & 0x0F00) | value as u16,
            0x03 => self.tone_period[1] = (self.tone_period[1] & 0x00FF) | (((value & 0x0F) as u16) << 8),
            0x04 => self.tone_period[2] = (self.tone_period[2] & 0x0F00) | value as u16,
            0x05 => self.tone_period[2] = (self.tone_period[2] & 0x00FF) | (((value & 0x0F) as u16) << 8),
            0x06 => self.noise_period = value & 0x1F,
            0x07 => self.mixer = value,
            0x08..=0x0A => {
                let channel = (self.selected_register - 0x08) as usize;
                self.volume[channel] = value & 0x1F;
            }
            0x0B => self.envelope_period = (self.envelope_period & 0xFF00) | value as u16,
            0x0C => self.envelope_period = (self.envelope_period & 0x00FF) | ((value as u16) << 8),
            0x0D => {
                self.envelope_shape = value & 0x0F;
                self.envelope_attack = value & 0x04 != 0;
                self.envelope_hold = false;
                // Writing the shape restarts the envelope; bit 7 of the
                // register (the continue flag in AY terms) is folded into the
                // shape bits here.
                if self.envelope_attack {
                    self.envelope_level = 0;
                } else {
                    self.envelope_level = 31;
                }
            }
            _ => {}
        }
    }

    /// Clock one CPU cycle.
    pub fn tick(&mut self) {
        // The 5B's internal generators run at half the YM2149 rate, which the
        // documentation expresses as 16 CPU clocks per tone half-cycle.
        self.clock_divider = (self.clock_divider + 1) & 0x0F;
        let sixteenth = self.clock_divider == 0;
        if !sixteenth {
            return;
        }

        for channel in 0..3 {
            let period = self.tone_period[channel].max(1) as u16;
            self.tone_counter[channel] += 1;
            if self.tone_counter[channel] >= period {
                self.tone_counter[channel] = 0;
                self.tone_output[channel] = !self.tone_output[channel];
            }
        }

        // Noise advances every other sixteenth, i.e. 32 CPU clocks per bit.
        self.noise_phase = !self.noise_phase;
        if self.noise_phase {
            self.noise_counter += 1;
            if self.noise_counter >= self.noise_period.max(1) as u16 {
                self.noise_counter = 0;
                // 17-bit LFSR with taps at bits 16 and 13.
                let feedback = ((self.noise_lfsr >> 16) ^ (self.noise_lfsr >> 13)) & 1;
                self.noise_lfsr = (self.noise_lfsr << 1) | feedback;
                self.noise_lfsr &= 0x1FFFF;
                self.noise_output = self.noise_lfsr & 1 != 0;
            }
        }

        // Envelope: 5-bit ramp driven by its own divider.
        self.envelope_counter += 1;
        if self.envelope_counter >= self.envelope_period.max(1) {
            self.envelope_counter = 0;
            self.clock_envelope();
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_hold {
            return;
        }
        if self.envelope_attack {
            if self.envelope_level >= 31 {
                self.end_envelope_cycle();
            } else {
                self.envelope_level += 1;
            }
        } else if self.envelope_level == 0 {
            self.end_envelope_cycle();
        } else {
            self.envelope_level -= 1;
        }
    }

    fn end_envelope_cycle(&mut self) {
        // Continue bit: 0 plays the ramp once, 1 repeats it.
        if self.envelope_shape & 0x08 == 0 {
            self.envelope_hold = true;
        }
        // Alternate bit flips the ramp direction between cycles.
        if self.envelope_shape & 0x02 != 0 {
            self.envelope_attack = !self.envelope_attack;
        }
    }

    /// Current expansion audio level, on roughly the APU's scale.
    pub fn output(&self) -> f32 {
        let mut sum = 0u32;
        for channel in 0..3 {
            let tone_disabled = self.mixer & (1 << channel) != 0;
            let noise_disabled = self.mixer & (1 << (channel + 3)) != 0;

            let tone_high = self.tone_output[channel];
            let noise_high = self.noise_output;

            // A 0 bit enables the source; both disabled means a constant level.
            let level_high = match (tone_disabled, noise_disabled) {
                (false, false) => tone_high && noise_high,
                (false, true) => tone_high,
                (true, false) => noise_high,
                (true, true) => true,
            };

            let amplitude = if self.volume[channel] & 0x10 != 0 {
                // Envelope-driven channels use the 5-bit ramp.
                self.envelope_level
            } else {
                (self.volume[channel] & 0x0F) << 1
            };

            if level_high {
                sum += amplitude as u32;
            }
        }

        // Three 5-bit channels, scaled to sit alongside the 2A03 output.
        (sum as f32 / (3.0 * 31.0)) * EXPANSION_GAIN
    }
}

impl Default for Sunsoft5bAudio {
    fn default() -> Self {
        Self::new()
    }
}

/// How much of the 2A03 output a fully-driven 5B adds.
const EXPANSION_GAIN: f32 = 0.30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_by_default() {
        let mut audio = Sunsoft5bAudio::new();
        for _ in 0..1000 {
            audio.tick();
        }
        assert_eq!(audio.output(), 0.0);
    }

    #[test]
    fn test_tone_channel_becomes_audible() {
        let mut audio = Sunsoft5bAudio::new();

        // Channel A: short period, tone enabled, full volume.
        audio.write_select(0x00);
        audio.write_data(0x10);
        audio.write_select(0x01);
        audio.write_data(0x00);
        audio.write_select(0x07);
        audio.write_data(0x3E); // tone A enabled, noise disabled
        audio.write_select(0x08);
        audio.write_data(0x0F); // volume 15, no envelope

        let mut peak = 0.0f32;
        for _ in 0..4000 {
            audio.tick();
            peak = peak.max(audio.output());
        }
        assert!(peak > 0.0, "an enabled tone channel should be audible");
    }

    #[test]
    fn test_disabling_tone_and_noise_gives_a_constant_level() {
        let mut audio = Sunsoft5bAudio::new();
        audio.write_select(0x07);
        audio.write_data(0x3F); // both tone and noise disabled for A
        audio.write_select(0x08);
        audio.write_data(0x0F);

        for _ in 0..100 {
            audio.tick();
        }
        assert!(audio.output() > 0.0);
    }
}
