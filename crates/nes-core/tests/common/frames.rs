//! Golden frame hashes: the same instrument Mesen's own test suite uses.
//!
//! Mesen's `.mtp` tests (`Core/Shared/RecordedRomTest`) hash **every** frame the
//! console produces — SHA1 over the raw frame buffer's bytes — and compare the
//! run-length-encoded list against the one recorded with the test. The verdict
//! is therefore about *pixels*, which is exactly what the self-reporting ROMs
//! cannot see: the sprite plane and the background palette bugs of 2026-10-15
//! (see `docs/mesen-alignment.md` §7, rows B1/B2) passed every suite in this
//! directory while drawing the wrong picture.
//!
//! This is that instrument on our side, with the same shape: one SHA1 per
//! frame, stored as `hash xCount` lines with runs of at most 255 (Mesen caps
//! them the same way). The hash of a frame is comparable with Mesen's — same
//! bytes, same order, and the palette was checked entry for entry — so the
//! goldens here can be *replaced* by values Mesen produced, once a `.mtp`
//! reader exists (its container is a zip holding `TestMovie.mmo` and the hash
//! table; §5.1 of the alignment document has the details).
//!
//! Until then these files are a **regression net, not a verdict**: they say a
//! ROM's picture did not move, not that it is right. When a rendering fix lands
//! the goldens must be re-recorded deliberately, and the diff read.

use std::collections::VecDeque;
use std::path::PathBuf;

use nes_core::Nes;

/// SHA1 of one frame of the console's own buffer, Mesen's `SHA1::GetHash(frame.
/// FrameBuffer, frame.FrameBufferSize)` — the pixels as the core produced them,
/// before any video filter, in memory order (little endian, so `AARRGGBB`).
fn frame_hash(nes: &Nes) -> String {
    let mut bytes = Vec::with_capacity(nes.frame_buffer().len() * 4);
    for pixel in nes.frame_buffer() {
        bytes.extend_from_slice(&pixel.to_le_bytes());
    }
    crate::common::sha1::hex(&bytes)
}

/// Collects one hash per frame, run-length encoded the way Mesen stores them.
/// A cheap fingerprint of a frame, used only to notice that the picture moved.
///
/// SHA1 over 245 KB costs about a tenth of a second in a debug build — a suite
/// would take ten minutes — so the expensive hash is taken when this says the
/// frame differs from the last one, which for these ROMs is a few dozen times
/// per run rather than thousands. It is FNV-1a over the pixels: not a substitute
/// for the SHA1, just a trigger for it. The cost is that a picture that changes
/// and changes back inside one frame goes unnoticed.
fn fingerprint(nes: &Nes) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for pixel in nes.frame_buffer() {
        hash ^= *pixel as u64;
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }
    hash
}

#[derive(Default)]
pub struct FrameRecorder {
    hashes: VecDeque<(String, u8)>,
    /// Frames seen for the ROM being recorded.
    frames: u32,
    /// The last frame's fingerprint, to notice a change without hashing.
    last: Option<u64>,
    /// Where the goldens for this suite live, and what to call this one.
    directory: PathBuf,
    name: String,
}

impl FrameRecorder {
    /// A recorder for `name`, whose goldens sit next to the other ROMs' ones.
    pub fn new(relative: &str) -> Self {
        let mut directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        directory.push("tests/golden");
        directory.push(relative);
        let name = String::new();
        Self {
            hashes: VecDeque::new(),
            frames: 0,
            last: None,
            directory,
            name,
        }
    }

    /// Start recording a ROM's frames under this name.
    pub fn begin(&mut self, name: &str) {
        self.name = name.to_string();
        self.hashes.clear();
        self.frames = 0;
        self.last = None;
    }

    /// One frame, as the console has just finished it.
    ///
    /// Only every [`SAMPLE_EVERY`]th frame is hashed; the run's length still
    /// counts every one, so a ROM that stops early and one that runs long are
    /// distinguishable even when the screen does not change.
    pub fn step(&mut self, nes: &Nes) {
        self.frames += 1;
        let seen = fingerprint(nes);
        if self.last == Some(seen) {
            // Same picture as the last frame: the run just gets longer.
            if let Some((_, count)) = self.hashes.back_mut() {
                if *count < 255 {
                    *count += 1;
                    return;
                }
            }
        }
        self.last = Some(seen);
        let hash = frame_hash(nes);
        match self.hashes.back_mut() {
            Some((last, count)) if *last == hash && *count < 255 => *count += 1,
            _ => self.hashes.push_back((hash, 1)),
        }
    }

    /// The path this ROM's goldens are read from and written to.
    fn path(&self) -> PathBuf {
        self.directory.join(format!("{}.hashes", self.name))
    }

    /// Compare what was recorded with the goldens, or write them out when
    /// `NES_GOLDEN_RECORD` is set (the deliberate, reviewed act).
    ///
    /// Returns the complaint, when there is one.
    pub fn finish(&self) -> Option<String> {
        let path = self.path();
        let recorded = self.render();

        if std::env::var_os("NES_GOLDEN_RECORD").is_some() {
            std::fs::create_dir_all(&self.directory).ok()?;
            std::fs::write(&path, &recorded).ok()?;
            return None;
        }

        let Ok(expected) = std::fs::read_to_string(&path) else {
            return Some(format!(
                "no goldens at {} — record them with NES_GOLDEN_RECORD=1",
                path.display()
            ));
        };
        if expected == recorded {
            return None;
        }

        Some(format!(
            "frames moved from the goldens ({}):\n  expected {} entries, recorded {}",
            path.display(),
            expected.lines().count(),
            recorded.lines().count()
        ))
    }

    /// The recording as a file: a header saying what it is, then the runs.
    fn render(&self) -> String {
        let mut out = String::from(
            "# SHA1 over the raw frame buffer (`SHA1::GetHash(frame.FrameBuffer, ...)`, the same\n\
             # hash Mesen's .mtp tests use), from power-on, run-length encoded as `hash xFrames`.\n\
             # A frame is hashed when its picture differs from the previous one, so this is a\n\
             # regression net and not a verdict -- and not yet Mesen's own recorded values.\n",
        );
        for (hash, count) in &self.hashes {
            out.push_str(&format!("{hash} x{count}\n"));
        }
        out
    }
}
