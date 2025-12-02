# ddddocr-musl

[English](README.md) | [简体中文](README-cn.md)

This project aims to use the ddddocr OCR series API in the most minimalist way, achieving cross-platform execution with zero dependencies.

## Features

- Implements API endpoints consistent with other ddddocr projects; no crate publication, no Model Context Protocol (MCP) functionality, runs solely as an HTTP service.
- Built using the `musl` toolchain and `onnxruntime`, resulting in a single file with zero dependencies.
- Optional `ort-tract` backup backend available (no onnxruntime required), though not currently recommended for production (see below).
- Builds a minimal binary; model integration is excluded (models must be downloaded separately, using the `update_models.sh` script is recommended).
- Default container images are provided with the `onnxruntime` backend, ready to use out-of-the-box after downloading models.
- The HTTP service supports startup via `ip:port` or `unix socket`. TLS is not provided, as this falls under the scope of an upstream API gateway.

## Project Structure

```shell
.
├── .cargo
│   └── config.toml           # Local build configuration file
├── model                     # Recommended directory for model storage
├── onnxruntime               # Storage for onnxruntime musl, used only for local builds
├── toolchains                # Storage for musl toolchains, used only for local builds
├── src
│   ├── lib.rs                # ddddocr implementation
│   └── main.rs               # http server implementation
├── .env.example              # Container orchestration variable substitution
├── compose.yaml              # Container orchestration config - Sub-service run
├── compose.override.yaml     # Container override config - Standalone run
├── Dockerfile                # Image generation configuration
├── .dockerignore
├── update_models.sh          # Model auto-update script
├── Cargo.lock
├── Cargo.toml
├── README-cn.md
└── README.md
```

## Usage

1.  Install Docker or Podman (recommended) yourself, or choose to run natively.
2.  [caddy-services](https://github.com/Lanrenbang/caddy-services) is recommended as an upstream API gateway; it supports JWT verification and works perfectly with this project.
3.  Clone this repository:
    ```shell
    git clone https://github.com/Lanrenbang/ddddocr-musl.git
    ```
4.  Copy or rename [.env.example](.env.example) to `.env` and modify it as needed.
5.  Execute the `update_models.sh` script to automatically download the onnx model files.
6.  Enter the root directory and start the container service:
    ```shell
    docker compose up -d
    podman compose up -d
    ```
    > **Tip:**
    > - If using Caddy upstream, this service will start as a sub-service. No action is required here; please refer to [caddy-services](https://github.com/Lanrenbang/caddy-services) for details.

<details>

<summary>

## API Interface

</summary>

The Web service provides the following HTTP interfaces. All interfaces support Swagger UI documentation viewing.

| Endpoint | Method | Description |
| :--- | :--- | :--- |
| `/ocr` | `POST` | Executes OCR text recognition. Supports Base64 image input, allows specifying character set ranges, color filtering, etc. |
| `/det` | `POST` | Executes object detection. Returns the target Bounding Box (BBox). |
| `/slide-match` | `POST` | Slider gap matching algorithm. |
| `/slide-comparison` | `POST` | Slider image comparison algorithm. |
| `/toggle-feature` | `POST` | Dynamically enable/disable features. Supports hot loading/unloading of models to free up memory. |
| `/status` | `GET` | Gets the service running status and the list of enabled features. |
| `/docs` | `GET` | Swagger UI documentation. Visualize API definitions, parameter descriptions, and perform direct online testing. |

**`/toggle-feature` Example Payload**:
```json
{
  "ocr": true,   // Enable OCR (loads model if not enabled)
  "det": false,  // Disable Detection (frees memory)
  "slide": true  // Enable slider functionality
}
```

</details>

<details>

<summary>

## Startup Arguments

</summary>

### Listening Parameters

| Format | Example | Description |
| :--- | :--- | :--- |
| **Unix Socket** | `--address /tmp/ddddocr.sock` | When the path starts with `/`, it is automatically identified as a Unix Domain Socket. |
| **Port Number** | `--address 8000` | When it is purely numeric, it automatically binds to `0.0.0.0:PORT`. |
| **TCP Address** | `--address 127.0.0.1:8080` | Standard IP:PORT format. The default value is `0.0.0.0:8000`. |

### Feature Toggles & Configuration

| Argument | Default | Description |
| :--- | :--- | :--- |
| `--ocr-path` | `model/common.onnx` | Path to the OCR model file. A `.json` charset file with the same name must exist in the same directory. |
| `--det-path` | `model/common_det.onnx` | Path to the object detection model file. |
| `--ocr-charset-range` | (None) | Global default OCR charset range (e.g., `"0123456789"` or a preset ID). If the API request does not specify one, this default is used. |
| `--disable-ocr` | `false` | Disable OCR functionality. When disabled, the OCR model is not loaded. |
| `--disable-det` | `false` | Disable object detection functionality. When disabled, the detection model is not loaded. |
| `--disable-slide` | `false` | Disable slider recognition functionality. When disabled, `/slide-*` interfaces will be unavailable. |

</details>

<details>

<summary>

## Docker Build

</summary>

### Default Images

The project provides default images on **ghcr.io** and **Docker Hub**; you can pull and use them directly.

```bash
docker pull lanrenbang/ddddocr-musl:latest
podman pull ghcr.io/lanrenbang/ddddocr-musl:latest
```

### Self-Build

You can use the [Dockerfile](Dockerfile) in the source code to build the image yourself. It supports automatic downloading of cross-compilation toolchains and dependency libraries, supports multi-architecture builds, and utilizes `cargo-chef` for cache optimization.

##### Build Arguments

| Argument | Default | Description |
| :--- | :--- | :--- |
| `BACKEND` | `onnxruntime` | Inference backend. Options: `onnxruntime` (default, recommended) or `tract` (experimental). |
| `TARGETARCH` | Auto-detected | Automatically injected by Docker, supports `amd64` and `arm64`. |

##### Build Commands

```bash
# Build AMD64 image
docker build -t ddddocr-musl .
# Build ARM64 image
docker build --platform linux/arm64 -t ddddocr-musl-arm64 .
# Build Tract backend (for testing only)
docker build --build-arg BACKEND=tract -t ddddocr-tract .
```

</details>

<details>

<summary>

## Local Build

</summary>

### Build Preparation

1.  Download and extract the Bootlin toolchain to `toolchains/`.
2.  Download and extract the Onnxruntime static library to `onnxruntime/`.
3.  `.cargo/config.toml` has preset the necessary environment variables and compiler configurations, which must match the downloads above.

### Build Commands

```bash
# Standard build (OnnxRuntime + Musl)
cargo build --release
# Build Tract backend (Experimental)
cargo build --release --no-default-features --features tract
```
> **Note**
> The tract backend is an experimental feature added in ort v2.0.0-rc.10. In actual stress tests, **performance is hundreds or even thousands of times slower than the default onnxruntime backend**. This may be due to operator issues with the ddddocr models. Therefore, it currently serves only as a toy and is strictly **not recommended** for use in production environments.

</details>

## Related Projects
- [caddy-services](https://github.com/Lanrenbang/caddy-services)

## Credits
- [86maid - ddddocr rust](https://github.com/86maid/ddddocr)
- [sml2h3 - ddddocr python](https://github.com/sml2h3/ddddocr)

## Support Me
[![BuyMeACoffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-ffdd00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/bobbynona) [![Ko-Fi](https://img.shields.io/badge/Ko--fi-F16061?style=for-the-badge&logo=ko-fi&logoColor=white)](https://ko-fi.com/bobbynona) [![USDT(TRC20)/Tether](https://img.shields.io/badge/Tether-168363?style=for-the-badge&logo=tether&logoColor=white)](https://github.com/Lanrenbang/.github/blob/5b06b0b2d0b8e4ce532c1c37c72115dd98d7d849/custom/USDT-TRC20.md) [![Litecoin](https://img.shields.io/badge/Litecoin-A6A9AA?style=for-the-badge&logo=litecoin&logoColor=white)](https://github.com/Lanrenbang/.github/blob/5b06b0b2d0b8e4ce532c1c37c72115dd98d7d849/custom/Litecoin.md)

## License
This project is distributed under the terms of the `LICENSE` file.
