# o1-verifier-zisk

A [ZisK](https://0xpolygonhermez.github.io/zisk/) port of the out-of-circuit
**Mina / Pickles blockchain-SNARK verifier**. This is the ZisK counterpart of
the SP1 guest in [`crates/o1-verifier`](../o1-verifier): same verification
logic (reused verbatim from [`crates/pickles-verifier`](../pickles-verifier)),
different zkVM shell.

The program proves the statement:

> *"I ran the Pickles wrap-proof verifier on this `VerifiableProof` and it
> returned `true`."*

i.e. it produces a zk-STARK attesting that a Mina blockchain SNARK was
verified correctly inside the ZisK zkVM.

## Layout

This is a **self-contained ZisK workspace** (not a member of the parent
`cronos-to-zkvm` workspace, because the guest is compiled for the ZisK RISC-V
target by `cargo-zisk`, not by the parent's host `cargo build`). It follows the
standard `cargo-zisk new` shape:

```
o1-verifier-zisk/
├── Cargo.toml          # workspace root (members: guest, host)
├── guest/              # the zkVM program (RV64IMA → riscv64ima-zisk-zkvm-elf)
│   ├── build.rs        # bakes the wrap VK + SRSes into verifier.bin (VK_JSON)
│   └── src/main.rs     # ziskos::entrypoint! → read proof → verify → commit
└── host/               # host driver: fixture dir → bincode input.bin
    └── src/main.rs
```

### What differs from the SP1 guest

The verification core is identical. Only the zkVM I/O shell changed:

| | SP1 (`o1-verifier`) | ZisK (`o1-verifier-zisk`) |
|--|--|--|
| entrypoint | `sp1_zkvm::entrypoint!(main)` | `ziskos::entrypoint!(main)` |
| read input | `sp1_zkvm::io::read::<T>()` | `ziskos::io::read::<T>()` |
| commit output | `sp1_zkvm::io::commit(&v)` | `ziskos::io::commit(&v)` |
| input wire format | SP1 stdin (bincode) | `input.bin` = bincode v2 `config::standard()` |
| host driver | runs SP1 prover/executor | writes `input.bin` for `cargo-zisk` |

Both bake the verifier blob the same way (`build.rs` + `include_bytes!` +
`#[repr(C, align(8))]`). The blob is zkVM-agnostic.

> **Precompile note.** The SP1 guest routes Pasta `Fp`/`Fq` Montgomery
> multiplication through SP1's `sys_bigint` precompile (the `sp1` feature on
> `mina-curves`). ZisK has no equivalent binding wired into this `mina-curves`
> fork, so this guest runs the field arithmetic in pure software. Verifying a
> blockchain SNARK is field-multiplication-heavy, so this is the obvious first
> optimization target — ZisK exposes `arith256`/`arith256_mod` precompiles that
> a future `zisk` feature on `mina-curves` could route Montgomery mul through.

## Prerequisites

1. Install the ZisK toolchain (`cargo-zisk`, `ziskemu`, the `zisk` rustc
   target) via ziskup — see the
   [ZisK installation guide](https://0xpolygonhermez.github.io/zisk/getting_started/installation.html):
   ```bash
   curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
   ```
   When prompted for installation options, select **4) None** (CPU build, no
   proving key needed for emulation).

   **macOS note:** If the installer fails downloading the Rust toolchain, download
   `rust-toolchain-aarch64-apple-darwin.tar.gz` manually from
   [0xPolygonHermez/rust releases](https://github.com/0xPolygonHermez/rust/releases),
   then extract and register:
   ```bash
   tar -xzf ~/Downloads/rust-toolchain-aarch64-apple-darwin.tar.gz -C ~/.zisk/
   xattr -rd com.apple.quarantine ~/.zisk/
   rustup toolchain link zisk ~/.zisk
   ```

2. Initialize the `mina` git submodule (if not already done):
   ```bash
   git submodule update --init --recursive
   ```

3. Have a fixture directory available (the repo ships several under
   `fixtures/`, e.g. `fixtures/mainnet-blockchain-snark`). Each contains
   `vk.serde.json`, `proof.serde.json`, `public_input_skeleton.json`,
   `app_statement.json`.

All commands below assume you are in `crates/o1-verifier-zisk/`. Use **absolute
paths** for `VK_JSON` and `--fixture-dir` to avoid build-script cwd confusion.

```bash
cd crates/o1-verifier-zisk
REPO=$(cd ../.. && pwd)
FIXTURE="$REPO/fixtures/mainnet-blockchain-snark"
```

## 1. Build the guest (bakes in the wrap VK)

The wrap VK is compiled into the ELF, so the build is fixture-VK-specific. Set
`VK_JSON` to the **absolute path** of the fixture's VK:

```bash
VK_JSON="$FIXTURE/vk.serde.json" cargo-zisk build --release --bin o1-verifier-zisk-guest
```

The ELF lands at
`target/elf/riscv64ima-zisk-zkvm-elf/release/o1-verifier-zisk-guest`.

## 2. Generate the input file

The host driver assembles the four wire files into a single
bincode-encoded `VerifiableProof` (the exact format `ziskos::io::read`
decodes). The output is automatically padded to 8-byte alignment as
required by the ZisK emulator.

```bash
cargo build --release -p o1-verifier-zisk-host
./target/release/gen-input \
  --fixture-dir "$FIXTURE" \
  --output tmp/input.bin
```

## 3. Execute in the emulator (no proof)

Sanity-check correctness first — this just runs the program and surfaces the
committed output:

```bash
VK_JSON="$FIXTURE/vk.serde.json" cargo-zisk run --release --bin o1-verifier-zisk-guest -i tmp/input.bin
```

If you hit `EmulationNoCompleted` (the verifier is a large computation), drive
`ziskemu` directly with a higher step bound:

```bash
ziskemu -e target/elf/riscv64ima-zisk-zkvm-elf/release/o1-verifier-zisk-guest \
  -i tmp/input.bin -n 100000000000
```

A valid proof commits a single `0x01` byte (bincode `true`) as the public
output. Add `-m` for performance metrics or `-p summary` for a cost breakdown.

> **Known issue (ZisK v1.0.0-alpha):** The emulator may panic with
> `opc_fcall() FCALL_INPUT_READY_ID called with required_address > 0x7fffffff`.
> This appears to be an address-space limitation in the alpha emulator when the
> guest's baked `verifier.bin` blob (~45 MB) pushes memory usage past the 2 GB
> boundary. Pending resolution upstream.

## 4. Prove

```bash
cargo-zisk program-setup
cargo-zisk prove -i tmp/input.bin -o proof.bin
cargo-zisk verify -p proof.bin
```

## Caveats

- **VK/fixture match is unchecked.** The guest has one wrap VK baked in; you
  must feed it a fixture against that same VK. There is no runtime mismatch
  guard (same limitation as the SP1 guest).
- **`VK_JSON` must be an absolute path.** Build scripts run with a different cwd
  than the workspace root; relative paths will fail with "No such file".
- **`ziskos` version pinning.** The `ziskos` git dependency in the root
  `Cargo.toml` should track the ZisK release your `cargo-zisk` came from. If
  the build complains about an ABI/intrinsics mismatch, pin it to the matching
  tag/rev.
- **Input alignment.** ZisK requires `input.bin` size to be a multiple of 8
  bytes. The host driver handles this automatically.
- **Single proof only.** This wires up `verify` (one proof). The underlying
  `pickles_verifier::verify_batch` supports batches; extend the guest +
  host I/O to a `Vec<VerifiableProof>` if you need batching.
