# syntax=docker/dockerfile:latest

ARG BOOTLIN_TOOLCHAIN_VERSION="14.2.0"
ARG ORT_VERSION="1.22.2"

# ==============================================================================
# Base Stage: Install common tools
# ==============================================================================
FROM --platform=$BUILDPLATFORM rust:alpine AS base
WORKDIR /app
# Install system dependencies required for downloading and extracting
# Update CA certificates & Create nonroot user
RUN apk add --no-cache ca-certificates git tzdata curl 7zip musl-dev && \
  update-ca-certificates && \
  adduser -D -u 65532 -h /home/nonroot -s /sbin/nologin nonroot

# Install cargo-chef
RUN cargo install cargo-chef --locked

# ==============================================================================
# Planner Stage: Compute recipe
# ==============================================================================
FROM --platform=$BUILDPLATFORM base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ==============================================================================
# Stage: Toolchain Downloader
# Independent stage to cache toolchain download based on ARCH and VERSION
# ==============================================================================
FROM --platform=$BUILDPLATFORM base AS toolchain-downloader
ARG TARGETARCH
ARG BOOTLIN_TOOLCHAIN_VERSION
WORKDIR /downloads

RUN set -e; \
    if [ "$TARGETARCH" = "amd64" ]; then \
        URL="https://github.com/benjaminwan/musl-cross-builder/releases/download/${BOOTLIN_TOOLCHAIN_VERSION}/x86_64-linux-musl-${BOOTLIN_TOOLCHAIN_VERSION}.7z"; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
        URL="https://github.com/benjaminwan/musl-cross-builder/releases/download/${BOOTLIN_TOOLCHAIN_VERSION}/aarch64-linux-musl-${BOOTLIN_TOOLCHAIN_VERSION}.7z"; \
    else \
        echo "Unsupported architecture: $TARGETARCH"; exit 1; \
    fi; \
    echo "Downloading toolchain from $URL"; \
    curl -L --retry 5 --retry-delay 5 --retry-connrefused -o toolchain.7z "$URL"

# Extract to a fixed path /out/toolchains for easier copying later
RUN mkdir -p /out/toolchains && \
    # Ignore "Dangerous link path" error (exit code 2)
    (7z x toolchain.7z -o/out/toolchains || true) && \
    rm toolchain.7z && \
    # Verify extraction
    [ -n "$(ls -A /out/toolchains)" ]

# ==============================================================================
# Stage: OnnxRuntime Downloader
# Independent stage to cache ORT download based on ARCH and VERSION
# ==============================================================================
FROM --platform=$BUILDPLATFORM base AS ort-downloader
ARG TARGETARCH
ARG ORT_VERSION
ARG BACKEND=onnxruntime
WORKDIR /downloads

RUN set -e; \
    if [ "$BACKEND" = "tract" ]; then \
        echo "Backend is tract, skipping OnnxRuntime download."; \
        mkdir -p /out/onnxruntime; \
        exit 0; \
    fi; \
    \
    if [ "$TARGETARCH" = "amd64" ]; then \
        URL="https://github.com/RapidAI/OnnxruntimeBuilder/releases/download/${ORT_VERSION}/onnxruntime-v${ORT_VERSION}-x86_64-linux-musl-static.7z"; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
        URL="https://github.com/RapidAI/OnnxruntimeBuilder/releases/download/${ORT_VERSION}/onnxruntime-v${ORT_VERSION}-aarch64-linux-musl-static.7z"; \
    else \
        echo "Unsupported architecture: $TARGETARCH"; exit 1; \
    fi; \
    echo "Downloading ORT from $URL"; \
    curl -L --retry 5 --retry-delay 5 --retry-connrefused -o ort.7z "$URL"

RUN mkdir -p /out/onnxruntime && \
    if [ -f ort.7z ]; then \
        7z x ort.7z -o/out/onnxruntime && \
        rm ort.7z; \
    fi

# ==============================================================================
# Builder Stage: Compile
# ==============================================================================
FROM --platform=$BUILDPLATFORM base AS builder
ARG TARGETARCH
ARG BACKEND=onnxruntime

WORKDIR /app

# 1. Copy pre-downloaded resources from previous stages
# This uses Docker cache: if toolchain-downloader didn't change, this layer is cached.
COPY --from=toolchain-downloader /out/toolchains /app/toolchains
COPY --from=ort-downloader /out/onnxruntime /app/onnxruntime

# 2. Configure Environment & Linker
# This runs fast and sets up the state for cargo
RUN set -e; \
    # Identify toolchain dir
    TOOLCHAIN_DIR=$(ls /app/toolchains | head -n 1); \
    if [ "$TARGETARCH" = "amd64" ]; then \
        RUST_TARGET="x86_64-unknown-linux-musl"; \
        LINKER_BIN="x86_64-linux-musl-gcc"; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
        RUST_TARGET="aarch64-unknown-linux-musl"; \
        LINKER_BIN="aarch64-linux-musl-gcc"; \
    fi; \
    \
    LINKER_PATH="/app/toolchains/$TOOLCHAIN_DIR/bin/$LINKER_BIN"; \
    if [ ! -f "$LINKER_PATH" ]; then echo "Linker not found at $LINKER_PATH"; exit 1; fi; \
    \
    # Configure Cargo Linker
    mkdir -p .cargo; \
    echo "[target.$RUST_TARGET]" > .cargo/config.toml; \
    echo "linker = \"$LINKER_PATH\"" >> .cargo/config.toml; \
    \
    # Configure ORT Environment
    if [ "$BACKEND" != "tract" ]; then \
        ORT_DIR_NAME=$(ls /app/onnxruntime | head -n 1); \
        echo "ORT_LIB_LOCATION=/app/onnxruntime/$ORT_DIR_NAME/lib" > /app/ort_env; \
        echo "ORT_STRATEGY=system" >> /app/ort_env; \
    else \
        echo "" > /app/ort_env; \
    fi; \
    \
    # Persist variables for next RUN instructions
    echo "$RUST_TARGET" > /app/rust_target

# 3. Cook dependencies (The heavy caching layer)
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    set -e; \
    RUST_TARGET=$(cat /app/rust_target); \
    . /app/ort_env; \
    export ORT_LIB_LOCATION ORT_STRATEGY; \
    \
    echo "Cooking dependencies for $RUST_TARGET (Backend: $BACKEND)"; \
    if [ "$BACKEND" = "tract" ]; then \
        cargo chef cook --release --target "$RUST_TARGET" --no-default-features --features tract; \
    else \
        cargo chef cook --release --target "$RUST_TARGET"; \
    fi

# 4. Build Application (Source code changes affect only from here)
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    set -e; \
    RUST_TARGET=$(cat /app/rust_target); \
    . /app/ort_env; \
    export ORT_LIB_LOCATION ORT_STRATEGY; \
    \
    echo "Building binary for $RUST_TARGET"; \
    if [ "$BACKEND" = "tract" ]; then \
        cargo build --release --target "$RUST_TARGET" --no-default-features --features tract; \
    else \
        cargo build --release --target "$RUST_TARGET"; \
    fi; \
    \
    mkdir -p /out; \
    cp target/"$RUST_TARGET"/release/ddddocr-musl /out/ddddocr-musl; \
    ls -lh /out/ddddocr-musl

# Ensure model directory exists for final copy
RUN mkdir -p /tmp/app/model

# ==============================================================================
# Final Stage: Scratch
# ==============================================================================
FROM scratch
WORKDIR /app

LABEL org.opencontainers.image.title="ddddocr-musl" \
      org.opencontainers.image.authors="bobbynona" \
      org.opencontainers.image.vendor="L.R.B" \
      org.opencontainers.image.source="https://github.com/Lanrenbang/ddddocr-musl" \
      org.opencontainers.image.url="https://github.com/Lanrenbang/ddddocr-musl"

COPY --from=base /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group
COPY --from=base /usr/share/zoneinfo /usr/share/zoneinfo

COPY --from=builder --chown=0:0 --chmod=755 /out/ddddocr-musl /app/ddddocr-musl

COPY --from=builder --chown=65532:65532 --chmod=0775 /tmp/app /app/

VOLUME /app/model

ARG TZ=Etc/UTC
ENV TZ=$TZ

ENTRYPOINT ["/app/ddddocr-musl"]
CMD ["--address", "0.0.0.0:8000"]

