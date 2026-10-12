//! Generates the Swift/Kotlin bindings for `nes-uniffi`.
//!
//! Kept as a crate-local binary so the bindgen used is always the one matching
//! the runtime crate; see the crate docs for the exact invocation.

fn main() {
    uniffi::uniffi_bindgen_main()
}
