export type NavId =
  | "dashboard"
  | "sources"
  | "services"
  | "devices"
  | "remote"
  | "marketplace"
  | "diagnostics"
  | "settings";

export type Health = "healthy" | "warning" | "offline" | "idle";

export type SourceRole = "stable" | "download" | "backup" | "fixed-exit";

export interface ProxySource {
  id: string;
  name: string;
  role: SourceRole;
  health: Health;
  nodes: number;
  latency: number | null;
  quota: number | null;
  nextUpdate: string;
  endpoint: string;
}

export interface ServicePolicy {
  id: string;
  name: string;
  category: "ai" | "game" | "stream" | "work" | "download" | "direct";
  icon: string;
  policy: string;
  exit: string;
  health: Health;
  strict: boolean;
}

export interface RemoteDevice {
  id: string;
  name: string;
  platform: string;
  status: Health;
  route: string;
  latency: number | null;
  resolution: string;
  codec: string;
}

export interface ReferralOffer {
  id: string;
  category: "vps" | "vpn" | "hardware" | "software";
  placement: "vps" | "stable-vpn" | "download-vpn" | "remote-control" | "general";
  provider: string;
  title: string;
  summary: string;
  badge: string;
  price: string;
  url: string;
  coupon: string;
  disclosure: string;
  enabled: boolean;
  priority: number;
  clickCount: number;
  origin?: "local" | "publisher";
  catalogVersion?: string;
}

export interface AppPreferences {
  mode: "preview" | "managed";
  strictAi: boolean;
  autoStart: boolean;
  autoCheckUpdates: boolean;
  updateEndpoint: string;
  allowSameRegionFallback: boolean;
  diagnosticsRetentionDays: number;
  activeScenario: string;
  showPublisherOffers: boolean;
}
