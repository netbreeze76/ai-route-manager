import { describe, expect, it } from "vitest";
import appSource from "./App.tsx?raw";
import offers from "../referrals/offers.json";

describe("正式版交互门禁", () => {
  it("不再展示没有实现来源的 SSH 转发与虚构设备图", () => {
    expect(appSource).not.toContain("SSH 转发");
    expect(appSource).not.toContain("尚未读取真实设备");
    expect(appSource).not.toContain("device-map panel");
  });

  it("完整链路和场景分类都有真实点击处理", () => {
    expect(appSource).toContain("onClick={() => setChainExpanded");
    expect(appSource).toContain("buildRoutePlan(services, preferences)");
    expect(appSource).toContain('onClick={() => setCategory("all")}');
    expect(appSource).toContain("onClick={() => setCategory(id)}");
  });

  it("诊断中心提供入口与监督器修复动作", () => {
    expect(appSource).toContain('action: "managed" as const');
    expect(appSource).toContain('action: "supervisor" as const');
    expect(appSource).toContain("await repairSupervisor()");
  });

  it("出口安全门展示逐项证据并保留 DNS UDP IPv6 未知边界", () => {
    expect(appSource).toContain("出口安全门");
    expect(appSource).toContain("runEgressSafetyAudit()");
    expect(appSource).toContain("监听端口存在不等于出口已经正确提交");
    expect(appSource).toContain("DNS、UDP、IPv6");
    expect(appSource).toContain("check.critical ? \"关键\"");
  });

  it("策略中心把单域名和服务组规则集成到场景列表并立即应用", () => {
    expect(appSource).toContain("新增域名 / 服务");
    expect(appSource).toContain("单个域名");
    expect(appSource).toContain("一组服务");
    expect(appSource).toContain("groupCustomRoutes");
    expect(appSource).toContain("loadCustomRoutes()");
    expect(appSource).toContain("saveCustomRoutes(routes)");
    expect(appSource).toContain("custom-service-row");
    expect(appSource).toContain("服务域名列表（每行一个，也支持逗号）");
    expect(appSource).toContain('download: "Telegram / 下载线路"');
    expect(appSource).toContain('direct: "直接连接"');
    expect(appSource).not.toContain('<section className="panel custom-routes-panel">');
  });

  it("推荐中心把八戒和魔戒归入大流量，把 VPS 归入稳定出口", () => {
    expect(offers.map((offer) => [offer.provider, offer.placement])).toEqual([
      ["八戒 VPN", "download-vpn"],
      ["魔戒", "download-vpn"],
      ["DigitalOcean", "stable-vpn"],
    ]);
    expect(appSource).toContain('{ placement: "download-vpn", title: "大流量线路"');
    expect(appSource).toContain('{ placement: "stable-vpn", title: "稳定路线出口"');
    expect(appSource).toContain("matchingOffers.map");
    expect(appSource).not.toContain('ReferralPurchaseCard placement="vps"');
  });
});
