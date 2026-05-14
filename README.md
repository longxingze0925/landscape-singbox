# landscape-singbox

Proxy runtime image for Landscape.

This repository builds and publishes the Docker image used by Landscape proxy nodes. The image contains:

- `sing-box`
- Landscape `redirect_pkg_handler`
- container startup routing rules for transparent proxy mode

## Image

Default image:

```text
ghcr.io/longxingze0925/landscape-singbox:latest
```

Landscape can override the image with:

```bash
LANDSCAPE_PROXY_IMAGE=ghcr.io/longxingze0925/landscape-singbox:latest
```

## Build Source

The GitHub Actions workflow checks out the Landscape repository and builds:

```bash
cargo build --release --package landscape-ebpf --bin redirect_pkg_handler --target x86_64-unknown-linux-gnu
```

Manual workflow inputs allow changing the Landscape repository, ref, sing-box version, and image tag.
