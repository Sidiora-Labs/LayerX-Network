#!/bin/bash
 
# Install build tools required for native dependencies
apt-get update && apt-get install -y \
    build-essential \
    python3 \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set environment variables to help with native compilation
export npm_config_build_from_source=false
export npm_config_prefer_offline=true
export NODE_PATH=$(pwd)/node_modules_linux

yarn install --modules-folder node_modules_linux