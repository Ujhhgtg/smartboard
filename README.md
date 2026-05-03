# uwu - ujhhgtg's whiteboard, unleashed

a high-performance whiteboard app written in rust

reinventing the wheel because ~~others suck~~ why not

## building

### prepare

```bash
rustup toolchain install nightly
rustup default nightly

# --- system deps ---
sudo apt install libasound2-dev libglib2.0-dev libgtk-3-dev libappindicator3-dev libxdo-dev pkg-config
# or for arch linux
yay -S alsa-lib gtk3 libappindicator xdotool pkgconf
# --- end ---
```

### compile

```bash
cargo build --release
# or with cjk font embedded
cargo build --release --no-default-features --features embedded_font
```

### cross-compiling for windows from linux

#### prepare

good luck figuring this out if you're not using arch

```bash
# first add chaotic-aur, then
yay -S llvm-mingw llvm lld
```

add the following to `~/.cargo/config.toml`

```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"

[target.aarch64-pc-windows-gnullvm]
linker = "aarch64-w64-mingw32-clang"
ar = "aarch64-w64-mingw32-ar"
```

#### compile x86_64

```bash
export PATH=/opt/llvm-mingw/bin/:$PATH
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

#### compile aarch64

```bash
export PATH=/opt/llvm-mingw/bin/:$PATH
rustup target add aarch64-pc-windows-gnullvm
cargo build --release --target aarch64-pc-windows-gnullvm
```

## tech stack

egui + wgpu + winit

## license

gpl 3
