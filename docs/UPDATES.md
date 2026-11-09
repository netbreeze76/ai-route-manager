# 签名自动更新

## 信任边界

- 应用内置 updater 公钥，任何更新包都必须由对应私钥签名。
- 更新清单与安装包必须托管在 HTTPS；清单 URL 不允许用户名、密码或片段。
- 自动检查只报告版本；下载和安装必须由用户确认。
- 更新私钥和密码不得写入仓库、日志、诊断包或发布目录。

## Windows 发布

本机默认读取：

- `%USERPROFILE%\.tauri\ai-route-manager-updater.key`：加密的 updater 私钥。
- `%USERPROFILE%\.tauri\ai-route-manager-updater.password.dpapi`：仅当前 Windows 用户可解密的密码。

也可以在 CI 中设置 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。构建示例：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-windows.ps1 `
  -Version 0.4.1 `
  -UpdateBaseUrl https://139-59-246-27.sslip.io/ai-route-manager/releases/0.4.1
```

输出包括安装包、`.sig`、便携包、`latest.json` 和 `SHA256SUMS.txt`。上传后以 `scripts/verify-update-feed.ps1` 对发布目录进行复核。

正式清单固定发布到：

```text
https://139-59-246-27.sslip.io/ai-route-manager/latest.json
```

`scripts/deploy-update-feed.ps1` 会先在本机校验版本、HTTPS 下载地址、签名一致性和哈希，再上传到远端暂存目录；远端通过 Caddy 配置校验后才切换 `latest.json`。

## macOS 发布

真实 Mac 必须同时提供 Apple Developer ID/公证凭据以及 Tauri updater 私钥。`build-macos.sh` 会在缺少 updater 私钥时停止，不生成不可更新的正式包。

## 密钥轮换

现有客户端只信任当前内置公钥。常规轮换应先发布同时信任旧/新密钥的过渡版本，再用旧密钥签名该过渡版本；遗失旧私钥将使现有安装无法通过自动更新迁移。
