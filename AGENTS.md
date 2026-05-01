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

- GUI app using **egui + wgpu + winit** (not a web app, not a library)
- Entrypoint: `src/main.rs`
- Core state: `src/state.rs`, rendering: `src/render.rs`, app logic: `src/app.rs`

## Features

- Wayland enabled (`egui-winit` and `winit` both have `wayland` feature)
- `wgpu` built with `webgpu` feature
