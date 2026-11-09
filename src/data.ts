import type {
  AppPreferences,
  ProxySource,
  ReferralOffer,
  RemoteDevice,
  ServicePolicy,
} from "./types";

export const officialUpdateEndpoint = "https://139-59-246-27.sslip.io/ai-route-manager/latest.json";
export const officialReferralCatalogEndpoint = "https://139-59-246-27.sslip.io/ai-route-manager/referrals/latest.json";

export const defaultPreferences: AppPreferences = {
  mode: "preview",
  strictAi: true,
  autoStart: true,
  autoCheckUpdates: true,
  updateEndpoint: officialUpdateEndpoint,
  allowSameRegionFallback: true,
  diagnosticsRetentionDays: 14,
  activeScenario: "日常模式",
  showPublisherOffers: true,
};

export const defaultSources: ProxySource[] = [
  {
    id: "stable",
    name: "稳定订阅",
    role: "stable",
    health: "healthy",
    nodes: 32,
    latency: 86,
    quota: 68,
    nextUpdate: "28 分钟后",
    endpoint: "https://••••••••/stable",
  },
  {
    id: "download-a",
    name: "下载源 1",
    role: "download",
    health: "healthy",
    nodes: 34,
    latency: 142,
    quota: 84,
    nextUpdate: "28 分钟后",
    endpoint: "https://••••••••/download-a",
  },
  {
    id: "download-b",
    name: "下载源 2",
    role: "download",
    health: "healthy",
    nodes: 13,
    latency: 119,
    quota: 41,
    nextUpdate: "28 分钟后",
    endpoint: "https://••••••••/download-b",
  },
  {
    id: "sg-vps",
    name: "用户配置的固定出口",
    role: "fixed-exit",
    health: "warning",
    nodes: 1,
    latency: 93,
    quota: null,
    nextUpdate: "手动配置",
    endpoint: "固定出口已加密保存",
  },
];

export const defaultServices: ServicePolicy[] = [
  { id: "openai", name: "ChatGPT / OpenAI", category: "ai", icon: "AI", policy: "严格目标地区", exit: "固定出口", health: "healthy", strict: true },
  { id: "anthropic", name: "Claude / Anthropic", category: "ai", icon: "CL", policy: "严格目标地区", exit: "固定出口", health: "healthy", strict: true },
  { id: "google-ai", name: "Gemini / Google AI", category: "ai", icon: "GM", policy: "目标地区优先", exit: "同地区兜底", health: "healthy", strict: false },
  { id: "steam", name: "Steam 商店与登录", category: "game", icon: "ST", policy: "账户地区保护", exit: "稳定订阅", health: "healthy", strict: true },
  { id: "steam-cdn", name: "Steam 游戏下载", category: "download", icon: "DL", policy: "便宜高速", exit: "下载源 2", health: "healthy", strict: false },
  { id: "netflix", name: "Netflix", category: "stream", icon: "NF", policy: "会话出口锁定", exit: "流媒体线路", health: "idle", strict: false },
  { id: "apple-tv", name: "Apple TV+", category: "stream", icon: "TV", policy: "会话出口锁定", exit: "流媒体线路", health: "idle", strict: false },
  { id: "telegram", name: "Telegram", category: "work", icon: "TG", policy: "便宜线路", exit: "下载源 1", health: "healthy", strict: false },
  { id: "china", name: "中国大陆服务", category: "direct", icon: "CN", policy: "直连保护", exit: "DIRECT", health: "healthy", strict: true },
];

export const defaultDevices: RemoteDevice[] = [
  { id: "desktop", name: "主力 Windows", platform: "Windows 11", status: "healthy", route: "本机", latency: 0, resolution: "2560×1440", codec: "NVENC HEVC" },
  { id: "mac-mini", name: "Mac mini", platform: "macOS", status: "idle", route: "等待配对", latency: null, resolution: "自动", codec: "VideoToolbox" },
  { id: "apple-tv", name: "客厅 Apple TV", platform: "tvOS", status: "idle", route: "局域网直连", latency: 4, resolution: "4K 60", codec: "HEVC" },
];

export const defaultOffers: ReferralOffer[] = [];
