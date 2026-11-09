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
