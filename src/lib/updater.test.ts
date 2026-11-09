import { describe, expect, it } from "vitest";
import { officialUpdateEndpoint } from "../data";
import { normalizeUpdateEndpoint, updateEndpointError, updateProgressLabel } from "./updater";

describe("signed updater", () => {
  it("ships with the production HTTPS manifest", () => {
    expect(officialUpdateEndpoint).toBe("https://139-59-246-27.sslip.io/ai-route-manager/latest.json");
    expect(updateEndpointError(officialUpdateEndpoint)).toBeNull();
  });

  it("accepts only credential-free HTTPS manifests", () => {
    expect(normalizeUpdateEndpoint(" https://updates.example.com/latest.json ")).toBe("https://updates.example.com/latest.json");
    expect(() => normalizeUpdateEndpoint("http://updates.example.com/latest.json")).toThrow("必须使用 HTTPS");
    expect(() => normalizeUpdateEndpoint("https://name:secret@updates.example.com/latest.json")).toThrow("不能包含用户名或密码");
    expect(() => normalizeUpdateEndpoint("https://updates.example.com/latest.json#old")).toThrow("不能包含 URL 片段");
  });

  it("keeps an empty endpoint visibly unconfigured", () => {
    expect(normalizeUpdateEndpoint("  ")).toBe("");
    expect(updateEndpointError("")).toBe("请先填写 HTTPS 更新清单地址");
  });

  it("reports bounded updater progress", () => {
    expect(updateProgressLabel(50, 100)).toBe("下载中 50%");
    expect(updateProgressLabel(200, 100)).toBe("下载中 100%");
    expect(updateProgressLabel(1048576, null)).toBe("已下载 1.0 MB");
  });
});
