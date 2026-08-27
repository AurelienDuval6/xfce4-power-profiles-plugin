#!/bin/bash
set -e

echo "=== Building xfce4-power-profiles-plugin ==="

# Stage 1: Build Rust staticlib
echo "[1/3] Building Rust staticlib..."
cargo build --release

# Stage 2: Compile C shim
echo "[2/3] Compiling C shim..."
gcc -c -fPIC plugin.c -o plugin.o \
    $(pkg-config --cflags libxfce4panel-2.0 gtk+-3.0)

# Stage 3: Link everything into final .so
echo "[3/3] Linking plugin shared library..."
gcc -shared -fPIC -o libpowerprofiles.so plugin.o \
    -Wl,--whole-archive target/release/libpowerprofiles.a -Wl,--no-whole-archive \
    $(pkg-config --libs libxfce4panel-2.0 gtk+-3.0) \
    -lpthread -ldl -lm

echo "=== Build successful: libpowerprofiles.so ==="
