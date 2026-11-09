# AI Route Manager

面向 Windows 与 macOS 的图形化分流、固定出口、设备和远程连接管理器。

## 当前可运行内容

- React/TypeScript 产品界面：总览、线路与 VPS、场景分流、设备、远程控制、推荐中心、诊断和设置。
- 安全预览模式：桌面端只读取既有本机入口，不接管或重启当前代理。
- Rust/Tauri 本地后端：端口快照、配置校验、原子保存与加载。
- 公共版地区策略：每位用户可填写两位地区代码、显示名称和多个优先节点关键词；固定 VPS、订阅节点筛选、出口健康检查和移动端导出共同使用该策略，不再把新加坡写死为产品规则。
- 旧版个人配置自动迁移为 `SG / 新加坡 / 李家坡2-直连`，因此原用户升级后保持既有行为；全新用户没有预设国家，必须自行填写 SG、US、JP、TW、HK 或其他合法两位地区代码。
- 首次配置只要求稳定订阅与目标地区；空白下载订阅和空白固定 VPS 不会被误当成无效配置，用户可以按需追加。总览只显示实际保存的下载线路数量，不展示虚构可用状态。
- 旧版自动导入优先检查当前用户与系统标准数据目录，并保留历史 `E:\Tools\DynamicProxyManager` 作为兼容位置；公共版本不依赖个人磁盘布局。
- 规则规划器：AI 严格出口不会故障回落到 `DIRECT`；中国大陆服务保持直连。
- 推荐位编辑器：VPS/VPN/软件/硬件服务链接、优惠码和合作关系披露校验；发行者推荐目录使用内置公钥验签，用户本机推荐位与发行目录隔离。
- Tauri 签名更新：HTTPS 清单、内置公钥验证、自动检查、确认安装和发布清单生成。
- 已上线正式 HTTPS 更新清单；安装包和清单均在发布前后校验 SHA-256 与 Tauri 签名。
- 远控防火墙向导：Sunshine 仅开放专用网络最小端口；RustDesk VPS 仅开放原生客户端最小端口，并提供对称回滚。
- Sunshine 实测参数可事务写入官方配置：原文件哈希门禁、完整备份、用户改动冲突拒绝和一键回滚；不自动重启串流或改动网络代理。
- OpenWrt / OpenClash 家庭网关支持 SSH 隔离预演、核心语法校验、固定目标原子下发、UCI 选择切换、服务重启失败自动恢复和冲突感知的一键回滚；不安装路由器软件或改动电脑代理。
- 设备中心使用只读 SSH 自动发现 OpenWrt 能力、DHCP/IPv6 邻居和真实局域网设备；可按设备保存国内直连、AI、流媒体、游戏或下载场景，并把 DNS、IPv4、IPv6 策略加入同一个可回滚候选配置。
- macOS 平台配置只打包 `engine-macos`，Developer ID 构建强制公证凭据门禁并复验 stapled ticket；Windows/macOS sidecar 不再混装。
- Mac 构建包默认生成无需发布私钥的 ad-hoc 本机测试 DMG；`--release` 才启用 updater 签名、Developer ID 与 Apple 公证，测试版和正式版分目录保存。

## 开发

```powershell
pnpm install
pnpm check
pnpm build
pnpm tauri dev
```

Rust 单元测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

## 原则

1. 先预演、再探测、再应用；应用失败恢复上一个已验证配置。
2. AI 严格策略只允许同地区批准出口或阻断，不静默直连。
3. 订阅令牌、VPS 密码和私钥不进入前端日志或普通配置文件。
4. 合作推广链接必须清楚说明链接关系，且始终允许自带服务。
5. Steam 功能只做账户地区一致性和流量拆分，不提供价格区规避。
6. 更新私钥不进入仓库或安装包；更新源只能提供通过内置公钥验证的签名产物。
7. 平台推荐目录验签失败即拒绝加载；用户可单独关闭发行者推荐而保留本机自定义推荐位。

## 合法使用与商业关系披露

AI Route Manager 只是管理用户自行合法取得的网络连接、路由策略与远程工具的管理软件，不提供节点、账号、翻墙、破解、越狱或其他违法服务。使用者必须遵守所在地法律法规及第三方服务条款，并自行确认配置、访问与使用行为合法合规。

推荐中心将直接赞赏与推荐返利分开展示。返利链接会明确披露开发者可能获得佣金，不增加用户价格，也不影响用户使用自有服务。发布前检查清单见 [`docs/SUPPORT_AND_DISCLOSURE.md`](docs/SUPPORT_AND_DISCLOSURE.md)。

详细设计见 [`docs/PRODUCT_SPEC.md`](docs/PRODUCT_SPEC.md)。

## 源码结构

- `src/`：React/TypeScript 桌面界面。
- `src-tauri/`：Rust/Tauri 本地后端与系统集成。
- `engine/`：DynamicProxyEngine 的 Python 源码与回归测试。
- `scripts/`：可复现构建、签名、校验与打包脚本。
- `THIRD_PARTY_NOTICES.md`：内置 Mihomo 与地理数据库的来源、许可证和哈希。

生成的安装包、签名私钥、本机凭据、运行状态和诊断归档不会提交到源码仓库。

## 许可证

本项目以 [`GPL-3.0-only`](LICENSE) 发布。内置第三方组件保留各自许可证；
完整归属和对应源码位置见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

提交问题前请先阅读 [`SECURITY.md`](SECURITY.md)，不要在公开 Issue 中粘贴订阅链接、
密码、Cookie、私钥或未经脱敏的诊断文件。
