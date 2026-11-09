# GitHub 开源发布前检查清单

## 必须完成

- [ ] 由项目所有者选定并添加开源许可证（例如 MIT、Apache-2.0 或 GPL-3.0）；未明确选择前不代替所有者做授权决定。
- [ ] 盘点 `src-tauri/resources/engine/` 中每个二进制和数据库的上游来源、版本、许可证和再分发条件，在 `THIRD_PARTY_NOTICES` 中逐项列出。
- [ ] 不提交 `release/`、`src-tauri/target/`、签名私钥、证书、DPAPI 密码文件和 `.env` 文件。
- [ ] 按 `SUPPORT_AND_DISCLOSURE.md` 实机复验已提供的 `public/support-qr.jpg` 收款码。
- [ ] 为每个实际返利项填写真实 HTTPS 链接和合作关系说明，不发布虚构推荐。
- [ ] 在干净克隆中运行 `pnpm install --frozen-lockfile`、`pnpm check`、`pnpm build` 和 Rust 测试。
- [ ] 在安装有 Visual Studio C++ Build Tools 的 Windows 环境里运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib`。

## 建议完成

- [ ] 添加 `CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、漏洞私下报告方式和 GitHub Issue / Pull Request 模板。
- [ ] 在 CI 中固定 Node、pnpm 和 Rust toolchain，并开启 TypeScript、Vitest、Rustfmt、Clippy 与双平台构建。
- [ ] 用 GitHub Release 发布签名安装包，不把历史安装包直接放入源码树。
