export function normalizeUpdateEndpoint(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  const parsed = new URL(trimmed);
  if (parsed.protocol !== "https:") throw new Error("更新源必须使用 HTTPS");
  if (parsed.username || parsed.password) throw new Error("更新源不能包含用户名或密码");
  if (parsed.hash) throw new Error("更新源不能包含 URL 片段");
  return parsed.toString();
}

export function updateEndpointError(value: string): string | null {
  try {
    const normalized = normalizeUpdateEndpoint(value);
    return normalized ? null : "请先填写 HTTPS 更新清单地址";
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

export function updateProgressLabel(downloaded: number, total: number | null): string {
  if (!total || total <= 0) return `已下载 ${(downloaded / 1024 / 1024).toFixed(1)} MB`;
  const percent = Math.min(100, Math.max(0, Math.round((downloaded / total) * 100)));
  return `下载中 ${percent}%`;
}
