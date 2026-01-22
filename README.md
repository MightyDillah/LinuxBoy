# LinuxBoy 0.1

Portable gaming manager for Linux. Creates self-contained AppImage capsules for Windows games using Wine/Proton.

## Prerequisites (Required to Build)

Debian/Ubuntu/Pop!_OS/Linux Mint:

```bash
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libgtk-4-dev \
    libglib2.0-dev \
    libcairo2-dev \
    libpango1.0-dev \
    libgdk-pixbuf-2.0-dev \
    libgraphene-1.0-dev
```

## Building from Source

After installing prerequisites:

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
