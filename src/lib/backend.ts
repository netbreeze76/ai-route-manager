import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AppPreferences, ReferralOffer } from "../types";

export interface PortStatus {
  label: string;
  host: string;
  port: number;
  reachable: boolean;
}

export interface SystemSnapshot {
  os: string;
  arch: string;
  processId: number;
  previewOnly: boolean;
  ports: PortStatus[];
}

export interface EngineReport {
  available: boolean;
  executable: string | null;
  success: boolean;
  status: Record<string, unknown> | null;
  message: string;
}

export interface SubscriptionInput {
  role: "stable" | "download1" | "download2";
  label: string;
  prefix: string;
  url: string;
}

export interface FixedEgressInput {
  name: string;
  server: string;
  port: number;
  cipher: string;
  password: string;
}

export interface ConnectionProfile {
  subscriptions: SubscriptionInput[];
  fixedEgress: FixedEgressInput | null;
  aiRegion: string;
  aiRegionLabel: string;
  preferredNodeKeywords: string[];
}

export interface ProfileSummary {
  configured: boolean;
  sources: Array<{ role: string; label: string; host: string }>;
  fixedEgressConfigured: boolean;
  fixedEgressHost: string | null;
  aiRegion: string;
  aiRegionLabel: string;
  preferredNodeKeywords: string[];
}

export interface PreviewReport {
  success: boolean;
  message: string;
  result: Record<string, unknown> | null;
  cleanupComplete: boolean;
}

export interface ManagedStatus {
  configured: boolean;
  running: boolean;
  listener: boolean;
  activeProxyPort: number | null;
  systemProxy: string | null;
  legacyConflict: boolean;
  message: string;
}

export interface ManagedActionReport {
  success: boolean;
  message: string;
  status: ManagedStatus;
  preview: PreviewReport | null;
}

export interface SupervisorStatus {
  active: boolean;
  mode: "preview" | "managed";
  heartbeatAtMs: number;
  consecutiveFailures: number;
  restartCount: number;
  lastRecoveryAtMs: number | null;
  lastError: string | null;
  restartPolicy: string;
}

export interface UpdateCheckReport {
  available: boolean;
  currentVersion: string;
  version: string | null;
  notes: string | null;
  publishedAt: string | null;
  message: string;
}

export type UpdateDownloadEvent =
  | { event: "started"; data: { contentLength: number | null } }
  | { event: "progress"; data: { chunkLength: number } }
  | { event: "finished" };

export interface MobileExportRequest {
  confirmedSecretExport: boolean;
  includeAndroid: boolean;
  includeShadowrocket: boolean;
}

export interface MobileExportFile {
  name: string;
  path: string;
  kind: string;
}

export interface MobileExportReport {
  success: boolean;
  directory: string;
  files: MobileExportFile[];
  warning: string;
  fixedEgressHost: string;
}

export interface GatewayDeployRequest {
  host: string;
  sshPort: number;
  user: string;
  identityFile: string;
  action: "preview" | "apply" | "rollback";
  confirmed: boolean;
}

export interface GatewayDeployReport {
  success: boolean;
  action: "preview" | "apply" | "rollback";
  host: string;
  integration: string;
  targetPath: string;
  configSha256: string | null;
  core: string;
  service: string;
  rollbackAvailable: boolean;
  message: string;
}

export interface GatewayInspectRequest {
  host: string;
  sshPort: number;
  user: string;
  identityFile: string;
  confirmed: boolean;
}

export interface GatewayDevice {
  id: string;
  name: string;
  mac: string;
  ipv4: string | null;
  ipv6: string[];
  kind: "xbox" | "apple-tv" | "apple-mobile" | "android" | "smart-tv" | "mac" | "computer" | "device";
  online: boolean;
}

export interface LocalNetworkReport {
  gateway: string;
  localIp: string;
  devices: GatewayDevice[];
}

export async function getLocalNetworkDevices(): Promise<LocalNetworkReport> {
  if (!isTauri()) throw new Error("请在桌面应用中读取本机网络");
  return invoke<LocalNetworkReport>("get_local_network_devices");
}

export interface GatewayCapabilities {
  openwrt: boolean;
  openclash: boolean;
  wireguard: boolean;
  openvpn: boolean;
  ipv6DefaultRoute: boolean;
  upnp: boolean;
  architecture: string;
  model: string;
  release: string;
}

export interface GatewayInspectReport {
  success: boolean;
  host: string;
  hostname: string;
  capabilities: GatewayCapabilities;
  devices: GatewayDevice[];
  message: string;
}

export type GatewayScenario = "direct" | "ai" | "streaming" | "gaming" | "download";

export interface GatewayDevicePolicy {
  id: string;
  name: string;
  ip: string;
  mac: string;
  scenario: GatewayScenario;
  enabled: boolean;
}

export interface LocalBackupStatus {
  available: boolean;
  createdAtMs: number | null;
  subscriptionCount: number;
  offerCount: number;
  gatewayPolicyCount: number;
  message: string;
}

export interface LocalBackupReport {
  success: boolean;
  status: LocalBackupStatus;
  message: string;
  requiresApply: boolean;
}

export interface DiagnosticExportReport {
  success: boolean;
  directory: string;
  files: MobileExportFile[];
  message: string;
}

export interface RemoteToolStatus {
  id: "rustdesk" | "sunshine" | "moonlight";
  name: string;
  installed: boolean;
  running: boolean;
  executable: string | null;
  officialUrl: string;
  installSupported: boolean;
}

export interface RemoteEnvironmentReport {
  tools: RemoteToolStatus[];
  gpu: string;
  encoder: "auto" | "nvenc" | "quicksync" | "amdvce";
  message: string;
}

export interface RemoteQualityReport {
  host: string;
  port: number;
  samples: number;
  successfulSamples: number;
  averageLatencyMs: number;
  jitterMs: number;
  lossPercent: number;
  score: number;
  recommendedPreset: string;
  resolution: string;
  fps: number;
  bitrateMbps: number;
  codec: string;
  message: string;
}

export interface SunshineConfigStatus {
  supported: boolean;
  configPath: string;
  exists: boolean;
  sunshineRunning: boolean;
  rollbackAvailable: boolean;
  appliedHash: string | null;
  message: string;
}

export interface SunshineConfigActionReport {
  success: boolean;
  action: "apply" | "rollback";
  configPath: string;
  backupPath: string | null;
  beforeHash: string | null;
  afterHash: string | null;
  managedValues: string[];
  restartRequired: boolean;
  message: string;
}

export interface RemoteInstallReport {
  success: boolean;
  toolId: string;
  packageId: string;
  exitCode: number | null;
  message: string;
}

export interface RustDeskDeployRequest {
  host: string;
  sshPort: number;
  user: string;
  identityFile: string;
  confirmed: boolean;
}

export interface RustDeskDeployReport {
  success: boolean;
  host: string;
  idServer: string;
  relayServer: string;
  publicKey: string;
  status: string;
  requiredPorts: string[];
  message: string;
}

export interface FirewallRuleSummary {
  label: string;
  protocol: string;
  ports: string;
  scope: string;
}

export interface FirewallActionReport {
  success: boolean;
  target: string;
  action: "apply" | "rollback";
  rules: FirewallRuleSummary[];
  message: string;
}

export interface PublisherReferralCatalogReport {
  catalogVersion: string;
  generatedAt: string;
  offers: ReferralOffer[];
  signatureVerified: boolean;
  message: string;
}

export async function loadPreferences(): Promise<AppPreferences | null> {
  if (!isTauri()) return null;
  return invoke<AppPreferences>("load_preferences");
}

export async function savePreferences(preferences: AppPreferences): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_preferences", { preferences });
}

export async function loadReferralOffers(): Promise<ReferralOffer[] | null> {
  if (!isTauri()) return null;
  return invoke<ReferralOffer[] | null>("load_referral_offers");
}

export async function saveReferralOffers(offers: ReferralOffer[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_referral_offers", { offers });
}

export async function loadPublisherReferralCatalog(endpoint: string): Promise<PublisherReferralCatalogReport | null> {
  if (!isTauri()) return null;
  const parsed = new URL(endpoint);
  if (parsed.protocol !== "https:" || !parsed.hostname || parsed.username || parsed.password || parsed.hash) {
    throw new Error("发行推荐目录必须使用不含账号、密码和片段的 HTTPS 地址");
  }
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 12_000);
  try {
    const response = await fetch(parsed.toString(), {
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: { Accept: "application/json" },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`发行推荐目录 HTTP ${response.status}`);
    const envelope = await response.text();
    if (envelope.length > 2_000_000) throw new Error("发行推荐目录超过 2 MB 限制");
    return invoke<PublisherReferralCatalogReport>("verify_referral_catalog", { envelope });
  } finally {
    window.clearTimeout(timer);
  }
}

export async function checkForUpdates(endpoint: string): Promise<UpdateCheckReport> {
  if (!isTauri()) throw new Error("请在桌面应用中检查更新");
  return invoke<UpdateCheckReport>("check_for_updates", { endpoint });
}

export async function installPendingUpdate(onEvent: (event: UpdateDownloadEvent) => void): Promise<void> {
  if (!isTauri()) throw new Error("请在桌面应用中安装更新");
  const channel = new Channel<UpdateDownloadEvent>();
  channel.onmessage = onEvent;
  await invoke("install_pending_update", { onEvent: channel });
}

export async function getSystemSnapshot(): Promise<SystemSnapshot | null> {
  if (!isTauri()) return null;
  return invoke<SystemSnapshot>("get_system_snapshot");
}

export async function getEngineStatus(): Promise<EngineReport | null> {
  if (!isTauri()) return null;
  return invoke<EngineReport>("get_engine_status");
}

export async function openExternalUrl(url: string): Promise<void> {
  const parsed = new URL(url);
  if (parsed.protocol !== "https:" || !parsed.hostname || parsed.username || parsed.password) {
    throw new Error("只允许打开不含网址账号或密码的有效 HTTPS 推荐链接");
  }
  if (isTauri()) {
    await openUrl(parsed.toString());
    return;
  }
  window.open(parsed.toString(), "_blank", "noopener,noreferrer");
}

export async function getConnectionProfileSummary(): Promise<ProfileSummary | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<ProfileSummary>("get_connection_profile_summary");
  } catch {
    return null;
  }
}

export async function saveConnectionProfile(profile: ConnectionProfile): Promise<ProfileSummary> {
  if (!isTauri()) throw new Error("请在桌面应用中保存连接配置");
  return invoke<ProfileSummary>("save_connection_profile", { profile });
}

export async function importExistingProfile(): Promise<ProfileSummary> {
  if (!isTauri()) throw new Error("请在桌面应用中导入现有配置");
  return invoke<ProfileSummary>("import_existing_profile");
}

export async function runEnginePreview(): Promise<PreviewReport> {
  if (!isTauri()) throw new Error("请在桌面应用中运行安全预演");
  return invoke<PreviewReport>("run_engine_preview");
}

export async function getManagedStatus(): Promise<ManagedStatus | null> {
  if (!isTauri()) return null;
  return invoke<ManagedStatus>("get_managed_status");
}

export async function getSupervisorStatus(): Promise<SupervisorStatus | null> {
  if (!isTauri()) return null;
  return invoke<SupervisorStatus>("get_supervisor_status");
}

export async function repairSupervisor(): Promise<SupervisorStatus> {
  if (!isTauri()) throw new Error("请在桌面应用中恢复监督器");
  return invoke<SupervisorStatus>("repair_supervisor");
}

export async function startManagedMode(): Promise<ManagedActionReport> {
  if (!isTauri()) throw new Error("请在桌面应用中启动托管分流");
  return invoke<ManagedActionReport>("start_managed_mode");
}

export async function migrateLegacyMode(): Promise<ManagedActionReport> {
  if (!isTauri()) throw new Error("请在桌面应用中执行无中断迁移");
  return invoke<ManagedActionReport>("migrate_legacy_mode");
}

export async function stopManagedMode(): Promise<ManagedActionReport> {
  if (!isTauri()) throw new Error("请在桌面应用中停止托管分流");
  return invoke<ManagedActionReport>("stop_managed_mode");
}

export async function exportMobileBundle(request: MobileExportRequest): Promise<MobileExportReport> {
  if (!isTauri()) throw new Error("请在桌面应用中导出移动端配置");
  return invoke<MobileExportReport>("export_mobile_bundle", { request });
}

export async function configureHomeGateway(request: GatewayDeployRequest): Promise<GatewayDeployReport> {
  if (!isTauri()) throw new Error("请在桌面应用中配置 OpenWrt / OpenClash 家庭网关");
  return invoke<GatewayDeployReport>("configure_home_gateway", { request });
}

export async function inspectHomeGateway(request: GatewayInspectRequest): Promise<GatewayInspectReport> {
  if (!isTauri()) throw new Error("请在桌面应用中发现 OpenWrt 家庭网关设备");
  return invoke<GatewayInspectReport>("inspect_home_gateway", { request });
}

export async function loadGatewayPolicies(): Promise<GatewayDevicePolicy[]> {
  if (!isTauri()) return [];
  return invoke<GatewayDevicePolicy[]>("load_gateway_policies");
}

export async function saveGatewayPolicies(policies: GatewayDevicePolicy[]): Promise<GatewayDevicePolicy[]> {
  if (!isTauri()) throw new Error("请在桌面应用中保存家庭网关设备策略");
  return invoke<GatewayDevicePolicy[]>("save_gateway_policies", { policies });
}

export async function exportDiagnostics(): Promise<DiagnosticExportReport> {
  if (!isTauri()) throw new Error("请在桌面应用中导出诊断包");
  return invoke<DiagnosticExportReport>("export_diagnostics");
}

export async function getLocalBackupStatus(): Promise<LocalBackupStatus | null> {
  if (!isTauri()) return null;
  return invoke<LocalBackupStatus>("get_local_backup_status");
}

export async function createLocalBackup(): Promise<LocalBackupReport> {
  if (!isTauri()) throw new Error("请在桌面应用中创建本机备份");
  return invoke<LocalBackupReport>("create_local_backup");
}

export async function restoreLocalBackup(): Promise<LocalBackupReport> {
  if (!isTauri()) throw new Error("请在桌面应用中恢复本机备份");
  return invoke<LocalBackupReport>("restore_local_backup");
}

export async function getRemoteEnvironment(): Promise<RemoteEnvironmentReport | null> {
  if (!isTauri()) return null;
  return invoke<RemoteEnvironmentReport>("get_remote_environment");
}

export async function getSunshineConfigStatus(): Promise<SunshineConfigStatus | null> {
  if (!isTauri()) return null;
  return invoke<SunshineConfigStatus>("get_sunshine_config_status");
}

export async function configureSunshineStreaming(
  action: "apply" | "rollback",
  bitrateMbps: number,
  encoder: RemoteEnvironmentReport["encoder"],
  confirmed: boolean,
): Promise<SunshineConfigActionReport> {
  if (!isTauri()) throw new Error("请在桌面应用中写入 Sunshine 配置");
  return invoke<SunshineConfigActionReport>("configure_sunshine_streaming", {
    request: { action, bitrateMbps, encoder, confirmed },
  });
}

export async function installRemoteTool(toolId: "sunshine" | "moonlight", confirmed: boolean): Promise<RemoteInstallReport> {
  if (!isTauri()) throw new Error("请在 Windows 桌面应用中安装远控工具");
  return invoke<RemoteInstallReport>("install_remote_tool", { request: { toolId, confirmed } });
}

export async function deployRustDeskServer(request: RustDeskDeployRequest): Promise<RustDeskDeployReport> {
  if (!isTauri()) throw new Error("请在桌面应用中部署 RustDesk Server");
  return invoke<RustDeskDeployReport>("deploy_rustdesk_server", { request });
}

export async function configureLocalStreamingFirewall(
  action: "apply" | "rollback",
  scope: "local-subnet" | "tailnet",
  confirmed: boolean,
): Promise<FirewallActionReport> {
  if (!isTauri()) throw new Error("请在 Windows 桌面应用中配置 Sunshine 防火墙");
  return invoke<FirewallActionReport>("configure_local_streaming_firewall", { request: { action, scope, confirmed } });
}

export async function configureRustDeskFirewall(
  request: Omit<RustDeskDeployRequest, "confirmed"> & { action: "apply" | "rollback"; confirmed: boolean },
): Promise<FirewallActionReport> {
  if (!isTauri()) throw new Error("请在桌面应用中配置 RustDesk VPS 防火墙");
  return invoke<FirewallActionReport>("configure_rustdesk_firewall", { request });
}

export async function probeRemotePath(host: string, port: number, samples = 5): Promise<RemoteQualityReport> {
  if (!isTauri()) throw new Error("请在桌面应用中实测远控路径");
  return invoke<RemoteQualityReport>("probe_remote_path", { request: { host, port, samples } });
}

export async function exportRemoteProfile(report: RemoteQualityReport, encoder: RemoteEnvironmentReport["encoder"]): Promise<DiagnosticExportReport> {
  if (!isTauri()) throw new Error("请在桌面应用中导出远控预设");
  return invoke<DiagnosticExportReport>("export_remote_profile", { request: {
    host: report.host,
    port: report.port,
    preset: report.recommendedPreset,
    resolution: report.resolution,
    fps: report.fps,
    bitrateMbps: report.bitrateMbps,
    codec: report.codec,
    encoder,
  } });
}
