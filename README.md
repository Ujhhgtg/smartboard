# smartboard

a high-performance whiteboard app written in rust

reinventing the wheel because ~~others suck~~ why not

## building

```bash
rustup toolchain install nightly
rustup default nightly

# --- system deps ---
sudo apt install libasound2-dev libglib2.0-dev libgtk-3-dev libappindicator3-dev libxdo-dev pkg-config
# or for arch linux
yay -S alsa-lib gtk3 libappindicator xdotool pkgconf
# --- end ---

# --- cargo build ---
cargo build --release
# or with cjk font embedded
cargo build --release --no-default-features --features embedded_font
# --- end ---
```

## tech stack

egui + wgpu + winit

## license

gpl 3
