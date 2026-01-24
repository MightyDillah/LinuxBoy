#!/usr/bin/env sh
set -eu

REINSTALL=0
if [ "${1:-}" = "--reinstall" ]; then
  REINSTALL=1
fi

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "sudo is required for system installs."
    exit 1
  fi
fi

APT_CMD=""
if command -v apt >/dev/null 2>&1; then
  APT_CMD="apt"
elif command -v apt-get >/dev/null 2>&1; then
  APT_CMD="apt-get"
else
  echo "apt/apt-get not found."
  exit 1
fi

APT_FLAGS=""
if [ "$REINSTALL" -eq 1 ]; then
  APT_FLAGS="--reinstall"
fi

apt_update() {
  $SUDO "$APT_CMD" update
}

apt_install() {
  $SUDO "$APT_CMD" install $APT_FLAGS "$@"
}

if [ -f /etc/os-release ]; then
  . /etc/os-release
else
  echo "/etc/os-release not found."
  exit 1
fi

if [ "${ID:-}" != "debian" ] && [ "${ID:-}" != "ubuntu" ]; then
  echo "This setup script currently supports Debian/Ubuntu."
  exit 1
fi

if ! dpkg --print-foreign-architectures | grep -qx "i386"; then
  $SUDO dpkg --add-architecture i386
fi

apt_update

BASE_PACKAGES="curl ca-certificates python3"
VULKAN_PACKAGES="vulkan-tools libvulkan1 libvulkan1:i386"
MESA_PACKAGES="mesa-vulkan-drivers mesa-vulkan-drivers:i386 libgl1-mesa-dri:amd64 libgl1-mesa-dri:i386 libglx-mesa0:amd64 libglx-mesa0:i386"

apt_install $BASE_PACKAGES
apt_install $VULKAN_PACKAGES
apt_install $MESA_PACKAGES

DEBIAN_ARCH="$(dpkg --print-architecture)"
if [ "$DEBIAN_ARCH" != "amd64" ] && [ "$DEBIAN_ARCH" != "arm64" ] && [ "$DEBIAN_ARCH" != "armhf" ]; then
  echo "Unsupported architecture: $DEBIAN_ARCH"
  exit 1
fi

if [ "${ID}" = "debian" ]; then
  DISTRO_TAG="debian-${VERSION_ID}"
else
  if [ -z "${VERSION_CODENAME:-}" ]; then
    echo "Missing VERSION_CODENAME for Ubuntu."
    exit 1
  fi
  DISTRO_TAG="ubuntu-${VERSION_CODENAME}"
fi

UMU_API="https://api.github.com/repos/Open-Wine-Components/umu-launcher/releases/latest"
ASSET_INFO="$(
  curl -sL "$UMU_API" | DEBIAN_ARCH="$DEBIAN_ARCH" DISTRO_TAG="$DISTRO_TAG" python3 -c '
import json, os, sys
data = json.load(sys.stdin)
assets = data.get("assets", [])
arch = os.environ.get("DEBIAN_ARCH", "")
distro = os.environ.get("DISTRO_TAG", "")

def match(name, prefix, arch, distro):
    return (
        name.endswith(".deb")
        and prefix in name
        and f"_{arch}_{distro}.deb" in name
    )

selected = None
for asset in assets:
    name = asset.get("name", "")
    if match(name, "python3-umu-launcher", arch, distro):
        selected = asset
        break

if selected is None:
    for asset in assets:
        name = asset.get("name", "")
        if match(name, "umu-launcher", "all", distro):
            selected = asset
            break

if selected is None:
    sys.exit(1)

print("{}|{}".format(selected["name"], selected["browser_download_url"]))
'
)"

if [ -z "$ASSET_INFO" ]; then
  echo "No matching UMU .deb found for ${DISTRO_TAG} (${DEBIAN_ARCH})."
  exit 1
fi

UMU_NAME="${ASSET_INFO%%|*}"
UMU_URL="${ASSET_INFO#*|}"

TMP_DIR="$(mktemp -d -t linuxboy-umu-XXXXXX)"
UMU_DEB="${TMP_DIR}/${UMU_NAME}"

echo "Downloading ${UMU_NAME}..."
curl -L -o "$UMU_DEB" "$UMU_URL"

echo "Installing UMU..."
apt_install "$UMU_DEB"

rm -rf "$TMP_DIR"

CACHE_DIR="$HOME/.linuxboy/cache/deps"
mkdir -p "$CACHE_DIR"

download_if_missing() {
  url="$1"
  dest="$2"
  insecure="${3:-0}"

  if [ "$REINSTALL" -eq 1 ] || [ ! -f "$dest" ]; then
    if [ "$insecure" -eq 1 ]; then
      if ! curl -L -k -o "$dest" "$url"; then
        echo "Failed to download: $url"
        rm -f "$dest"
        return 1
      fi
    else
      if ! curl -L -o "$dest" "$url"; then
        echo "Failed to download: $url"
        rm -f "$dest"
        return 1
      fi
    fi
  fi
  return 0
}

VCREDIST_API="https://api.github.com/repos/abbodi1406/vcredist/releases/latest"
VCREDIST_URL=""
set +e
VCREDIST_URL="$(
  curl -sL "$VCREDIST_API" | python3 -c '
import json, sys
data = json.load(sys.stdin)
assets = data.get("assets", [])
preferred = ["VisualCppRedist_AIO_x86_x64.exe", "VisualCppRedist_AIO_x86only.exe"]
for name in preferred:
    for asset in assets:
        if asset.get("name") == name:
            print(asset.get("browser_download_url", ""))
            sys.exit(0)
sys.exit(1)
'
)"
set -e

VCREDIST_DEST="${CACHE_DIR}/vcredist_aio.exe"
if [ -n "$VCREDIST_URL" ]; then
  echo "Downloading VC++ AIO (abbodi1406)..."
  if ! download_if_missing "$VCREDIST_URL" "$VCREDIST_DEST" 0; then
    echo "VC++ AIO download failed (optional)."
  fi
else
  echo "VC++ AIO release asset not found (optional)."
fi

DXREDIST_URL="https://download.microsoft.com/download/8/4/a/84a35bf1-dafe-4ae8-82af-ad2ae20b6b14/directx_Jun2010_redist.exe"
DXREDIST_DEST="${CACHE_DIR}/directx_Jun2010_redist.exe"
echo "Downloading DirectX June 2010 redist..."
if ! download_if_missing "$DXREDIST_URL" "$DXREDIST_DEST" 1; then
  echo "DirectX redist download failed (optional)."
fi

if [ -f "./Cargo.toml" ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found, installing..."
    apt_install cargo
  fi

  echo "Building LinuxBoy..."
  cargo build --release

  if [ -f "target/release/linuxboy" ]; then
    $SUDO install -m 755 target/release/linuxboy /usr/local/bin/linuxboy
    echo "Installed LinuxBoy to /usr/local/bin/linuxboy"
  else
    echo "LinuxBoy build output not found."
  fi
else
  echo "LinuxBoy source not found in current directory; skipping app install."
fi
