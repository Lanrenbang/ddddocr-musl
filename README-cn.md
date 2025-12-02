# ddddocr-musl

[English](README.md) | [简体中文](README-cn.md)

本项目旨在以最精简的方式使用 ddddocr ocr 系列 API，实现无任何依赖的跨平台运行。

## 特性

- 实现其他 ddddocr 项目一致的 API 端点，无 ~~crate~~ 发布，无 ~~MCP~~ 功能，仅作为 http 服务运行；
- 使用 `musl` 工具链和 `onnxruntime` 构建，单文件无任何依赖；
- 可选使用 `ort-tract` 备用后端，无 onnxruntime（当前不推荐用于生产环境，见下文）；
- 构建最小二进制，无模型集成功能，需单独下载，推荐使用 `update_models.sh` 脚本；
- 默认提供 `onnxruntime` 后端的容器镜像，下载模型后开箱即用；
- http 服务支持 `ip:port` 或者 `unix 套接字` 启动，不提供 tls，这应该属于前置 API 网关范畴。

## 项目结构

```shell
.
├── .cargo
│   └── config.toml           # 本地构建配置文件
├── model                     # 推荐模型存储目录
├── onnxruntime               # onnxruntime musl 存储目录，仅用于 本地构建
├── toolchains                # musl toolchains 存储目录，仅用于 本地构建
├── src
│   ├── lib.rs                # ddddocr 实现
│   └── main.rs               # http server 实现
├── .env.example              # 容器编排变量替换
├── compose.yaml              # 容器编排配置 - 子服务运行
├── compose.override.yaml     # 容器覆盖配置 - 独立运行
├── Dockerfile                # 镜像生成配置
├── .dockerignore
├── update_models.sh          # 模型自动更新脚本
├── Cargo.lock
├── Cargo.toml
├── README-cn.md
└── README.md
```

## 用法

1. 自行安装 Docker 或者 Podman（推荐），也可以选择本机运行；
2. 推荐 [caddy-services](https://github.com/Lanrenbang/caddy-services) 作为前置 API 网关，支持 JWT 验证，与本项目完美搭配；
3. 克隆本仓库：
```shell
git clone https://github.com/Lanrenbang/ddddocr-musl.git
```
4. 复制或更名 [.env.example](.env.example) 为 `.env`，可按需修改；
5. 执行 `update_models.sh` 脚本自动下载 onnx 模型文件；
6. 进入根目录后，启动容器服务：
```shell
docker compose up -d
podman compose up -d
```
> 提示：
>  - 如果前置 caddy，本服务将作为子服务启动，这里无需操作，具体查看 [caddy-services](https://github.com/Lanrenbang/caddy-services)

<details>

<summary>

## API 接口

</summary>

Web 服务提供以下 HTTP 接口，全部接口均支持 Swagger UI 文档查看。

| 端点 | 方法 | 说明 |
| :--- | :--- | :--- |
| `/ocr` | `POST` | 执行 OCR 文字识别。支持 Base64 图片输入，可指定字符集范围、颜色过滤等。 |
| `/det` | `POST` | 执行目标检测。返回目标边界框 (BBox)。 |
| `/slide-match` | `POST` | 滑块缺口匹配算法 。 |
| `/slide-comparison` | `POST` | 滑块图片对比算法。 |
| `/toggle-feature` | `POST` | 动态开启/关闭功能。支持热加载/卸载模型，释放内存。 |
| `/status` | `GET` | 获取服务运行状态及已启用功能列表。 |
| `/docs` | `GET` | Swagger UI 文档。可视化查看 API 定义、参数说明并直接进行在线测试。 |

**`/toggle-feature` 示例 Payload**:
```json
{
  "ocr": true,   // 开启 OCR (若未开启则加载模型)
  "det": false,  // 关闭 Detection (释放内存)
  "slide": true  // 开启滑块功能
}
```

</details>

<details>

<summary>

## 启动参数

</summary>

### 监听参数

| 格式 | 示例 | 说明 |
| :--- | :--- | :--- |
| **Unix Socket** | `--address /tmp/ddddocr.sock` | 路径以 `/` 开头时，自动识别为 Unix Domain Socket。|
| **端口号** | `--address 8000` | 纯数字时，自动绑定到 `0.0.0.0:PORT`。 |
| **TCP 地址** | `--address 127.0.0.1:8080` | 标准 IP:PORT 格式。默认值为 `0.0.0.0:8000`。 |

### 功能开关与配置

| 参数 | 默认值 | 说明 |
| :--- | :--- | :--- |
| `--ocr-path` | `model/common.onnx` | OCR 模型文件路径。同目录需存在同名 `.json` 字符集文件。 |
| `--det-path` | `model/common_det.onnx` | 目标检测模型文件路径。 |
| `--ocr-charset-range` | (无) | 全局默认 OCR 字符集范围 (例如 `"0123456789"` 或预设 ID)。若 API 请求未指定，将使用此默认值。 |
| `--disable-ocr` | `false` | 禁用 OCR 功能。禁用后不加载 OCR 模型。 |
| `--disable-det` | `false` | 禁用目标检测功能。禁用后不加载检测模型。 |
| `--disable-slide` | `false` | 禁用滑块识别功能。禁用后 `/slide-*` 接口将不可用。 |

</details>

<details>

<summary>

## Docker 构建

</summary>

### 默认镜像

项目默认提供 **ghcr.io** 和 **Docker Hub** 镜像，直接拉取使用即可

```bash
docker pull lanrenbang/ddddocr-musl:latest
podman pull ghcr.io/lanrenbang/ddddocr-musl:latest
```

### 自行构建

可使用源码中的 [Dockerfile](Dockerfile) 自行构建镜像，支持自动下载交叉编译工具链和依赖库，支持多架构构建，并利用了 `cargo-chef` 进行缓存优化

##### 构建参数

| 参数 | 默认值 | 说明 |
| :--- | :--- | :--- |
| `BACKEND` | `onnxruntime` | 推理后端。可选 `onnxruntime` (默认, 推荐) 或 `tract` (实验性)。 |
| `TARGETARCH` | 自动识别 | Docker 自动注入，支持 `amd64` 和 `arm64`。 |

##### 构建命令

```bash
# 构建 AMD64 镜像
docker build -t ddddocr-musl .
# 构建 ARM64 镜像
docker build --platform linux/arm64 -t ddddocr-musl-arm64 .
# 构建 Tract 后端 (仅供测试)
docker build --build-arg BACKEND=tract -t ddddocr-tract .
```

</details>

<details>

<summary>

## 本地构建

</summary>

### 构建准备

1. 下载并解压 Bootlin 工具链到 `toolchains/`；
2. 下载并解压 Onnxruntime 静态库到 `onnxruntime/`；
3. `.cargo/config.toml` 预置了必要的环境变量和编译器配置，必须与上述下载一致。

### 构建命令

```bash
# 标准构建 (OnnxRuntime + Musl)
cargo build --release
# 构建 Tract 后端 (实验性)
cargo build --release --no-default-features --features tract
```
> 注意
tract 后端是 ort v2.0.0-rc.10 新增的实验性功能，实际压力测试中**比默认 onnxruntime 后端性能差距在数百倍甚至上千倍**，也可能是 ddddocr 模型的算子问题，因此当前仅能作为玩具，极度不推荐在生产环境中使用。

</details>

## 相关项目
- [caddy-services](https://github.com/Lanrenbang/caddy-services)

## 鸣谢
- [86maid - ddddocr rust](https://github.com/86maid/ddddocr)
- [sml2h3 - ddddocr python](https://github.com/sml2h3/ddddocr)

## 通过捐赠支持我
[![BuyMeACoffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-ffdd00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/bobbynona) [![Ko-Fi](https://img.shields.io/badge/Ko--fi-F16061?style=for-the-badge&logo=ko-fi&logoColor=white)](https://ko-fi.com/bobbynona) [![USDT(TRC20)/Tether](https://img.shields.io/badge/Tether-168363?style=for-the-badge&logo=tether&logoColor=white)](https://github.com/Lanrenbang/.github/blob/5b06b0b2d0b8e4ce532c1c37c72115dd98d7d849/custom/USDT-TRC20.md) [![Litecoin](https://img.shields.io/badge/Litecoin-A6A9AA?style=for-the-badge&logo=litecoin&logoColor=white)](https://github.com/Lanrenbang/.github/blob/5b06b0b2d0b8e4ce532c1c37c72115dd98d7d849/custom/Litecoin.md)

## 许可
本项目按照 `LICENSE` 文件中的条款进行分发。



