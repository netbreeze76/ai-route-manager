import { describe, expect, it } from "vitest";
import type { GatewayDevice, GatewayDevicePolicy } from "./backend";
import { defaultGatewayScenario, mergeGatewayDiscovery, preferredGatewayDeviceIp } from "./gateway";

const devices: GatewayDevice[] = [
  { id: "apple", name: "AppleTV-Living", mac: "aa:bb:cc:dd:ee:01", ipv4: "192.168.1.20", ipv6: ["fd00::20"], kind: "apple-tv", online: true },
  { id: "xbox", name: "Xbox-Series-X", mac: "aa:bb:cc:dd:ee:02", ipv4: null, ipv6: ["fd00::30"], kind: "xbox", online: true },
];

describe("gateway device policy helpers", () => {
  it("preserves the user-assigned name on rediscovery", () => {
    const existing: GatewayDevicePolicy = { id: "apple", name: "我的电视", ip: "192.168.1.19", mac: devices[0].mac, scenario: "gaming", enabled: false };
    const [merged] = mergeGatewayDiscovery([devices[0]], [existing]);
    expect(merged.name).toBe("我的电视");
    expect(merged.ip).toBe("192.168.1.20");
    expect(merged.scenario).toBe("gaming");
    expect(merged.enabled).toBe(false);
  });
  it("selects IPv4 first and assigns device-aware defaults", () => {
    expect(preferredGatewayDeviceIp(devices[0])).toBe("192.168.1.20");
    expect(preferredGatewayDeviceIp(devices[1])).toBe("fd00::30");
    expect(defaultGatewayScenario(devices[0])).toBe("streaming");
    expect(defaultGatewayScenario(devices[1])).toBe("gaming");
  });

  it("preserves user scenarios and retains offline saved policies", () => {
    const saved: GatewayDevicePolicy[] = [
      { id: "apple", name: "客厅 Apple TV", ip: "192.168.1.20", mac: "aa:bb:cc:dd:ee:01", scenario: "direct", enabled: false },
      { id: "offline", name: "离线设备", ip: "192.168.1.99", mac: "aa:bb:cc:dd:ee:99", scenario: "download", enabled: true },
    ];
    const merged = mergeGatewayDiscovery(devices, saved);
    expect(merged).toHaveLength(3);
    expect(merged[0]).toMatchObject({ id: "apple", scenario: "direct", enabled: false });
    expect(merged[1]).toMatchObject({ id: "xbox", scenario: "gaming" });
    expect(merged[2]).toMatchObject({ id: "offline", scenario: "download" });
  });
});
