# LinuxBoy 0.1

Portable gaming manager for Linux. Creates self-contained AppImage capsules for Windows games using Wine/Proton.

## Features

- Drag-and-drop installer or portable game folder to create AppImage capsules
- System dependency checker (GPU, Vulkan, Wine, DXVK)
- Per-capsule Wine/Proton version selection
- Redistributables installer (Visual C++, .NET, DirectX)
- Capsule editor for mods and file browsing
- Backup and restore capsules

## Requirements

- Debian-based Linux distribution (Ubuntu, Pop!_OS, Linux Mint, etc.)
- Wine or Proton installed
- Vulkan support (for 3D games)

## Installation

Download the latest release or build from source:

```bash
cargo build --release
```

## Usage

1. Launch LinuxBoy
2. Drag installer (.exe, .msi) or game folder into the window
3. Select main executable and launch options
4. AppImage capsule is created in ~/Games/

## Capsule Structure

```
GameName.AppImage              # Game files (read-only)
GameName.AppImage.home/        # Saves, Wine prefix, cache (created on first launch)
```

## License

GPL v3. See LICENSE file.

## Contributing

Contributions welcome. Submit pull requests or open issues on GitHub.
