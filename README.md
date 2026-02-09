<div align="center">

# 🚀 Sptzx Uploader

[![Build Status](https://github.com/siputzx/uploader/actions/workflows/main.yml/badge.svg)](https://github.com/siputzx/uploader/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

Temporary CDN service built with Rust (Axum) for maximum speed and security.

</div>

---

## ✨ Features

- **Ultra Fast** — Rust-powered with RAM Disk (tmpfs) storage
- **Secure** — HMAC-SHA256 signed URLs prevent unauthorized access
- **Smart View** — Auto-detect media types for browser preview
- **Auto Cleanup** — Files automatically deleted after expiration (default: 5 minutes)
- **Resource Efficient** — Minimal RAM and CPU footprint

---

## 🚀 Quick Start

```bash
podman run -d \
  --name sptzx-cdn \
  --restart always \
  --memory 12g \
  --cpus 2 \
  --mount type=tmpfs,destination=/app/uploads,tmpfs-size=10737418240,tmpfs-mode=1777 \
  -p 3003:3003 \
  -e SPTZX_PORT=3003 \
  -e SPTZX_BIND_ADDR=0.0.0.0:3003 \
  -e SPTZX_BASE_URL='https://cdn.siputzx.my.id' \
  -e SPTZX_SECRET_KEY='your-secret-key-here' \
  -e SPTZX_MAX_FILE_SIZE=536870912 \
  -e SPTZX_FILE_LIFETIME=300 \
  -e RUST_LOG=info \
  ghcr.io/siputzx/uploader:latest
```

---

## ⚙️ Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `SPTZX_PORT` | Internal healthcheck port | `3000` |
| `SPTZX_BIND_ADDR` | Application listen address | `0.0.0.0:3000` |
| `SPTZX_BASE_URL` | Base URL for generated links | `http://localhost:3000` |
| `SPTZX_SECRET_KEY` | HMAC signing secret key | `""` |
| `SPTZX_MAX_FILE_SIZE` | Max file size in bytes | `536870912` (512MB) |
| `SPTZX_FILE_LIFETIME` | File retention in seconds | `300` (5 min) |
| `RUST_LOG` | Log level | `info` |

---

## 💡 Usage

**Upload a file:**

```bash
curl -X POST http://localhost:3003/upload \
  -F "file=@image.jpg"
```

**Response:**

```json
{
  "url": "https://cdn.siputzx.my.id/files/abc123?sig=xyz&exp=1234567890"
}
```

**Access the file:**

Open the URL in browser or download:

```bash
curl "https://cdn.siputzx.my.id/files/abc123?sig=xyz&exp=1234567890" -o image.jpg
```

---

## 🔒 Security

Files are protected with HMAC-SHA256 signed URLs:

- Prevents unauthorized access
- Prevents URL tampering
- Automatic expiration

**Generate a strong secret key:**

```bash
openssl rand -hex 32
```

---

## 📝 License

MIT License - see [LICENSE](LICENSE) file for details.

---

## 🔗 Links

- **GitHub**: [siputzx/uploader](https://github.com/siputzx/uploader)
- **Demo**: [cdn.siputzx.my.id](https://cdn.siputzx.my.id)
- **Issues**: [Report Bug](https://github.com/siputzx/uploader/issues)

---

<div align="center">

Made with ⚡ by [Siputzx](https://github.com/siputzx)

</div>
