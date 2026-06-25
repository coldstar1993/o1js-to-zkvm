//! ZisK guest: out-of-circuit Pickles verification.
//!
//! This is the ZisK counterpart of the SP1 guest in `crates/o1-verifier`. The
//! heavy lifting — `decode_verifier_blob` + `verify` — is the same `no_std`
//! `pickles-verifier` code; only the zkVM-specific shell differs:
//!
//!   SP1                              ZisK
//!   ---------------------------      ------------------------------------
//!   `sp1_zkvm::entrypoint!(main)`    `ziskos::entrypoint!(main)`
//!   `sp1_zkvm::io::read::<T>()`      `ziskos::io::read::<T>()`   (bincode)
//!   `sp1_zkvm::io::commit(&v)`       `ziskos::io::commit(&v)`    (bincode)
//!
//! At build time, `build.rs` reads the wrap `vk.serde.json` (path via the
//! `VK_JSON` env var) and writes a serialized [`pickles_verifier::Verifier`]
//! blob to `OUT_DIR/verifier.bin`. The guest `include_bytes!`s it and
//! reinstantiates the verifier mostly zero-parse via
//! [`pickles_verifier::serialize::decode_verifier_blob`] (pod-cast SRSes +
//! postcard wrap VK).
//!
//! At runtime, the host driver (`../host`) writes one bincode-encoded
//! [`pickles_verifier::types::VerifiableProof`] to `input.bin`; the guest
//! reads it with `ziskos::io::read` (ZisK decodes the input buffer with
//! `bincode::config::standard()`), runs [`pickles_verifier::verify`], and
//! commits the boolean result as a public output.

#![no_main]

ziskos::entrypoint!(main);

use pickles_verifier::serialize::decode_verifier_blob;
use pickles_verifier::types::VerifiableProof;
use pickles_verifier::verify;

/// 8-byte aligned wrapper around `include_bytes!`. The blob's pod-cast
/// sections (`PodVesta` / `PodPallas`) require 8-byte alignment for the
/// `bytemuck::cast_slice` casts inside `decode_verifier_blob`; raw
/// `include_bytes!` data is 1-byte aligned. (Same trick as the SP1 guest.)
#[repr(C, align(8))]
struct Aligned<T: ?Sized>(T);

static VERIFIER_BYTES: &Aligned<[u8]> =
    &Aligned(*include_bytes!(concat!(env!("OUT_DIR"), "/verifier.bin")));

pub fn main() {
    // Rebuild the per-tag verifier constants from the baked blob.
    let verifier = decode_verifier_blob(&VERIFIER_BYTES.0);

    // Read the runtime proof from ZisK's input buffer. `ziskos::io::read`
    // bincode-decodes the whole `input.bin` (config::standard()), which is
    // exactly what the host writes.
    let proof: VerifiableProof = ziskos::io::read();

    // The actual out-of-circuit Pickles check (shared no_std logic).
    let valid: bool = verify(&verifier, &proof);

    // Commit the result as a public output. bincode-encodes a single byte
    // (0x01 = valid, 0x00 = invalid) into the first output slot.
    ziskos::io::commit(&valid);
}
