//! Host driver for the `o1-verifier-zisk` guest.
//!
//! This mirrors `crates/o1-verifier-host` (the SP1 host), but instead of
//! streaming the assembled proof into `SP1Stdin` and invoking the SP1 prover,
//! it serializes the proof to a file that ZisK's `cargo-zisk run` / `prove` /
//! `ziskemu` tools consume via their `-i/--input` flag.
//!
//! Takes a fixture directory containing the OCaml-dumped pickles wire files —
//! ```text
//!   <fixture_dir>/
//!     vk.serde.json
//!     proof.serde.json
//!     public_input_skeleton.json
//!     app_statement.json
//! ```
//! — assembles a [`pickles_verifier::types::VerifiableProof`] host-side (via
//! `pickles_verifier::wire` parsers + `OcamlProof::into_verifiable`), and
//! writes it bincode-encoded to `--output` (default `tmp/input.bin`).
//!
//! IMPORTANT: the encoding MUST match what the guest's `ziskos::io::read`
//! expects — bincode v2 with `bincode::config::standard()`. The guest ELF
//! also has a wrap VK baked in at build time (via `VK_JSON`); the fixture
//! passed here MUST be against that same VK, exactly as in the SP1 flow.

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use pickles_verifier::wire::{parse_app_statement, parse_wrap_proof, parse_wrap_vk, OcamlProof};

#[derive(Parser)]
#[command(name = "gen-input")]
#[command(about = "Assemble a ZisK input.bin (bincode VerifiableProof) from a pickles fixture dir")]
struct Cli {
    /// Path to a fixture directory containing vk.serde.json, proof.serde.json,
    /// public_input_skeleton.json, and app_statement.json. The VK must match
    /// the one the guest was built against (see the guest's VK_JSON env var).
    #[arg(short, long)]
    fixture_dir: PathBuf,

    /// Where to write the ZisK input file. Pass this path to
    /// `cargo-zisk run -i <output>` / `cargo-zisk prove -i <output>`.
    #[arg(short, long, default_value = "tmp/input.bin")]
    output: PathBuf,
}

fn main() {
    let cli = Cli::parse();

    // Load the four wire files.
    let read = |name: &str| {
        let p = cli.fixture_dir.join(name);
        fs::read_to_string(&p).unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
    };
    let vk_json = read("vk.serde.json");
    let proof_json = read("proof.serde.json");
    let skeleton_json = read("public_input_skeleton.json");
    let app_stmt_json = read("app_statement.json");

    // Parse + assemble the VerifiableProof host-side (identical to the SP1 host).
    let wrap_vk = parse_wrap_vk(&vk_json).expect("parse vk.serde.json");
    let wrap_proof = parse_wrap_proof(&proof_json).expect("parse proof.serde.json");
    let ocaml = OcamlProof::parse(&skeleton_json).expect("parse public_input_skeleton.json");
    let app_stmt = parse_app_statement(&app_stmt_json).expect("parse app_statement.json");

    let verifiable = ocaml
        .into_verifiable(wrap_proof, &wrap_vk, &[app_stmt])
        .expect("OcamlProof::into_verifiable");

    // Encode exactly the way ZisK's `ziskos::io::read` decodes the input
    // buffer: bincode v2, standard config.
    let mut bytes = bincode::serde::encode_to_vec(&verifiable, bincode::config::standard())
        .expect("bincode-encode VerifiableProof");

    // ZisK emulator requires input size to be a multiple of 8 bytes.
    let remainder = bytes.len() % 8;
    if remainder != 0 {
        bytes.resize(bytes.len() + (8 - remainder), 0);
    }

    if let Some(parent) = cli.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
        }
    }
    fs::write(&cli.output, &bytes)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", cli.output.display()));

    println!(
        "wrote {} bytes ({} fixture) -> {}",
        bytes.len(),
        cli.fixture_dir.display(),
        cli.output.display()
    );
}
