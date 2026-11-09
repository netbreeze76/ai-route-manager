# 远程控制低延迟方案

## 自动选择

| 场景 | 首选 | 兜底 | 目标 |
|---|---|---|---|
| 文档与维护 | RustDesk P2P | 自建 RustDesk 中继 | 低带宽、稳定输入 |
| 1440p 桌面 | Sunshine + Moonlight | RustDesk 高画质 | 60 fps、硬件编码 |
| 游戏 | Sunshine + Moonlight UDP | 同城/近端中继 | 低 RTT、120 fps 可选 |
| 移动网络 | 动态码率 HEVC | H.264 兼容 | 抗抖动与快速恢复 |

## 调优顺序

1. 主机有线接入，客户端 5 GHz/Wi-Fi 6。
2. 先争取 UDP 点对点，VPS 只作信令或最后中继。
3. 使用 NVENC/AMF/Quick Sync/VideoToolbox 硬件编码。
4. 匹配客户端分辨率，RTT 或丢包升高时先降码率，再降分辨率。
5. 持续采样 RTT、抖动、丢包、解码时间和丢帧，自动切换预设。

## 当前桌面端实现

- 只读检测 RustDesk、Sunshine、Moonlight 的安装路径、运行进程以及 NVIDIA NVENC 能力，不会自行启动服务。
- 对 Sunshine TCP 47984 或 RustDesk Direct IP TCP 21118 连续执行 5 次握手，计算成功率、平均握手时延与抖动；该结果是建连路径指标，不冒充 ICMP RTT、实际串流丢帧或可用带宽。
- 根据实测路径生成四档建议，并导出 Sunshine 配置片段、Moonlight 客户端参数和 RustDesk 安全/直连说明。
- Windows Sunshine 防火墙向导只创建 Private Profile 入站规则，来源只能选择 `LocalSubnet` 或 Tailscale/Headscale `100.64.0.0/10`；不会配置公网 `Any`，并可一键等范围删除。
- Sunshine 的 NVENC 建议保持 P1、quarter-res two-pass、关闭 spatial AQ 与 VBV burst，优先控制编码延迟和突发丢包；Moonlight 分辨率尽量与主机显示器匹配。
- RustDesk 路径顺序保持 P2P/直连优先，自建服务器只负责 rendezvous；打洞失败才使用 hbbr relay。
- Windows 缺少 Sunshine 或 Moonlight 时可在明确确认后调用系统 `winget` 安装固定白名单包；RustDesk 因当前源没有已核验包 ID，始终引导到官方下载页。安装不会自动启动软件或切换系统代理。
- RustDesk Server 部署器只接受 IP/域名、端口、受限 SSH 用户名和已存在的绝对私钥路径；固定脚本从 stdin 发送给 `ssh ... sh -s`，用户输入不拼进远端 Shell 脚本。
- 部署器使用官方 `rustdesk/rustdesk-server:latest` 镜像启动 hbbs/hbbr，共享 `~/ai-route-rustdesk/data` 持久目录，只回传 `id_ed25519.pub` 公钥与容器状态，不读取或返回服务器私钥。
- 部署成功后列出原生客户端所需的 `21115/tcp`、`21116/tcp+udp`、`21117/tcp`；用户二次确认后可通过固定 UFW 脚本应用或回滚这四条最小规则。未使用 Web 客户端时保持 `21118/tcp`、`21119/tcp` 关闭。
- RustDesk Server 镜像锁定为 `rustdesk/rustdesk-server:1.1.16`，避免 `latest` 在无人确认时漂移。
