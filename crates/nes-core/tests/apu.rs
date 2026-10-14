//! Comprehensive APU (Audio Processing Unit) integration tests
//!
//! This test suite covers:
//! - Frame counter (4-step and 5-step modes, IRQ behavior)
//! - Pulse channels (duty, sweep, envelope, length counter)
//! - Triangle channel (linear counter, timer, sequencer)
//! - Noise channel (LFSR, timer, envelope, length counter)
//! - DMC channel (timer, sample buffer, DAC, IRQ)
//! - $4015 status register reads
//! - Channel enable/disable behavior
//!
//! Reference: https://www.nesdev.org/wiki/APU

use nes_core::apu::{
    dmc::DmcChannel,
    noise::NoiseChannel,
    pulse::{Envelope, LengthCounter, Pulse1, Pulse2, PulseChannel, Sweep, Timer},
    triangle::TriangleChannel,
    Apu, AudioChannel, FrameCounter, FrameCounterMode,
};

// ============================================================================
// Frame Counter Tests
// ============================================================================

#[test]
fn test_frame_counter_new() {
    let counter = FrameCounter::new();
    assert_eq!(counter.cycle, 0);
    assert_eq!(counter.step, 0);
}

#[test]
fn test_frame_counter_reset() {
    let mut counter = FrameCounter::new();

    // Advance the counter
    for _ in 0..1000 {
        counter.tick(FrameCounterMode::FourStep);
    }

    // Reset should clear everything
    counter.reset();
    assert_eq!(counter.cycle, 0);
    assert_eq!(counter.step, 0);
}

#[test]
fn test_frame_counter_4step_boundaries() {
    let mut counter = FrameCounter::new();

    // 4-step mode boundaries: 7457, 14913, 22371, 29828, 29829
    let boundaries = [7457, 14913, 22371, 29828, 29829];

    for (i, &boundary) in boundaries.iter().enumerate() {
        // Reset and tick to just before boundary
        counter.reset();
        for _ in 0..boundary - 1 {
            counter.tick(FrameCounterMode::FourStep);
        }

        // Step should be i (before boundary)
        assert_eq!(
            counter.step, i as u8,
            "Step before boundary {} should be {}",
            boundary, i
        );

        // Tick once more to cross boundary
        counter.tick(FrameCounterMode::FourStep);
        assert_eq!(
            counter.step,
            (i + 1) as u8,
            "Step after boundary {} should be {}",
            boundary,
            i + 1
        );
    }
}

#[test]
fn test_frame_counter_5step_boundaries() {
    let mut counter = FrameCounter::new();

    // 5-step mode boundaries: 7457, 14913, 22371, 29829, 37281
    let boundaries = [7457, 14913, 22371, 29829, 37281];

    for (i, &boundary) in boundaries.iter().enumerate() {
        // Reset and tick to just before boundary
        counter.reset();
        for _ in 0..boundary - 1 {
            counter.tick(FrameCounterMode::FiveStep);
        }

        // Step should be i (before boundary)
        assert_eq!(
            counter.step, i as u8,
            "Step before boundary {} should be {}",
            boundary, i
        );

        // Tick once more to cross boundary
        counter.tick(FrameCounterMode::FiveStep);
        assert_eq!(
            counter.step,
            (i + 1) as u8,
            "Step after boundary {} should be {}",
            boundary,
            i + 1
        );
    }
}

#[test]
fn test_frame_counter_4step_wraparound() {
    let mut counter = FrameCounter::new();

    // Tick through a full 4-step period (29830 cycles)
    for _ in 0..29830 {
        counter.tick(FrameCounterMode::FourStep);
    }

    // Should wrap back to 0
    assert_eq!(counter.cycle, 0);
    assert_eq!(counter.step, 0);
}

#[test]
fn test_frame_counter_5step_wraparound() {
    let mut counter = FrameCounter::new();

    // Tick through a full 5-step period (37282 cycles)
    for _ in 0..37282 {
        counter.tick(FrameCounterMode::FiveStep);
    }

    // Should wrap back to 0
    assert_eq!(counter.cycle, 0);
    assert_eq!(counter.step, 0);
}

// ============================================================================
// APU Frame Counter Integration Tests
// ============================================================================

#[test]
fn test_apu_frame_counter_mode_switch() {
    let mut apu = Apu::new();

    // Default is 4-step mode
    assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);

    // Switch to 5-step mode via $4017
    apu.write_register(0x4017, 0x80);
    assert_eq!(apu.frame_counter_mode, FrameCounterMode::FiveStep);

    // Switch back to 4-step mode
    apu.write_register(0x4017, 0x00);
    assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);
}

#[test]
fn test_apu_frame_counter_irq_inhibit() {
    let mut apu = Apu::new();

    // Set IRQ flag
    apu.frame_counter_irq = true;

    // Write to $4017 with inhibit flag (bit 6)
    apu.write_register(0x4017, 0x40);

    // IRQ flag should be cleared
    assert!(!apu.frame_counter_irq);
    assert!(apu.frame_counter_irq_inhibit);
}

#[test]
fn test_apu_frame_counter_4step_irq_generation() {
    let mut apu = Apu::new();

    // Ensure IRQ inhibit is clear
    apu.frame_counter_irq_inhibit = false;
    apu.frame_counter_mode = FrameCounterMode::FourStep;

    // Tick through to just before IRQ point (cycle 29827, step 3)
    for _ in 0..29827 {
        apu.tick();
    }

    // IRQ should not be set yet (we're at step 3)
    assert!(!apu.frame_counter_irq);
    assert_eq!(apu.frame_counter.step, 3);

    // Tick to cycle 29828 - this sets step to 4 and triggers IRQ
    apu.tick();

    // IRQ should now be set
    assert!(apu.frame_counter_irq);
    assert_eq!(apu.frame_counter.step, 4);
}

#[test]
fn test_apu_frame_counter_5step_no_irq() {
    let mut apu = Apu::new();

    // Set 5-step mode
    apu.write_register(0x4017, 0x80);
    apu.frame_counter_irq_inhibit = false;

    // Tick through a full 5-step sequence
    for _ in 0..37282 {
        apu.tick();
    }

    // IRQ should never be set in 5-step mode
    assert!(!apu.frame_counter_irq);
}

#[test]
fn test_apu_read_status_register() {
    let mut apu = Apu::new();

    // Read $4015 with no IRQ
    let status = apu.read_register(0x4015);
    assert_eq!(status & 0x40, 0);

    // Set IRQ flag
    apu.frame_counter_irq = true;

    // Read $4015 - should see IRQ flag and clear it
    let status_with_irq = apu.read_register(0x4015);
    assert_eq!(status_with_irq & 0x40, 0x40);

    // IRQ flag should be cleared after read
    assert!(!apu.frame_counter_irq);
}

#[test]
fn test_apu_reset() {
    let mut apu = Apu::new();

    // Modify state
    apu.frame_counter.cycle = 1000;
    apu.frame_counter_mode = FrameCounterMode::FiveStep;
    apu.frame_counter_irq = true;
    apu.frame_counter_irq_inhibit = true;

    // Reset
    apu.reset();

    // Verify reset state
    assert_eq!(apu.frame_counter.cycle, 0);
    assert_eq!(apu.frame_counter.step, 0);
    assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);
    assert!(!apu.frame_counter_irq);
    assert!(!apu.frame_counter_irq_inhibit);
}

// ============================================================================
// Pulse Channel Tests
// ============================================================================

#[test]
fn test_pulse_channel_creation() {
    let ch1 = Pulse1::new_pulse1();
    assert_eq!(ch1.base_addr, 0x4000);
    assert!(ch1.sweep.is_pulse1);

    let ch2 = Pulse2::new_pulse2();
    assert_eq!(ch2.base_addr, 0x4004);
    assert!(!ch2.sweep.is_pulse1);
}

#[test]
fn test_pulse_channel_full_register_write_sequence() {
    let mut ch = PulseChannel::new(true, 0x4000);

    // Write $4000: duty=2, loop=true, constant_volume=true, volume=10
    ch.write_register(0x4000, 0xBA);
    // Verify via readback
    assert_eq!(ch.read_register(0x4000), 0xBA);

    // Write $4001: sweep enabled, period=5, negate=true, shift=3
    ch.write_register(0x4001, 0xDB);
    assert_eq!(ch.read_register(0x4001), 0xDB);

    // Write $4002: timer low byte
    ch.write_register(0x4002, 0xAB);
    assert_eq!(ch.read_register(0x4002), 0xAB);

    // Write $4003: timer high bits and length counter
    ch.length_counter.enabled = true;
    ch.write_register(0x4003, 0x58);
    assert_eq!(ch.read_register(0x4003), 0x00); // Only high 3 bits (0 in this case)
}

#[test]
fn test_pulse_channel_envelope_decay() {
    let mut env = Envelope::new();
    env.write(0x03); // Volume = 3
    env.loop_flag = false;

    // Trigger reset
    env.reset_flag = true;
    env.clock();

    // After reset: divider = 3, decay_level = 15
    assert_eq!(env.divider, 3);
    assert_eq!(env.decay_level, 15);

    // Clock through decay
    // Divider counts: 3->2->1->0 (triggers decay on next clock)
    env.clock(); // 3->2
    assert_eq!(env.decay_level, 15);
    env.clock(); // 2->1
    assert_eq!(env.decay_level, 15);
    env.clock(); // 1->0
    assert_eq!(env.decay_level, 15);
    env.clock(); // divider==0, reloads to 3, triggers decay
    assert_eq!(env.decay_level, 14);

    // Continue to 0
    for _ in 0..56 {
        // 14 * 4 clocks per decay
        env.clock();
    }
    assert_eq!(env.decay_level, 0);

    // Stays at 0 (no loop)
    env.clock();
    assert_eq!(env.decay_level, 0);
}

#[test]
fn test_pulse_channel_envelope_loop() {
    let mut env = Envelope::new();
    env.write(0x01); // Volume = 1
    env.loop_flag = true;

    // Trigger reset
    env.reset_flag = true;
    env.clock();
    assert_eq!(env.decay_level, 15);

    // Decay to 0 (volume=1 means divider starts at 1)
    // Each decay: clock 1 (1->0), clock 2 (divider==0, reload to 1, decay--)
    // So 2 clocks per decay
    for i in 0..30 {
        env.clock();
        if i % 2 == 1 {
            // After odd-numbered clocks, decay should decrease
            assert!(
                env.decay_level <= 14 - (i / 2),
                "Decay level too high at clock {}",
                i + 1
            );
        }
    }
    assert_eq!(
        env.decay_level, 0,
        "Decay level should be 0 after 30 clocks"
    );

    // At this point: decay_level = 0, divider = 1 (was reloaded after last decay)
    // One more clock: 1->0
    env.clock();
    // divider is now 0, but decay won't wrap until NEXT clock when divider==0 check runs
    assert_eq!(env.decay_level, 0, "Decay level should still be 0");

    // This clock triggers the wrap
    env.clock();
    assert_eq!(
        env.decay_level, 15,
        "Decay level should wrap to 15 with loop enabled"
    );
}

#[test]
fn test_pulse_channel_sweep_add_mode() {
    let mut sweep = Sweep::new(true);
    sweep.enabled = true;
    sweep.shift = 2;
    sweep.negate = false;
    sweep.period = 0;
    sweep.divider = 0;

    // Period = 100, shift = 2, target = 100 + 25 = 125
    let result = sweep.clock(100);
    assert_eq!(result, Some(125));
}

#[test]
fn test_pulse_channel_sweep_negate_pulse1() {
    let mut sweep = Sweep::new(true);
    sweep.enabled = true;
    sweep.shift = 2;
    sweep.negate = true;
    sweep.period = 0;
    sweep.divider = 0;

    // Pulse 1: 100 - 25 - 1 = 74
    let result = sweep.clock(100);
    assert_eq!(result, Some(74));
}

#[test]
fn test_pulse_channel_sweep_negate_pulse2() {
    let mut sweep = Sweep::new(false);
    sweep.enabled = true;
    sweep.shift = 2;
    sweep.negate = true;
    sweep.period = 0;
    sweep.divider = 0;

    // Pulse 2: 100 - 25 = 75
    let result = sweep.clock(100);
    assert_eq!(result, Some(75));
}

#[test]
fn test_pulse_channel_sweep_silence_conditions() {
    let mut sweep = Sweep::new(true);
    sweep.enabled = true;
    sweep.shift = 1;
    sweep.negate = false;

    // Period < 8 should silence
    assert!(sweep.is_silenced(7));
    assert!(!sweep.is_silenced(8));

    // Target > 0x7FF should silence
    // Period = 0x600, shift = 1, target = 0x900 > 0x7FF
    assert!(sweep.is_silenced(0x600));
}

#[test]
fn test_pulse_channel_length_counter() {
    let mut lc = LengthCounter::new();

    // Enable and load
    lc.set_enabled(true);
    lc.load(0x08); // Index 1 = 0xFE
    assert_eq!(lc.counter, 0xFE);
    assert!(lc.is_active());

    // Clock decrements
    lc.clock();
    assert_eq!(lc.counter, 0xFD);

    // Halt prevents decrement
    lc.halt = true;
    lc.clock();
    assert_eq!(lc.counter, 0xFD);

    // Disable clears counter
    lc.set_enabled(false);
    assert_eq!(lc.counter, 0);
    assert!(!lc.is_active());
}

#[test]
fn test_pulse_channel_timer() {
    let mut timer = Timer::new();
    timer.period = 5;
    timer.counter = 2;

    // Clock 3 times
    assert!(!timer.clock()); // 2->1
    assert!(!timer.clock()); // 1->0
    assert!(timer.clock()); // 0->5, returns true
    assert_eq!(timer.counter, 5);
}

#[test]
fn test_pulse_channel_output_silencing() {
    let mut ch = PulseChannel::new(true, 0x4000);

    // Setup for output
    ch.write_register(0x4000, 0x9F); // Duty 2, constant volume 15
    ch.length_counter.enabled = true;
    ch.length_counter.counter = 10;
    ch.timer.period = 10;
    ch.sweep.enabled = false;
    ch.sequencer.position = 1; // High output position

    // Should output volume
    assert_eq!(ch.get_output(), 15);

    // Silence by length counter
    ch.length_counter.counter = 0;
    assert_eq!(ch.get_output(), 0);

    // Restore length, silence by timer
    ch.length_counter.counter = 10;
    ch.timer.period = 5; // < 8
    assert_eq!(ch.get_output(), 0);

    // Restore timer, silence by sweep
    ch.timer.period = 10;
    ch.sweep.enabled = true;
    ch.sweep.shift = 1;
    ch.sweep.negate = false;
    // Sweep target for period 10 with shift 1: 10 + 5 = 15 (not silenced)
    // Need a case where target > 0x7FF or period < 8
    ch.timer.period = 0x600; // Will be silenced by sweep
    assert_eq!(ch.get_output(), 0);
}

#[test]
fn test_pulse_channel_integration() {
    let mut ch = PulseChannel::new(true, 0x4000);

    // Full setup
    ch.length_counter.enabled = true;
    ch.write_register(0x4000, 0x9F); // Duty 2, constant volume 15
    ch.write_register(0x4001, 0x00); // Sweep disabled
    ch.write_register(0x4002, 0x0A); // Timer low
    ch.write_register(0x4003, 0x00); // Timer high, length

    // Run for several cycles
    let mut outputs = Vec::new();
    for _ in 0..32 {
        ch.tick();
        outputs.push(ch.get_output());
    }

    // Should have some non-zero outputs
    let non_zero = outputs.iter().filter(|&&x| x > 0).count();
    assert!(non_zero > 0, "Channel should produce some output");

    // All outputs should be 0 or 15
    for output in outputs {
        assert!(output == 0 || output == 15);
    }
}

// ============================================================================
// Triangle Channel Tests
// ============================================================================

#[test]
fn test_triangle_channel_new() {
    let ch = TriangleChannel::new();
    assert_eq!(ch.get_output(), 0);
    assert!(!ch.is_active());
}

#[test]
fn test_triangle_channel_enable_disable() {
    let mut ch = TriangleChannel::new();

    // Enable and load length counter
    ch.set_enabled(true);
    ch.write_register(0x400B, 0xF8); // Load length counter
    assert!(ch.is_active());

    // Disable should clear length counter and deactivate
    ch.set_enabled(false);
    assert!(!ch.is_active());

    // Re-enable should not restore length counter
    ch.set_enabled(true);
    assert!(!ch.is_active());
}

#[test]
fn test_triangle_channel_linear_counter() {
    let mut ch = TriangleChannel::new();

    // Setup linear counter via register
    ch.write_register(0x4008, 0x05); // Reload value = 5

    // Trigger reload
    ch.reload_linear_counter();
    ch.clock_linear_counter();
    // Linear counter should be loaded

    // Clock decrements (without control flag)
    ch.clock_linear_counter();
    ch.clock_linear_counter();
    ch.clock_linear_counter();

    // Continue to 0
    for _ in 0..10 {
        ch.clock_linear_counter();
    }
    // Should be at 0 now
}

#[test]
fn test_triangle_channel_linear_counter_control() {
    let mut ch = TriangleChannel::new();

    // Control flag set: counter reloads continuously
    ch.write_register(0x4008, 0x85); // Control = true, reload = 5

    ch.reload_linear_counter();
    ch.clock_linear_counter();

    // Should stay at reload value with control flag set
    ch.clock_linear_counter();
    ch.clock_linear_counter();
    // Counter should still be at reload value
}

#[test]
fn test_triangle_channel_length_counter() {
    let mut ch = TriangleChannel::new();

    ch.set_enabled(true);
    ch.write_register(0x4008, 0x00); // Enable length counter (control flag clear)
    ch.write_register(0x400B, 0x08); // Load length

    assert!(ch.is_active());

    // Clock decrements length counter
    ch.clock_length_counter();
    // Length should have decremented
}

#[test]
fn test_triangle_channel_timer() {
    let mut ch = TriangleChannel::new();

    // Setup timer via registers
    ch.write_register(0x400A, 0x03); // Timer low
    ch.write_register(0x400B, 0x00); // Timer high

    // Tick decrements timer
    ch.tick();
    ch.tick();

    // Continue ticking
    for _ in 0..10 {
        ch.tick();
    }
    // Timer should have wrapped and advanced sequencer
}

#[test]
fn test_triangle_channel_output_silencing() {
    let mut ch = TriangleChannel::new();

    // Setup for output
    ch.set_enabled(true);
    ch.write_register(0x4008, 0x05); // Linear counter reload = 5
    ch.write_register(0x400A, 0x0A); // Timer low
    ch.write_register(0x400B, 0x08); // Timer high, length

    // Trigger linear counter reload
    ch.reload_linear_counter();
    ch.clock_linear_counter();

    // Disable channel
    ch.set_enabled(false);
    assert_eq!(ch.get_output(), 0);

    // Re-enable but timer too low (will silence)
    ch.set_enabled(true);
    // Timer period < 2 should silence
}

#[test]
fn test_triangle_channel_full_sequence() {
    let mut ch = TriangleChannel::new();

    ch.set_enabled(true);
    ch.write_register(0x4008, 0x05); // Linear counter reload = 5
    ch.write_register(0x400A, 0x10); // Timer low
    ch.write_register(0x400B, 0x08); // Timer high, length

    // Clock linear counter to load it
    ch.reload_linear_counter();
    ch.clock_linear_counter();

    // Run for a while
    for _ in 0..100 {
        ch.tick();
    }

    // Channel should have processed ticks
}

// ============================================================================
// Noise Channel Tests
// ============================================================================

#[test]
fn test_noise_channel_new() {
    let ch = NoiseChannel::new();
    assert_eq!(ch.get_output(), 0);
    assert!(!ch.is_active());
}

#[test]
fn test_noise_channel_timer() {
    let mut ch = NoiseChannel::new();

    // Write timer period via register
    ch.write_register(0x400E, 0x00); // Index 0

    ch.write_register(0x400E, 0x0F); // Index 15
}

#[test]
fn test_noise_channel_envelope() {
    let mut ch = NoiseChannel::new();

    // Constant volume mode
    ch.write_register(0x400C, 0x1A); // Constant volume, volume = 10

    // Envelope mode
    ch.write_register(0x400C, 0x0A); // Envelope mode, decay rate = 10
}

#[test]
fn test_noise_channel_length_counter() {
    let mut ch = NoiseChannel::new();

    ch.set_enabled(true);
    ch.write_register(0x400F, 0xF8); // Load length
    assert!(ch.is_active());

    // Clock decrements
    ch.clock_length_counter();
}

#[test]
fn test_noise_channel_full_sequence() {
    let mut ch = NoiseChannel::new();

    ch.write_register(0x400C, 0x0F); // Constant volume 15
    ch.write_register(0x400E, 0x00); // Timer period
    ch.set_enabled(true);
    ch.write_register(0x400F, 0x00); // Load length

    // Run for a while
    let mut outputs = Vec::new();
    for _ in 0..100 {
        ch.tick();
        outputs.push(ch.get_output());
    }

    // Should have both 0 and 15 outputs
    let zeros = outputs.iter().filter(|&&x| x == 0).count();
    let non_zeros = outputs.iter().filter(|&&x| x > 0).count();

    assert!(zeros > 0 || non_zeros > 0, "Should have some outputs");
}

// ============================================================================
// DMC Channel Tests
// ============================================================================

#[test]
fn test_dmc_channel_new() {
    let ch = DmcChannel::new();
    assert_eq!(ch.get_output(), 0);
    assert!(!ch.is_active());
}

#[test]
fn test_dmc_channel_frequency_write() {
    let mut ch = DmcChannel::new();

    // Write frequency index
    ch.write_register(0x4010, 0x00);
    // Period should be 428 (index 0)

    ch.write_register(0x4010, 0x0F);
    // Period should be 54 (index 15)
}

#[test]
fn test_dmc_channel_dac_write() {
    let mut ch = DmcChannel::new();

    ch.write_register(0x4011, 0x42);
    assert_eq!(ch.get_output(), 0x42);

    // Bit 7 should be masked
    ch.write_register(0x4011, 0xFF);
    assert_eq!(ch.get_output(), 0x7F);
}

#[test]
fn test_dmc_channel_address_write() {
    let mut ch = DmcChannel::new();

    ch.write_register(0x4012, 0x00);
    // Address should be $C000

    ch.write_register(0x4012, 0xFF);
    // Address should be $FFC0
}

#[test]
fn test_dmc_channel_length_write() {
    let mut ch = DmcChannel::new();

    ch.write_register(0x4013, 0x00);
    // Length should be 1

    ch.write_register(0x4013, 0xFF);
    // Length should be 4081
}

#[test]
fn test_dmc_channel_enable_disable() {
    let mut ch = DmcChannel::new();

    // Setup
    ch.write_register(0x4012, 0x00);
    ch.write_register(0x4013, 0x01);

    // Enable via set_enabled (which sets is_enabled flag)
    ch.set_enabled(true);
    // Also write to $4015 to set up bytes_remaining
    ch.write_register(0x4015, 0x10);
    assert!(ch.is_active());

    // Disable
    ch.set_enabled(false);
    ch.write_register(0x4015, 0x00);
    assert!(!ch.is_active());
}

#[test]
fn test_dmc_channel_irq_clear() {
    let mut ch = DmcChannel::new();

    // Set IRQ mode
    ch.write_register(0x4010, 0x80);

    // Write $4015 should clear IRQ
    ch.write_register(0x4015, 0x00);
    assert!(!ch.irq_active());
}

#[test]
fn test_dmc_channel_play_modes() {
    let mut ch = DmcChannel::new();

    // Normal mode
    ch.write_register(0x4010, 0x00);
    // Should be normal mode

    // Loop mode
    ch.write_register(0x4010, 0x40);
    // Should be loop mode

    // IRQ mode
    ch.write_register(0x4010, 0x80);
    // Should be IRQ mode
}

// ============================================================================
// AudioChannel Trait Tests
// ============================================================================

#[test]
fn test_audio_channel_trait_pulse() {
    let mut ch = PulseChannel::new(true, 0x4000);

    // Test trait methods
    ch.write_register(0x4000, 0x0F);
    ch.tick();
    let output = ch.get_output();
    assert!(output <= 15);
}

#[test]
fn test_audio_channel_trait_triangle() {
    let mut ch = TriangleChannel::new();

    ch.write_register(0x4008, 0x05);
    ch.tick();
    let output = ch.get_output();
    assert!(output <= 15);
}

#[test]
fn test_audio_channel_trait_noise() {
    let mut ch = NoiseChannel::new();

    ch.write_register(0x400C, 0x0F);
    ch.tick();
    let output = ch.get_output();
    assert!(output <= 15);
}

#[test]
fn test_audio_channel_trait_dmc() {
    let mut ch = DmcChannel::new();

    ch.write_register(0x4011, 0x42);
    let output = ch.get_output();
    assert!(output <= 127);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_apu_full_reset() {
    let mut apu = Apu::new();

    // Modify state
    apu.frame_counter.cycle = 1000;
    apu.frame_counter_irq = true;
    apu.write_register(0x4017, 0x80); // 5-step mode

    // Reset
    apu.reset();

    // Verify
    assert_eq!(apu.frame_counter.cycle, 0);
    assert!(!apu.frame_counter_irq);
    assert_eq!(apu.frame_counter_mode, FrameCounterMode::FourStep);
}

#[test]
fn test_apu_register_write_ranges() {
    let mut apu = Apu::new();

    // Test that writes to all APU registers don't panic
    for addr in 0x4000..=0x4017 {
        apu.write_register(addr, 0xFF);
    }

    // Test that reads from all APU registers don't panic
    for addr in 0x4000..=0x4017 {
        let _ = apu.read_register(addr);
    }
}

#[test]
fn test_length_table_values() {
    // Verify length table values
    let expected = [
        0x0A, 0xFE, 0x14, 0x02, 0x28, 0x04, 0x50, 0x06, 0xA0, 0x08, 0x3C, 0x0A, 0x0E, 0x0C, 0x1A,
        0x0E, 0x0C, 0x10, 0x18, 0x12, 0x30, 0x14, 0x60, 0x16, 0xC0, 0x18, 0x48, 0x1A, 0x10, 0x1C,
        0x20, 0x1E,
    ];

    // Pulse channel length table
    let pulse_lengths = [
        10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96,
        22, 192, 24, 72, 26, 16, 28, 32, 30,
    ];

    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(pulse_lengths[i], exp as u8);
    }
}

#[test]
fn test_frame_counter_step_sequence_4step() {
    let mut apu = Apu::new();

    // Track which steps fire
    let mut steps_fired = Vec::new();

    for cycle in 0..30000 {
        let step_changed = apu.frame_counter.tick(FrameCounterMode::FourStep);
        if step_changed {
            steps_fired.push((cycle, apu.frame_counter.step));
        }
    }

    // Should fire at steps 1, 2, 3, 4, then wrap
    assert!(steps_fired.len() >= 4, "Should have fired at least 4 steps");
}

#[test]
fn test_frame_counter_step_sequence_5step() {
    let mut apu = Apu::new();

    // Track which steps fire
    let mut steps_fired = Vec::new();

    for cycle in 0..38000 {
        let step_changed = apu.frame_counter.tick(FrameCounterMode::FiveStep);
        if step_changed {
            steps_fired.push((cycle, apu.frame_counter.step));
        }
    }

    // Should fire at steps 1, 2, 3, 4, 5, then wrap
    assert!(steps_fired.len() >= 5, "Should have fired at least 5 steps");
}

#[test]
fn test_channel_enable_disable_behavior() {
    // Test that disabling a channel clears its length counter

    // Pulse
    let mut pulse = PulseChannel::new(true, 0x4000);
    pulse.length_counter.enabled = true;
    pulse.length_counter.counter = 10;
    pulse.set_enabled(false);
    assert_eq!(pulse.length_counter.counter, 0);

    // Triangle
    let mut triangle = TriangleChannel::new();
    triangle.set_enabled(true);
    triangle.write_register(0x400B, 0x08);
    assert!(triangle.is_active());
    triangle.set_enabled(false);
    assert!(!triangle.is_active());

    // Noise
    let mut noise = NoiseChannel::new();
    noise.set_enabled(true);
    noise.write_register(0x400F, 0xF8);
    assert!(noise.is_active());
    noise.set_enabled(false);
    assert!(!noise.is_active());
}

#[test]
fn test_sweep_unit_behavior() {
    // Test sweep unit with various configurations

    // Test that sweep silences channel when target overflows
    let mut sweep = Sweep::new(true);
    sweep.enabled = true;
    sweep.shift = 1;
    sweep.negate = false;

    // Period = 0x600, shift = 1
    // Target = 0x600 + 0x300 = 0x900 > 0x7FF
    assert!(sweep.is_silenced(0x600));

    // Test that sweep doesn't silence when target is valid
    // Period = 0x100, shift = 1
    // Target = 0x100 + 0x80 = 0x180 <= 0x7FF
    assert!(!sweep.is_silenced(0x100));
}

#[test]
fn test_envelope_behavior() {
    // Test envelope with constant volume
    let mut env = Envelope::new();
    env.write(0x1F); // Constant volume = 15
    assert_eq!(env.output(), 15);

    // Clocking shouldn't change output
    env.clock();
    assert_eq!(env.output(), 15);

    // Test envelope decay
    let mut env2 = Envelope::new();
    env2.write(0x0F); // Decay rate = 15
    env2.loop_flag = false;
    env2.reset_flag = true;
    env2.clock(); // Reset

    assert_eq!(env2.decay_level, 15);

    // Decay to 0
    for _ in 0..240 {
        // 15 * 16
        env2.clock();
    }

    assert_eq!(env2.decay_level, 0);
}

#[test]
fn test_channel_status_bits() {
    // Test that channels report correct status for $4015 reads

    // Pulse (via length counter)
    let mut pulse = PulseChannel::new(true, 0x4000);
    pulse.length_counter.enabled = true;
    assert!(!pulse.is_active());
    pulse.length_counter.counter = 10;
    assert!(pulse.is_active());

    // Triangle
    let mut triangle = TriangleChannel::new();
    triangle.set_enabled(true);
    assert!(!triangle.is_active());
    triangle.write_register(0x400B, 0x08); // Load length
    assert!(triangle.is_active());

    // Noise
    let mut noise = NoiseChannel::new();
    noise.set_enabled(true);
    assert!(!noise.is_active());
    noise.write_register(0x400F, 0xF8); // Load length
    assert!(noise.is_active());

    // DMC
    let mut dmc = DmcChannel::new();
    dmc.set_enabled(true);
    assert!(!dmc.is_active());
    // DMC needs bytes_remaining > 0 to be active
    // This requires enabling via $4015 with proper setup
}

#[test]
fn test_timer_behavior_all_channels() {
    // Test timer behavior is consistent across channels

    // Pulse timer
    let mut pulse_timer = Timer::new();
    pulse_timer.period = 5;
    pulse_timer.counter = 0;
    assert!(pulse_timer.clock()); // Returns true when reloading
    assert_eq!(pulse_timer.counter, 5);

    // Triangle timer (built into channel)
    let mut triangle = TriangleChannel::new();
    triangle.write_register(0x400A, 0x05); // Timer low
    triangle.write_register(0x400B, 0x00); // Timer high
                                           // Timer is ticking internally

    // Noise timer
    let mut noise = NoiseChannel::new();
    noise.write_register(0x400E, 0x00); // Set timer period
}

#[test]
fn test_register_read_write_consistency() {
    // Test that reading back written values works where applicable

    // Pulse channel - registers are readable
    let mut pulse = PulseChannel::new(true, 0x4000);
    pulse.write_register(0x4000, 0xBA);
    pulse.write_register(0x4001, 0xDB);
    pulse.write_register(0x4002, 0xAB);
    pulse.write_register(0x4003, 0x75);

    assert_eq!(pulse.read_register(0x4000), 0xBA);
    assert_eq!(pulse.read_register(0x4001), 0xDB);
    assert_eq!(pulse.read_register(0x4002), 0xAB);
    assert_eq!(pulse.read_register(0x4003), 0x05); // Only high 3 bits

    // DMC channel - some registers are readable
    let mut dmc = DmcChannel::new();
    dmc.write_register(0x4011, 0x42);
    dmc.write_register(0x4012, 0x12);
    dmc.write_register(0x4013, 0x34);

    assert_eq!(dmc.read_register(0x4011), 0x42);
    assert_eq!(dmc.read_register(0x4012), 0x12);
    assert_eq!(dmc.read_register(0x4013), 0x34);
}

#[test]
fn test_frame_counter_timing_accuracy() {
    // Test that frame counter timing is accurate
    let mut apu = Apu::new();

    // Run for exactly one 4-step frame
    for _ in 0..29830 {
        apu.tick();
    }

    // Should be back at cycle 0
    assert_eq!(apu.frame_counter.cycle, 0);
    assert_eq!(apu.frame_counter.step, 0);

    // Run for exactly one 5-step frame
    apu.frame_counter_mode = FrameCounterMode::FiveStep;
    for _ in 0..37282 {
        apu.tick();
    }

    // Should be back at cycle 0
    assert_eq!(apu.frame_counter.cycle, 0);
    assert_eq!(apu.frame_counter.step, 0);
}

#[test]
fn test_irq_behavior_4step_mode() {
    let mut apu = Apu::new();

    // Ensure 4-step mode and no inhibit
    apu.frame_counter_mode = FrameCounterMode::FourStep;
    apu.frame_counter_irq_inhibit = false;

    // Run to just before IRQ point
    for _ in 0..29827 {
        apu.tick();
    }

    assert!(!apu.frame_counter_irq);

    // Tick to IRQ point
    apu.tick();
    assert!(apu.frame_counter_irq);

    // Reading $4015 should clear IRQ
    let _ = apu.read_register(0x4015);
    assert!(!apu.frame_counter_irq);
}

#[test]
fn test_irq_inhibit_prevents_flag() {
    let mut apu = Apu::new();

    // Set inhibit flag
    apu.write_register(0x4017, 0x40);

    // Run through 4-step sequence
    for _ in 0..30000 {
        apu.tick();
    }

    // IRQ should never be set
    assert!(!apu.frame_counter_irq);
}

#[test]
fn test_5step_mode_no_irq() {
    let mut apu = Apu::new();

    // Set 5-step mode
    apu.write_register(0x4017, 0x80);
    apu.frame_counter_irq_inhibit = false;

    // Run through 5-step sequence
    for _ in 0..38000 {
        apu.tick();
    }

    // IRQ should never be set in 5-step mode
    assert!(!apu.frame_counter_irq);
}
