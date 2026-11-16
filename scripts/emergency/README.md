# AI Route Manager 紧急国内直连恢复

双击 `一键恢复国内网络.cmd`。

它只处理本机，不接触路由器、Xbox、Apple TV 或其他家庭设备。操作顺序：

1. 备份当前代理、偏好设置和监督器状态；
2. 把 AI Route Manager 切到预览模式并关闭自动恢复；
3. 关闭 WinINET、用户环境变量和用户级 WinHTTP 代理；
4. 只停止 AI Route Manager 自己的后台、引擎和托管 Mihomo；
5. 不停止 FlClash、Telegram 独立代理或其他软件；
6. 使用不经过任何代理的请求验证百度、QQ、阿里云，至少一个成功才报告恢复完成。

检查而不修改：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\Restore-Domestic-Network.ps1 -DryRun
```

离线自检：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\Restore-Domestic-Network.ps1 -SelfTest
```
