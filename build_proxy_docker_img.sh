#!/usr/bin/env bash

set -euo pipefail

source ./build_env.sh

SING_BOX_VERSION="${SING_BOX_VERSION:-1.12.22}"
IMAGE_TAG="${IMAGE_TAG:-landscape_proxy_image:latest}"

echo "Building redirect package handler for $TARGET_ARCH..."
cargo build --release \
    --package landscape-ebpf \
    --bin redirect_pkg_handler \
    --target "$TARGET_ARCH"

BUILD_DIR="$SCRIPT_DIR/dockerfiles/proxy"
COPY_DIR="$BUILD_DIR/apps"

rm -rf "$COPY_DIR"
mkdir -p "$COPY_DIR"

cp "$BUILD_DIR/start.sh" "$COPY_DIR/start.sh"
chmod +x "$COPY_DIR/start.sh"

cp "$SCRIPT_DIR/target/$TARGET_ARCH/release/redirect_pkg_handler" \
   "$COPY_DIR/redirect_pkg_handler"
chmod +x "$COPY_DIR/redirect_pkg_handler"

echo "Building proxy Docker image $IMAGE_TAG for linux/$DOCKER_ARCH..."
docker buildx build \
    --platform "linux/$DOCKER_ARCH" \
    -t "$IMAGE_TAG" \
    --build-arg "SING_BOX_VERSION=$SING_BOX_VERSION" \
    --load \
    -f "$BUILD_DIR/Dockerfile" \
    "$BUILD_DIR"

echo "Done. Proxy image: $IMAGE_TAG"
