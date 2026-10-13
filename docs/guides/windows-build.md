# Building SpaceKit on Windows

Two supported routes. **WSL is the recommended one** — it needs no extra
configuration and uses the same toolchain as every other Linux build.

| | WSL 2 | Native MSVC |
|---|---|---|
| Setup | install WSL, then build normally | seven tools, four environment variables |
| Produces | Linux ELF binaries, run inside WSL | native `.exe`, run from Explorer or PowerShell |
| Use when | you just need SpaceKit running | you need a Windows-native artifact to ship |

If you are running a workshop or hackathon, tell Windows participants to
install WSL first. It removes the whole native toolchain from the critical path.

---

## Route 1 — WSL 2 (recommended)

From an administrator PowerShell:

```powershell
wsl --install -d Ubuntu
```

Reboot if prompted, then open Ubuntu and build as you would on any Linux host:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev \
    clang llvm libclang-dev cmake ninja-build git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

git clone https://github.com/spacekit-xyz/spacekit-core.git
cd spacekit-core
cargo build --release -p spacekit
```

No SpaceKit-specific environment variables are needed on this route.

The resulting binaries live in `target/release/` inside the WSL filesystem and
are Linux ELF executables. Keep the checkout inside WSL (`~/…`, not `/mnt/c/…`)
— building across the Windows filesystem boundary is substantially slower.

---

## Route 2 — native MSVC

Produces real Windows `.exe` files. Needs more setup.

### Toolchain

Install, in any order:

- **Visual Studio 2022 Build Tools** with the *Desktop development with C++*
  workload — supplies `link.exe` and the Windows SDK
- **Rust** via [rustup](https://rustup.rs), MSVC host triple
  (`x86_64-pc-windows-msvc`, the default on Windows)
- **LLVM** — `oqs-sys` runs `bindgen`, which needs `libclang`
- **OpenSSL (Win64)** — liboqs builds with `OQS_USE_OPENSSL=Yes` by default
- **CMake** and **Ninja** — liboqs build system
- **Strawberry Perl** and **NASM** — OpenSSL and the PQ assembly paths

Chocolatey covers most of it:

```powershell
choco install -y llvm openssl cmake ninja strawberryperl nasm git
```

### Environment variables

All four are required. Without them the build fails in `oqs-sys` or liboqs,
usually with a `libclang` or `Could NOT find OpenSSL` error:

```powershell
setx LIBCLANG_PATH       "C:\Program Files\LLVM\bin"
setx OPENSSL_ROOT_DIR    "C:\Program Files\OpenSSL-Win64"
setx OPENSSL_LIB_DIR     "C:\Program Files\OpenSSL-Win64\lib\VC\x64\MD"
setx OPENSSL_INCLUDE_DIR "C:\Program Files\OpenSSL-Win64\include"
```

Open a new shell afterwards so the values are picked up.

### Build

```powershell
git clone https://github.com/spacekit-xyz/spacekit-core.git
cd spacekit-core
cargo build --release -p spacekit
cargo build --release -p spacekit-messaging-node --features standalone
```

Expect roughly 15–20 minutes for the first CLI build; liboqs and the
post-quantum crates dominate it.

### Known results

Measured on a clean Windows 11 machine:

| Binary | Size | Check |
|---|---|---|
| `spacekit.exe` | 72.9 MB | `--version` → `spacekit 0.1.0` |
| `spacekit-messaging-node.exe` | 11.5 MB | builds, runs |
| `spacekit-messaging-http.exe` | 10.4 MB | builds, runs |

---

## Troubleshooting

**`LNK2019: unresolved external symbol llm_inference`**, usually with
`LNK1120: unresolved externals`, on every binary.

`foundation/spacekit-primitives/src/v1/contracts/spacekit.rs` declares
`#[link(wasm_import_module = ...)]` host imports for the contract runtime.
They have no native definition. `link.exe` rejects undefined externs, where the
Apple and GNU linkers tolerate them — which is why this appears only on
Windows. Gate the module on `target_arch = "wasm32"` so it is not compiled for
native targets.

**`Unable to find libclang`** — `LIBCLANG_PATH` is unset or points at the wrong
directory. It must be the directory containing `libclang.dll`, normally
`C:\Program Files\LLVM\bin`.

**`Could NOT find OpenSSL`** — set all three OpenSSL variables, not just
`OPENSSL_ROOT_DIR`. The `lib\VC\x64\MD` path matters; `lib` alone is not enough
for a default Win64 install.

**`--help` exits non-zero.** Configuration is validated before help is printed,
so `spacekit --help` returns `-1`, `spacekit-messaging-http --help` reports
`Private key cannot be empty`, and `growformer-runtime --help` panics. Use
`--version` to confirm a binary works.

**Builds are very slow under WSL** — the checkout is probably on `/mnt/c`. Move
it into the WSL filesystem.
