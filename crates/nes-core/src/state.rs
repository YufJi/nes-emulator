//! Binary save-state encoding.
//!
//! A save state is the machine's *mutable* state: registers, RAM, bank
//! selections, IRQ counters and the like. Immutable cartridge data (the
//! large PRG-ROM image) is deliberately left out, so a state is applied to the
//! cartridge the host already has loaded. That keeps states small and makes
//! them independent of the ROM contents.
//!
//! The encoding is a dependency-free little-endian byte stream. [`StateField`]
//! is implemented for the primitive and container types the machine is built
//! from, and the [`state_fields`] / [`mapper_state`] macros generate the
//! field-by-field code for structs and mappers.

use crate::prelude::*;

/// A type that can be written into, and restored from, a save state.
pub trait StateField {
    /// Append this value to the stream.
    fn write(&self, w: &mut StateWriter);
    /// Overwrite this value with the next bytes of the stream.
    ///
    /// Reading past the end of the stream yields zeroes so a truncated state
    /// cannot panic; callers that care check the length up front.
    fn read(&mut self, r: &mut StateReader<'_>);
}

/// Little-endian save-state writer.
#[derive(Default)]
pub struct StateWriter {
    buf: Vec<u8>,
}

impl StateWriter {
    /// Create an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finish and return the encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
}

/// Save-state reader; see [`StateWriter`].
pub struct StateReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

/// Upper bound on a length-prefixed collection, guarding against a corrupt
/// state claiming a huge length and exhausting memory.
const MAX_VEC_LEN: usize = 1 << 24;

impl<'a> StateReader<'a> {
    /// Wrap a byte slice to read from.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Whether every byte has been consumed.
    pub fn is_exhausted(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn u8(&mut self) -> u8 {
        let value = self.buf.get(self.pos).copied().unwrap_or(0);
        self.pos = self.pos.saturating_add(1);
        value
    }

    pub fn i8(&mut self) -> i8 {
        self.u8() as i8
    }

    pub fn u16(&mut self) -> u16 {
        self.u8() as u16 | ((self.u8() as u16) << 8)
    }

    pub fn i16(&mut self) -> i16 {
        self.u16() as i16
    }

    pub fn u32(&mut self) -> u32 {
        self.u16() as u32 | ((self.u16() as u32) << 16)
    }

    pub fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    pub fn u64(&mut self) -> u64 {
        self.u32() as u64 | ((self.u32() as u64) << 32)
    }

    pub fn usize(&mut self) -> usize {
        self.u64() as usize
    }

    pub fn f64(&mut self) -> f64 {
        f64::from_le_bytes(self.u64().to_le_bytes())
    }

    pub fn bool(&mut self) -> bool {
        self.u8() != 0
    }

    /// Read a length prefix for a collection.
    pub fn collection_len(&mut self) -> usize {
        (self.u32() as usize).min(MAX_VEC_LEN)
    }
}

macro_rules! state_field_scalar {
    ($($ty:ty => $method:ident),* $(,)?) => {$(
        impl StateField for $ty {
            fn write(&self, w: &mut StateWriter) {
                w.$method(*self);
            }
            fn read(&mut self, r: &mut StateReader<'_>) {
                *self = r.$method();
            }
        }
    )*};
}

state_field_scalar!(
    u8 => u8,
    i8 => i8,
    u16 => u16,
    i16 => i16,
    u32 => u32,
    i32 => i32,
    u64 => u64,
    usize => usize,
    f64 => f64,
);

impl StateField for bool {
    fn write(&self, w: &mut StateWriter) {
        w.bool(*self);
    }
    fn read(&mut self, r: &mut StateReader<'_>) {
        *self = r.bool();
    }
}

impl<T: StateField, const N: usize> StateField for [T; N] {
    fn write(&self, w: &mut StateWriter) {
        for value in self {
            value.write(w);
        }
    }
    fn read(&mut self, r: &mut StateReader<'_>) {
        for value in self.iter_mut() {
            value.read(r);
        }
    }
}

macro_rules! state_field_vec {
    ($($ty:ty),* $(,)?) => {$(
        impl StateField for Vec<$ty> {
            fn write(&self, w: &mut StateWriter) {
                w.u32(self.len() as u32);
                for value in self {
                    value.write(w);
                }
            }
            fn read(&mut self, r: &mut StateReader<'_>) {
                let len = r.collection_len();
                self.clear();
                self.resize(len, <$ty>::default());
                for value in self.iter_mut() {
                    value.read(r);
                }
            }
        }
    )*};
}

state_field_vec!(u8, u16, u32);

impl<T: StateField + Default> StateField for Option<T> {
    fn write(&self, w: &mut StateWriter) {
        match self {
            Some(value) => {
                w.u8(1);
                value.write(w);
            }
            None => w.u8(0),
        }
    }
    fn read(&mut self, r: &mut StateReader<'_>) {
        if r.u8() != 0 {
            self.get_or_insert_with(T::default).read(r);
        } else {
            *self = None;
        }
    }
}

/// Generate a [`StateField`] impl that stores each listed field in order.
macro_rules! state_fields {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::state::StateField for $ty {
            fn write(&self, w: &mut $crate::state::StateWriter) {
                $( $crate::state::StateField::write(&self.$field, w); )*
            }
            fn read(&mut self, r: &mut $crate::state::StateReader<'_>) {
                $( $crate::state::StateField::read(&mut self.$field, r); )*
            }
        }
    };
}
pub(crate) use state_fields;

/// Generate the [`crate::mapper::Mapper`] save/load methods for one mapper.
///
/// Invoke inside the mapper's `impl Mapper for ...` block, listing only the
/// mutable fields (ROM images are skipped).
macro_rules! mapper_state {
    // `chr_if ram: <flag>, <chr field>, <rest…>` handles the common case where
    // the CHR image doubles as writable CHR-RAM: the bytes are only stored when
    // the flag says the cartridge actually uses CHR-RAM.
    (chr_if ram: $ram:ident, $chr:ident $(, $field:ident)* $(,)?) => {
        fn save_state(&self, w: &mut $crate::state::StateWriter) {
            $crate::state::StateField::write(&self.$ram, w);
            let is_ram = self.$ram;
            $crate::state::write_chr_ram(w, is_ram, &self.$chr);
            $( $crate::state::StateField::write(&self.$field, w); )*
        }
        fn load_state(&mut self, r: &mut $crate::state::StateReader<'_>) {
            $crate::state::StateField::read(&mut self.$ram, r);
            let is_ram = self.$ram;
            $crate::state::read_chr_ram(r, is_ram, &mut self.$chr);
            $( $crate::state::StateField::read(&mut self.$field, r); )*
        }
    };
    ($($field:ident),* $(,)?) => {
        fn save_state(&self, w: &mut $crate::state::StateWriter) {
            $( $crate::state::StateField::write(&self.$field, w); )*
        }
        fn load_state(&mut self, r: &mut $crate::state::StateReader<'_>) {
            $( $crate::state::StateField::read(&mut self.$field, r); )*
        }
    };
}
pub(crate) use mapper_state;

/// Write the CHR-RAM bytes that share a field with the CHR-ROM image.
///
/// Nothing is written when the cartridge has CHR-ROM, which keeps the (large,
/// immutable) ROM image out of the save state.
pub fn write_chr_ram(w: &mut StateWriter, is_ram: bool, data: &[u8]) {
    if is_ram {
        w.u32(data.len() as u32);
        for byte in data {
            w.u8(*byte);
        }
    }
}

/// Read the CHR-RAM bytes written by [`write_chr_ram`].
pub fn read_chr_ram(r: &mut StateReader<'_>, is_ram: bool, data: &mut Vec<u8>) {
    if is_ram {
        let len = r.collection_len();
        data.clear();
        data.resize(len, 0);
        for byte in data.iter_mut() {
            *byte = r.u8();
        }
    }
}
