# macOS 构建与验收

## 支持范围

- Apple Silicon：在 arm64 Mac 上生成 `aarch64-apple-darwin` DMG。
- Intel：在 Intel Mac 上生成 `x86_64-apple-darwin` DMG。
- 不把 PyInstaller 单架构可执行文件伪装成通用二进制；两种架构分别构建和发布。
- macOS 11 及以上。

## 本机测试版：一条命令，不需要发布私钥

```bash
cd ai-route-manager
bash ./scripts/build-macos.sh --local-test
```

`--local-test` 是默认模式：使用 ad-hoc 签名，临时关闭 updater 产物，不读取 Tauri updater 私钥或 Apple 账户凭据。它用于先在真实 Mac 上完成启动、界面、sidecar、预演和恢复验收。

脚本会依次完成：

1. 建立隔离 Python venv 并按固定版本构建 `DynamicProxyEngine`。
2. 从 MetaCubeX 官方 GitHub Release 下载 Mihomo v1.19.30，并校验发布方 SHA-256。
3. 放入独立的 `resources/engine-macos`，不覆盖 Windows sidecar。
4. 由 `tauri.macos.conf.json` 替换基础资源数组，确保 DMG 只收录 macOS sidecar，不把 Windows `.exe` 带入包内。
5. 对内部可执行文件签名并生成资源清单。
6. 运行 Python、Rust、TypeScript 测试。
7. 构建 `.app`、`.dmg` 和 Tauri updater 产物。
8. `--release` 模式必须同时提供 updater 私钥、Developer ID，以及 Apple ID 三元组或 App Store Connect API 三元组；构建后验证 stapled notarization ticket。
9. 重新校验包内资源哈希、可执行权限、代码签名和 Gatekeeper 状态，并输出独立 `SHA256SUMS.txt`。

## 正式签名、公证与 updater 产物

构建机已安装 Developer ID 证书时：

```bash
export APPLE_SIGNING_IDENTITY='Developer ID Application: Your Name (TEAMID)'
export APPLE_CERTIFICATE='base64-p12-if-used-by-tauri-ci'
export APPLE_CERTIFICATE_PASSWORD='...'
export APPLE_ID='你的 Apple ID 邮箱'
export APPLE_PASSWORD='Apple 为此应用生成的专用密码'
export APPLE_TEAM_ID='你的 Team ID'
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/ai-route-manager-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD='从安全密码库读取，不要写入脚本或仓库'
bash ./scripts/build-macos.sh --release
```

`TAURI_SIGNING_PRIVATE_KEY` 用于生成 Tauri updater 的 `.app.tar.gz.sig`，与 Apple Developer ID 签名是两套独立凭据，二者都必须妥善备份。

也可改用 `APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_KEY_PATH` 三项 App Store Connect API 凭据完成公证。凭据不完整时脚本在构建前停止，避免产生被误当作正式版的未公证 Developer ID 包。

本机验收直接使用 `--local-test`；正式发布只接受 `--release`，并拒绝 ad-hoc 身份或缺少公证凭据的 Developer ID 构建。

## 独立复验

```bash
bash ./scripts/verify-macos-bundle.sh \
  'src-tauri/target/aarch64-apple-darwin/release/bundle/macos/AI Route Manager.app'
```

成功标志：`MACOS_BUNDLE_VERIFY_OK`。脚本不会启动托管模式，也不会修改当前系统代理。

## 固定依赖证据

- Mihomo arm64 gzip SHA-256：`2c7f3a7904fa1cee291e124123e630e7b1ebd13765dd9bf26c0a28432004d9f4`
- Mihomo Intel compatible gzip SHA-256：`6e75de0732e8afabe413ff7c235e8f16226ce136672371c60787cbf9607402c5`
- 版本：`v1.19.30`
