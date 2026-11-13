FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Native build dependencies, Wayland/X11 support, and Mesa software rendering.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build pkg-config clang libclang-dev \
    libssl-dev libsqlite3-dev libdbus-1-dev \
    libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
    libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
    libegl1-mesa-dev libgl1-mesa-dev libvulkan1 mesa-vulkan-drivers \
    fonts-dejavu-core dbus-x11 xdg-utils wayland-utils vulkan-tools \
    strace ripgrep \
    && rm -rf /var/lib/apt/lists/*

# Refresh stable instead of relying on the toolchain bundled in the base image.
USER vscode
RUN rustup update stable \
    && rustup default stable \
    && rustup component add --toolchain stable rustfmt clippy rust-src

