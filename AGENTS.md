# AGENTS.md — smartboard

A digital whiteboard app.

## Toolchain

- **Rust nightly** required (edition 2024).

## Build

```bash
cargo build            # debug
cargo build --release  # release
```

## Architecture

- GUI app using **egui + wgpu + winit**
- Entrypoint: `src/main.rs`
- States: `src/state.rs`, rendering: `src/render.rs`, app logic: `src/app.rs`
- Utilities: `src/utils.rs`

## Features

- Wayland enabled (`egui-winit` and `winit` both have `wayland` feature)
- `wgpu` built with `webgpu` feature
