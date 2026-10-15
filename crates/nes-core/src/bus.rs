//! Bus trait - Memory bus interface
//!
//! The Bus trait is the CPU's side of the machine, and its shape is Mesen's: a
//! memory access *is* the cycle the CPU spends on it. `read_cycle`/`write_cycle`
//! are the CPU's `MemoryRead`/`MemoryWrite` — they hand the access to the
//! machine, which advances the PPU and APU around it — while plain `read`/`write`
//! reach memory without spending a cycle and are what the host and the
//! debuggery paths use.
//!
//! The interrupt lines are the machine's: it samples them at the end of every
//! CPU cycle, and `pending_interrupt` is the answer the CPU asks for once the
//! instruction's cycles are over.

/// Memory bus interface for NES
///
/// This trait abstracts the memory access and interrupt handling
/// that the CPU needs to interact with the system.
pub trait Bus {
    /// Read a byte from the bus at the given address, without spending a cycle.
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the bus at the given address, without spending a cycle.
    fn write(&mut self, addr: u16, value: u8);

    /// Read a byte the way the CPU does: this *is* one CPU cycle, and the
    /// machine advances the PPU/APU around the access (Mesen's `MemoryRead`,
    /// which brackets the access with `StartCpuCycle`/`EndCpuCycle`).
    ///
    /// A bus used on its own — the CPU's unit tests — gets the plain access
    /// back, so a test can drive the CPU without a machine.
    fn read_cycle(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }

    /// Write a byte the way the CPU does: one CPU cycle, the write strobed
    /// inside it (Mesen's `MemoryWrite`).
    fn write_cycle(&mut self, addr: u16, value: u8) {
        self.write(addr, value);
    }

    /// Called when the CPU completes a cycle
    ///
    /// This allows the bus to update other components (PPU, APU)
    /// based on the number of CPU cycles that have passed.
    fn on_cpu_cycle(&mut self, cycles: u8);

    /// CPU cycles the machine has run since it was created.
    fn cycle_count(&self) -> u64 {
        0
    }

    /// The NMI line's level — the VBlank flag with the NMI enable, in Mesen's
    /// terms the PPU's `_state.NmiFlag`.
    ///
    /// The CPU samples this once per cycle and latches a rising edge; what it
    /// makes of it is [`crate::cpu::Cpu`]'s business, as in Mesen.
    fn nmi_line(&self) -> bool {
        false
    }

    /// The IRQ line's level — any source asserting it (APU frame counter, DMC,
    /// a mapper), Mesen's `_state.IrqFlag`.
    fn irq_pending(&self) -> bool;
}
