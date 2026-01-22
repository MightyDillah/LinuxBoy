#!/bin/bash

# LinuxBoy AppImage Runtime Launcher
# This script is embedded in each game capsule

set -e

# Get the AppImage path
APPIMAGE_PATH="$(readlink -f "${APPIMAGE}")"
APPIMAGE_DIR="$(dirname "${APPIMAGE_PATH}")"
APPIMAGE_NAME="$(basename "${APPIMAGE_PATH}")"

# Define paths
HOME_DIR="${APPIMAGE_DIR}/${APPIMAGE_NAME}.home"
PREFIX_DIR="${HOME_DIR}/prefix"
CACHE_DIR="${HOME_DIR}/cache"
METADATA_FILE="${HOME_DIR}/metadata.json"

# Game executable and args (replaced during build)
GAME_EXE="{{GAME_EXE}}"
LAUNCH_ARGS="{{LAUNCH_ARGS}}"

# Initialize home directory structure
init_home_dir() {
    if [ ! -d "${HOME_DIR}" ]; then
        echo "First launch detected. Initializing capsule..."
        mkdir -p "${HOME_DIR}"
        mkdir -p "${PREFIX_DIR}"
        mkdir -p "${CACHE_DIR}"
        
        # Copy metadata template if exists
        if [ -f "${HERE}/metadata.template.json" ]; then
            cp "${HERE}/metadata.template.json" "${METADATA_FILE}"
        fi
        
        echo "Capsule initialized at: ${HOME_DIR}"
    fi
}

# Load metadata and configuration
load_config() {
    if [ -f "${METADATA_FILE}" ]; then
        # TODO: Parse JSON and set environment variables
        # For now, use defaults
        export DXVK_HUD=0
    fi
}

# Check for Wine
check_wine() {
    if ! command -v wine64 &> /dev/null; then
        echo "ERROR: Wine is not installed!"
        echo "Please install LinuxBoy manager to set up dependencies."
        exit 1
    fi
}

# Main launch sequence
main() {
    echo "LinuxBoy Capsule Launcher"
    echo "========================="
    echo "AppImage: ${APPIMAGE_NAME}"
    echo "Home Dir: ${HOME_DIR}"
    echo ""
    
    # Initialize if needed
    init_home_dir
    
    # Load configuration
    load_config
    
    # Check dependencies
    check_wine
    
    # Set Wine environment
    export WINEPREFIX="${PREFIX_DIR}"
    export WINEDEBUG=-all
    export WINE_LARGE_ADDRESS_AWARE=1
    
    # Set cache paths
    export __GL_SHADER_DISK_CACHE_PATH="${CACHE_DIR}"
    export DXVK_STATE_CACHE_PATH="${CACHE_DIR}"
    
    # Launch game
    cd "${HERE}/game"
    echo "Launching: ${GAME_EXE} ${LAUNCH_ARGS}"
    echo ""
    
    wine64 "${GAME_EXE}" ${LAUNCH_ARGS}
    
    EXIT_CODE=$?
    echo ""
    echo "Game exited with code: ${EXIT_CODE}"
    
    exit ${EXIT_CODE}
}

# Run
main "$@"
