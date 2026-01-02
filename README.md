# Smartboard - Digital Whiteboard Application

A powerful digital whiteboard and drawing application built with Rust, winit, egui, and wgpu.

## Features

### 🎨 Drawing Tools

- **Brush Tool**: Draw with customizable width and color
- **Dynamic Brush Width**: Brush tip simulation and speed-based width adjustment
- **Object Eraser**: Remove entire objects with a single click
- **Pixel Eraser**: Erase parts of strokes with precision
- **Quick Colors**: Fast access to frequently used colors
- **Brush Size Preview**: Visual feedback for brush/eraser size

### 🖼️ Object Management

- **Image Insertion**: Add images from files with aspect ratio preservation
- **Text Objects**: Insert and edit text with customizable font size and color
- **Shapes**: Add lines, arrows, rectangles, triangles, and circles
- **Object Selection**: Click to select and manipulate objects
- **Object Transformation**: Resize and rotate objects with visual anchors
- **Object Deletion**: Remove unwanted objects easily

### 📝 Canvas Operations

- **Save/Load**: Persist your work to JSON files
- **Multi-touch Support**: Draw with multiple fingers on touch devices
- **Stroke Smoothing**: Automatic stroke smoothing for cleaner lines
- **Point Interpolation**: Adjustable interpolation frequency for smoother curves
- **Undo/Redo**: Basic undo functionality through save/load

### ⚙️ Customization

- **Theme Modes**: System, Light, and Dark themes
- **Background Colors**: Customize your canvas background
- **Window Modes**: Windowed, Fullscreen, and Borderless Fullscreen
- **Performance Settings**: Optimization policies (Performance vs Resource Usage)
- **Vertical Sync**: Multiple present modes for optimal rendering
- **Quick Color Editor**: Manage your palette of quick-access colors

### 🚀 Advanced Features

- **Startup Animation**: Beautiful animated introduction with audio
- **FPS Display**: Real-time performance monitoring
- **Touch Point Visualization**: Debug touch input (enable in settings)
- **Console Toggle**: Show/hide console on Windows
- **Pressure Testing**: Generate test strokes for performance evaluation

## Installation

### Prerequisites

- Rust (latest stable version)
- Cargo (Rust package manager)

### Build and Run

```bash
# Clone the repository
git clone https://github.com/Ujhhgtg/smartboard.git
cd smartboard

# Build the project
cargo build --release

# Run the application
cargo run --release
```

## Usage

### Basic Controls

- **Mouse/Touch**: Draw on the canvas
- **Toolbar**: Access tools and settings at the bottom
- **ESC Key**: Exit the application
- **Window Close**: Properly saves settings before exiting

### Tool Selection

1. **Brush**: Draw freehand strokes
2. **Object Eraser**: Click objects to remove them
3. **Pixel Eraser**: Erase parts of strokes
4. **Insert**: Add images, text, or shapes
5. **Settings**: Configure application preferences
6. **Select**: Choose and manipulate existing objects

### Keyboard Shortcuts

- **ESC**: Exit application

## Configuration

Smartboard automatically saves your preferences to:

- **Windows**: `%APPDATA%\smartboard\settings.json`
- **Linux/macOS**: `~/.config/smartboard/settings.json`

### Settings Categories

- **Appearance**: Theme, background color, startup behavior
- **Drawing**: Stroke smoothing, interpolation, quick colors
- **Performance**: Window mode, optimization policy, VSync
- **Debug**: FPS display, touch points, console visibility

## Technical Details

### Architecture

- **Rendering**: GPU-accelerated with wgpu
- **UI Framework**: egui for immediate-mode UI
- **Window Management**: winit for cross-platform windows
- **Audio**: rodio for startup animation sound
- **Serialization**: serde for saving/loading canvas state

### Performance Optimization

- **Memory Management**: Configurable optimization policies
- **Rendering Pipeline**: Efficient GPU resource usage
- **Input Handling**: Low-latency touch and mouse processing

### Cross-Platform Support

- **Windows**: Full support with console toggle
- **Linux**: Wayland support
- **macOS**: Native window management
- **Web**: Potential WebAssembly target (experimental)

## Development

### Building from Source

```bash
# Install dependencies
cargo build

# Run in development mode (shows console)
cargo run

# Run tests
cargo test
```

### Project Structure

```none
smartboard/
├── src/
│   ├── main.rs          # Entry point
│   ├── app.rs           # Main application logic
│   ├── state.rs         # State management
│   ├── render.rs        # Rendering pipeline
│   └── utils.rs         # Utility functions
├── assets/
│   └── startup_animation/ # Animation frames and audio
├── Cargo.toml          # Dependencies and build configuration
└── README.md           # This file
```

### Dependencies

- **egui**: Immediate mode GUI library
- **wgpu**: WebGPU implementation for Rust
- **winit**: Cross-platform window creation
- **rodio**: Audio playback
- **serde**: Serialization framework
- **rfd**: File dialogs
- **image**: Image processing

## Troubleshooting

### Common Issues

- **Missing CJK Fonts**: Ensure you have Chinese/Japanese/Korean fonts installed
- **GPU Compatibility**: Update graphics drivers for best performance
- **Audio Playback**: Check system audio settings if startup sound doesn't play

### Debugging

Enable the following in settings for troubleshooting:

- **Show Console**: Access detailed logs
- **Show FPS**: Monitor performance
- **Show Touch Points**: Visualize touch input

## Contributing

Contributions are welcome! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch
3. Implement your changes
4. Submit a pull request
5. Ensure all tests pass

### Development Tips

- Use `cargo fmt` for code formatting
- Run `cargo clippy` for linting
- Test on multiple platforms when possible

## License

This project is licensed under the terms specified in the LICENSE file.

## Contact

For questions or support, please open an issue on the GitHub repository.
