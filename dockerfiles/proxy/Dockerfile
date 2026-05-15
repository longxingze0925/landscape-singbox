FROM debian:bookworm-slim

ARG TARGETARCH
ARG TARGETVARIANT
ARG SING_BOX_VERSION=1.12.22

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gzip iproute2 tar \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    case "${TARGETARCH}${TARGETVARIANT}" in \
        amd64) sb_arch="amd64" ;; \
        arm64) sb_arch="arm64" ;; \
        armv7) sb_arch="armv7" ;; \
        *) echo "unsupported sing-box arch: ${TARGETARCH}${TARGETVARIANT}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL \
        "https://github.com/SagerNet/sing-box/releases/download/v${SING_BOX_VERSION}/sing-box-${SING_BOX_VERSION}-linux-${sb_arch}.tar.gz" \
        -o /tmp/sing-box.tar.gz; \
    mkdir -p /tmp/sing-box; \
    tar -xzf /tmp/sing-box.tar.gz -C /tmp/sing-box --strip-components=1; \
    install -m 0755 /tmp/sing-box/sing-box /usr/local/bin/sing-box; \
    rm -rf /tmp/sing-box /tmp/sing-box.tar.gz

COPY apps/ ./

RUN chmod +x /app/start.sh /app/redirect_pkg_handler

CMD ["/app/start.sh"]
