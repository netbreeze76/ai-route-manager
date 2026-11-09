import type { AppPreferences, ReferralOffer, ServicePolicy } from "../types";
import type { ConnectionProfile } from "./backend";

export interface RoutePlanItem {
  service: string;
  target: string;
  fallback: string;
  failClosed: boolean;
}

export function normalizeConnectionProfileForSave(profile: ConnectionProfile): ConnectionProfile {
  const fixed = profile.fixedEgress;
  const fixedIsBlank = Boolean(fixed && !fixed.server.trim() && !fixed.password);
  return {
    ...profile,
    subscriptions: profile.subscriptions
      .map((source) => ({ ...source, url: source.url.trim() }))
      .filter((source) => source.url),
    fixedEgress: fixedIsBlank ? null : fixed,
    aiRegion: profile.aiRegion.trim().toUpperCase(),
    aiRegionLabel: profile.aiRegionLabel.trim(),
    preferredNodeKeywords: profile.preferredNodeKeywords.map((item) => item.trim()).filter(Boolean),
  };
}

export function buildRoutePlan(
  services: ServicePolicy[],
  preferences: AppPreferences,
): RoutePlanItem[] {
  return services.map((service) => {
    const isAi = service.category === "ai";
    const failClosed = service.strict || (isAi && preferences.strictAi);
    const fallback = failClosed
      ? preferences.allowSameRegionFallback
        ? "仅同地区批准出口"
        : "阻断"
      : service.category === "direct"
        ? "DIRECT"
        : "稳定订阅";

    return {
      service: service.name,
      target: service.exit,
      fallback,
      failClosed,
    };
  });
}

export function validateOffer(offer: ReferralOffer): string[] {
  const errors: string[] = [];
  if (!offer.provider.trim()) errors.push("服务商名称不能为空");
  if (!offer.title.trim()) errors.push("推荐标题不能为空");
  if (offer.enabled) {
    try {
      const url = new URL(offer.url);
      if (url.protocol !== "https:" || !url.hostname) {
        errors.push("启用的推荐位必须填写有效的 HTTPS 链接");
      }
      if (url.username || url.password) {
        errors.push("推荐链接不能包含网址用户名或密码");
      }
    } catch {
      errors.push("启用的推荐位必须填写有效的 HTTPS 链接");
    }
  }
  if (!Number.isInteger(offer.priority) || offer.priority < 0 || offer.priority > 100) {
    errors.push("推荐优先级必须是 0 到 100 的整数");
  }
  if (!offer.disclosure.includes("推广") && !offer.disclosure.includes("返利")) {
    errors.push("必须向用户披露合作推广关系");
  }
  return errors;
}

export function referralForPlacement(
  offers: ReferralOffer[],
  placement: ReferralOffer["placement"],
): ReferralOffer | null {
  return referralsForPlacement(offers, placement)[0] ?? null;
}

export function referralsForPlacement(
  offers: ReferralOffer[],
  placement: ReferralOffer["placement"],
): ReferralOffer[] {
  return offers
    .filter((offer) => offer.enabled && offer.placement === placement && validateOffer(offer).length === 0)
    .sort((left, right) => right.priority - left.priority || left.title.localeCompare(right.title));
}

export function normalizeReferralOffers(offers: ReferralOffer[]): ReferralOffer[] {
  const legacyEmptyTemplateIds = new Set(["vps-primary", "vpn-stable", "vpn-download"]);
  const correctedPlacements: Partial<Record<string, ReferralOffer["placement"]>> = {
    "bajie-vpn": "download-vpn",
    "publisher-bajie-vpn": "download-vpn",
    "vpn-second": "download-vpn",
    "publisher-vpn-second": "download-vpn",
    "vps-primary": "stable-vpn",
    "publisher-digitalocean-vps": "stable-vpn",
  };
  return offers.filter((offer) => !(legacyEmptyTemplateIds.has(offer.id) && !offer.enabled && !(offer.url ?? "").trim())).map((offer) => {
    let secureConfiguredUrl = false;
    try {
      const url = new URL(offer.url ?? "");
      secureConfiguredUrl = url.protocol === "https:" && Boolean(url.hostname) && !url.username && !url.password;
    } catch {
      secureConfiguredUrl = false;
    }
    return {
      ...offer,
      placement: correctedPlacements[offer.id] ?? offer.placement ?? "general",
      enabled: Boolean(offer.enabled && secureConfiguredUrl),
      priority: Number.isInteger(offer.priority) ? offer.priority : 50,
      clickCount: Number.isInteger(offer.clickCount) ? offer.clickCount : 0,
      origin: offer.origin === "publisher" ? "publisher" : "local",
      catalogVersion: offer.origin === "publisher" ? offer.catalogVersion : undefined,
    };
  });
}

export function referralDestinationHost(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" || parsed.username || parsed.password) return "无效目标";
    return parsed.hostname;
  } catch {
    return "无效目标";
  }
}

export function maskEndpoint(endpoint: string): string {
  try {
    const url = new URL(endpoint);
    return `${url.protocol}//${url.hostname}/••••••••`;
  } catch {
    return "未配置";
  }
}
