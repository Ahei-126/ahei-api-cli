# AHEIAPI Codex CLI

A branded build of OpenAI's [Codex CLI](https://github.com/openai/codex) that connects directly to the **AHEIAPI** relay through [New API](https://github.com/Calcium-Ion/new-api).

- Default relay: `https://new.ahei.asia`
- Default product name: `AHEIAPI`

After logging in with your relay account, the CLI can create a new API key or pick an existing one, then write the provider config so `codex` talks to your relay out of the box.

> This is a community / self-hosted build. It is **not** affiliated with OpenAI. Usage with any third-party relay is subject to that provider's terms of service.

---

## Quick start

### Download a package

Each GitHub Release contains pre-built packages. Pick the one for your platform:

| Platform | Archive |
| --- | --- |
| Windows x64 | `codex-x86_64-pc-windows-msvc.zip` |
| macOS Apple Silicon (arm64) | `codex-aarch64-apple-darwin.tar.gz` |
| macOS Intel (x86_64) | `codex-x86_64-apple-darwin.tar.gz` |

Extract the archive, then rename the binary to `codex` (if needed) and put it on `PATH`.

### Sign in to your relay

Run the binary:

```sh
codex
```

You will be prompted to:

1. Enter the relay base URL (default: `https://new.ahei.asia`, omit `/v1`).
2. Log in with your relay account (username / password).
3. Create a new API key or select an existing one.
4. Choose a model id (default: `gpt-4o`).

The CLI writes a provider config on disk so subsequent `codex` runs use your New API key automatically.

---

## Building from source

### Prerequisites

- Rust toolchain (stable). Install via [rustup](https://rustup.rs).
- Python 3
- [just](https://github.com/casey/just) (optional but recommended)

### Build

```sh
cd codex-rs
just build
```

To bake in a different relay / product name at build time:

```sh
NEWAPI_BASE_URL="https://new.ahei.asia" \
NEWAPI_PRODUCT_NAME="AHEIAPI" \
just build
```

### Packaging

The release workflow (`.github/workflows/release-newapi.yml`) builds Windows / macOS archives and publishes a GitHub Release whenever you push a `v*` tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

---

## License

Apache-2.0. See [LICENSE](LICENSE).
