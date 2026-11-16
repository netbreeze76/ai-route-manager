# Claude Cage 源码审阅与融合记录

审阅时间：2026-11-15（Asia/Shanghai）
上游仓库：<https://github.com/Lee-xeo/Claude-Cage>
固定版本：`89a36b866400cb53113b8e2ce09a6ca6e5af0552`

## 审阅范围

本次直接阅读了上游公开源码中的以下关键路径，而不是只根据 README 猜功能：

- `internal/cage/egress.go`：单一上游、无直连回退、DNS 经同一代理通道、拒绝公共 UDP/IPv6。
- `internal/workbench/network_intent.go`：网络暂停意图持久化；记录缺失或损坏时保持离线。
- `internal/workbench/proxy_exit_policy.go`：活动节点与出口策略绑定；写入失败时恢复旧配置。
- `internal/workbench/device_check.go`：检查项使用 `passed`、`failed`、`warning`、`unknown` 与具体证据，不把不可观察状态冒充成功。
- `internal/workbench/device_check_test.go`、`network_intent_test.go`、`proxy_exit_policy_test.go`：核对失败分支、恢复与证据边界。

## 取长补短

| Claude Cage 做法 | AI Route Manager 融合结果 |
| --- | --- |
| 关键证据不完整时不开放网络 | 新增“出口安全门”；托管启动后必须通过关键检查，否则停止核心并恢复原系统代理 |
| 活动出口与配置身份绑定 | 同时核对 `active_slot`、活动端口、固定出口开关与保存的固定出口身份 |
| 不以“端口监听”代替完整证明 | 额外核对 WinINET 实际归属、`MATCH,DIRECT`、AI 链不含 `DIRECT` |
| 检查结果携带证据和未知状态 | 诊断中心逐项展示证据；DNS、UDP、IPv6 无直接观测时显示 `unknown` |
| 无直连回退 | 保留 AI 严格链失败关闭；默认未匹配流量继续明确为 `DIRECT`，避免误入下载线路 |

## 明确没有照搬的部分

AI Route Manager 是宿主系统的代理与路由策略管理器，不是 Claude Cage 的虚拟机网络层。因此本次没有复制 VM、Lima、gVisor、浏览器指纹、多账号或移动远控代码，也没有声称系统代理已经隔离宿主 DNS、UDP、IPv6。要达到全流量强隔离，需要独立的 TUN、虚拟机或网关执行层与相应实测，不能由 HTTP 代理监听状态推断。

## 许可

Claude Cage 主项目采用 MIT 许可。本项目没有引入其源文件或二进制，只吸收公开设计思想并重新实现；来源和固定审阅版本仍记录在 `THIRD_PARTY_NOTICES.md` 中。
