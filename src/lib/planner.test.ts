import { describe, expect, it } from "vitest";
import { defaultPreferences, defaultServices } from "../data";
import type { ReferralOffer } from "../types";
import type { ConnectionProfile } from "./backend";
import { buildRoutePlan, maskEndpoint, normalizeConnectionProfileForSave, normalizeReferralOffers, referralDestinationHost, referralForPlacement, referralsForPlacement, validateOffer } from "./planner";

describe("public connection profile", () => {
  it("keeps new-user defaults neutral and omits optional blank fields", () => {
    const profile: ConnectionProfile = {
      subscriptions: [
        { role: "stable", label: "稳定订阅", prefix: "稳定", url: " https://example.com/stable " },
        { role: "download1", label: "下载订阅 1", prefix: "下载1", url: "" },
        { role: "download2", label: "下载订阅 2", prefix: "下载2", url: "   " },
      ],
      fixedEgress: { name: "固定出口", server: "", port: 443, cipher: "aes-128-gcm", password: "" },
      aiRegion: " us ",
      aiRegionLabel: " 美国 ",
      preferredNodeKeywords: [" Premium ", "", " 低延迟 "],
    };
    const normalized = normalizeConnectionProfileForSave(profile);
    expect(normalized.subscriptions.map((source) => source.role)).toEqual(["stable"]);
    expect(normalized.fixedEgress).toBeNull();
    expect(normalized.aiRegion).toBe("US");
    expect(normalized.aiRegionLabel).toBe("美国");
    expect(normalized.preferredNodeKeywords).toEqual(["Premium", "低延迟"]);
  });

  it("preserves a user-configured fixed exit", () => {
    const profile: ConnectionProfile = {
      subscriptions: [{ role: "stable", label: "稳定订阅", prefix: "稳定", url: "https://example.com/stable" }],
      fixedEgress: { name: "我的出口", server: "vps.example.com", port: 443, cipher: "aes-128-gcm", password: "secret" },
      aiRegion: "JP",
      aiRegionLabel: "日本",
      preferredNodeKeywords: [],
    };
    expect(normalizeConnectionProfileForSave(profile).fixedEgress).toEqual(profile.fixedEgress);
  });
});

describe("route planner", () => {
  it("never sends strict AI services to DIRECT", () => {
    const plan = buildRoutePlan(defaultServices, defaultPreferences);
    const ai = plan.filter((item) => item.service.includes("OpenAI") || item.service.includes("Claude"));
    expect(ai).toHaveLength(2);
    expect(ai.every((item) => item.failClosed)).toBe(true);
    expect(ai.every((item) => item.fallback !== "DIRECT")).toBe(true);
  });

  it("keeps China services direct", () => {
    const plan = buildRoutePlan(defaultServices, defaultPreferences);
    expect(plan.find((item) => item.service === "中国大陆服务")?.target).toBe("DIRECT");
  });
});

describe("referral offer validation", () => {
  const offer: ReferralOffer = {
    id: "x",
    category: "vps",
    placement: "vps",
    provider: "Provider",
    title: "Offer",
    summary: "Summary",
    badge: "推荐",
    price: "$5",
    url: "https://example.com/ref/code",
    coupon: "SAVE",
    disclosure: "此链接为推广链接，推荐人可能获得返利。",
    enabled: true,
    priority: 50,
    clickCount: 0,
  };

  it("requires an HTTPS link for enabled offers", () => {
    expect(validateOffer({ ...offer, url: "http://example.com" })).toContain(
      "启用的推荐位必须填写有效的 HTTPS 链接",
    );
    expect(validateOffer({ ...offer, url: "https://?missing-host" })).toContain(
      "启用的推荐位必须填写有效的 HTTPS 链接",
    );
  });

  it("requires disclosure", () => {
    expect(validateOffer({ ...offer, disclosure: "普通链接" })).toContain(
      "必须向用户披露合作推广关系",
    );
  });

  it("rejects embedded URL credentials and invalid priority", () => {
    expect(validateOffer({ ...offer, url: "https://name:secret@example.com/ref", priority: 101 })).toEqual(
      expect.arrayContaining(["推荐链接不能包含网址用户名或密码", "推荐优先级必须是 0 到 100 的整数"]),
    );
  });

  it("chooses the highest-priority valid placement", () => {
    const lower = { ...offer, id: "lower", priority: 10 };
    const higher = { ...offer, id: "higher", priority: 90 };
    const invalid = { ...offer, id: "invalid", priority: 100, url: "" };
    expect(referralForPlacement([lower, invalid, higher], "vps")?.id).toBe("higher");
    expect(referralsForPlacement([lower, invalid, higher], "vps").map((item) => item.id)).toEqual(["higher", "lower"]);
  });

  it("removes legacy empty cards and migrates known recommendations to the intended sections", () => {
    const legacyBlank = { ...offer, id: "vpn-download", url: "", enabled: false } as ReferralOffer;
    const bajie = { ...offer, id: "bajie-vpn", placement: "general" as const };
    const mojie = { ...offer, id: "vpn-second", placement: "general" as const };
    const digitalOcean = { ...offer, id: "vps-primary", placement: "vps" as const };
    const migrated = normalizeReferralOffers([legacyBlank, bajie, mojie, digitalOcean]);
    expect(migrated.map((item) => [item.id, item.placement])).toEqual([
      ["bajie-vpn", "download-vpn"],
      ["vpn-second", "download-vpn"],
      ["vps-primary", "stable-vpn"],
    ]);
  });

  it("keeps signed publisher metadata separate from local offers", () => {
    const [local, publisher] = normalizeReferralOffers([
      offer,
      { ...offer, id: "publisher-vps", origin: "publisher", catalogVersion: "2026-11-04" },
    ]);
    expect(local.origin).toBe("local");
    expect(local.catalogVersion).toBeUndefined();
    expect(publisher.origin).toBe("publisher");
    expect(publisher.catalogVersion).toBe("2026-11-04");
  });

  it("shows only the destination host before opening a referral", () => {
    expect(referralDestinationHost("https://shop.example.com/ref/user?token=hidden")).toBe("shop.example.com");
    expect(referralDestinationHost("https://name:secret@example.com/ref")).toBe("无效目标");
    expect(referralDestinationHost("javascript:alert(1)")).toBe("无效目标");
  });
});

describe("secret masking", () => {
  it("removes path and query tokens", () => {
    expect(maskEndpoint("https://example.com/sub?token=secret")).toBe(
      "https://example.com/••••••••",
    );
  });
});
