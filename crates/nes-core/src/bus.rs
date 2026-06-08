//! Bus trait - Memory bus interface
//!
//! The Bus trait defines the interface between the CPU and the rest of the system.
//! It handles memory reads/writes and interrupt signaling.

/// Memory bus interface for NES
///
/// This trait abstracts the memory access and interrupt handling
/// that the CPU needs to interact with the system.
pub trait Bus {
    /// Read a byte from the bus at the given address
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the bus at the given address
    fn write(&mut self, addr: u16, value: u8);

    /// Called when the CPU completes a cycle
    ///
    /// This allows the bus to update other components (PPU, APU)
    /// based on the number of CPU cycles that have passed.
    fn on_cpu_cycle(&mut self, cycles: u8);

    /// Check if an NMI (Non-Maskable Interrupt) is pending
    fn nmi_pending(&self) -> bool;

    /// Clear the pending NMI flag
    fn clear_nmi(&mut self);

    /// Check if an IRQ (Interrupt Request) is pending
    fn irq_pending(&self) -> bool;
}
