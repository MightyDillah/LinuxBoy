# LinuxBoy app overview

LinuxBoy is a portable gaming manager for Linux that builds self-contained
AppImage "capsules" for Windows games using Proton/Wine runtimes. It takes a
Windows installer or a pre-installed game folder, creates a dedicated prefix,
and packages everything into a single, portable AppImage plus a companion
`.AppImage.home/` directory for saves and runtime data.

## What it does

- Creates and manages Proton/Wine-based capsules for Windows games
- Builds a portable AppImage that can be moved between Linux machines
- Keeps game data and the prefix in the `.AppImage.home/` directory
- Handles launcher configuration so the game runs from the capsule

## Typical workflow

1. Launch LinuxBoy
2. Drag an installer (`.exe`, `.msi`) or game folder into the window
3. Select the main executable and launch options
4. LinuxBoy builds the AppImage in `~/Games/`

## Capsule layout

```
GameName.AppImage              # Read-only game and runtime bundle
GameName.AppImage.home/        # Saves, Wine prefix, cache (created on first run)
```

## Dependencies

LinuxBoy expects a Debian/Ubuntu-based system with required system packages
installed for building the app. Runtime setup scripts also install required
graphics and runtime packages.
