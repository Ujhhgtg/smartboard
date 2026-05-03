# uwu (ujhhgtg's whiteboard, unleashed)

A high-performance digital whiteboard app, designed for touchscreen devices.

## Toolchain

- **Rust nightly** required (edition 2024).

## Build

```bash
cargo build            # debug
cargo build --release  # release
```

## Lint

```bash
cargo clippy --release
```

## Architecture

- GUI app using **egui + wgpu + winit**
- Entrypoint: `src/main.rs`
- States: `src/state/mod.rs`
- rkyv serialization states: `src/state/flat.rs`
- Rendering: `src/render.rs`
- App logic: `src/app.rs`
- Utilities: `src/utils/*.rs`
- UI content: `src/ui.rs`

