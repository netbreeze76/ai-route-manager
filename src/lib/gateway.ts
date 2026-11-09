import type { GatewayDevice, GatewayDevicePolicy, GatewayScenario } from "./backend";

export const gatewayScenarioOptions: Array<{
  id: GatewayScenario;
  label: string;
  detail: string;
}> = [
  { id: "direct", label: "国内优先 / 直连", detail: "中国大陆与设备默认流量直连" },
  { id: "ai", label: "AI 固定地区", detail: "设备默认流量使用 AI 严格出口" },
  { id: "streaming", label: "Apple TV / 流媒体", detail: "视频与设备默认流量使用高带宽线路" },
  { id: "gaming", label: "Xbox / 低延迟游戏", detail: "设备默认流量使用稳定低延迟线路" },
  { id: "download", label: "主机 / 大文件下载", detail: "设备默认流量使用下载线路池" },
];

export function preferredGatewayDeviceIp(device: GatewayDevice): string | null {
  return device.ipv4 ?? device.ipv6[0] ?? null;
}

export function defaultGatewayScenario(device: GatewayDevice): GatewayScenario {
  if (device.kind === "apple-tv" || device.kind === "smart-tv") return "streaming";
  if (device.kind === "xbox") return "gaming";
  return "direct";
}

export function policyFromGatewayDevice(
  device: GatewayDevice,
  existing?: GatewayDevicePolicy,
): GatewayDevicePolicy | null {
  const ip = preferredGatewayDeviceIp(device);
  if (!ip) return null;
  return {
    id: device.id,
    name: existing?.name ?? device.name,
    ip,
    mac: device.mac,
    scenario: existing?.scenario ?? defaultGatewayScenario(device),
    enabled: existing?.enabled ?? true,
  };
}

export function mergeGatewayDiscovery(
  devices: GatewayDevice[],
  policies: GatewayDevicePolicy[],
): GatewayDevicePolicy[] {
  const byId = new Map(policies.map((policy) => [policy.id, policy]));
  const discovered = devices
    .map((device) => policyFromGatewayDevice(device, byId.get(device.id)))
    .filter((policy): policy is GatewayDevicePolicy => policy !== null);
  const discoveredIds = new Set(discovered.map((policy) => policy.id));
  return [...discovered, ...policies.filter((policy) => !discoveredIds.has(policy.id))];
}
