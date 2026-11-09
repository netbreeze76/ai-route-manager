#![cfg_attr(test, allow(dead_code))]

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
#[cfg(feature = "desktop-runtime")]
use fs2::FileExt;
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
use std::net::TcpListener;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "desktop-runtime")]
use std::{
    fs::OpenOptions,
    io::Write,
    process::Stdio,
    sync::{Mutex, atomic::Ordering},
};
#[cfg(feature = "desktop-runtime")]
use tauri::{Manager, ipc::Channel};
#[cfg(feature = "desktop-runtime")]
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

const CONFIG_VERSION: u32 = 1;
const PROFILE_SERVICE: &str = "com.airoute.manager";
const PROFILE_ACCOUNT: &str = "routing-profile-v1";
const BACKUP_ACCOUNT: &str = "local-backup-v1";
const MANAGED_SLOT_OFFSET: &str = "10000";
const MANAGED_FRONTEND_PORT: &str = "17901";
const STABLE_FRONTEND_PORT: u16 = 17901;
#[cfg(target_os = "windows")]
const SUPERVISOR_TASK_NAME: &str = "AI Route Manager Supervisor";
static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);
static SUPERVISOR_SUSPENDED: AtomicBool = AtomicBool::new(false);
static STABLE_FRONTDOOR_STARTED: AtomicBool = AtomicBool::new(false);

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn OpenProcess(
        desired_access: u32,
        inherit_handle: i32,
        process_id: u32,
    ) -> *mut std::ffi::c_void;
    fn GetExitCodeProcess(process: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

fn process_is_running(process_id: u32) -> bool {
    if process_id == 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0_u32;
        let success = GetExitCodeProcess(handle, &mut exit_code) != 0;
        let _ = CloseHandle(handle);
        success && exit_code == STILL_ACTIVE
    }
    #[cfg(not(target_os = "windows"))]
    hidden_command("kill")
        .args(["-0", &process_id.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PortStatus {
    label: &'static str,
    host: &'static str,
    port: u16,
    reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemSnapshot {
    os: &'static str,
    arch: &'static str,
    process_id: u32,
    preview_only: bool,
    ports: Vec<PortStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_true")]
    strict_ai: bool,
    #[serde(default)]
    auto_start: bool,
    #[serde(default = "default_true")]
    auto_check_updates: bool,
    #[serde(default = "default_update_endpoint")]
    update_endpoint: String,
    #[serde(default = "default_true")]
    allow_same_region_fallback: bool,
    #[serde(default = "default_retention")]
    diagnostics_retention_days: u16,
    #[serde(default = "default_scenario")]
    active_scenario: String,
    #[serde(default = "default_true")]
    show_publisher_offers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesEnvelope {
    version: u32,
    preferences: Preferences,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ReferralOffer {
    id: String,
    category: String,
    #[serde(default = "default_referral_placement")]
    placement: String,
    provider: String,
    title: String,
    summary: String,
    badge: String,
    price: String,
    url: String,
    coupon: String,
    disclosure: String,
    enabled: bool,
    #[serde(default = "default_referral_priority")]
    priority: u16,
    #[serde(default)]
    click_count: u64,
    #[serde(default = "default_referral_origin")]
    origin: String,
    #[serde(default)]
    catalog_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferralCatalogEnvelope {
    schema_version: u32,
    payload: String,
    signature: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferralCatalogPayload {
    catalog_version: String,
    generated_at: String,
    offers: Vec<ReferralOffer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublisherReferralCatalogReport {
    catalog_version: String,
    generated_at: String,
    offers: Vec<ReferralOffer>,
    signature_verified: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferralOffersEnvelope {
    version: u32,
    offers: Vec<ReferralOffer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationResult {
    valid: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineReport {
    available: bool,
    executable: Option<String>,
    success: bool,
    status: Option<serde_json::Value>,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SubscriptionInput {
    role: String,
    label: String,
    prefix: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FixedEgressInput {
    name: String,
    server: String,
    port: u16,
    cipher: String,
    password: String,
}

fn legacy_ai_region() -> String {
    "SG".to_owned()
}

fn legacy_ai_region_label() -> String {
    "新加坡".to_owned()
}

fn legacy_preferred_node_keywords() -> Vec<String> {
    vec!["李家坡2-直连".to_owned()]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ConnectionProfile {
    subscriptions: Vec<SubscriptionInput>,
    fixed_egress: Option<FixedEgressInput>,
    #[serde(default = "legacy_ai_region")]
    ai_region: String,
    #[serde(default = "legacy_ai_region_label")]
    ai_region_label: String,
    #[serde(default = "legacy_preferred_node_keywords")]
    preferred_node_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileSourceSummary {
    role: String,
    label: String,
    host: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileSummary {
    configured: bool,
    sources: Vec<ProfileSourceSummary>,
    fixed_egress_configured: bool,
    fixed_egress_host: Option<String>,
    ai_region: String,
    ai_region_label: String,
    preferred_node_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewReport {
    success: bool,
    message: String,
    result: Option<serde_json::Value>,
    cleanup_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedStatus {
    configured: bool,
    running: bool,
    listener: bool,
    active_proxy_port: Option<u16>,
    system_proxy: Option<String>,
    legacy_conflict: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedActionReport {
    success: bool,
    message: String,
    status: ManagedStatus,
    preview: Option<PreviewReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SupervisorStatus {
    active: bool,
    mode: String,
    heartbeat_at_ms: u64,
    consecutive_failures: u32,
    restart_count: u32,
    last_recovery_at_ms: Option<u64>,
    last_error: Option<String>,
    restart_policy: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheckReport {
    available: bool,
    current_version: String,
    version: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
    message: String,
}

#[cfg(feature = "desktop-runtime")]
struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
enum UpdateDownloadEvent {
    Started {
        #[serde(rename = "contentLength")]
        content_length: Option<u64>,
    },
    Progress {
        #[serde(rename = "chunkLength")]
        chunk_length: usize,
    },
    Finished,
}

impl Default for SupervisorStatus {
    fn default() -> Self {
        Self {
            active: false,
            mode: "preview".to_owned(),
            heartbeat_at_ms: 0,
            consecutive_failures: 0,
            restart_count: 0,
            last_recovery_at_ms: None,
            last_error: None,
            restart_policy: platform_restart_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MobileExportRequest {
    confirmed_secret_export: bool,
    include_android: bool,
    include_shadowrocket: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MobileExportFile {
    name: String,
    path: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MobileExportReport {
    success: bool,
    directory: String,
    files: Vec<MobileExportFile>,
    warning: String,
    fixed_egress_host: String,
}

#[derive(Debug, Clone)]
struct MobileBundle {
    android_yaml: String,
    shadowrocket_conf: String,
    subscriptions_txt: String,
    ss_uri: String,
    readme: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayDeployRequest {
    host: String,
    ssh_port: u16,
    user: String,
    identity_file: String,
    action: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayDeployReport {
    success: bool,
    action: String,
    host: String,
    integration: String,
    target_path: String,
    config_sha256: Option<String>,
    core: String,
    service: String,
    rollback_available: bool,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayInspectRequest {
    host: String,
    ssh_port: u16,
    user: String,
    identity_file: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct GatewayDevice {
    id: String,
    name: String,
    mac: String,
    ipv4: Option<String>,
    ipv6: Vec<String>,
    kind: String,
    online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalNetworkReport {
    gateway: String,
    local_ip: String,
    devices: Vec<GatewayDevice>,
}

fn parse_local_network_report(bytes: &[u8]) -> Result<LocalNetworkReport, String> {
    let mut report: LocalNetworkReport =
        serde_json::from_slice(bytes).map_err(|error| format!("本机网络信息格式错误：{error}"))?;
    let private = |value: &str| {
        value
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_private())
    };
    if !private(&report.gateway) || !private(&report.local_ip) {
        return Err("当前默认网络不是受支持的私有 IPv4 局域网".to_owned());
    }
    report
        .devices
        .retain(|device| device.ipv4.as_deref().is_some_and(private));
    report.devices.truncate(256);
    Ok(report)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
async fn get_local_network_devices() -> Result<LocalNetworkReport, String> {
    #[cfg(target_os = "windows")]
    {
        let result = hidden_command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                include_str!("local_network.ps1"),
            ])
            .output()
            .map_err(|error| format!("读取本机邻居表失败：{error}"))?;
        if !result.status.success() {
            return Err("读取默认网关或邻居表失败，请检查本机网络连接".to_owned());
        }
        parse_local_network_report(&result.stdout)
    }
    #[cfg(not(target_os = "windows"))]
    Err("本机邻居发现暂支持 Windows；其他平台可使用下方 OpenWrt SSH 发现".to_owned())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayCapabilities {
    openwrt: bool,
    openclash: bool,
    wireguard: bool,
    openvpn: bool,
    ipv6_default_route: bool,
    upnp: bool,
    architecture: String,
    model: String,
    release: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayInspectReport {
    success: bool,
    host: String,
    hostname: String,
    capabilities: GatewayCapabilities,
    devices: Vec<GatewayDevice>,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct GatewayDevicePolicy {
    id: String,
    name: String,
    ip: String,
    mac: String,
    scenario: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayPolicyEnvelope {
    version: u32,
    policies: Vec<GatewayDevicePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalBackupEnvelope {
    version: u32,
    created_at_ms: u128,
    profile: ConnectionProfile,
    preferences: Preferences,
    offers: Vec<ReferralOffer>,
    #[serde(default)]
    gateway_policies: Vec<GatewayDevicePolicy>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalBackupStatus {
    available: bool,
    created_at_ms: Option<u128>,
    subscription_count: usize,
    offer_count: usize,
    gateway_policy_count: usize,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalBackupReport {
    success: bool,
    status: LocalBackupStatus,
    message: String,
    requires_apply: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticExportReport {
    success: bool,
    directory: String,
    files: Vec<MobileExportFile>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteToolStatus {
    id: String,
    name: String,
    installed: bool,
    running: bool,
    executable: Option<String>,
    official_url: String,
    install_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteEnvironmentReport {
    tools: Vec<RemoteToolStatus>,
    gpu: String,
    encoder: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteProbeRequest {
    host: String,
    port: u16,
    samples: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteQualityReport {
    host: String,
    port: u16,
    samples: u8,
    successful_samples: u8,
    average_latency_ms: f64,
    jitter_ms: f64,
    loss_percent: f64,
    score: u8,
    recommended_preset: String,
    resolution: String,
    fps: u16,
    bitrate_mbps: u16,
    codec: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteProfileExportRequest {
    host: String,
    port: u16,
    preset: String,
    resolution: String,
    fps: u16,
    bitrate_mbps: u16,
    codec: String,
    encoder: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SunshineConfigRequest {
    action: String,
    bitrate_mbps: u16,
    encoder: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SunshineConfigState {
    version: u32,
    phase: String,
    config_path: String,
    backup_path: String,
    before_hash: Option<String>,
    applied_hash: String,
    applied_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SunshineConfigStatus {
    supported: bool,
    config_path: String,
    exists: bool,
    sunshine_running: bool,
    rollback_available: bool,
    applied_hash: Option<String>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SunshineConfigActionReport {
    success: bool,
    action: String,
    config_path: String,
    backup_path: Option<String>,
    before_hash: Option<String>,
    after_hash: Option<String>,
    managed_values: Vec<String>,
    restart_required: bool,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteInstallRequest {
    tool_id: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteInstallReport {
    success: bool,
    tool_id: String,
    package_id: String,
    exit_code: Option<i32>,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RustDeskDeployRequest {
    host: String,
    ssh_port: u16,
    user: String,
    identity_file: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RustDeskDeployReport {
    success: bool,
    host: String,
    id_server: String,
    relay_server: String,
    public_key: String,
    status: String,
    required_ports: Vec<String>,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalFirewallRequest {
    action: String,
    scope: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RustDeskFirewallRequest {
    host: String,
    ssh_port: u16,
    user: String,
    identity_file: String,
    action: String,
    confirmed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FirewallRuleSummary {
    label: String,
    protocol: String,
    ports: String,
    scope: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FirewallActionReport {
    success: bool,
    target: String,
    action: String,
    rules: Vec<FirewallRuleSummary>,
    message: String,
}

#[derive(Debug, Clone)]
struct SshRunResult {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

const RUSTDESK_DEPLOY_SCRIPT: &str = r#"#!/bin/sh
set -eu
command -v docker >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=docker_missing'; exit 20; }
docker compose version >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=compose_missing'; exit 21; }
umask 077
WORKDIR="$HOME/ai-route-rustdesk"
mkdir -p "$WORKDIR/data"
cd "$WORKDIR"
cat > compose.yaml <<'AI_ROUTE_COMPOSE'
services:
  hbbs:
    container_name: ai-route-hbbs
    image: rustdesk/rustdesk-server:1.1.16
    command: hbbs
    volumes:
      - ./data:/root
    network_mode: "host"
    depends_on:
      - hbbr
    restart: unless-stopped
  hbbr:
    container_name: ai-route-hbbr
    image: rustdesk/rustdesk-server:1.1.16
    command: hbbr
    volumes:
      - ./data:/root
    network_mode: "host"
    restart: unless-stopped
# Native clients require 21115/tcp, 21116/tcp+udp and 21117/tcp.
# Web client ports 21118/tcp and 21119/tcp stay firewall-closed unless separately configured.
AI_ROUTE_COMPOSE
docker compose pull >/dev/null
docker compose up -d >/dev/null
test -s data/id_ed25519.pub || { echo 'AI_ROUTE_ERROR=public_key_missing'; exit 22; }
HBBS_STATUS="$(docker inspect -f '{{.State.Status}}' ai-route-hbbs 2>/dev/null || true)"
HBBR_STATUS="$(docker inspect -f '{{.State.Status}}' ai-route-hbbr 2>/dev/null || true)"
test "$HBBS_STATUS" = running && test "$HBBR_STATUS" = running || { echo 'AI_ROUTE_ERROR=containers_not_running'; exit 23; }
printf 'AI_ROUTE_STATUS=running\n'
printf 'AI_ROUTE_PUBLIC_KEY='
tr -d '\r\n' < data/id_ed25519.pub
printf '\n'
"#;

const RUSTDESK_FIREWALL_APPLY_SCRIPT: &str = r#"#!/bin/sh
set -eu
command -v ufw >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=ufw_missing'; exit 30; }
ufw allow 21115/tcp comment 'AI Route RustDesk NAT test' >/dev/null
ufw allow 21116/tcp comment 'AI Route RustDesk signaling' >/dev/null
ufw allow 21116/udp comment 'AI Route RustDesk heartbeat' >/dev/null
ufw allow 21117/tcp comment 'AI Route RustDesk relay' >/dev/null
printf 'AI_ROUTE_FIREWALL=applied\n'
"#;

const RUSTDESK_FIREWALL_ROLLBACK_SCRIPT: &str = r#"#!/bin/sh
set -eu
command -v ufw >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=ufw_missing'; exit 30; }
ufw --force delete allow 21115/tcp >/dev/null 2>&1 || true
ufw --force delete allow 21116/tcp >/dev/null 2>&1 || true
ufw --force delete allow 21116/udp >/dev/null 2>&1 || true
ufw --force delete allow 21117/tcp >/dev/null 2>&1 || true
printf 'AI_ROUTE_FIREWALL=rolled_back\n'
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTaskState {
    name: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyMigrationBackup {
    version: u32,
    phase: String,
    created_at_ms: u128,
    tasks: Vec<LegacyTaskState>,
}

struct PreviewWorkspace {
    path: PathBuf,
    armed: bool,
}

impl PreviewWorkspace {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn cleanup(&mut self) -> bool {
        if !self.armed {
            return true;
        }
        let removed = !self.path.exists() || fs::remove_dir_all(&self.path).is_ok();
        if removed {
            self.armed = false;
        }
        removed
    }
}

impl Drop for PreviewWorkspace {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubscriptionEnvelope {
    subscriptions: Vec<SubscriptionInput>,
}

fn default_true() -> bool {
    true
}

fn default_mode() -> String {
    "preview".to_owned()
}

fn default_retention() -> u16 {
    14
}

fn default_scenario() -> String {
    "日常模式".to_owned()
}

fn default_update_endpoint() -> String {
    "https://139-59-246-27.sslip.io/ai-route-manager/latest.json".to_owned()
}

fn default_referral_placement() -> String {
    "general".to_owned()
}

fn default_referral_priority() -> u16 {
    50
}

fn default_referral_origin() -> String {
    "local".to_owned()
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            strict_ai: true,
            auto_start: false,
            auto_check_updates: true,
            update_endpoint: default_update_endpoint(),
            allow_same_region_fallback: true,
            diagnostics_retention_days: default_retention(),
            active_scenario: default_scenario(),
            show_publisher_offers: true,
        }
    }
}

fn can_connect(host: &'static str, port: u16) -> bool {
    let address = format!("{host}:{port}");
    let Ok(address) = address.parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&address, Duration::from_millis(180)).is_ok()
}

#[cfg(feature = "desktop-runtime")]
fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法确定配置目录：{error}"))?;
    Ok(directory.join("preferences.json"))
}

#[cfg(feature = "desktop-runtime")]
fn referral_offers_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法确定推荐配置目录：{error}"))?;
    Ok(directory.join("referral-offers.json"))
}

#[cfg(feature = "desktop-runtime")]
fn gateway_policies_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法确定家庭网关策略目录：{error}"))?;
    Ok(directory.join("gateway-policies.json"))
}

fn validate_referral_offers_inner(offers: &[ReferralOffer]) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = HashSet::new();
    let allowed_categories = ["vps", "vpn", "hardware", "software"];
    let allowed_origins = ["local", "publisher"];
    let allowed_placements = [
        "vps",
        "stable-vpn",
        "download-vpn",
        "remote-control",
        "general",
    ];

    if offers.len() > 100 {
        errors.push("推荐位不能超过 100 个".to_owned());
    }
    for offer in offers {
        let label = if offer.title.trim().is_empty() {
            &offer.id
        } else {
            &offer.title
        };
        if offer.id.trim().is_empty() || !ids.insert(offer.id.clone()) {
            errors.push(format!("推荐位 ID 为空或重复：{}", offer.id));
        }
        if !allowed_categories.contains(&offer.category.as_str()) {
            errors.push(format!("{label} 的推荐类型无效"));
        }
        if !allowed_placements.contains(&offer.placement.as_str()) {
            errors.push(format!("{label} 的展示位置无效"));
        }
        if !allowed_origins.contains(&offer.origin.as_str()) {
            errors.push(format!("{label} 的来源无效"));
        }
        if offer.origin == "publisher"
            && offer
                .catalog_version
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            errors.push(format!("{label} 缺少签名目录版本"));
        }
        if offer.provider.trim().is_empty() || offer.title.trim().is_empty() {
            errors.push(format!("{label} 缺少服务商或标题"));
        }
        if offer.priority > 100 {
            errors.push(format!("{label} 的优先级必须在 0 到 100 之间"));
        }
        if offer.enabled {
            match Url::parse(&offer.url) {
                Ok(url)
                    if url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none() => {}
                _ => errors.push(format!("{label} 必须使用不含用户名或密码的 HTTPS 推荐链接")),
            }
        } else if offer.url.trim().is_empty() {
            warnings.push(format!("{label} 尚未填写推荐链接"));
        }
        if !offer.disclosure.contains("推广") && !offer.disclosure.contains("返利") {
            errors.push(format!("{label} 必须披露合作推广关系"));
        }
        if offer.provider.len() > 120 || offer.title.len() > 160 || offer.url.len() > 2048 {
            errors.push(format!("{label} 的字段超过长度限制"));
        }
    }
    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn engine_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("AI_ROUTE_MANAGER_ENGINE") {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from(
            r"E:\Tools\DynamicProxyManager\app\DynamicProxyManager-v2.exe",
        ));
        candidates.push(PathBuf::from(
            r"E:\Tools\DynamicProxyManager\app\DynamicProxyManager.exe",
        ));
    }
    candidates
}

fn profile_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(PROFILE_SERVICE, PROFILE_ACCOUNT)
        .map_err(|error| format!("无法打开系统凭据库：{error}"))
}

fn backup_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(PROFILE_SERVICE, BACKUP_ACCOUNT)
        .map_err(|error| format!("无法打开本机备份凭据库：{error}"))
}

fn validate_profile_inner(profile: &ConnectionProfile) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let allowed_roles = ["stable", "download1", "download2"];

    let region = profile.ai_region.trim();
    if region.len() != 2 || !region.bytes().all(|value| value.is_ascii_alphabetic()) {
        errors.push("AI 目标地区必须是两位 ISO 国家/地区代码，例如 SG、US、JP".to_owned());
    }
    if profile.ai_region_label.trim().is_empty()
        || profile.ai_region_label.chars().count() > 40
        || profile.ai_region_label.chars().any(char::is_control)
    {
        errors.push("AI 目标地区名称必须为 1–40 个可见字符".to_owned());
    }
    if profile.preferred_node_keywords.len() > 10
        || profile.preferred_node_keywords.iter().any(|keyword| {
            keyword.trim().is_empty()
                || keyword.chars().count() > 80
                || keyword.chars().any(char::is_control)
        })
    {
        errors.push("优先节点关键词最多 10 个，每个为 1–80 个可见字符".to_owned());
    }

    if profile.subscriptions.is_empty() {
        errors.push("至少需要一个订阅源".to_owned());
    }
    if !profile
        .subscriptions
        .iter()
        .any(|source| source.role == "stable")
    {
        errors.push("必须配置 stable 稳定订阅".to_owned());
    }
    for source in &profile.subscriptions {
        if !allowed_roles.contains(&source.role.as_str()) {
            errors.push(format!("不支持的订阅角色：{}", source.role));
        }
        if source.label.trim().is_empty() {
            errors.push("订阅名称不能为空".to_owned());
        }
        match Url::parse(&source.url) {
            Ok(url) if url.scheme() == "https" && url.host_str().is_some() => {}
            _ => errors.push(format!("{} 必须使用有效的 HTTPS 订阅地址", source.label)),
        }
    }
    if let Some(fixed) = &profile.fixed_egress {
        if fixed.server.trim().is_empty()
            || fixed.password.is_empty()
            || fixed.cipher.trim().is_empty()
        {
            errors.push("固定出口必须填写服务器、加密方式和密码".to_owned());
        }
    } else {
        warnings.push(format!(
            "未配置固定 VPS；AI 只能使用订阅内符合 {} ({}) 的出口",
            profile.ai_region_label.trim(),
            region.to_ascii_uppercase()
        ));
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn store_profile(profile: &ConnectionProfile) -> Result<(), String> {
    let validation = validate_profile_inner(profile);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    let secret =
        serde_json::to_string(profile).map_err(|error| format!("无法序列化连接配置：{error}"))?;
    profile_entry()?
        .set_password(&secret)
        .map_err(|error| format!("无法写入系统凭据库：{error}"))
}

fn load_profile() -> Result<ConnectionProfile, String> {
    let secret = profile_entry()?
        .get_password()
        .map_err(|error| format!("系统凭据库中没有连接配置：{error}"))?;
    serde_json::from_str(&secret).map_err(|error| format!("连接配置格式错误：{error}"))
}

fn summarize_profile(profile: &ConnectionProfile) -> ProfileSummary {
    ProfileSummary {
        configured: true,
        sources: profile
            .subscriptions
            .iter()
            .map(|source| ProfileSourceSummary {
                role: source.role.clone(),
                label: source.label.clone(),
                host: Url::parse(&source.url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .unwrap_or_else(|| "无效地址".to_owned()),
            })
            .collect(),
        fixed_egress_configured: profile.fixed_egress.is_some(),
        fixed_egress_host: profile
            .fixed_egress
            .as_ref()
            .map(|fixed| fixed.server.clone()),
        ai_region: profile.ai_region.trim().to_ascii_uppercase(),
        ai_region_label: profile.ai_region_label.trim().to_owned(),
        preferred_node_keywords: profile.preferred_node_keywords.clone(),
    }
}

fn yaml_quote(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("无法编码移动端配置字段：{error}"))
}

fn shadowsocks_uri(fixed: &FixedEgressInput) -> String {
    let credentials = URL_SAFE_NO_PAD.encode(format!("{}:{}", fixed.cipher, fixed.password));
    let host = if fixed.server.contains(':') && !fixed.server.starts_with('[') {
        format!("[{}]", fixed.server)
    } else {
        fixed.server.clone()
    };
    let tag: String = url::form_urlencoded::byte_serialize(fixed.name.as_bytes()).collect();
    format!("ss://{credentials}@{host}:{}#{tag}", fixed.port)
}

fn shadowrocket_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}

fn mobile_bundle_for_profile(profile: &ConnectionProfile) -> Result<MobileBundle, String> {
    let validation = validate_profile_inner(profile);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    let fixed = profile
        .fixed_egress
        .as_ref()
        .ok_or_else(|| "移动端严格 AI 配置需要固定 VPS 出口".to_owned())?;
    let stable = profile
        .subscriptions
        .iter()
        .find(|source| source.role == "stable")
        .ok_or_else(|| "移动端配置需要 stable 稳定订阅".to_owned())?;
    let downloads: Vec<&SubscriptionInput> = profile
        .subscriptions
        .iter()
        .filter(|source| source.role.starts_with("download"))
        .collect();

    let mut providers = Vec::new();
    for source in &profile.subscriptions {
        providers.push(format!(
            "  {}:\n    type: http\n    url: {}\n    path: ./providers/{}.yaml\n    interval: 3600\n    health-check:\n      enable: true\n      url: https://www.gstatic.com/generate_204\n      interval: 300",
            source.role,
            yaml_quote(&source.url)?,
            source.role
        ));
    }
    let download_uses = if downloads.is_empty() {
        "      - stable".to_owned()
    } else {
        downloads
            .iter()
            .map(|source| format!("      - {}", source.role))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let android_yaml = format!(
        "# AI Route Manager 生成；目标地区：{} ({})；文件含订阅 Token 与 VPS 密码，请勿分享。\nmixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: warning\nipv6: true\nunified-delay: true\ntcp-concurrent: true\ngeo-auto-update: true\ngeo-update-interval: 24\n\nproxies:\n  - name: FIXED-EGRESS\n    type: ss\n    server: {}\n    port: {}\n    cipher: {}\n    password: {}\n    udp: true\n\nproxy-providers:\n{}\n\nproxy-groups:\n  - name: AI-STRICT\n    type: select\n    proxies:\n      - FIXED-EGRESS\n  - name: STABLE-AUTO\n    type: url-test\n    use:\n      - {}\n    proxies:\n      - FIXED-EGRESS\n    url: https://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 80\n  - name: DOWNLOAD-AUTO\n    type: url-test\n    use:\n{}\n    proxies:\n      - FIXED-EGRESS\n    url: https://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 120\n\nrules:\n  - DOMAIN-SUFFIX,openai.com,AI-STRICT\n  - DOMAIN-SUFFIX,chatgpt.com,AI-STRICT\n  - DOMAIN-SUFFIX,oaistatic.com,AI-STRICT\n  - DOMAIN-SUFFIX,oaiusercontent.com,AI-STRICT\n  - DOMAIN-SUFFIX,anthropic.com,AI-STRICT\n  - DOMAIN-SUFFIX,claude.ai,AI-STRICT\n  - DOMAIN,gemini.google.com,AI-STRICT\n  - DOMAIN,generativelanguage.googleapis.com,AI-STRICT\n  - DOMAIN-SUFFIX,ai.google.dev,AI-STRICT\n  - DOMAIN-SUFFIX,perplexity.ai,AI-STRICT\n  - DOMAIN,copilot.microsoft.com,AI-STRICT\n  - DOMAIN-SUFFIX,telegram.org,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,t.me,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,youtube.com,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,googlevideo.com,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,ytimg.com,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,steamcontent.com,DOWNLOAD-AUTO\n  - DOMAIN-SUFFIX,steamserver.net,DOWNLOAD-AUTO\n  - GEOSITE,cn,DIRECT\n  - GEOIP,CN,DIRECT,no-resolve\n  - MATCH,STABLE-AUTO\n",
        profile.ai_region_label.trim(),
        profile.ai_region.trim().to_ascii_uppercase(),
        yaml_quote(&fixed.server)?,
        fixed.port,
        yaml_quote(&fixed.cipher)?,
        yaml_quote(&fixed.password)?,
        providers.join("\n"),
        stable.role,
        download_uses
    );

    let shadowrocket_conf = format!(
        "# AI Route Manager 生成；文件含 VPS 密码，请勿分享。\n[General]\nbypass-system = true\nskip-proxy = 127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, localhost, *.local\ndns-server = system\nipv6 = true\n\n[Proxy]\nFIXED-EGRESS = ss, {}, {}, encrypt-method={}, password={}, udp-relay=true\n\n[Proxy Group]\nAI-STRICT = select, FIXED-EGRESS\nSTABLE = select, FIXED-EGRESS\nDOWNLOAD = select, FIXED-EGRESS\n\n[Rule]\nDOMAIN-SUFFIX,openai.com,AI-STRICT\nDOMAIN-SUFFIX,chatgpt.com,AI-STRICT\nDOMAIN-SUFFIX,oaistatic.com,AI-STRICT\nDOMAIN-SUFFIX,oaiusercontent.com,AI-STRICT\nDOMAIN-SUFFIX,anthropic.com,AI-STRICT\nDOMAIN-SUFFIX,claude.ai,AI-STRICT\nDOMAIN,gemini.google.com,AI-STRICT\nDOMAIN,generativelanguage.googleapis.com,AI-STRICT\nDOMAIN-SUFFIX,ai.google.dev,AI-STRICT\nDOMAIN-SUFFIX,perplexity.ai,AI-STRICT\nDOMAIN,copilot.microsoft.com,AI-STRICT\nDOMAIN-SUFFIX,telegram.org,DOWNLOAD\nDOMAIN-SUFFIX,t.me,DOWNLOAD\nDOMAIN-SUFFIX,youtube.com,DOWNLOAD\nDOMAIN-SUFFIX,googlevideo.com,DOWNLOAD\nDOMAIN-SUFFIX,steamcontent.com,DOWNLOAD\nGEOIP,CN,DIRECT\nFINAL,STABLE\n",
        shadowrocket_quote(&fixed.server),
        fixed.port,
        shadowrocket_quote(&fixed.cipher),
        shadowrocket_quote(&fixed.password)
    );
    let subscriptions_txt = profile
        .subscriptions
        .iter()
        .map(|source| format!("{}\t{}\t{}", source.role, source.label, source.url))
        .collect::<Vec<_>>()
        .join("\n");
    let ss_uri = shadowsocks_uri(fixed);
    let readme = format!(
        "AI Route Manager 移动端导出\r\n\r\n目标地区：{} ({})。\r\n\r\n安全：本目录含订阅 Token 和 VPS 密码。只传给自己的设备；导入成功后删除整个目录并清空聊天/网盘副本。\r\n\r\nAndroid：在 FlClash、Mihomo Party 或兼容客户端导入 AI-Route-Android.yaml。AI 域名严格走固定目标地区出口，国内直连，Telegram/YouTube/Steam CDN 优先走下载订阅。\r\n\r\niPhone/iPad：先用 Shadowrocket 扫描 Fixed-Egress-SS-QR.svg（或导入 Fixed-Egress-SS.txt），再导入 AI-Route-Shadowrocket.conf。Shadowrocket 配置中的下载组为固定出口；如需便宜下载源，可从 Mobile-Subscriptions.txt 逐个添加订阅。\r\n\r\n说明：本次导出只写文件，不会修改电脑系统代理，也不会启动或停止现有分流。\r\n",
        profile.ai_region_label.trim(),
        profile.ai_region.trim().to_ascii_uppercase()
    );

    Ok(MobileBundle {
        android_yaml,
        shadowrocket_conf,
        subscriptions_txt,
        ss_uri,
        readme,
    })
}

fn gateway_policy_target(scenario: &str) -> Result<&'static str, String> {
    match scenario {
        "direct" => Ok("DIRECT"),
        "ai" => Ok("AI-STRICT"),
        "streaming" | "download" => Ok("DOWNLOAD-AUTO"),
        "gaming" => Ok("STABLE-AUTO"),
        _ => Err("设备场景只允许 direct、ai、streaming、gaming 或 download".to_owned()),
    }
}

fn gateway_policy_ip(value: &str) -> Result<(std::net::IpAddr, u8), String> {
    let address = value
        .trim()
        .parse::<std::net::IpAddr>()
        .map_err(|_| "设备策略必须使用单个有效 IP 地址".to_owned())?;
    let local = match address {
        std::net::IpAddr::V4(value) => {
            value.is_private() || value.is_loopback() || value.is_link_local()
        }
        std::net::IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unicast_link_local()
                || value.segments()[0] & 0xfe00 == 0xfc00
        }
    };
    if !local {
        return Err("设备策略只接受局域网 IPv4、链路本地或 ULA IPv6 地址".to_owned());
    }
    let prefix = if address.is_ipv4() { 32 } else { 128 };
    Ok((address, prefix))
}

fn valid_gateway_mac(value: &str) -> bool {
    let compact = value.replace(&[':', '-'][..], "");
    compact.len() == 12
        && compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn validate_gateway_policies(
    policies: &[GatewayDevicePolicy],
) -> Result<Vec<GatewayDevicePolicy>, String> {
    if policies.len() > 64 {
        return Err("家庭网关最多保存 64 条设备策略".to_owned());
    }
    let mut ids = HashSet::new();
    let mut addresses = HashSet::new();
    let mut normalized = Vec::with_capacity(policies.len());
    for policy in policies {
        let id = policy.id.trim();
        let name = policy.name.trim();
        if id.is_empty()
            || id.len() > 80
            || !id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err("设备策略 ID 无效".to_owned());
        }
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err("设备策略包含重复 ID".to_owned());
        }
        if name.is_empty()
            || name.len() > 80
            || name.chars().any(|character| character.is_control())
        {
            return Err("设备名称不能为空、过长或包含控制字符".to_owned());
        }
        let (address, _) = gateway_policy_ip(&policy.ip)?;
        if !addresses.insert(address) {
            return Err("同一设备 IP 不能重复绑定多个场景".to_owned());
        }
        let mac = policy.mac.trim().to_ascii_lowercase();
        if !mac.is_empty() && !valid_gateway_mac(&mac) {
            return Err("设备 MAC 地址格式无效".to_owned());
        }
        gateway_policy_target(policy.scenario.trim())?;
        normalized.push(GatewayDevicePolicy {
            id: id.to_owned(),
            name: name.to_owned(),
            ip: address.to_string(),
            mac,
            scenario: policy.scenario.trim().to_owned(),
            enabled: policy.enabled,
        });
    }
    Ok(normalized)
}

fn gateway_device_rules(policies: &[GatewayDevicePolicy]) -> Result<String, String> {
    let policies = validate_gateway_policies(policies)?;
    let mut rules = Vec::new();
    for policy in policies.into_iter().filter(|policy| policy.enabled) {
        let (address, prefix) = gateway_policy_ip(&policy.ip)?;
        let rule_type = if address.is_ipv4() {
            "SRC-IP-CIDR"
        } else {
            "SRC-IP-CIDR6"
        };
        rules.push(format!(
            "  - {rule_type},{address}/{prefix},{}",
            gateway_policy_target(&policy.scenario)?
        ));
    }
    Ok(rules.join("\n"))
}

fn gateway_yaml_for_profile_and_policies(
    profile: &ConnectionProfile,
    policies: &[GatewayDevicePolicy],
) -> Result<String, String> {
    let bundle = mobile_bundle_for_profile(profile)?;
    let original = "mixed-port: 7890\nallow-lan: false\n";
    if !bundle.android_yaml.contains(original) {
        return Err("移动端模板缺少受管的 LAN 监听字段".to_owned());
    }
    let mut yaml = bundle.android_yaml.replacen(
        original,
        "mixed-port: 7890\nallow-lan: true\nbind-address: \"*\"\nlan-allowed-ips:\n  - 0.0.0.0/0\n  - ::/0\n",
        1,
    );
    yaml = yaml.replacen(
        "tcp-concurrent: true\n",
        "tcp-concurrent: true\n\ndns:\n  enable: true\n  ipv6: true\n  enhanced-mode: fake-ip\n  respect-rules: true\n  nameserver:\n    - 223.5.5.5\n    - 119.29.29.29\n  proxy-server-nameserver:\n    - https://1.1.1.1/dns-query\n  nameserver-policy:\n    'geosite:cn':\n      - 223.5.5.5\n      - 119.29.29.29\n",
        1,
    );
    let rules = gateway_device_rules(policies)?;
    if !rules.is_empty() {
        let marker = "  - MATCH,STABLE-AUTO\n";
        if !yaml.contains(marker) {
            return Err("家庭网关模板缺少最终匹配规则".to_owned());
        }
        yaml = yaml.replacen(marker, &format!("{rules}\n{marker}"), 1);
    }
    Ok(yaml)
}

#[cfg(test)]
fn gateway_yaml_for_profile(profile: &ConnectionProfile) -> Result<String, String> {
    gateway_yaml_for_profile_and_policies(profile, &[])
}

fn qr_svg(payload: &str) -> Result<String, String> {
    let code =
        QrCode::new(payload.as_bytes()).map_err(|error| format!("无法生成二维码：{error}"))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(640, 640)
        .dark_color(svg::Color("#111827"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

#[cfg(unix)]
fn restrict_secret_path(path: &std::path::Path, directory: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if directory { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("无法限制导出权限：{error}"))
}

#[cfg(not(unix))]
fn restrict_secret_path(_path: &std::path::Path, _directory: bool) -> Result<(), String> {
    Ok(())
}

fn write_mobile_export_file(
    directory: &std::path::Path,
    name: &str,
    contents: &str,
    kind: &str,
) -> Result<MobileExportFile, String> {
    let path = directory.join(name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write as _;
    let mut file = options
        .open(&path)
        .map_err(|error| format!("无法创建 {name}：{error}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("无法写入 {name}：{error}"))?;
    file.sync_all()
        .map_err(|error| format!("无法落盘 {name}：{error}"))?;
    restrict_secret_path(&path, false)?;
    Ok(MobileExportFile {
        name: name.to_owned(),
        path: path.to_string_lossy().into_owned(),
        kind: kind.to_owned(),
    })
}

fn load_backup_envelope() -> Result<LocalBackupEnvelope, String> {
    let secret = backup_entry()?
        .get_password()
        .map_err(|error| format!("尚无可用的本机配置备份：{error}"))?;
    let backup: LocalBackupEnvelope =
        serde_json::from_str(&secret).map_err(|error| format!("本机备份格式错误：{error}"))?;
    if backup.version != CONFIG_VERSION {
        return Err(format!("不支持的本机备份版本：{}", backup.version));
    }
    let profile_validation = validate_profile_inner(&backup.profile);
    if !profile_validation.valid {
        return Err(format!(
            "备份连接配置无效：{}",
            profile_validation.errors.join("；")
        ));
    }
    let preferences_validation = validate_preferences_inner(&backup.preferences);
    if !preferences_validation.valid {
        return Err(format!(
            "备份应用设置无效：{}",
            preferences_validation.errors.join("；")
        ));
    }
    let offers_validation = validate_referral_offers_inner(&backup.offers);
    if !offers_validation.valid {
        return Err(format!(
            "备份推荐配置无效：{}",
            offers_validation.errors.join("；")
        ));
    }
    validate_gateway_policies(&backup.gateway_policies)
        .map_err(|error| format!("备份家庭网关策略无效：{error}"))?;
    Ok(backup)
}

fn backup_status_from(envelope: Option<&LocalBackupEnvelope>) -> LocalBackupStatus {
    match envelope {
        Some(backup) => LocalBackupStatus {
            available: true,
            created_at_ms: Some(backup.created_at_ms),
            subscription_count: backup.profile.subscriptions.len(),
            offer_count: backup.offers.len(),
            gateway_policy_count: backup.gateway_policies.len(),
            message: "本机凭据库中存在可恢复备份".to_owned(),
        },
        None => LocalBackupStatus {
            available: false,
            created_at_ms: None,
            subscription_count: 0,
            offer_count: 0,
            gateway_policy_count: 0,
            message: "尚未创建本机配置备份".to_owned(),
        },
    }
}

fn diagnostic_payload(
    profile: Option<&ConnectionProfile>,
    preferences: &Preferences,
    offers: &[ReferralOffer],
    snapshot: &SystemSnapshot,
    managed: &ManagedStatus,
    engine: &EngineReport,
) -> serde_json::Value {
    let system_proxy = managed.system_proxy.as_deref().map(|value| {
        if value.starts_with("127.0.0.1:") || value.starts_with("localhost:") {
            value.to_owned()
        } else {
            "<non-local-endpoint-redacted>".to_owned()
        }
    });
    let roles = profile
        .map(|value| {
            value
                .subscriptions
                .iter()
                .map(|item| item.role.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::json!({
        "schemaVersion": CONFIG_VERSION,
        "generatedAtMs": SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_millis()).unwrap_or_default(),
        "application": { "version": env!("CARGO_PKG_VERSION"), "os": snapshot.os, "arch": snapshot.arch },
        "runtime": {
            "previewOnly": snapshot.preview_only,
            "ports": snapshot.ports,
            "managedRunning": managed.running,
            "managedListener": managed.listener,
            "activeProxyPort": managed.active_proxy_port,
            "systemProxy": system_proxy,
            "legacyConflict": managed.legacy_conflict
        },
        "engine": {
            "available": engine.available,
            "success": engine.success,
            "result": if engine.success { "status query passed" } else if engine.available { "status query failed" } else { "engine not found" }
        },
        "configuration": {
            "profileConfigured": profile.is_some(),
            "subscriptionCount": profile.map(|value| value.subscriptions.len()).unwrap_or_default(),
            "subscriptionRoles": roles,
            "fixedEgressConfigured": profile.and_then(|value| value.fixed_egress.as_ref()).is_some(),
            "referralOfferCount": offers.len(),
            "preferences": preferences
        },
        "privacy": {
            "rawLogsIncluded": false,
            "subscriptionUrlsIncluded": false,
            "passwordsIncluded": false,
            "nodeAddressesIncluded": false,
            "cookiesIncluded": false
        }
    })
}

#[cfg(feature = "desktop-runtime")]
fn restore_optional_file(path: &std::path::Path, bytes: Option<&[u8]>) -> Result<(), String> {
    match bytes {
        Some(contents) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("无法恢复配置目录：{error}"))?;
            }
            fs::write(path, contents).map_err(|error| format!("无法恢复配置文件：{error}"))
        }
        None if path.exists() => {
            fs::remove_file(path).map_err(|error| format!("无法移除新配置文件：{error}"))
        }
        None => Ok(()),
    }
}

#[cfg(feature = "desktop-runtime")]
fn cleanup_old_diagnostics(parent: &std::path::Path, keep: &std::path::Path, retention_days: u16) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let max_age = Duration::from_secs(u64::from(retention_days) * 24 * 60 * 60);
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with("diagnostic-")
        {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
            .map(|age| age > max_age)
            .unwrap_or(false);
        if expired {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn validate_remote_host(host: &str) -> Result<String, String> {
    let value = host.trim();
    if value.is_empty() || value.len() > 253 {
        return Err("远程主机不能为空且不能超过 253 个字符".to_owned());
    }
    if !value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | ':' | '[' | ']')
    }) {
        return Err("远程主机只能填写 IP 地址或主机名".to_owned());
    }
    Ok(value.to_owned())
}

fn remote_package_id(tool_id: &str) -> Result<&'static str, String> {
    match tool_id {
        "sunshine" => Ok("LizardByte.Sunshine"),
        "moonlight" => Ok("MoonlightGameStreamingProject.Moonlight"),
        _ => Err("只允许安装已核验的 Sunshine 或 Moonlight 软件包".to_owned()),
    }
}

fn validate_ssh_user(user: &str) -> Result<String, String> {
    let value = user.trim();
    if value.is_empty()
        || value.len() > 32
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        return Err(
            "SSH 用户名只能由字母、数字、下划线、短横线或点组成，并以字母或数字开头".to_owned(),
        );
    }
    Ok(value.to_owned())
}

fn validate_gateway_deploy_request(
    request: &GatewayDeployRequest,
    identity_is_file: bool,
) -> Result<(String, String, PathBuf), String> {
    if !matches!(request.action.as_str(), "preview" | "apply" | "rollback") {
        return Err("家庭网关操作只允许 preview、apply 或 rollback".to_owned());
    }
    if !request.confirmed {
        return Err("必须明确确认后才能通过 SSH 连接家庭网关".to_owned());
    }
    let host = validate_remote_host(&request.host)?;
    let user = validate_ssh_user(&request.user)?;
    if user != "root" {
        return Err("OpenClash 事务部署只接受 OpenWrt 的 root SSH 用户".to_owned());
    }
    if request.ssh_port == 0 {
        return Err("SSH 端口无效".to_owned());
    }
    let identity = PathBuf::from(request.identity_file.trim());
    if request.identity_file.trim().is_empty() || !identity.is_absolute() || !identity_is_file {
        return Err("SSH 私钥路径必须是一个已存在的绝对文件路径".to_owned());
    }
    Ok((host, user, identity))
}

fn validate_gateway_inspect_request(
    request: &GatewayInspectRequest,
    identity_is_file: bool,
) -> Result<(String, String, PathBuf), String> {
    if !request.confirmed {
        return Err("必须明确确认后才能通过 SSH 读取家庭网关状态".to_owned());
    }
    let host = validate_remote_host(&request.host)?;
    let user = validate_ssh_user(&request.user)?;
    if user != "root" {
        return Err("OpenWrt 能力与设备发现只接受 root SSH 用户".to_owned());
    }
    if request.ssh_port == 0 {
        return Err("SSH 端口无效".to_owned());
    }
    let identity = PathBuf::from(request.identity_file.trim());
    if request.identity_file.trim().is_empty() || !identity.is_absolute() || !identity_is_file {
        return Err("SSH 私钥路径必须是一个已存在的绝对文件路径".to_owned());
    }
    Ok((host, user, identity))
}

const OPENWRT_INSPECT_SCRIPT: &str = r#"#!/bin/sh
set -eu
test -f /etc/openwrt_release || { echo 'AI_ROUTE_ERROR=openwrt_required'; exit 40; }
sanitize() { printf '%s' "$1" | tr -cd 'A-Za-z0-9._ -'; }
release='unknown'
architecture='unknown'
model='unknown'
hostname='OpenWrt'
. /etc/openwrt_release
release="${DISTRIB_RELEASE:-unknown}"
architecture="${DISTRIB_ARCH:-unknown}"
if command -v uci >/dev/null 2>&1; then
  hostname="$(uci -q get system.@system[0].hostname || echo OpenWrt)"
  model="$(uci -q get system.@system[0].description || true)"
fi
[ -n "$model" ] || model="$(cat /tmp/sysinfo/model 2>/dev/null || echo unknown)"
openclash=0
[ -x /etc/init.d/openclash ] && openclash=1
wireguard=0
{ command -v wg >/dev/null 2>&1 || command -v wireguard >/dev/null 2>&1; } && wireguard=1
openvpn=0
command -v openvpn >/dev/null 2>&1 && openvpn=1
ipv6=0
ip -6 route show default 2>/dev/null | grep -q '^default' && ipv6=1
upnp=0
if [ -x /etc/init.d/miniupnpd ]; then
  /etc/init.d/miniupnpd enabled >/dev/null 2>&1 && upnp=1 || true
fi
printf 'AI_ROUTE_GATEWAY=openwrt\n'
printf 'AI_ROUTE_HOSTNAME=%s\n' "$(sanitize "$hostname")"
printf 'AI_ROUTE_RELEASE=%s\n' "$(sanitize "$release")"
printf 'AI_ROUTE_ARCH=%s\n' "$(sanitize "$architecture")"
printf 'AI_ROUTE_MODEL=%s\n' "$(sanitize "$model")"
printf 'AI_ROUTE_OPENCLASH=%s\n' "$openclash"
printf 'AI_ROUTE_WIREGUARD=%s\n' "$wireguard"
printf 'AI_ROUTE_OPENVPN=%s\n' "$openvpn"
printf 'AI_ROUTE_IPV6=%s\n' "$ipv6"
printf 'AI_ROUTE_UPNP=%s\n' "$upnp"
if [ -f /tmp/dhcp.leases ]; then
  while read -r expiry mac ip name rest; do
    [ -n "${mac:-}" ] && [ -n "${ip:-}" ] || continue
    [ "${name:-*}" = '*' ] && name=''
    printf 'AI_ROUTE_DEVICE=%s|%s|%s\n' "$(printf '%s' "$mac" | tr 'A-F' 'a-f')" "$ip" "$(sanitize "$name")"
  done < /tmp/dhcp.leases
fi
ip -6 neigh show 2>/dev/null | while read -r ip dev interface status mac state; do
  [ "${status:-}" = lladdr ] || continue
  [ -n "${mac:-}" ] || continue
  printf 'AI_ROUTE_DEVICE=%s|%s|\n' "$(printf '%s' "$mac" | tr 'A-F' 'a-f')" "$ip"
done
"#;

fn gateway_flag(output: &str, name: &str) -> Result<bool, String> {
    match marker_value(output, name).as_deref() {
        Some("1") => Ok(true),
        Some("0") => Ok(false),
        _ => Err(format!("家庭网关没有返回有效的 {name} 能力标志")),
    }
}

fn valid_gateway_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|character| !character.is_control() && character != '|' && character != '=')
}

fn classify_gateway_device(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("xbox") {
        "xbox".to_owned()
    } else if lower.contains("apple-tv") || lower.contains("appletv") {
        "apple-tv".to_owned()
    } else if lower.contains("iphone") || lower.contains("ipad") {
        "apple-mobile".to_owned()
    } else if lower.contains("android") || lower.contains("pixel") {
        "android".to_owned()
    } else if lower.contains("tv") || lower.contains("bravia") || lower.contains("roku") {
        "smart-tv".to_owned()
    } else if lower.contains("mac") {
        "mac".to_owned()
    } else if lower.contains("windows") || lower.contains("desktop") || lower.contains("laptop") {
        "computer".to_owned()
    } else {
        "device".to_owned()
    }
}

fn parse_gateway_devices(output: &str) -> Vec<GatewayDevice> {
    let mut devices: Vec<GatewayDevice> = Vec::new();
    for value in marker_values(output, "AI_ROUTE_DEVICE") {
        let mut fields = value.splitn(3, '|');
        let mac = fields
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let ip = fields.next().unwrap_or_default().trim();
        let name = fields.next().unwrap_or_default().trim();
        if !valid_gateway_mac(&mac) {
            continue;
        }
        let Ok((address, _)) = gateway_policy_ip(ip) else {
            continue;
        };
        let existing = devices.iter_mut().find(|device| device.mac == mac);
        if let Some(device) = existing {
            match address {
                std::net::IpAddr::V4(value) => device.ipv4 = Some(value.to_string()),
                std::net::IpAddr::V6(value) => {
                    let value = value.to_string();
                    if !device.ipv6.contains(&value) {
                        device.ipv6.push(value);
                    }
                }
            }
            if device.name == device.mac && valid_gateway_text(name) {
                device.name = name.to_owned();
                device.kind = classify_gateway_device(name);
            }
            continue;
        }
        let display_name = if valid_gateway_text(name) {
            name.to_owned()
        } else {
            mac.clone()
        };
        let (ipv4, ipv6) = match address {
            std::net::IpAddr::V4(value) => (Some(value.to_string()), Vec::new()),
            std::net::IpAddr::V6(value) => (None, vec![value.to_string()]),
        };
        devices.push(GatewayDevice {
            id: mac.replace(':', "-"),
            name: display_name.clone(),
            mac,
            ipv4,
            ipv6,
            kind: classify_gateway_device(&display_name),
            online: true,
        });
    }
    devices.sort_by_key(|left| left.name.to_lowercase());
    devices
}

fn inspect_home_gateway_with_runner<F>(
    request: &GatewayInspectRequest,
    identity_is_file: bool,
    runner: F,
) -> Result<GatewayInspectReport, String>
where
    F: FnOnce(&[String], &str) -> Result<SshRunResult, String>,
{
    let (host, user, identity) = validate_gateway_inspect_request(request, identity_is_file)?;
    let args = vec![
        "-p".to_owned(),
        request.ssh_port.to_string(),
        "-i".to_owned(),
        identity.to_string_lossy().into_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=15".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
        format!("{user}@{host}"),
        "sh".to_owned(),
        "-s".to_owned(),
    ];
    let output = runner(&args, OPENWRT_INSPECT_SCRIPT)?;
    if !output.success {
        let reason = marker_value(&output.stdout, "AI_ROUTE_ERROR")
            .or_else(|| marker_value(&output.stderr, "AI_ROUTE_ERROR"))
            .unwrap_or_else(|| "ssh_or_gateway_inspection_failed".to_owned());
        return Err(format!(
            "家庭网关只读发现失败（exit={}，reason={reason}）",
            output
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ));
    }
    if marker_value(&output.stdout, "AI_ROUTE_GATEWAY").as_deref() != Some("openwrt") {
        return Err("远端没有返回可验证的 OpenWrt 标识".to_owned());
    }
    let text = |name: &str| {
        marker_value(&output.stdout, name)
            .filter(|value| valid_gateway_text(value))
            .unwrap_or_else(|| "unknown".to_owned())
    };
    let devices = parse_gateway_devices(&output.stdout);
    Ok(GatewayInspectReport {
        success: true,
        host,
        hostname: text("AI_ROUTE_HOSTNAME"),
        capabilities: GatewayCapabilities {
            openwrt: true,
            openclash: gateway_flag(&output.stdout, "AI_ROUTE_OPENCLASH")?,
            wireguard: gateway_flag(&output.stdout, "AI_ROUTE_WIREGUARD")?,
            openvpn: gateway_flag(&output.stdout, "AI_ROUTE_OPENVPN")?,
            ipv6_default_route: gateway_flag(&output.stdout, "AI_ROUTE_IPV6")?,
            upnp: gateway_flag(&output.stdout, "AI_ROUTE_UPNP")?,
            architecture: text("AI_ROUTE_ARCH"),
            model: text("AI_ROUTE_MODEL"),
            release: text("AI_ROUTE_RELEASE"),
        },
        devices,
        message:
            "已通过只读 SSH 检查 OpenWrt 能力并合并 DHCP/IPv6 邻居设备；未修改路由器或本机代理"
                .to_owned(),
    })
}

const OPENCLASH_GATEWAY_BODY: &str = r#"umask 077
TARGET=/etc/openclash/config/ai-route-manager.yaml
STATE=/etc/ai-route-manager/openclash-state
CANDIDATE="/tmp/ai-route-manager-gateway.$$"
cleanup() { rm -f -- "$CANDIDATE"; }
trap cleanup EXIT HUP INT TERM

test -f /etc/openwrt_release || { echo 'AI_ROUTE_ERROR=openwrt_required'; exit 40; }
command -v uci >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=uci_missing'; exit 41; }
command -v base64 >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=base64_missing'; exit 42; }
command -v sha256sum >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=sha256sum_missing'; exit 43; }
test -x /etc/init.d/openclash || { echo 'AI_ROUTE_ERROR=openclash_service_missing'; exit 44; }

CORE=''
for item in /etc/openclash/core/clash_meta /etc/openclash/core/clash /etc/openclash/clash; do
  if [ -x "$item" ]; then CORE="$item"; break; fi
done
test -n "$CORE" || { echo 'AI_ROUTE_ERROR=openclash_core_missing'; exit 45; }

rollback_flag=0
if [ -f "$STATE/phase" ]; then
  state_phase="$(cat "$STATE/phase")"
  if [ "$state_phase" = applied ] || [ "$state_phase" = prepared ]; then rollback_flag=1; fi
fi

wait_for_core() {
  wait_count=0
  while ! pidof clash >/dev/null 2>&1; do
    wait_count=$((wait_count + 1))
    [ "$wait_count" -lt 15 ] || return 1
    sleep 1
  done
}

restore_previous_uci() {
  previous_path="$(cat "$STATE/previous-path")"
  previous_enable="$(cat "$STATE/previous-enable")"
  if [ -n "$previous_path" ]; then
    uci -q set openclash.config.config_path="$previous_path"
  else
    uci -q delete openclash.config.config_path || true
  fi
  if [ -n "$previous_enable" ]; then
    uci -q set openclash.config.enable="$previous_enable"
  else
    uci -q delete openclash.config.enable || true
  fi
  uci -q commit openclash
}

restore_previous_target() {
  if [ "$(cat "$STATE/target-existed")" = 1 ]; then
    cp -p "$STATE/target.before" "$TARGET"
    expected_before="$(cat "$STATE/before.sha256")"
    actual_before="$(sha256sum "$TARGET" | awk '{print $1}')"
    test "$actual_before" = "$expected_before" || return 1
  else
    rm -f -- "$TARGET"
    test ! -e "$TARGET" || return 1
  fi
}

if [ "$ACTION" = rollback ]; then
  test -f "$STATE/phase" || { echo 'AI_ROUTE_ERROR=rollback_state_missing'; exit 46; }
  phase="$(cat "$STATE/phase")"
  test "$phase" = applied || test "$phase" = prepared || { echo 'AI_ROUTE_ERROR=rollback_state_invalid'; exit 47; }
  expected_applied="$(cat "$STATE/applied.sha256")"
  current_path="$(uci -q get openclash.config.config_path || true)"
  current_enable="$(uci -q get openclash.config.enable || true)"
  previous_path="$(cat "$STATE/previous-path")"
  previous_enable="$(cat "$STATE/previous-enable")"
  current_target_exists=0
  current_hash=''
  if [ -f "$TARGET" ]; then
    current_target_exists=1
    current_hash="$(sha256sum "$TARGET" | awk '{print $1}')"
    cp -p "$TARGET" "$STATE/current.before-rollback"
  fi
  if [ "$phase" = applied ]; then
    test "$current_target_exists" = 1 && test "$current_hash" = "$expected_applied" && test "$current_path" = "$TARGET" && test "$current_enable" = 1 || { echo 'AI_ROUTE_ERROR=rollback_conflict'; exit 48; }
  else
    target_existed="$(cat "$STATE/target-existed")"
    before_hash="$(cat "$STATE/before.sha256")"
    if [ "$target_existed" = 1 ]; then
      test "$current_target_exists" = 1 && { test "$current_hash" = "$expected_applied" || test "$current_hash" = "$before_hash"; } || { echo 'AI_ROUTE_ERROR=prepared_state_target_conflict'; exit 48; }
    else
      { test "$current_target_exists" = 0 || test "$current_hash" = "$expected_applied"; } || { echo 'AI_ROUTE_ERROR=prepared_state_target_conflict'; exit 48; }
    fi
    { test "$current_path" = "$TARGET" || test "$current_path" = "$previous_path"; } || { echo 'AI_ROUTE_ERROR=prepared_state_uci_conflict'; exit 48; }
    { test "$current_enable" = 1 || test "$current_enable" = "$previous_enable"; } || { echo 'AI_ROUTE_ERROR=prepared_state_uci_conflict'; exit 48; }
  fi
  printf '%s\n' "$current_target_exists" > "$STATE/current-target-existed"
  printf '%s\n' "$current_path" > "$STATE/current-path"
  printf '%s\n' "$current_enable" > "$STATE/current-enable"
  restore_previous_target || { echo 'AI_ROUTE_ERROR=rollback_target_restore_failed'; exit 49; }
  restore_previous_uci
  if ! /etc/init.d/openclash restart >/dev/null 2>&1 || { [ "$previous_enable" = 1 ] && ! wait_for_core; }; then
    if [ "$current_target_exists" = 1 ]; then cp -p "$STATE/current.before-rollback" "$TARGET"; else rm -f -- "$TARGET"; fi
    if [ -n "$current_path" ]; then uci -q set openclash.config.config_path="$current_path"; else uci -q delete openclash.config.config_path || true; fi
    if [ -n "$current_enable" ]; then uci -q set openclash.config.enable="$current_enable"; else uci -q delete openclash.config.enable || true; fi
    uci -q commit openclash
    /etc/init.d/openclash restart >/dev/null 2>&1 || true
    echo 'AI_ROUTE_ERROR=rollback_restart_failed_reapplied'
    exit 50
  fi
  rm -f -- "$TARGET".new.*
  rm -rf -- "$STATE"
  printf 'AI_ROUTE_GATEWAY=rolled_back\n'
  printf 'AI_ROUTE_CORE=%s\n' "$(basename "$CORE")"
  printf 'AI_ROUTE_TARGET=%s\n' "$TARGET"
  printf 'AI_ROUTE_SERVICE=openclash\n'
  printf 'AI_ROUTE_ROLLBACK=0\n'
  exit 0
fi

test -n "$CONFIG_B64" && test -n "$EXPECTED_SHA256" || { echo 'AI_ROUTE_ERROR=config_payload_missing'; exit 51; }
printf '%s' "$CONFIG_B64" | base64 -d > "$CANDIDATE" || { echo 'AI_ROUTE_ERROR=config_decode_failed'; exit 52; }
actual_hash="$(sha256sum "$CANDIDATE" | awk '{print $1}')"
test "$actual_hash" = "$EXPECTED_SHA256" || { echo 'AI_ROUTE_ERROR=config_hash_mismatch'; exit 53; }
"$CORE" -t -d /etc/openclash -f "$CANDIDATE" >/dev/null 2>&1 || { echo 'AI_ROUTE_ERROR=config_validation_failed'; exit 54; }

if [ "$ACTION" = preview ]; then
  printf 'AI_ROUTE_GATEWAY=previewed\n'
  printf 'AI_ROUTE_CORE=%s\n' "$(basename "$CORE")"
  printf 'AI_ROUTE_TARGET=%s\n' "$TARGET"
  printf 'AI_ROUTE_SERVICE=openclash\n'
  printf 'AI_ROUTE_CONFIG_SHA256=%s\n' "$actual_hash"
  printf 'AI_ROUTE_ROLLBACK=%s\n' "$rollback_flag"
  exit 0
fi

test "$ACTION" = apply || { echo 'AI_ROUTE_ERROR=action_invalid'; exit 55; }
test "$rollback_flag" = 0 || { echo 'AI_ROUTE_ERROR=rollback_pending'; exit 56; }
test ! -e "$STATE" || { echo 'AI_ROUTE_ERROR=incomplete_state_present'; exit 57; }
mkdir -p /etc/ai-route-manager /etc/openclash/config "$STATE"
chmod 700 /etc/ai-route-manager "$STATE"
uci -q get openclash.config.config_path > "$STATE/previous-path" || : > "$STATE/previous-path"
uci -q get openclash.config.enable > "$STATE/previous-enable" || : > "$STATE/previous-enable"
if [ -f "$TARGET" ]; then
  printf '1\n' > "$STATE/target-existed"
  cp -p "$TARGET" "$STATE/target.before"
  sha256sum "$TARGET" | awk '{print $1}' > "$STATE/before.sha256"
else
  printf '0\n' > "$STATE/target-existed"
  : > "$STATE/before.sha256"
fi
printf '%s\n' "$actual_hash" > "$STATE/applied.sha256"
printf 'prepared\n' > "$STATE/phase"
cp "$CANDIDATE" "$TARGET.new.$$"
chmod 600 "$TARGET.new.$$"
mv -f "$TARGET.new.$$" "$TARGET"
uci -q set openclash.config.config_path="$TARGET"
uci -q set openclash.config.enable=1
uci -q commit openclash
if ! /etc/init.d/openclash restart >/dev/null 2>&1 || ! wait_for_core; then
  restore_previous_target || true
  restore_previous_uci
  /etc/init.d/openclash restart >/dev/null 2>&1 || true
  rm -rf -- "$STATE"
  echo 'AI_ROUTE_ERROR=apply_restart_failed_rolled_back'
  exit 58
fi
committed_hash="$(sha256sum "$TARGET" | awk '{print $1}')"
committed_path="$(uci -q get openclash.config.config_path || true)"
test "$committed_hash" = "$actual_hash" && test "$committed_path" = "$TARGET" || {
  restore_previous_target || true
  restore_previous_uci
  /etc/init.d/openclash restart >/dev/null 2>&1 || true
  rm -rf -- "$STATE"
  echo 'AI_ROUTE_ERROR=apply_verification_failed_rolled_back'
  exit 59
}
printf 'applied\n' > "$STATE/phase"
printf 'AI_ROUTE_GATEWAY=applied\n'
printf 'AI_ROUTE_CORE=%s\n' "$(basename "$CORE")"
printf 'AI_ROUTE_TARGET=%s\n' "$TARGET"
printf 'AI_ROUTE_SERVICE=openclash\n'
printf 'AI_ROUTE_CONFIG_SHA256=%s\n' "$committed_hash"
printf 'AI_ROUTE_ROLLBACK=1\n'
"#;

fn openclash_gateway_script(
    action: &str,
    config: Option<&str>,
) -> Result<(String, Option<String>), String> {
    if !matches!(action, "preview" | "apply" | "rollback") {
        return Err("家庭网关操作只允许 preview、apply 或 rollback".to_owned());
    }
    if action == "rollback" && config.is_some() {
        return Err("家庭网关回滚不得携带配置载荷".to_owned());
    }
    if action != "rollback" && config.is_none() {
        return Err("家庭网关预演或应用缺少配置载荷".to_owned());
    }
    let (encoded, hash) = if let Some(config) = config {
        if config.len() > 1024 * 1024 {
            return Err("家庭网关配置超过 1 MB 限制".to_owned());
        }
        (
            STANDARD.encode(config.as_bytes()),
            Some(sha256_hex(config.as_bytes())),
        )
    } else {
        (String::new(), None)
    };
    let script = format!(
        "#!/bin/sh\nset -eu\nACTION='{action}'\nEXPECTED_SHA256='{}'\nCONFIG_B64='{}'\n{}",
        hash.as_deref().unwrap_or(""),
        encoded,
        OPENCLASH_GATEWAY_BODY
    );
    Ok((script, hash))
}

fn valid_gateway_marker(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn configure_home_gateway_with_runner<F>(
    request: &GatewayDeployRequest,
    identity_is_file: bool,
    profile: Option<&ConnectionProfile>,
    policies: &[GatewayDevicePolicy],
    runner: F,
) -> Result<GatewayDeployReport, String>
where
    F: FnOnce(&[String], &str) -> Result<SshRunResult, String>,
{
    let (host, user, identity) = validate_gateway_deploy_request(request, identity_is_file)?;
    let config = if request.action == "rollback" {
        None
    } else {
        Some(gateway_yaml_for_profile_and_policies(
            profile.ok_or_else(|| "家庭网关预演或应用需要已保存的线路配置".to_owned())?,
            policies,
        )?)
    };
    let (script, expected_hash) = openclash_gateway_script(&request.action, config.as_deref())?;
    let args = vec![
        "-p".to_owned(),
        request.ssh_port.to_string(),
        "-i".to_owned(),
        identity.to_string_lossy().into_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=15".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
        format!("{user}@{host}"),
        "sh".to_owned(),
        "-s".to_owned(),
    ];
    let output = runner(&args, &script)?;
    if !output.success {
        let reason = marker_value(&output.stdout, "AI_ROUTE_ERROR")
            .or_else(|| marker_value(&output.stderr, "AI_ROUTE_ERROR"))
            .unwrap_or_else(|| "ssh_or_gateway_command_failed".to_owned());
        return Err(format!(
            "家庭网关操作失败（exit={}，reason={reason}）；本机代理未改动",
            output
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ));
    }
    let expected_status = match request.action.as_str() {
        "preview" => "previewed",
        "apply" => "applied",
        "rollback" => "rolled_back",
        _ => unreachable!(),
    };
    if marker_value(&output.stdout, "AI_ROUTE_GATEWAY").as_deref() != Some(expected_status) {
        return Err("远端没有返回可验证的家庭网关提交状态".to_owned());
    }
    let target = marker_value(&output.stdout, "AI_ROUTE_TARGET")
        .filter(|value| value == "/etc/openclash/config/ai-route-manager.yaml")
        .ok_or_else(|| "远端返回了非受管的 OpenClash 配置路径".to_owned())?;
    let core = marker_value(&output.stdout, "AI_ROUTE_CORE")
        .filter(|value| valid_gateway_marker(value))
        .ok_or_else(|| "远端没有返回有效的 OpenClash 核心标识".to_owned())?;
    let service = marker_value(&output.stdout, "AI_ROUTE_SERVICE")
        .filter(|value| value == "openclash")
        .ok_or_else(|| "远端没有返回有效的 OpenClash 服务标识".to_owned())?;
    let rollback_available = match marker_value(&output.stdout, "AI_ROUTE_ROLLBACK").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => return Err("远端没有返回有效的回滚状态".to_owned()),
    };
    let remote_hash = marker_value(&output.stdout, "AI_ROUTE_CONFIG_SHA256");
    if let Some(expected) = &expected_hash
        && remote_hash.as_deref() != Some(expected.as_str())
    {
        return Err("远端配置哈希与本机候选不一致".to_owned());
    }
    Ok(GatewayDeployReport {
        success: true,
        action: request.action.clone(),
        host,
        integration: "OpenWrt / OpenClash".to_owned(),
        target_path: target,
        config_sha256: remote_hash,
        core,
        service,
        rollback_available,
        message: match request.action.as_str() {
            "preview" => {
                "OpenClash 候选配置已在路由器临时目录通过核心校验；未写入配置或重启服务".to_owned()
            }
            "apply" => {
                "OpenClash 配置已原子写入并重启；原配置和 UCI 选择已保存为单一回滚点".to_owned()
            }
            "rollback" => "OpenClash 配置、UCI 选择和服务已恢复到应用前状态".to_owned(),
            _ => unreachable!(),
        },
    })
}

fn validate_rustdesk_deploy_request(
    request: &RustDeskDeployRequest,
    identity_is_file: bool,
) -> Result<(String, String, PathBuf), String> {
    if !request.confirmed {
        return Err("必须明确确认后才能连接 VPS 并部署 RustDesk Server".to_owned());
    }
    let host = validate_remote_host(&request.host)?;
    let user = validate_ssh_user(&request.user)?;
    if request.ssh_port == 0 {
        return Err("SSH 端口无效".to_owned());
    }
    let identity = PathBuf::from(request.identity_file.trim());
    if request.identity_file.trim().is_empty() || !identity.is_absolute() || !identity_is_file {
        return Err("SSH 私钥路径必须是一个已存在的绝对文件路径".to_owned());
    }
    Ok((host, user, identity))
}

fn marker_value(output: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn marker_values(output: &str, name: &str) -> Vec<String> {
    let prefix = format!("{name}=");
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn valid_rustdesk_public_key(value: &str) -> bool {
    (20..=512).contains(&value.len())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '_' | '-')
        })
}

fn deploy_rustdesk_with_runner<F>(
    request: &RustDeskDeployRequest,
    identity_is_file: bool,
    runner: F,
) -> Result<RustDeskDeployReport, String>
where
    F: FnOnce(&[String], &str) -> Result<SshRunResult, String>,
{
    let (host, user, identity) = validate_rustdesk_deploy_request(request, identity_is_file)?;
    let args = vec![
        "-p".to_owned(),
        request.ssh_port.to_string(),
        "-i".to_owned(),
        identity.to_string_lossy().into_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=15".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
        format!("{user}@{host}"),
        "sh".to_owned(),
        "-s".to_owned(),
    ];
    let output = runner(&args, RUSTDESK_DEPLOY_SCRIPT)?;
    if !output.success {
        let reason = marker_value(&output.stdout, "AI_ROUTE_ERROR")
            .or_else(|| marker_value(&output.stderr, "AI_ROUTE_ERROR"))
            .unwrap_or_else(|| "ssh_or_remote_command_failed".to_owned());
        return Err(format!(
            "RustDesk Server 部署失败（exit={}，reason={reason}）；未修改本机代理或防火墙",
            output
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ));
    }
    let status = marker_value(&output.stdout, "AI_ROUTE_STATUS")
        .ok_or_else(|| "远端没有返回可验证的容器状态".to_owned())?;
    let public_key = marker_value(&output.stdout, "AI_ROUTE_PUBLIC_KEY")
        .filter(|value| valid_rustdesk_public_key(value))
        .ok_or_else(|| "远端没有返回有效的 RustDesk 公钥".to_owned())?;
    if status != "running" {
        return Err("远端 RustDesk 容器未进入 running 状态".to_owned());
    }
    Ok(RustDeskDeployReport {
        success: true,
        host: host.clone(),
        id_server: format!("{host}:21116"),
        relay_server: format!("{host}:21117"),
        public_key,
        status,
        required_ports: vec![
            "21115/tcp".to_owned(),
            "21116/tcp".to_owned(),
            "21116/udp".to_owned(),
            "21117/tcp".to_owned(),
        ],
        message: "RustDesk Server OSS 已启动；未自动修改 VPS 防火墙，也未改动本机代理".to_owned(),
    })
}

fn sunshine_firewall_rules(scope: &str) -> Result<Vec<FirewallRuleSummary>, String> {
    let scope_label = match scope {
        "local-subnet" => "Windows 专用网络 · LocalSubnet",
        "tailnet" => "Tailscale/Headscale · 100.64.0.0/10",
        _ => return Err("Sunshine 防火墙范围只允许 local-subnet 或 tailnet".to_owned()),
    };
    Ok(vec![
        FirewallRuleSummary {
            label: "Sunshine 控制与配对".to_owned(),
            protocol: "TCP".to_owned(),
            ports: "47984,47989,47990,48010".to_owned(),
            scope: scope_label.to_owned(),
        },
        FirewallRuleSummary {
            label: "Moonlight 视频、控制与音频".to_owned(),
            protocol: "UDP".to_owned(),
            ports: "47998-48000".to_owned(),
            scope: scope_label.to_owned(),
        },
    ])
}

fn rustdesk_firewall_rules() -> Vec<FirewallRuleSummary> {
    vec![
        FirewallRuleSummary {
            label: "RustDesk NAT 类型检测".to_owned(),
            protocol: "TCP".to_owned(),
            ports: "21115".to_owned(),
            scope: "VPS 公网入站".to_owned(),
        },
        FirewallRuleSummary {
            label: "RustDesk 信令与打洞".to_owned(),
            protocol: "TCP+UDP".to_owned(),
            ports: "21116".to_owned(),
            scope: "VPS 公网入站".to_owned(),
        },
        FirewallRuleSummary {
            label: "RustDesk 中继".to_owned(),
            protocol: "TCP".to_owned(),
            ports: "21117".to_owned(),
            scope: "VPS 公网入站".to_owned(),
        },
    ]
}

fn sunshine_firewall_script(action: &str, scope: &str) -> Result<String, String> {
    let remote_address = match scope {
        "local-subnet" => "LocalSubnet",
        "tailnet" => "100.64.0.0/10",
        _ => return Err("Sunshine 防火墙范围只允许 local-subnet 或 tailnet".to_owned()),
    };
    match action {
        "apply" => Ok(format!(
            "$ErrorActionPreference = 'Stop'\r\n\
$group = 'AI Route Manager Sunshine'\r\n\
Get-NetFirewallRule -Group $group -ErrorAction SilentlyContinue | Remove-NetFirewallRule\r\n\
New-NetFirewallRule -DisplayName 'AI Route Sunshine TCP' -Group $group -Direction Inbound -Action Allow -Profile Private -Protocol TCP -LocalPort '47984,47989,47990,48010' -RemoteAddress '{remote_address}' | Out-Null\r\n\
New-NetFirewallRule -DisplayName 'AI Route Sunshine UDP' -Group $group -Direction Inbound -Action Allow -Profile Private -Protocol UDP -LocalPort '47998-48000' -RemoteAddress '{remote_address}' | Out-Null\r\n\
Write-Output 'AI_ROUTE_FIREWALL=applied'\r\n"
        )),
        "rollback" => Ok("$ErrorActionPreference = 'Stop'\r\n\
$group = 'AI Route Manager Sunshine'\r\n\
Get-NetFirewallRule -Group $group -ErrorAction SilentlyContinue | Remove-NetFirewallRule\r\n\
Write-Output 'AI_ROUTE_FIREWALL=rolled_back'\r\n"
            .to_owned()),
        _ => Err("防火墙操作只允许 apply 或 rollback".to_owned()),
    }
}

fn validate_firewall_confirmation(action: &str, confirmed: bool) -> Result<(), String> {
    if !matches!(action, "apply" | "rollback") {
        return Err("防火墙操作只允许 apply 或 rollback".to_owned());
    }
    if !confirmed {
        return Err("必须明确确认后才能修改防火墙".to_owned());
    }
    Ok(())
}

fn configure_rustdesk_firewall_with_runner<F>(
    request: &RustDeskFirewallRequest,
    identity_is_file: bool,
    runner: F,
) -> Result<FirewallActionReport, String>
where
    F: FnOnce(&[String], &str) -> Result<SshRunResult, String>,
{
    validate_firewall_confirmation(&request.action, request.confirmed)?;
    let deploy_request = RustDeskDeployRequest {
        host: request.host.clone(),
        ssh_port: request.ssh_port,
        user: request.user.clone(),
        identity_file: request.identity_file.clone(),
        confirmed: true,
    };
    let (host, user, identity) =
        validate_rustdesk_deploy_request(&deploy_request, identity_is_file)?;
    let args = vec![
        "-p".to_owned(),
        request.ssh_port.to_string(),
        "-i".to_owned(),
        identity.to_string_lossy().into_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=15".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
        format!("{user}@{host}"),
        "sh".to_owned(),
        "-s".to_owned(),
    ];
    let script = if request.action == "apply" {
        RUSTDESK_FIREWALL_APPLY_SCRIPT
    } else {
        RUSTDESK_FIREWALL_ROLLBACK_SCRIPT
    };
    let output = runner(&args, script)?;
    if !output.success {
        let reason = marker_value(&output.stdout, "AI_ROUTE_ERROR")
            .or_else(|| marker_value(&output.stderr, "AI_ROUTE_ERROR"))
            .unwrap_or_else(|| "ssh_or_remote_firewall_failed".to_owned());
        return Err(format!(
            "RustDesk 防火墙操作失败（exit={}，reason={reason}）",
            output
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ));
    }
    let expected = if request.action == "apply" {
        "applied"
    } else {
        "rolled_back"
    };
    if marker_value(&output.stdout, "AI_ROUTE_FIREWALL").as_deref() != Some(expected) {
        return Err("远端没有返回可验证的防火墙提交状态".to_owned());
    }
    Ok(FirewallActionReport {
        success: true,
        target: format!("RustDesk VPS {host}"),
        action: request.action.clone(),
        rules: rustdesk_firewall_rules(),
        message: if request.action == "apply" {
            "RustDesk 最小端口已开放；21118/21119 WebSocket 端口保持关闭".to_owned()
        } else {
            "RustDesk 最小端口规则已回滚；容器和本机代理均未改动".to_owned()
        },
    })
}

fn recommend_remote_quality(
    host: String,
    port: u16,
    samples: u8,
    successful_samples: u8,
    average_latency_ms: f64,
    jitter_ms: f64,
    loss_percent: f64,
) -> RemoteQualityReport {
    let score = if successful_samples == 0 {
        0
    } else {
        (100.0
            - average_latency_ms.min(80.0) * 0.65
            - jitter_ms.min(40.0) * 1.2
            - loss_percent.min(20.0) * 3.0)
            .clamp(0.0, 100.0)
            .round() as u8
    };
    let (preset, resolution, fps, bitrate, codec) = if successful_samples == 0 {
        ("不可达", "1280×720", 30, 5, "H.264")
    } else if average_latency_ms <= 10.0 && jitter_ms <= 2.0 && loss_percent <= 0.5 {
        ("局域网极致", "3840×2160", 60, 70, "HEVC")
    } else if average_latency_ms <= 25.0 && jitter_ms <= 5.0 && loss_percent <= 1.0 {
        ("均衡高清", "2560×1440", 60, 35, "HEVC")
    } else if average_latency_ms <= 50.0 && jitter_ms <= 10.0 && loss_percent <= 2.0 {
        ("远程流畅", "1920×1080", 60, 20, "HEVC")
    } else {
        ("弱网稳定", "1280×720", 60, 10, "H.264")
    };
    RemoteQualityReport {
        host,
        port,
        samples,
        successful_samples,
        average_latency_ms,
        jitter_ms,
        loss_percent,
        score,
        recommended_preset: preset.to_owned(),
        resolution: resolution.to_owned(),
        fps,
        bitrate_mbps: bitrate,
        codec: codec.to_owned(),
        message: if successful_samples == 0 {
            "目标端口不可达；请先确认服务已启动、防火墙放行且地址正确".to_owned()
        } else {
            format!("完成 {samples} 次 TCP 握手实测；建议使用 {preset}")
        },
    }
}

fn probe_remote_path_inner(request: &RemoteProbeRequest) -> Result<RemoteQualityReport, String> {
    let host = validate_remote_host(&request.host)?;
    if request.port == 0 || !(3..=12).contains(&request.samples) {
        return Err("端口必须有效，探测次数必须在 3 到 12 之间".to_owned());
    }
    let addresses = (host.as_str(), request.port)
        .to_socket_addrs()
        .map_err(|error| format!("无法解析远程主机：{error}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("远程主机没有可用地址".to_owned());
    }
    let mut timings = Vec::new();
    for index in 0..request.samples {
        let started = Instant::now();
        let connected = addresses
            .iter()
            .any(|address| TcpStream::connect_timeout(address, Duration::from_millis(900)).is_ok());
        if connected {
            timings.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        if index + 1 < request.samples {
            thread::sleep(Duration::from_millis(80));
        }
    }
    let successful = timings.len() as u8;
    let average = if timings.is_empty() {
        0.0
    } else {
        timings.iter().sum::<f64>() / timings.len() as f64
    };
    let jitter = if timings.len() < 2 {
        0.0
    } else {
        timings
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .sum::<f64>()
            / (timings.len() - 1) as f64
    };
    let loss = f64::from(request.samples - successful) * 100.0 / f64::from(request.samples);
    Ok(recommend_remote_quality(
        host,
        request.port,
        request.samples,
        successful,
        average,
        jitter,
        loss,
    ))
}

const SUNSHINE_CONFIG_STATE_VERSION: u32 = 1;
const SUNSHINE_CONFIG_LIMIT: usize = 1_048_576;
const SUNSHINE_MANAGED_KEYS: [&str; 8] = [
    "capture",
    "encoder",
    "max_bitrate",
    "upnp",
    "nvenc_preset",
    "nvenc_twopass",
    "nvenc_spatial_aq",
    "nvenc_vbv_increase",
];

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn validate_sunshine_config_request(request: &SunshineConfigRequest) -> Result<(), String> {
    if !matches!(request.action.as_str(), "apply" | "rollback") {
        return Err("Sunshine 配置操作只允许 apply 或 rollback".to_owned());
    }
    if !request.confirmed {
        return Err("必须明确确认后才能写入或回滚 Sunshine 配置".to_owned());
    }
    if request.action == "apply" {
        if !(3..=150).contains(&request.bitrate_mbps) {
            return Err("Sunshine 码率必须在 3 到 150 Mbps 之间".to_owned());
        }
        if !matches!(
            request.encoder.as_str(),
            "auto" | "nvenc" | "quicksync" | "amdvce"
        ) {
            return Err("不支持的 Sunshine 编码器".to_owned());
        }
    }
    Ok(())
}

fn sunshine_managed_values(
    request: &SunshineConfigRequest,
    platform: &str,
) -> Result<Vec<(String, String)>, String> {
    validate_sunshine_config_request(request)?;
    if request.action != "apply" {
        return Ok(Vec::new());
    }
    let mut values = vec![
        (
            "max_bitrate".to_owned(),
            (u32::from(request.bitrate_mbps) * 1000).to_string(),
        ),
        ("upnp".to_owned(), "disabled".to_owned()),
    ];
    if platform == "windows" {
        values.push(("capture".to_owned(), "ddx".to_owned()));
    }
    if request.encoder != "auto" {
        values.push(("encoder".to_owned(), request.encoder.clone()));
    }
    if request.encoder == "nvenc" {
        values.extend([
            ("nvenc_preset".to_owned(), "1".to_owned()),
            ("nvenc_twopass".to_owned(), "quarter_res".to_owned()),
            ("nvenc_spatial_aq".to_owned(), "disabled".to_owned()),
            ("nvenc_vbv_increase".to_owned(), "0".to_owned()),
        ]);
    }
    Ok(values)
}

fn sunshine_assignment_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(key.to_ascii_lowercase())
}

fn merge_sunshine_config(
    current: &str,
    request: &SunshineConfigRequest,
    platform: &str,
) -> Result<(String, Vec<String>), String> {
    if current.len() > SUNSHINE_CONFIG_LIMIT {
        return Err("Sunshine 配置超过 1 MB 限制".to_owned());
    }
    let values = sunshine_managed_values(request, platform)?;
    let newline = if current.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut retained = Vec::new();
    for line in current.lines() {
        let trimmed = line.trim();
        if trimmed == "# BEGIN AI ROUTE MANAGER STREAMING PROFILE"
            || trimmed == "# END AI ROUTE MANAGER STREAMING PROFILE"
            || sunshine_assignment_key(line)
                .as_deref()
                .is_some_and(|key| SUNSHINE_MANAGED_KEYS.contains(&key))
        {
            continue;
        }
        retained.push(line.to_owned());
    }
    while retained.last().is_some_and(|line| line.trim().is_empty()) {
        retained.pop();
    }
    if !retained.is_empty() {
        retained.push(String::new());
    }
    retained.push("# BEGIN AI ROUTE MANAGER STREAMING PROFILE".to_owned());
    retained.push("# Generated transactionally; use AI Route Manager to roll back.".to_owned());
    if request.encoder == "auto" {
        retained.push("# encoder = auto (Sunshine automatic detection)".to_owned());
    }
    let mut managed_values = Vec::new();
    for (key, value) in values {
        retained.push(format!("{key} = {value}"));
        managed_values.push(format!("{key}={value}"));
    }
    retained.push("# END AI ROUTE MANAGER STREAMING PROFILE".to_owned());
    Ok((
        format!("{}{}", retained.join(newline), newline),
        managed_values,
    ))
}

fn sunshine_elevated_script() -> &'static str {
    r#"$ErrorActionPreference = 'Stop'
$config = [IO.Path]::GetFullPath($env:AI_ROUTE_SUNSHINE_CONFIG)
$candidate = [IO.Path]::GetFullPath($env:AI_ROUTE_SUNSHINE_CANDIDATE)
$backup = [IO.Path]::GetFullPath($env:AI_ROUTE_SUNSHINE_BACKUP)
$expectedBefore = $env:AI_ROUTE_SUNSHINE_BEFORE
$expectedAfter = $env:AI_ROUTE_SUNSHINE_AFTER
$action = $env:AI_ROUTE_SUNSHINE_ACTION
$current = if (Test-Path -LiteralPath $config -PathType Leaf) { (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash } else { '__MISSING__' }
if ($action -eq 'apply') {
    if ($current -cne $expectedBefore) { exit 41 }
    if ($current -ne '__MISSING__') {
        New-Item -ItemType Directory -Path (Split-Path -Parent $backup) -Force | Out-Null
        Copy-Item -LiteralPath $config -Destination $backup -Force
    }
    New-Item -ItemType Directory -Path (Split-Path -Parent $config) -Force | Out-Null
    $temporary = "$config.ai-route-manager.$PID.tmp"
    Copy-Item -LiteralPath $candidate -Destination $temporary -Force
    if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash -cne $expectedAfter) { Remove-Item -LiteralPath $temporary -Force; exit 42 }
    Move-Item -LiteralPath $temporary -Destination $config -Force
    if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -cne $expectedAfter) { exit 43 }
    exit 0
}
if ($action -eq 'rollback') {
    if ($current -cne $expectedAfter) { exit 44 }
    if ($expectedBefore -eq '__MISSING__') {
        Remove-Item -LiteralPath $config -Force
        if (Test-Path -LiteralPath $config) { exit 45 }
        exit 0
    }
    if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) { exit 46 }
    if ((Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash -cne $expectedBefore) { exit 47 }
    $temporary = "$config.ai-route-manager.$PID.tmp"
    Copy-Item -LiteralPath $backup -Destination $temporary -Force
    Move-Item -LiteralPath $temporary -Destination $config -Force
    if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -cne $expectedBefore) { exit 48 }
    exit 0
}
exit 49
"#
}

#[cfg(feature = "desktop-runtime")]
fn executable_from_candidates(names: &[&str], candidates: &[PathBuf]) -> Option<PathBuf> {
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            for name in names {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(feature = "desktop-runtime")]
fn remote_process_listing() -> String {
    #[cfg(target_os = "windows")]
    let output = hidden_command("tasklist.exe")
        .args(["/FO", "CSV", "/NH"])
        .output();
    #[cfg(not(target_os = "windows"))]
    let output = hidden_command("ps").args(["-A", "-o", "comm="]).output();
    output
        .map(|value| String::from_utf8_lossy(&value.stdout).to_lowercase())
        .unwrap_or_default()
}

#[cfg(feature = "desktop-runtime")]
fn sunshine_config_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let program_files = std::env::var_os("ProgramFiles")
            .ok_or_else(|| "无法确定 Windows Program Files 目录".to_owned())?;
        Ok(PathBuf::from(program_files)
            .join("Sunshine")
            .join("config")
            .join("sunshine.conf"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "无法确定 macOS 用户目录".to_owned())?;
        return Ok(PathBuf::from(home)
            .join(".config")
            .join("sunshine")
            .join("sunshine.conf"));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    Err("Sunshine 自动配置当前只支持 Windows 和 macOS".to_owned())
}

#[cfg(feature = "desktop-runtime")]
fn sunshine_profile_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("sunshine-profile"))
        .map_err(|error| format!("无法确定 Sunshine 配置事务目录：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn sunshine_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(sunshine_profile_root(app)?.join("state.json"))
}

#[cfg(feature = "desktop-runtime")]
fn commit_temporary_file(temporary: &Path, path: &Path, label: &str) -> Result<(), String> {
    let backup = path.with_extension(format!(
        "{}.bak",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("data")
    ));
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| format!("无法清理{label}旧备份：{error}"))?;
    }
    if path.exists() {
        fs::rename(path, &backup).map_err(|error| format!("无法备份现有{label}：{error}"))?;
    }
    if let Err(error) = fs::rename(temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(temporary);
        return Err(format!("无法提交{label}：{error}"));
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("{label}已提交，但旧备份清理失败：{error}"))?;
    }
    Ok(())
}

#[cfg(feature = "desktop-runtime")]
fn write_sunshine_state(app: &tauri::AppHandle, state: &SunshineConfigState) -> Result<(), String> {
    let path = sunshine_state_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Sunshine 状态路径缺少父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建 Sunshine 状态目录：{error}"))?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("无法序列化 Sunshine 状态：{error}"))?;
    fs::write(&temporary, bytes).map_err(|error| format!("无法写入 Sunshine 临时状态：{error}"))?;
    commit_temporary_file(&temporary, &path, "Sunshine 状态")
}

#[cfg(feature = "desktop-runtime")]
fn load_sunshine_state(app: &tauri::AppHandle) -> Result<Option<SunshineConfigState>, String> {
    let path = sunshine_state_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| format!("无法读取 Sunshine 状态：{error}"))?;
    let state: SunshineConfigState = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Sunshine 状态格式错误：{error}"))?;
    if state.version != SUNSHINE_CONFIG_STATE_VERSION {
        return Err("Sunshine 状态版本不受支持".to_owned());
    }
    Ok(Some(state))
}

#[cfg(feature = "desktop-runtime")]
fn validate_sunshine_state_paths(
    app: &tauri::AppHandle,
    state: &SunshineConfigState,
) -> Result<(PathBuf, PathBuf), String> {
    let config = sunshine_config_path()?;
    if Path::new(&state.config_path) != config {
        return Err("Sunshine 回滚状态中的配置路径不可信".to_owned());
    }
    let backup_root = sunshine_profile_root(app)?.join("backups");
    let backup = PathBuf::from(&state.backup_path);
    if !backup.starts_with(&backup_root)
        || backup.file_name().and_then(|name| name.to_str()).is_none()
    {
        return Err("Sunshine 回滚状态中的备份路径不可信".to_owned());
    }
    Ok((config, backup))
}

fn current_file_hash(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        return Err(format!("配置路径不是普通文件：{}", path.display()));
    }
    let bytes = fs::read(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    Ok(Some(sha256_hex(&bytes)))
}

#[cfg(any(test, not(target_os = "windows")))]
fn commit_sunshine_direct(
    action: &str,
    config: &Path,
    candidate: &Path,
    backup: &Path,
    expected_before: Option<&str>,
    expected_after: &str,
) -> Result<(), String> {
    let current = current_file_hash(config)?;
    match action {
        "apply" => {
            if current.as_deref() != expected_before {
                return Err("Sunshine 配置在提交前发生变化，已取消写入".to_owned());
            }
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("无法创建 Sunshine 备份目录：{error}"))?;
            }
            if current.is_some() {
                fs::copy(config, backup)
                    .map_err(|error| format!("无法备份 Sunshine 配置：{error}"))?;
            }
            let candidate_bytes = fs::read(candidate)
                .map_err(|error| format!("无法读取 Sunshine 候选配置：{error}"))?;
            if sha256_hex(&candidate_bytes) != expected_after {
                return Err("Sunshine 候选配置哈希不匹配".to_owned());
            }
            let parent = config
                .parent()
                .ok_or_else(|| "Sunshine 配置路径缺少父目录".to_owned())?;
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建 Sunshine 配置目录：{error}"))?;
            let temporary = parent.join(format!(".ai-route-manager-{}.tmp", std::process::id()));
            fs::write(&temporary, candidate_bytes)
                .map_err(|error| format!("无法写入 Sunshine 临时配置：{error}"))?;
            if config.exists() {
                fs::remove_file(config)
                    .map_err(|error| format!("无法替换 Sunshine 现有配置：{error}"))?;
            }
            if let Err(error) = fs::rename(&temporary, config) {
                if backup.is_file() {
                    let _ = fs::copy(backup, config);
                }
                return Err(format!("无法提交 Sunshine 配置：{error}"));
            }
            if current_file_hash(config)?.as_deref() != Some(expected_after) {
                if backup.is_file() {
                    let _ = fs::copy(backup, config);
                }
                return Err("Sunshine 配置提交后哈希不匹配，已尝试恢复备份".to_owned());
            }
            Ok(())
        }
        "rollback" => {
            if current.as_deref() != Some(expected_after) {
                return Err("Sunshine 配置在应用后已被修改，拒绝覆盖用户新改动".to_owned());
            }
            match expected_before {
                Some(hash) => {
                    if current_file_hash(backup)?.as_deref() != Some(hash) {
                        return Err("Sunshine 回滚备份哈希不匹配".to_owned());
                    }
                    fs::copy(backup, config)
                        .map_err(|error| format!("无法恢复 Sunshine 备份：{error}"))?;
                    if current_file_hash(config)?.as_deref() != Some(hash) {
                        return Err("Sunshine 回滚后哈希不匹配".to_owned());
                    }
                }
                None => {
                    fs::remove_file(config)
                        .map_err(|error| format!("无法移除新建 Sunshine 配置：{error}"))?;
                }
            }
            Ok(())
        }
        _ => Err("Sunshine 配置操作只允许 apply 或 rollback".to_owned()),
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn run_sunshine_elevated(
    action: &str,
    config: &Path,
    candidate: &Path,
    backup: &Path,
    before_hash: Option<&str>,
    after_hash: &str,
) -> Result<(), String> {
    let script_path = std::env::temp_dir().join(format!(
        "ai-route-sunshine-{}-{}.ps1",
        std::process::id(),
        unix_time_ms()
    ));
    fs::write(&script_path, sunshine_elevated_script().as_bytes())
        .map_err(|error| format!("无法写入 Sunshine 提权脚本：{error}"))?;
    let launcher = r#"$script = $env:AI_ROUTE_SUNSHINE_SCRIPT; $args = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('`"' + $script + '`"')); $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $args -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $process.ExitCode"#;
    let output = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", launcher])
        .env("AI_ROUTE_SUNSHINE_SCRIPT", &script_path)
        .env("AI_ROUTE_SUNSHINE_ACTION", action)
        .env("AI_ROUTE_SUNSHINE_CONFIG", config)
        .env("AI_ROUTE_SUNSHINE_CANDIDATE", candidate)
        .env("AI_ROUTE_SUNSHINE_BACKUP", backup)
        .env(
            "AI_ROUTE_SUNSHINE_BEFORE",
            before_hash.unwrap_or("__MISSING__"),
        )
        .env("AI_ROUTE_SUNSHINE_AFTER", after_hash)
        .output();
    let cleanup = fs::remove_file(&script_path);
    let output = output.map_err(|error| format!("无法启动 Sunshine 提权流程：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Sunshine 配置事务未提交（exit={}）；临时脚本{}清理",
            output
                .status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            if cleanup.is_ok() { "已" } else { "未" }
        ));
    }
    cleanup.map_err(|error| format!("Sunshine 配置已提交，但临时脚本清理失败：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn get_remote_environment_inner() -> RemoteEnvironmentReport {
    let processes = remote_process_listing();
    #[cfg(target_os = "windows")]
    let program_files = PathBuf::from(
        std::env::var_os("ProgramFiles").unwrap_or_else(|| "C:\\Program Files".into()),
    );
    #[cfg(target_os = "windows")]
    let local_app = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default());
    #[cfg(target_os = "windows")]
    let definitions = vec![
        (
            "rustdesk",
            "RustDesk",
            vec!["rustdesk.exe"],
            vec![
                program_files.join("RustDesk/rustdesk.exe"),
                local_app.join("RustDesk/rustdesk.exe"),
            ],
            "https://rustdesk.com/",
        ),
        (
            "sunshine",
            "Sunshine",
            vec!["sunshine.exe"],
            vec![program_files.join("Sunshine/sunshine.exe")],
            "https://app.lizardbyte.dev/Sunshine/",
        ),
        (
            "moonlight",
            "Moonlight",
            vec!["Moonlight.exe"],
            vec![program_files.join("Moonlight Game Streaming/Moonlight.exe")],
            "https://moonlight-stream.org/",
        ),
    ];
    #[cfg(target_os = "macos")]
    let definitions = vec![
        (
            "rustdesk",
            "RustDesk",
            vec!["rustdesk"],
            vec![PathBuf::from(
                "/Applications/RustDesk.app/Contents/MacOS/RustDesk",
            )],
            "https://rustdesk.com/",
        ),
        (
            "sunshine",
            "Sunshine",
            vec!["sunshine"],
            vec![PathBuf::from(
                "/Applications/Sunshine.app/Contents/MacOS/sunshine",
            )],
            "https://app.lizardbyte.dev/Sunshine/",
        ),
        (
            "moonlight",
            "Moonlight",
            vec!["Moonlight"],
            vec![PathBuf::from(
                "/Applications/Moonlight.app/Contents/MacOS/Moonlight",
            )],
            "https://moonlight-stream.org/",
        ),
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let definitions = vec![
        (
            "rustdesk",
            "RustDesk",
            vec!["rustdesk"],
            vec![],
            "https://rustdesk.com/",
        ),
        (
            "sunshine",
            "Sunshine",
            vec!["sunshine"],
            vec![],
            "https://app.lizardbyte.dev/Sunshine/",
        ),
        (
            "moonlight",
            "Moonlight",
            vec!["moonlight"],
            vec![],
            "https://moonlight-stream.org/",
        ),
    ];
    let tools = definitions
        .into_iter()
        .map(|(id, name, names, candidates, official_url)| {
            let executable = executable_from_candidates(&names, &candidates);
            RemoteToolStatus {
                id: id.to_owned(),
                name: name.to_owned(),
                installed: executable.is_some(),
                running: processes.contains(id),
                executable: executable.map(|path| path.to_string_lossy().into_owned()),
                official_url: official_url.to_owned(),
                install_supported: cfg!(target_os = "windows")
                    && matches!(id, "sunshine" | "moonlight"),
            }
        })
        .collect::<Vec<_>>();
    let nvidia = executable_from_candidates(
        &[if cfg!(target_os = "windows") {
            "nvidia-smi.exe"
        } else {
            "nvidia-smi"
        }],
        &[],
    );
    let gpu = nvidia
        .as_ref()
        .and_then(|path| {
            hidden_command(path)
                .args(["--query-gpu=name", "--format=csv,noheader"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "未检测到 NVIDIA NVENC；Sunshine 可继续自动选择编码器".to_owned());
    RemoteEnvironmentReport {
        tools,
        encoder: if nvidia.is_some() {
            "nvenc".to_owned()
        } else {
            "auto".to_owned()
        },
        gpu,
        message: "环境检测只读取安装路径、进程和 GPU 能力，不启动远控服务".to_owned(),
    }
}

#[cfg(feature = "desktop-runtime")]
fn bundled_engine_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("无法确定资源目录：{error}"))?;
    let engine_directory = platform_engine_directory_name();
    let candidates = [
        resource_dir.join("resources").join(engine_directory),
        resource_dir.join(engine_directory),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(engine_directory),
    ];
    candidates
        .into_iter()
        .find(|path| engine_binary_path(path).is_file())
        .ok_or_else(|| "安装包内缺少分流引擎".to_owned())
}

fn platform_engine_directory_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "engine-macos"
    } else {
        "engine"
    }
}

#[cfg(feature = "desktop-runtime")]
fn engine_binary_path(engine_dir: &std::path::Path) -> PathBuf {
    engine_dir.join(if cfg!(target_os = "windows") {
        "DynamicProxyEngine.exe"
    } else {
        "DynamicProxyEngine"
    })
}

#[cfg(feature = "desktop-runtime")]
fn unique_preview_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法确定缓存目录：{error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间错误：{error}"))?
        .as_millis();
    Ok(cache.join(format!("engine-preview-{}-{stamp}", std::process::id())))
}

fn engine_profile_bytes(profile: &ConnectionProfile) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&serde_json::json!({
        "subscriptions": profile.subscriptions,
        "fixed_egress": profile.fixed_egress,
        "ai_region": profile.ai_region.trim().to_ascii_uppercase(),
        "ai_region_label": profile.ai_region_label.trim(),
        "preferred_node_keywords": profile.preferred_node_keywords,
    }))
    .map_err(|error| format!("无法序列化引擎配置：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn managed_home(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("managed-runtime"))
        .map_err(|error| format!("无法确定托管运行目录：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn harden_managed_home(path: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("无法创建托管运行目录：{error}"))?;
    #[cfg(target_os = "windows")]
    {
        let user = std::env::var("USERNAME").map_err(|_| "无法识别当前 Windows 用户".to_owned())?;
        let grant = format!("{user}:(OI)(CI)F");
        let output = hidden_command("icacls.exe")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &grant])
            .output()
            .map_err(|error| format!("无法调用 Windows ACL：{error}"))?;
        if !output.status.success() {
            return Err("无法把托管运行目录限制为当前用户".to_owned());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("无法限制托管运行目录权限：{error}"))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn legacy_manager_detected() -> bool {
    PathBuf::from(r"E:\Tools\DynamicProxyManager\data\state.json").is_file()
        && [17898, 17899, 17901]
            .into_iter()
            .any(|port| can_connect("127.0.0.1", port))
}

#[cfg(not(target_os = "windows"))]
fn legacy_manager_detected() -> bool {
    false
}

fn scheduled_task_enabled_from_xml(xml: &str) -> bool {
    !xml.to_ascii_lowercase()
        .contains("<enabled>false</enabled>")
}

#[cfg(target_os = "windows")]
fn decode_windows_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let words = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn scheduled_task_enabled(name: &str) -> Option<bool> {
    let output = hidden_command("schtasks.exe")
        .args(["/Query", "/TN", name, "/XML"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(scheduled_task_enabled_from_xml(&decode_windows_text(
        &output.stdout,
    )))
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn set_scheduled_task_enabled(name: &str, enabled: bool) -> Result<(), String> {
    let action = if enabled { "/Enable" } else { "/Disable" };
    let output = hidden_command("schtasks.exe")
        .args(["/Change", "/TN", name, action])
        .output()
        .map_err(|error| format!("无法修改旧版计划任务 {name}：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("旧版计划任务 {name} 修改失败"))
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn restore_legacy_tasks(backup: &LegacyMigrationBackup) -> Result<(), String> {
    for task in &backup.tasks {
        set_scheduled_task_enabled(&task.name, task.enabled)?;
    }
    if backup
        .tasks
        .iter()
        .any(|task| task.name == "DynamicProxyManager" && task.enabled)
    {
        let output = hidden_command("schtasks.exe")
            .args(["/Run", "/TN", "DynamicProxyManager"])
            .output()
            .map_err(|error| format!("无法重启旧版分流任务：{error}"))?;
        if !output.status.success() {
            return Err("旧版分流任务重启失败".to_owned());
        }
    }
    Ok(())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn stop_legacy_manager_loops() -> Result<(), String> {
    for task in ["DynamicProxyManager", "DynamicProxyManager-Watchdog"] {
        let _ = hidden_command("schtasks.exe")
            .args(["/End", "/TN", task])
            .output();
    }
    for image in ["DynamicProxyManager-v2.exe", "DynamicProxyManager.exe"] {
        let _ = hidden_command("taskkill.exe")
            .args(["/IM", image, "/T", "/F"])
            .output();
    }
    thread::sleep(Duration::from_secs(1));
    if !can_connect("127.0.0.1", 17901) {
        return Err("停止旧版守护后稳定入口 17901 不再监听".to_owned());
    }
    Ok(())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn migration_backup_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    managed_home(app).map(|home| home.join("legacy-migration.json"))
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn load_migration_backup(app: &tauri::AppHandle) -> Result<Option<LegacyMigrationBackup>, String> {
    let path = migration_backup_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }
    serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("无法读取迁移恢复记录：{error}"))?,
    )
    .map(Some)
    .map_err(|error| format!("迁移恢复记录格式错误：{error}"))
}

fn managed_engine_status_value_from_home(home: &Path) -> Result<serde_json::Value, String> {
    let state_path = home.join("state.json");
    let state = if state_path.is_file() {
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(&state_path).map_err(|error| format!("无法读取托管状态：{error}"))?,
        )
        .map_err(|error| format!("托管状态格式错误：{error}"))?
    } else {
        serde_json::json!({})
    };
    let manager_running = state
        .get("manager_pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .is_some_and(process_is_running);
    let active_proxy_port = state
        .get("active_proxy_port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let listener = active_proxy_port.is_some_and(|port| can_connect("127.0.0.1", port));
    let proxy_server = active_proxy_port
        .filter(|_| listener)
        .map(|port| format!("127.0.0.1:{port}"));
    Ok(serde_json::json!({
        "state": state,
        "wininet": { "ProxyServer": proxy_server },
        "listener": listener,
        "manager_running": manager_running,
    }))
}

#[cfg(feature = "desktop-runtime")]
fn managed_engine_status_value(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    managed_engine_status_value_from_home(&managed_home(app)?)
}

fn legacy_conflict_for_state(legacy_detected: bool, migration_phase: Option<&str>) -> bool {
    legacy_detected && migration_phase != Some("managed")
}

#[cfg(feature = "desktop-runtime")]
fn committed_migration_phase(app: &tauri::AppHandle) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        load_migration_backup(app)
            .ok()
            .flatten()
            .map(|backup| backup.phase)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        None
    }
}

#[cfg(feature = "desktop-runtime")]
fn managed_status(app: &tauri::AppHandle) -> ManagedStatus {
    let configured = load_profile().is_ok();
    let migration_phase = committed_migration_phase(app);
    let legacy_conflict =
        legacy_conflict_for_state(legacy_manager_detected(), migration_phase.as_deref());
    match managed_engine_status_value(app) {
        Ok(value) => {
            let running = value
                .get("manager_running")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let listener = value
                .get("listener")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let active_proxy_port = value
                .pointer("/state/active_proxy_port")
                .and_then(serde_json::Value::as_u64)
                .and_then(|port| u16::try_from(port).ok());
            let system_proxy = active_proxy_port.map(|port| {
                let consumer = if can_connect("127.0.0.1", STABLE_FRONTEND_PORT) {
                    STABLE_FRONTEND_PORT
                } else {
                    port
                };
                format!("127.0.0.1:{consumer}")
            });
            ManagedStatus {
                configured,
                running,
                listener,
                active_proxy_port,
                system_proxy,
                legacy_conflict,
                message: if running && listener {
                    "托管分流正在运行".to_owned()
                } else if legacy_conflict {
                    "发现旧版分流守护；应用前需要执行无中断迁移".to_owned()
                } else {
                    "托管分流尚未启动".to_owned()
                },
            }
        }
        Err(error) => ManagedStatus {
            configured,
            running: false,
            listener: false,
            active_proxy_port: None,
            system_proxy: None,
            legacy_conflict,
            message: error,
        },
    }
}

fn supervisor_should_recover(preferences: &Preferences, status: &ManagedStatus) -> bool {
    preferences.mode == "managed"
        && status.configured
        && !status.legacy_conflict
        && !status.listener
}

fn supervisor_backoff_seconds(consecutive_failures: u32) -> u64 {
    match consecutive_failures {
        0 => 0,
        1 => 15,
        2 => 30,
        3 => 60,
        4 => 120,
        _ => 300,
    }
}

#[cfg(feature = "desktop-runtime")]
fn supervisor_status_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("supervisor-status.json"))
        .map_err(|error| format!("无法确定监督器状态目录：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn supervisor_suspension_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("supervisor-suspend"))
        .map_err(|error| format!("无法确定监督器挂起标记：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn set_supervisor_suspended(app: &tauri::AppHandle, suspended: bool) -> Result<(), String> {
    let path = supervisor_suspension_path(app)?;
    if suspended {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建监督器挂起目录：{error}"))?;
        }
        fs::write(path, unix_time_ms().to_string())
            .map_err(|error| format!("无法写入监督器挂起标记：{error}"))?;
        SUPERVISOR_SUSPENDED.store(true, Ordering::SeqCst);
        Ok(())
    } else if path.exists() {
        fs::remove_file(path).map_err(|error| format!("无法清理监督器挂起标记：{error}"))?;
        SUPERVISOR_SUSPENDED.store(false, Ordering::SeqCst);
        Ok(())
    } else {
        SUPERVISOR_SUSPENDED.store(false, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(feature = "desktop-runtime")]
fn supervisor_is_suspended(app: &tauri::AppHandle) -> bool {
    if SUPERVISOR_SUSPENDED.load(Ordering::SeqCst) {
        return true;
    }
    let Ok(path) = supervisor_suspension_path(app) else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    let fresh = path
        .metadata()
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
        .map(|age| age < Duration::from_secs(300))
        .unwrap_or(false);
    if !fresh {
        let _ = fs::remove_file(path);
    }
    fresh
}

#[cfg(feature = "desktop-runtime")]
fn write_supervisor_status(
    app: &tauri::AppHandle,
    status: &SupervisorStatus,
) -> Result<(), String> {
    let path = supervisor_status_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "监督器状态路径没有父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建监督器状态目录：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(status)
            .map_err(|error| format!("无法序列化监督器状态：{error}"))?,
    )
    .map_err(|error| format!("无法写入监督器状态：{error}"))?;
    commit_temporary_file(&temporary, &path, "监督器状态")
}

#[cfg(feature = "desktop-runtime")]
fn read_supervisor_status(app: &tauri::AppHandle) -> SupervisorStatus {
    let Ok(path) = supervisor_status_path(app) else {
        return SupervisorStatus::default();
    };
    let Ok(bytes) = fs::read(path) else {
        return SupervisorStatus::default();
    };
    let Ok(mut status) = serde_json::from_slice::<SupervisorStatus>(&bytes) else {
        return SupervisorStatus::default();
    };
    if unix_time_ms().saturating_sub(status.heartbeat_at_ms) > 30_000 {
        status.active = false;
        status.last_error =
            Some("监督器心跳已超过 30 秒；计划任务将在进程异常退出后重启".to_owned());
    }
    status
}

#[cfg(feature = "desktop-runtime")]
fn start_supervisor(app: tauri::AppHandle) {
    if SUPERVISOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        let mut status = read_supervisor_status(&app);
        status.active = true;
        status.restart_policy = platform_restart_policy();
        status.last_error = None;
        let mut next_attempt = Instant::now();
        loop {
            let preferences = load_preferences(app.clone()).unwrap_or_default();
            status.mode = preferences.mode.clone();
            if supervisor_is_suspended(&app) {
                status.last_error = Some("用户操作进行中，自动恢复暂时挂起".to_owned());
            } else if preferences.mode != "managed" {
                status.consecutive_failures = 0;
                status.last_error = None;
                next_attempt = Instant::now();
            } else {
                let current = managed_status(&app);
                if current.listener {
                    status.consecutive_failures = 0;
                    status.last_error = None;
                    next_attempt = Instant::now();
                } else if !current.configured {
                    status.last_error = Some("托管模式缺少连接配置，未执行自动恢复".to_owned());
                } else if current.legacy_conflict {
                    status.last_error = Some("旧版分流仍在运行，等待用户执行无中断迁移".to_owned());
                } else if supervisor_should_recover(&preferences, &current)
                    && Instant::now() >= next_attempt
                {
                    status.consecutive_failures = status.consecutive_failures.saturating_add(1);
                    status.heartbeat_at_ms = unix_time_ms();
                    let _ = write_supervisor_status(&app, &status);
                    match start_managed_mode_inner(app.clone(), false) {
                        Ok(report) if report.success => {
                            status.restart_count = status.restart_count.saturating_add(1);
                            status.last_recovery_at_ms = Some(unix_time_ms());
                            status.consecutive_failures = 0;
                            status.last_error = None;
                            next_attempt = Instant::now();
                        }
                        Ok(report) => {
                            status.last_error = Some(report.message);
                            next_attempt = Instant::now()
                                + Duration::from_secs(supervisor_backoff_seconds(
                                    status.consecutive_failures,
                                ));
                        }
                        Err(error) => {
                            status.last_error = Some(error);
                            next_attempt = Instant::now()
                                + Duration::from_secs(supervisor_backoff_seconds(
                                    status.consecutive_failures,
                                ));
                        }
                    }
                }
            }
            status.active = true;
            status.heartbeat_at_ms = unix_time_ms();
            let _ = write_supervisor_status(&app, &status);
            thread::sleep(Duration::from_secs(5));
        }
    });
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn get_supervisor_status(app: tauri::AppHandle) -> SupervisorStatus {
    read_supervisor_status(&app)
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn launch_headless_supervisor_process() -> Result<u32, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法确定监督器程序路径：{error}"))?;
    hidden_command(executable)
        .arg("--background")
        .spawn()
        .map(|child| child.id())
        .map_err(|error| format!("无法启动无界面监督器：{error}"))
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn repair_supervisor(app: tauri::AppHandle) -> Result<SupervisorStatus, String> {
    let mut preferences = load_preferences(app.clone())?;
    if !preferences.auto_start {
        preferences.auto_start = true;
        save_preferences(app.clone(), preferences)?;
    } else {
        configure_auto_start(true)?;
    }
    set_supervisor_suspended(&app, false)?;

    #[cfg(target_os = "windows")]
    {
        let _process_id = launch_headless_supervisor_process()?;
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(250));
            let status = read_supervisor_status(&app);
            if status.active && unix_time_ms().saturating_sub(status.heartbeat_at_ms) <= 10_000 {
                return Ok(status);
            }
        }
        Err("监督器已重新注册，但 10 秒内没有恢复心跳".to_owned())
    }

    #[cfg(not(target_os = "windows"))]
    {
        start_supervisor(app.clone());
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(250));
            let status = read_supervisor_status(&app);
            if status.active {
                return Ok(status);
            }
        }
        Err("监督器已启动，但 5 秒内没有恢复心跳".to_owned())
    }
}

fn validate_preferences_inner(preferences: &Preferences) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if !matches!(preferences.mode.as_str(), "preview" | "managed") {
        errors.push("mode 只能是 preview 或 managed".to_owned());
    }
    if !(1..=90).contains(&preferences.diagnostics_retention_days) {
        errors.push("诊断记录保留天数必须在 1 到 90 之间".to_owned());
    }
    if preferences.active_scenario.trim().is_empty() {
        errors.push("当前场景不能为空".to_owned());
    }
    if !preferences.update_endpoint.trim().is_empty()
        && let Err(error) = validate_update_endpoint_inner(&preferences.update_endpoint)
    {
        errors.push(error);
    }
    if preferences.auto_check_updates && preferences.update_endpoint.trim().is_empty() {
        warnings.push("自动检查更新已开启，但尚未配置 HTTPS 更新清单".to_owned());
    }
    if !preferences.strict_ai {
        warnings.push("AI 严格出口已关闭；出口故障时可能发生地区变化".to_owned());
    }
    if preferences.mode == "managed" {
        warnings.push("托管模式只有通过预检后才可接管系统代理".to_owned());
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn validate_update_endpoint_inner(endpoint: &str) -> Result<Url, String> {
    let parsed = Url::parse(endpoint.trim()).map_err(|_| "更新源 URL 格式无效".to_owned())?;
    if parsed.scheme() != "https" {
        return Err("更新源必须使用 HTTPS".to_owned());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("更新源不能包含用户名或密码".to_owned());
    }
    if parsed.fragment().is_some() {
        return Err("更新源不能包含 URL 片段".to_owned());
    }
    if parsed.host_str().is_none() {
        return Err("更新源必须包含主机名".to_owned());
    }
    Ok(parsed)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
async fn check_for_updates(
    app: tauri::AppHandle,
    endpoint: String,
    pending_update: tauri::State<'_, PendingUpdate>,
) -> Result<UpdateCheckReport, String> {
    let endpoint = validate_update_endpoint_inner(&endpoint)?;
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| format!("无法配置更新源：{error}"))?
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("无法初始化签名更新器：{error}"))?
        .check()
        .await
        .map_err(|error| format!("检查更新失败：{error}"))?;

    let report = if let Some(update) = update.as_ref() {
        UpdateCheckReport {
            available: true,
            current_version,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            published_at: update.date.map(|value| value.to_string()),
            message: format!("发现经过签名验证的版本 {}", update.version),
        }
    } else {
        UpdateCheckReport {
            available: false,
            current_version,
            version: None,
            notes: None,
            published_at: None,
            message: "当前已经是最新版本".to_owned(),
        }
    };
    *pending_update
        .0
        .lock()
        .map_err(|_| "更新状态锁已损坏".to_owned())? = update;
    Ok(report)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
async fn install_pending_update(
    app: tauri::AppHandle,
    pending_update: tauri::State<'_, PendingUpdate>,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<(), String> {
    let update = pending_update
        .0
        .lock()
        .map_err(|_| "更新状态锁已损坏".to_owned())?
        .take()
        .ok_or_else(|| "没有等待安装的已验证更新，请先重新检查".to_owned())?;
    let mut started = false;
    let progress_channel = on_event.clone();
    let finished_channel = on_event.clone();
    update
        .download_and_install(
            move |chunk_length, content_length| {
                if !started {
                    let _ = progress_channel.send(UpdateDownloadEvent::Started { content_length });
                    started = true;
                }
                let _ = progress_channel.send(UpdateDownloadEvent::Progress { chunk_length });
            },
            move || {
                let _ = finished_channel.send(UpdateDownloadEvent::Finished);
            },
        )
        .await
        .map_err(|error| format!("签名更新下载或安装失败：{error}"))?;
    app.restart()
}

fn background_start_requested_from<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == "--background")
}

fn managed_repair_requested_from<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == "--repair-managed")
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn windows_app_data_root() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("com.airoute.manager"))
        .ok_or_else(|| "无法确定 Windows AppData 目录".to_owned())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn managed_active_proxy_port(root: &Path) -> Option<u16> {
    let state = fs::read(root.join("managed-runtime").join("state.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())?;
    let port = state
        .get("active_proxy_port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())?;
    if !matches!(port, 27898 | 27899) || !can_connect("127.0.0.1", port) {
        return None;
    }
    Some(port)
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn write_stable_frontdoor_status(
    root: &Path,
    phase: &str,
    active_proxy_port: Option<u16>,
    error: Option<&str>,
) {
    let path = root.join("stable-frontdoor-status.json");
    let temporary = root.join(format!(
        "stable-frontdoor-status.{}.tmp",
        std::process::id()
    ));
    let payload = serde_json::json!({
        "version": 1,
        "owner": "AI Route Manager",
        "owner_pid": std::process::id(),
        "phase": phase,
        "frontend_port": STABLE_FRONTEND_PORT,
        "active_proxy_port": active_proxy_port,
        "updated_at_ms": unix_time_ms(),
        "error": error,
    });
    if fs::create_dir_all(root).is_ok()
        && fs::write(
            &temporary,
            serde_json::to_vec_pretty(&payload).unwrap_or_default(),
        )
        .is_ok()
    {
        let _ = commit_temporary_file(&temporary, &path, "稳定入口状态");
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn windows_compatibility_profile(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("managed-winhttp.json");
    let payload = serde_json::json!({
        "Proxy": format!("127.0.0.1:{STABLE_FRONTEND_PORT}"),
        "ProxyBypass": "<local>;localhost;*.local;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*",
        "AutoconfigUrl": "",
        "AutoDetect": false,
    });
    fs::write(
        &path,
        serde_json::to_vec(&payload)
            .map_err(|error| format!("无法序列化 WinHTTP 兼容配置：{error}"))?,
    )
    .map_err(|error| format!("无法写入 WinHTTP 兼容配置：{error}"))?;
    Ok(path)
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn sync_windows_compatibility(root: &Path) -> Result<(), String> {
    let proxy_url = format!("http://127.0.0.1:{STABLE_FRONTEND_PORT}");
    for name in ["HTTP_PROXY", "HTTPS_PROXY"] {
        let query = hidden_command("reg.exe")
            .args(["query", r"HKCU\Environment", "/v", name])
            .output()
            .map_err(|error| format!("无法读取 {name}：{error}"))?;
        let current = decode_windows_text(&query.stdout);
        if !query.status.success() || !current.contains(&proxy_url) {
            let status = hidden_command("reg.exe")
                .args([
                    "add",
                    r"HKCU\Environment",
                    "/v",
                    name,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &proxy_url,
                    "/f",
                ])
                .status()
                .map_err(|error| format!("无法写入 {name}：{error}"))?;
            if !status.success() {
                return Err(format!("写入 {name} 失败"));
            }
        }
    }

    let show = hidden_command("netsh.exe")
        .args(["winhttp", "show", "advproxy"])
        .output()
        .map_err(|error| format!("无法读取用户 WinHTTP：{error}"))?;
    let current = decode_windows_text(&show.stdout);
    let expected = format!("127.0.0.1:{STABLE_FRONTEND_PORT}");
    if !show.status.success() || !current.contains(&expected) {
        let profile = windows_compatibility_profile(root)?;
        let settings_file = format!("settings-file={}", profile.to_string_lossy());
        let status = hidden_command("netsh.exe")
            .args([
                "winhttp",
                "set",
                "advproxy",
                "setting-scope=user",
                &settings_file,
            ])
            .status()
            .map_err(|error| format!("无法写入用户 WinHTTP：{error}"))?;
        if !status.success() {
            return Err("写入用户 WinHTTP 失败".to_owned());
        }
    }
    Ok(())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn relay_stable_frontdoor(mut client: TcpStream, mut upstream: TcpStream) {
    let Ok(mut client_reader) = client.try_clone() else {
        return;
    };
    let Ok(mut upstream_writer) = upstream.try_clone() else {
        return;
    };
    let upload = thread::spawn(move || {
        let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
    });
    let _ = std::io::copy(&mut upstream, &mut client);
    let _ = upload.join();
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn run_stable_frontdoor(root: PathBuf) {
    loop {
        match TcpListener::bind(("127.0.0.1", STABLE_FRONTEND_PORT)) {
            Ok(listener) => {
                let _ = listener.set_nonblocking(true);
                write_stable_frontdoor_status(
                    &root,
                    "listening",
                    managed_active_proxy_port(&root),
                    None,
                );
                let mut next_compatibility_sync = Instant::now();
                loop {
                    match listener.accept() {
                        Ok((client, _)) => {
                            if client.set_nonblocking(false).is_err() {
                                drop(client);
                                continue;
                            }
                            let Some(port) = managed_active_proxy_port(&root) else {
                                drop(client);
                                continue;
                            };
                            let Ok(upstream) = TcpStream::connect_timeout(
                                &SocketAddr::from(([127, 0, 0, 1], port)),
                                Duration::from_secs(3),
                            ) else {
                                drop(client);
                                continue;
                            };
                            let _ = client.set_nodelay(true);
                            let _ = upstream.set_nodelay(true);
                            thread::spawn(move || relay_stable_frontdoor(client, upstream));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(error) => {
                            write_stable_frontdoor_status(
                                &root,
                                "accept_error",
                                managed_active_proxy_port(&root),
                                Some(&error.to_string()),
                            );
                            break;
                        }
                    }
                    if Instant::now() >= next_compatibility_sync {
                        let result = sync_windows_compatibility(&root);
                        write_stable_frontdoor_status(
                            &root,
                            if result.is_ok() {
                                "listening"
                            } else {
                                "compatibility_error"
                            },
                            managed_active_proxy_port(&root),
                            result.as_ref().err().map(String::as_str),
                        );
                        next_compatibility_sync = Instant::now() + Duration::from_secs(30);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
            Err(error) => {
                if !can_connect("127.0.0.1", STABLE_FRONTEND_PORT) {
                    write_stable_frontdoor_status(
                        &root,
                        "bind_wait",
                        managed_active_proxy_port(&root),
                        Some(&error.to_string()),
                    );
                }
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn start_stable_frontdoor(root: PathBuf) {
    if STABLE_FRONTDOOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || run_stable_frontdoor(root));
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn read_preferences_file(root: &Path) -> Result<Preferences, String> {
    let path = root.join("preferences.json");
    if !path.is_file() {
        return Ok(Preferences::default());
    }
    let mut envelope: PreferencesEnvelope = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("无法读取后台配置：{error}"))?,
    )
    .map_err(|error| format!("后台配置格式错误：{error}"))?;
    if envelope.version != CONFIG_VERSION {
        return Err(format!("不支持的后台配置版本：{}", envelope.version));
    }
    if envelope.preferences.update_endpoint.trim().is_empty() {
        envelope.preferences.update_endpoint = default_update_endpoint();
    }
    Ok(envelope.preferences)
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn write_supervisor_status_file(root: &Path, status: &SupervisorStatus) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| format!("无法创建监督器目录：{error}"))?;
    let path = root.join("supervisor-status.json");
    let temporary = root.join(format!("supervisor-status.{}.tmp", std::process::id()));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(status)
            .map_err(|error| format!("无法序列化监督器状态：{error}"))?,
    )
    .map_err(|error| format!("无法写入监督器状态：{error}"))?;
    commit_temporary_file(&temporary, &path, "监督器状态")
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn headless_supervisor_suspended(root: &Path) -> bool {
    let path = root.join("supervisor-suspend");
    if !path.exists() {
        return false;
    }
    let fresh = path
        .metadata()
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
        .map(|age| age < Duration::from_secs(300))
        .unwrap_or(false);
    if !fresh {
        let _ = fs::remove_file(path);
    }
    fresh
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn record_headless_exit(root: &Path, reason: &str) {
    let _ = fs::write(
        root.join(format!("background-exit-{}.txt", std::process::id())),
        reason,
    );
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn installed_engine_dir() -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法确定后台监督器路径：{error}"))?;
    let parent = executable
        .parent()
        .ok_or_else(|| "后台监督器路径没有父目录".to_owned())?;
    let candidates = [
        parent.join("resources").join("engine"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("engine"),
    ];
    candidates
        .into_iter()
        .find(|path| engine_binary_path(path).is_file())
        .ok_or_else(|| "后台监督器找不到分流引擎".to_owned())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn headless_managed_status(root: &Path) -> ManagedStatus {
    let home = root.join("managed-runtime");
    let configured = load_profile().is_ok();
    let migration_phase = fs::read(home.join("legacy-migration.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LegacyMigrationBackup>(&bytes).ok())
        .map(|backup| backup.phase);
    let legacy_conflict =
        legacy_conflict_for_state(legacy_manager_detected(), migration_phase.as_deref());
    match managed_engine_status_value_from_home(&home) {
        Ok(value) => {
            let running = value
                .get("manager_running")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let listener = value
                .get("listener")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let active_proxy_port = value
                .pointer("/state/active_proxy_port")
                .and_then(serde_json::Value::as_u64)
                .and_then(|port| u16::try_from(port).ok());
            ManagedStatus {
                configured,
                running,
                listener,
                active_proxy_port,
                system_proxy: active_proxy_port.map(|port| {
                    let consumer = if can_connect("127.0.0.1", STABLE_FRONTEND_PORT) {
                        STABLE_FRONTEND_PORT
                    } else {
                        port
                    };
                    format!("127.0.0.1:{consumer}")
                }),
                legacy_conflict,
                message: if running && listener {
                    "托管分流正在运行".to_owned()
                } else if legacy_conflict {
                    "发现旧版分流守护；等待用户迁移".to_owned()
                } else {
                    "托管分流尚未启动".to_owned()
                },
            }
        }
        Err(error) => ManagedStatus {
            configured,
            running: false,
            listener: false,
            active_proxy_port: None,
            system_proxy: None,
            legacy_conflict,
            message: error,
        },
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn start_headless_managed_engine(root: &Path) -> Result<u32, String> {
    let profile = load_profile()?;
    let engine_dir = installed_engine_dir()?;
    let managed_runtime = root.join("managed-runtime");
    harden_managed_home(&managed_runtime)?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(managed_runtime.join("launcher.log"))
        .map_err(|error| format!("无法打开后台托管日志：{error}"))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("无法复制后台托管日志：{error}"))?;
    let mut child = hidden_command(engine_binary_path(&engine_dir))
        .current_dir(&engine_dir)
        .env("DPM_LOG_STDERR", "1")
        .env("DPM_SLOT_OFFSET", MANAGED_SLOT_OFFSET)
        .env("DPM_FRONTEND_PORT", MANAGED_FRONTEND_PORT)
        .arg("run")
        .arg("--home")
        .arg(&managed_runtime)
        .arg("--interval")
        .arg("1800")
        .arg("--profile-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log))
        .spawn()
        .map_err(|error| format!("无法启动无界面托管引擎：{error}"))?;
    let payload = engine_profile_bytes(&profile)?;
    child
        .stdin
        .take()
        .ok_or_else(|| "无法打开无界面引擎输入".to_owned())?
        .write_all(&payload)
        .map_err(|error| format!("无法传入无界面引擎配置：{error}"))?;
    Ok(child.id())
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
struct HeadlessInstanceGuard {
    path: PathBuf,
    file: Option<fs::File>,
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
impl HeadlessInstanceGuard {
    fn acquire(root: &Path) -> Result<Option<Self>, String> {
        fs::create_dir_all(root).map_err(|error| format!("无法创建监督器目录：{error}"))?;
        let path = root.join("headless-supervisor.lock");
        let process_id = std::process::id();
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
        {
            Ok(mut file) => {
                match file.try_lock_exclusive() {
                    Ok(()) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.raw_os_error() == Some(33) =>
                    {
                        return Ok(None);
                    }
                    Err(error) => return Err(format!("无法锁定监督器实例文件：{error}")),
                }
                file.set_len(0)
                    .map_err(|error| format!("无法清空监督器锁：{error}"))?;
                if let Err(error) = file.write_all(process_id.to_string().as_bytes()) {
                    return Err(format!("无法写入监督器锁：{error}"));
                }
                Ok(Some(Self {
                    path,
                    file: Some(file),
                }))
            }
            Err(error) => Err(format!("无法创建监督器锁：{error}")),
        }
    }

    fn is_held(&self) -> bool {
        self.file.is_some()
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
impl Drop for HeadlessInstanceGuard {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
            drop(file);
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
fn run_headless_supervisor() -> Result<(), String> {
    let root = windows_app_data_root()?;
    let Some(guard) = HeadlessInstanceGuard::acquire(&root)? else {
        record_headless_exit(&root, "duplicate_instance");
        return Ok(());
    };
    let mut status = SupervisorStatus {
        active: true,
        restart_policy: platform_restart_policy(),
        ..SupervisorStatus::default()
    };
    start_stable_frontdoor(root.clone());
    let mut next_attempt = Instant::now();
    loop {
        if !guard.is_held() {
            return Err("无界面监督器实例锁意外丢失".to_owned());
        }
        let preferences = read_preferences_file(&root)?;
        if !preferences.auto_start {
            record_headless_exit(&root, "auto_start_disabled");
            return Ok(());
        }
        status.mode = preferences.mode.clone();
        if headless_supervisor_suspended(&root) {
            status.last_error = Some("用户操作进行中，无界面监督暂时挂起".to_owned());
        } else if preferences.mode != "managed" {
            status.consecutive_failures = 0;
            status.last_error = None;
            next_attempt = Instant::now();
        } else {
            let current = headless_managed_status(&root);
            if current.listener {
                if status.consecutive_failures > 0 {
                    status.restart_count = status.restart_count.saturating_add(1);
                    status.last_recovery_at_ms = Some(unix_time_ms());
                }
                status.consecutive_failures = 0;
                status.last_error = None;
                next_attempt = Instant::now();
            } else if !current.configured {
                status.last_error = Some("托管模式缺少连接配置，未执行无界面恢复".to_owned());
            } else if current.legacy_conflict {
                status.last_error = Some("旧版分流仍在运行，等待用户执行无中断迁移".to_owned());
            } else if Instant::now() >= next_attempt {
                status.consecutive_failures = status.consecutive_failures.saturating_add(1);
                match start_headless_managed_engine(&root) {
                    Ok(_) => {
                        status.last_error = Some("恢复进程已启动，等待入口实测通过".to_owned())
                    }
                    Err(error) => status.last_error = Some(error),
                }
                next_attempt = Instant::now()
                    + Duration::from_secs(supervisor_backoff_seconds(status.consecutive_failures));
            }
        }
        status.active = true;
        status.heartbeat_at_ms = unix_time_ms();
        write_supervisor_status_file(&root, &status)?;
        thread::sleep(Duration::from_secs(5));
    }
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
pub fn run_background_if_requested() -> bool {
    if !background_start_requested_from(std::env::args()) {
        return false;
    }
    if let Err(error) = run_headless_supervisor()
        && let Ok(root) = windows_app_data_root()
    {
        let _ = fs::create_dir_all(&root);
        let _ = fs::write(root.join("background-start-error.txt"), error);
    }
    true
}

#[cfg(all(feature = "desktop-runtime", target_os = "windows"))]
pub fn run_managed_repair_if_requested() -> bool {
    if !managed_repair_requested_from(std::env::args()) {
        return false;
    }
    let result = (|| -> Result<serde_json::Value, String> {
        let root = windows_app_data_root()?;
        let before = headless_managed_status(&root);
        if before.running && before.listener {
            return Ok(serde_json::json!({
                "success": true,
                "message": "托管管理循环和入口已经运行",
                "activeProxyPort": before.active_proxy_port,
            }));
        }
        let process_id = start_headless_managed_engine(&root)?;
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(500));
            let current = headless_managed_status(&root);
            if current.running && current.listener {
                return Ok(serde_json::json!({
                    "success": true,
                    "message": "托管管理循环已恢复并通过入口实测",
                    "managerPid": process_id,
                    "activeProxyPort": current.active_proxy_port,
                }));
            }
            if !process_is_running(process_id) {
                return Err("托管修复进程在入口恢复前退出".to_owned());
            }
        }
        Err("托管修复进程已启动，但 180 秒内没有恢复入口".to_owned())
    })();
    if let Ok(root) = windows_app_data_root() {
        let payload = match result {
            Ok(report) => report,
            Err(error) => serde_json::json!({ "success": false, "message": error }),
        };
        let _ = fs::write(
            root.join("managed-repair-result.json"),
            serde_json::to_vec_pretty(&payload).unwrap_or_default(),
        );
    }
    true
}

#[cfg(not(all(feature = "desktop-runtime", target_os = "windows")))]
pub fn run_managed_repair_if_requested() -> bool {
    false
}

#[cfg(not(all(feature = "desktop-runtime", target_os = "windows")))]
pub fn run_background_if_requested() -> bool {
    false
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn platform_restart_policy() -> String {
    if cfg!(target_os = "windows") {
        "Windows 用户级监督：计划任务优先，受限环境回退隐藏看守".to_owned()
    } else if cfg!(target_os = "macos") {
        "macOS LaunchAgent：登录启动，异常退出后自动重启".to_owned()
    } else {
        "应用内监督器：故障后退避重试".to_owned()
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "windows", test))]
fn windows_supervisor_task_xml(executable: &std::path::Path, user_sid: &str) -> String {
    let command = xml_escape(&executable.to_string_lossy());
    let working_directory = executable
        .parent()
        .map(|path| xml_escape(&path.to_string_lossy()))
        .unwrap_or_default();
    let sid = xml_escape(user_sid);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  <RegistrationInfo><Description>AI Route Manager headless crash recovery supervisor</Description></RegistrationInfo>\n  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>\n  <Principals><Principal id=\"Author\"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n  <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>true</AllowHardTerminate><StartWhenAvailable>true</StartWhenAvailable><RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable><AllowStartOnDemand>true</AllowStartOnDemand><Enabled>true</Enabled><Hidden>true</Hidden><RunOnlyIfIdle>false</RunOnlyIfIdle><WakeToRun>false</WakeToRun><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>7</Priority><RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure></Settings>\n  <Actions Context=\"Author\"><Exec><Command>{command}</Command><Arguments>--background</Arguments><WorkingDirectory>{working_directory}</WorkingDirectory></Exec></Actions>\n</Task>\n"
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_task_xml_bytes(xml: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[cfg(any(target_os = "windows", test))]
fn vbs_escape(value: &str) -> String {
    value.replace('"', "\"\"")
}

#[cfg(any(target_os = "windows", test))]
fn windows_watchdog_vbs(executable: &std::path::Path, marker: &std::path::Path) -> String {
    let executable = vbs_escape(&executable.to_string_lossy());
    let marker = vbs_escape(&marker.to_string_lossy());
    format!(
        "Option Explicit\r\nDim shell, fso, marker, command, target, service, processes, process, running\r\nSet shell = CreateObject(\"WScript.Shell\")\r\nSet fso = CreateObject(\"Scripting.FileSystemObject\")\r\nSet service = GetObject(\"winmgmts:\\\\.\\root\\cimv2\")\r\nmarker = \"{marker}\"\r\ntarget = LCase(\"{executable}\")\r\ncommand = \"\"\"{executable}\"\" --background\"\r\nDo While fso.FileExists(marker)\r\n  running = False\r\n  Set processes = service.ExecQuery(\"SELECT ExecutablePath, CommandLine FROM Win32_Process\")\r\n  For Each process In processes\r\n    If LCase(CStr(process.ExecutablePath)) = target Then\r\n      If InStr(1, CStr(process.CommandLine), \"--background\", 1) > 0 Then running = True\r\n    End If\r\n  Next\r\n  If Not running Then shell.Run command, 0, False\r\n  WScript.Sleep 15000\r\nLoop\r\n"
    )
}

#[cfg(target_os = "windows")]
fn windows_supervisor_files() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let app_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定 Windows AppData 目录".to_owned())?
        .join("com.airoute.manager");
    Ok((
        app_data.join("supervisor.enabled"),
        app_data.join("supervisor.vbs"),
        app_data.join("restart-backend.txt"),
    ))
}

#[cfg(any(target_os = "macos", test))]
fn macos_supervisor_plist(executable: &std::path::Path) -> String {
    let command = xml_escape(&executable.to_string_lossy());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.airoute.manager</string><key>ProgramArguments</key><array><string>{command}</string><string>--background</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>ThrottleInterval</key><integer>15</integer><key>ProcessType</key><string>Background</string></dict></plist>\n"
    )
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_user_sid(text: &str) -> Option<String> {
    text.trim()
        .trim_matches('"')
        .rsplit_once("\",\"")
        .map(|(_, value)| value.trim_matches('"').trim())
        .filter(|value| value.starts_with("S-1-"))
        .map(str::to_owned)
}

#[cfg(target_os = "windows")]
fn current_windows_user_sid() -> Result<String, String> {
    let output = hidden_command("whoami.exe")
        .args(["/USER", "/FO", "CSV", "/NH"])
        .output()
        .map_err(|error| format!("无法读取当前用户 SID：{error}"))?;
    if !output.status.success() {
        return Err("无法读取当前用户 SID".to_owned());
    }
    let sid = parse_windows_user_sid(&decode_windows_text(&output.stdout))
        .ok_or_else(|| "当前用户 SID 格式无效".to_owned())?;
    Ok(sid)
}

#[cfg(target_os = "windows")]
fn configure_windows_auto_start(executable: &std::path::Path, enabled: bool) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let (marker, watchdog, backend) = windows_supervisor_files()?;
    if enabled {
        let sid = current_windows_user_sid()?;
        let directory = marker
            .parent()
            .ok_or_else(|| "Windows 监督器路径没有父目录".to_owned())?;
        fs::create_dir_all(directory)
            .map_err(|error| format!("无法创建 Windows 监督器目录：{error}"))?;
        fs::write(&marker, b"enabled\n")
            .map_err(|error| format!("无法启用 Windows 监督器：{error}"))?;
        fs::write(&watchdog, windows_watchdog_vbs(executable, &marker))
            .map_err(|error| format!("无法写入 Windows 隐藏看守：{error}"))?;
        let xml = windows_supervisor_task_xml(executable, &sid);
        let temporary = std::env::temp_dir().join(format!(
            "ai-route-supervisor-{}-{}.xml",
            std::process::id(),
            unix_time_ms()
        ));
        fs::write(&temporary, windows_task_xml_bytes(&xml))
            .map_err(|error| format!("无法写入计划任务定义：{error}"))?;
        let output = hidden_command("schtasks.exe")
            .args(["/Create", "/TN", SUPERVISOR_TASK_NAME, "/XML"])
            .arg(&temporary)
            .arg("/F")
            .output()
            .map_err(|error| format!("无法注册崩溃恢复任务：{error}"));
        let _ = fs::remove_file(&temporary);
        let output = output?;
        if output.status.success() {
            fs::write(&backend, b"scheduled-task\n")
                .map_err(|error| format!("无法记录监督器后端：{error}"))?;
            let _ = hidden_command("reg.exe")
                .args(["DELETE", key, "/V", "AIRouteManager", "/F"])
                .output();
        } else {
            let wscript = std::env::var_os("WINDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join("System32/wscript.exe");
            if !wscript.is_file() {
                let _ = fs::remove_file(&marker);
                let _ = fs::remove_file(&watchdog);
                return Err(format!(
                    "计划任务注册失败且系统没有 wscript 回退：{}",
                    decode_windows_text(&output.stderr)
                        .lines()
                        .last()
                        .unwrap_or("unknown")
                ));
            }
            let command = format!("\"{}\" \"{}\"", wscript.display(), watchdog.display());
            let fallback = hidden_command("reg.exe")
                .args(["ADD", key, "/V", "AIRouteManager", "/T", "REG_SZ", "/D"])
                .arg(command)
                .arg("/F")
                .output()
                .map_err(|error| format!("无法注册用户级隐藏看守：{error}"))?;
            if !fallback.status.success() {
                let _ = fs::remove_file(&marker);
                let _ = fs::remove_file(&watchdog);
                return Err("计划任务与用户级隐藏看守均注册失败".to_owned());
            }
            fs::write(&backend, b"run-key-watchdog\n")
                .map_err(|error| format!("无法记录监督器后端：{error}"))?;
        }
    } else {
        let _ = hidden_command("schtasks.exe")
            .args(["/Delete", "/TN", SUPERVISOR_TASK_NAME, "/F"])
            .output();
        let _ = hidden_command("reg.exe")
            .args(["DELETE", key, "/V", "AIRouteManager", "/F"])
            .output();
        if marker.exists() {
            fs::remove_file(&marker)
                .map_err(|error| format!("无法停用 Windows 监督器：{error}"))?;
        }
        let _ = fs::remove_file(&watchdog);
        let _ = fs::remove_file(&backend);
    }
    Ok(())
}

#[cfg(feature = "desktop-runtime")]
fn configure_auto_start(enabled: bool) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法确定应用路径：{error}"))?;
    #[cfg(target_os = "windows")]
    configure_windows_auto_start(&executable, enabled)?;
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "无法确定用户目录".to_owned())?;
        let directory = PathBuf::from(home).join("Library/LaunchAgents");
        let path = directory.join("com.airoute.manager.plist");
        if enabled {
            fs::create_dir_all(&directory)
                .map_err(|error| format!("无法创建 LaunchAgents：{error}"))?;
            let plist = macos_supervisor_plist(&executable);
            fs::write(path, plist).map_err(|error| format!("无法写入 LaunchAgent：{error}"))?;
        } else if path.exists() {
            fs::remove_file(path).map_err(|error| format!("无法移除 LaunchAgent：{error}"))?;
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = (enabled, executable);
    Ok(())
}

#[tauri::command]
fn get_system_snapshot() -> SystemSnapshot {
    let probes = [
        ("主入口", "127.0.0.1", 17901),
        ("Telegram", "127.0.0.1", 17896),
        ("FlClash 热备", "127.0.0.1", 7890),
        ("托管核心 A", "127.0.0.1", 27898),
        ("托管核心 B", "127.0.0.1", 27899),
    ];
    let managed_reachable = can_connect("127.0.0.1", 27898) || can_connect("127.0.0.1", 27899);
    SystemSnapshot {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        process_id: std::process::id(),
        preview_only: !managed_reachable,
        ports: probes
            .into_iter()
            .map(|(label, host, port)| PortStatus {
                label,
                host,
                port,
                reachable: can_connect(host, port),
            })
            .collect(),
    }
}

#[tauri::command]
fn validate_preferences(preferences: Preferences) -> ValidationResult {
    validate_preferences_inner(&preferences)
}

#[tauri::command]
fn get_engine_status() -> EngineReport {
    let Some(executable) = engine_candidates().into_iter().find(|path| path.is_file()) else {
        return EngineReport {
            available: false,
            executable: None,
            success: false,
            status: None,
            message: "尚未安装本机分流引擎".to_owned(),
        };
    };

    match hidden_command(&executable).arg("status").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let status = serde_json::from_str::<serde_json::Value>(&stdout).ok();
            EngineReport {
                available: true,
                executable: Some(executable.to_string_lossy().into_owned()),
                success: output.status.success() && status.is_some(),
                status,
                message: if output.status.success() {
                    "已读取本机分流引擎状态".to_owned()
                } else {
                    format!("分流引擎返回状态码 {}", output.status)
                },
            }
        }
        Err(error) => EngineReport {
            available: true,
            executable: Some(executable.to_string_lossy().into_owned()),
            success: false,
            status: None,
            message: format!("无法读取分流引擎状态：{error}"),
        },
    }
}

#[tauri::command]
fn validate_connection_profile(profile: ConnectionProfile) -> ValidationResult {
    validate_profile_inner(&profile)
}

#[tauri::command]
fn save_connection_profile(profile: ConnectionProfile) -> Result<ProfileSummary, String> {
    store_profile(&profile)?;
    Ok(summarize_profile(&profile))
}

#[tauri::command]
fn get_connection_profile_summary() -> Result<ProfileSummary, String> {
    load_profile().map(|profile| summarize_profile(&profile))
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn export_mobile_bundle(
    app: tauri::AppHandle,
    request: MobileExportRequest,
) -> Result<MobileExportReport, String> {
    if !request.confirmed_secret_export {
        return Err("必须先确认导出文件包含订阅 Token 与 VPS 密码".to_owned());
    }
    if !request.include_android && !request.include_shadowrocket {
        return Err("至少选择 Android 或 Shadowrocket 导出".to_owned());
    }
    let profile = load_profile()?;
    let bundle = mobile_bundle_for_profile(&profile)?;
    let fixed_host = profile
        .fixed_egress
        .as_ref()
        .map(|fixed| fixed.server.clone())
        .ok_or_else(|| "固定 VPS 出口尚未配置".to_owned())?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| format!("无法确定下载目录：{error}"))?;
    let parent = downloads.join("AI-Route-Mobile-Exports");
    fs::create_dir_all(&parent).map_err(|error| format!("无法创建移动端导出根目录：{error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间异常：{error}"))?
        .as_millis();
    let directory = parent.join(format!("export-{stamp}-{}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| format!("无法创建本次导出目录：{error}"))?;
    if let Err(error) = restrict_secret_path(&directory, true) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }

    let result = (|| -> Result<Vec<MobileExportFile>, String> {
        let mut files = Vec::new();
        if request.include_android {
            files.push(write_mobile_export_file(
                &directory,
                "AI-Route-Android.yaml",
                &bundle.android_yaml,
                "Android / Mihomo",
            )?);
        }
        if request.include_shadowrocket {
            files.push(write_mobile_export_file(
                &directory,
                "AI-Route-Shadowrocket.conf",
                &bundle.shadowrocket_conf,
                "iPhone / Shadowrocket",
            )?);
            files.push(write_mobile_export_file(
                &directory,
                "Mobile-Subscriptions.txt",
                &bundle.subscriptions_txt,
                "订阅清单（敏感）",
            )?);
            files.push(write_mobile_export_file(
                &directory,
                "Fixed-Egress-SS.txt",
                &format!("{}\n", bundle.ss_uri),
                "固定出口 URI（敏感）",
            )?);
            files.push(write_mobile_export_file(
                &directory,
                "Fixed-Egress-SS-QR.svg",
                &qr_svg(&bundle.ss_uri)?,
                "固定出口二维码（敏感）",
            )?);
        }
        files.push(write_mobile_export_file(
            &directory,
            "README.txt",
            &bundle.readme,
            "导入说明",
        )?);
        Ok(files)
    })();

    let files = match result {
        Ok(files) => files,
        Err(error) => {
            let cleanup = fs::remove_dir_all(&directory).is_ok();
            return Err(format!(
                "{error}；失败导出已{}",
                if cleanup {
                    "清理"
                } else {
                    "保留，请手动删除"
                }
            ));
        }
    };
    Ok(MobileExportReport {
        success: true,
        directory: directory.to_string_lossy().into_owned(),
        files,
        warning:
            "导出含订阅 Token 与 VPS 密码；导入自己的设备后请删除整个目录。电脑当前代理未被修改。"
                .to_owned(),
        fixed_egress_host: fixed_host,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct LegacyProfileLocation {
    subscriptions: PathBuf,
    fixed_egress: PathBuf,
}

fn legacy_profile_locations(
    local_app_data: Option<PathBuf>,
    program_data: Option<PathBuf>,
    compatibility_root: Option<PathBuf>,
) -> Vec<LegacyProfileLocation> {
    let mut roots = Vec::new();
    if let Some(root) = local_app_data {
        roots.push(root.join("DynamicProxyManager"));
    }
    if let Some(root) = program_data {
        roots.push(root.join("DynamicProxyManager"));
    }
    if let Some(root) = compatibility_root {
        roots.push(root);
    }
    let mut locations = Vec::new();
    for root in roots {
        let location = LegacyProfileLocation {
            subscriptions: root.join("app").join("subscriptions.local.json"),
            fixed_egress: root.join("data").join("fixed-egress.json"),
        };
        if !locations.contains(&location) {
            locations.push(location);
        }
    }
    locations
}

#[tauri::command]
fn import_existing_profile() -> Result<ProfileSummary, String> {
    #[cfg(target_os = "windows")]
    {
        let locations = legacy_profile_locations(
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            std::env::var_os("PROGRAMDATA").map(PathBuf::from),
            Some(PathBuf::from(r"E:\Tools\DynamicProxyManager")),
        );
        let location = locations
            .into_iter()
            .find(|candidate| candidate.subscriptions.is_file())
            .ok_or_else(|| {
                "没有在当前用户或系统的旧版目录中发现可导入订阅；也可以直接在连接向导中填写"
                    .to_owned()
            })?;
        let subscriptions: SubscriptionEnvelope = serde_json::from_slice(
            &fs::read(&location.subscriptions)
                .map_err(|error| format!("无法读取现有订阅：{error}"))?,
        )
        .map_err(|error| format!("现有订阅格式错误：{error}"))?;
        let fixed_egress = if location.fixed_egress.is_file() {
            Some(
                serde_json::from_slice::<FixedEgressInput>(
                    &fs::read(&location.fixed_egress)
                        .map_err(|error| format!("无法读取现有固定出口：{error}"))?,
                )
                .map_err(|error| format!("现有固定出口格式错误：{error}"))?,
            )
        } else {
            None
        };
        let profile = ConnectionProfile {
            subscriptions: subscriptions.subscriptions,
            fixed_egress,
            ai_region: legacy_ai_region(),
            ai_region_label: legacy_ai_region_label(),
            preferred_node_keywords: legacy_preferred_node_keywords(),
        };
        store_profile(&profile)?;
        Ok(summarize_profile(&profile))
    }
    #[cfg(not(target_os = "windows"))]
    Err("当前设备没有可自动导入的旧版配置".to_owned())
}

#[cfg(feature = "desktop-runtime")]
fn execute_engine_preview(
    app: &tauri::AppHandle,
    profile: &ConnectionProfile,
) -> Result<PreviewReport, String> {
    let validation = validate_profile_inner(profile);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    let engine_dir = bundled_engine_dir(app)?;
    let preview_root = unique_preview_root(app)?;
    let mut workspace = PreviewWorkspace::new(preview_root.clone());
    let runtime_dir = preview_root.join("runtime");
    fs::create_dir_all(&runtime_dir).map_err(|error| format!("无法创建预演目录：{error}"))?;

    let mut child = hidden_command(engine_binary_path(&engine_dir))
        .current_dir(&engine_dir)
        .env("DPM_LOG_STDERR", "1")
        .env("DPM_SLOT_OFFSET", "2000")
        .env("DPM_FRONTEND_PORT", "0")
        .arg("preview")
        .arg("--home")
        .arg(&runtime_dir)
        .arg("--profile-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动分流预演：{error}"))?;

    let payload = engine_profile_bytes(profile)?;
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| "无法打开预演引擎输入".to_owned())
        .and_then(|mut stdin| {
            stdin
                .write_all(&payload)
                .map_err(|error| format!("无法传入预演配置：{error}"))
        });
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("无法等待分流预演：{error}"));
    let cleanup_complete = workspace.cleanup();
    let output = output?;
    let result = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    Ok(PreviewReport {
        success: output.status.success() && result.is_some() && cleanup_complete,
        message: if output.status.success() && result.is_some() {
            if cleanup_complete {
                "预演通过；候选核心已停止，内存凭据与临时运行目录已清除".to_owned()
            } else {
                "预演通过，但临时运行目录回收失败".to_owned()
            }
        } else {
            "预演未通过；当前系统代理保持不变".to_owned()
        },
        result,
        cleanup_complete,
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn run_engine_preview(app: tauri::AppHandle) -> Result<PreviewReport, String> {
    let profile = load_profile()?;
    execute_engine_preview(&app, &profile)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn get_managed_status(app: tauri::AppHandle) -> ManagedStatus {
    managed_status(&app)
}

#[cfg(feature = "desktop-runtime")]
fn stop_managed_engine_process(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let engine_dir = bundled_engine_dir(app)?;
    let home = managed_home(app)?;
    let output = hidden_command(engine_binary_path(&engine_dir))
        .current_dir(&engine_dir)
        .env("DPM_LOG_STDERR", "1")
        .env("DPM_SLOT_OFFSET", MANAGED_SLOT_OFFSET)
        .env("DPM_FRONTEND_PORT", MANAGED_FRONTEND_PORT)
        .arg("stop")
        .arg("--home")
        .arg(home)
        .output()
        .map_err(|error| format!("无法停止托管引擎：{error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "托管引擎停止失败：{}",
            detail.lines().last().unwrap_or("unknown")
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("托管停止结果格式错误：{error}"))
}

#[cfg(feature = "desktop-runtime")]
fn start_managed_mode_inner(
    app: tauri::AppHandle,
    allow_legacy: bool,
) -> Result<ManagedActionReport, String> {
    let before = managed_status(&app);
    if before.running && before.listener {
        return Ok(ManagedActionReport {
            success: true,
            message: "托管分流已经在运行".to_owned(),
            status: before,
            preview: None,
        });
    }
    if before.legacy_conflict && !allow_legacy {
        return Ok(ManagedActionReport {
            success: false,
            message: "检测到旧版守护和稳定入口；请先执行无中断迁移，当前网络未改动".to_owned(),
            status: before,
            preview: None,
        });
    }

    let profile = load_profile()?;
    let preview = execute_engine_preview(&app, &profile)?;
    if !preview.success {
        return Ok(ManagedActionReport {
            success: false,
            message: "隔离预演没有通过；当前网络未改动".to_owned(),
            status: managed_status(&app),
            preview: Some(preview),
        });
    }

    let engine_dir = bundled_engine_dir(&app)?;
    let home = managed_home(&app)?;
    harden_managed_home(&home)?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("launcher.log"))
        .map_err(|error| format!("无法打开托管日志：{error}"))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("无法复制托管日志句柄：{error}"))?;
    let mut command = hidden_command(engine_binary_path(&engine_dir));
    command
        .current_dir(&engine_dir)
        .env("DPM_LOG_STDERR", "1")
        .env("DPM_SLOT_OFFSET", MANAGED_SLOT_OFFSET)
        .env("DPM_FRONTEND_PORT", MANAGED_FRONTEND_PORT)
        .arg("run")
        .arg("--home")
        .arg(&home)
        .arg("--interval")
        .arg("1800")
        .arg("--profile-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log));
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动托管引擎：{error}"))?;
    let payload = engine_profile_bytes(&profile)?;
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| "无法打开托管引擎输入".to_owned())
        .and_then(|mut stdin| {
            stdin
                .write_all(&payload)
                .map_err(|error| format!("无法传入托管配置：{error}"))
        });
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    fs::write(
        home.join("launcher.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "manager_pid": child.id(),
            "started_at_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "slot_offset": MANAGED_SLOT_OFFSET,
        }))
        .map_err(|error| format!("无法序列化启动记录：{error}"))?,
    )
    .map_err(|error| format!("无法写入启动记录：{error}"))?;

    let deadline = Instant::now() + Duration::from_secs(150);
    while Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("无法检查托管进程：{error}"))?
        {
            return Err(format!("托管引擎提前退出：{status}"));
        }
        let status = managed_status(&app);
        if status.running && status.listener {
            return Ok(ManagedActionReport {
                success: true,
                message: "隔离预演通过，托管分流已接管并持续自愈".to_owned(),
                status,
                preview: Some(preview),
            });
        }
        thread::sleep(Duration::from_millis(500));
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = stop_managed_engine_process(&app);
    Err("托管引擎启动超时；已回滚到原系统代理".to_owned())
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn start_managed_mode(app: tauri::AppHandle) -> Result<ManagedActionReport, String> {
    let report = start_managed_mode_inner(app.clone(), false)?;
    if report.success {
        let mut preferences = load_preferences(app.clone()).unwrap_or_default();
        preferences.mode = "managed".to_owned();
        if let Err(error) = save_preferences(app.clone(), preferences) {
            let _ = stop_managed_engine_process(&app);
            return Err(format!("托管模式未能提交设置：{error}；已恢复原系统代理"));
        }
    }
    Ok(report)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn migrate_legacy_mode(app: tauri::AppHandle) -> Result<ManagedActionReport, String> {
    #[cfg(target_os = "windows")]
    {
        if !legacy_manager_detected() {
            return start_managed_mode_inner(app, false);
        }
        let profile = load_profile()?;
        let preview = execute_engine_preview(&app, &profile)?;
        if !preview.success {
            return Ok(ManagedActionReport {
                success: false,
                message: "迁移预演没有通过；旧版分流继续运行".to_owned(),
                status: managed_status(&app),
                preview: Some(preview),
            });
        }

        let home = managed_home(&app)?;
        harden_managed_home(&home)?;
        let tasks = ["DynamicProxyManager", "DynamicProxyManager-Watchdog"]
            .into_iter()
            .filter_map(|name| {
                scheduled_task_enabled(name).map(|enabled| LegacyTaskState {
                    name: name.to_owned(),
                    enabled,
                })
            })
            .collect::<Vec<_>>();
        let mut backup = LegacyMigrationBackup {
            version: 1,
            phase: "prepared".to_owned(),
            created_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            tasks,
        };
        let backup_path = migration_backup_path(&app)?;
        fs::write(
            &backup_path,
            serde_json::to_vec_pretty(&backup)
                .map_err(|error| format!("无法序列化迁移恢复记录：{error}"))?,
        )
        .map_err(|error| format!("无法写入迁移恢复记录：{error}"))?;

        for task in &backup.tasks {
            if task.enabled
                && let Err(error) = set_scheduled_task_enabled(&task.name, false)
            {
                let _ = restore_legacy_tasks(&backup);
                return Err(error);
            }
        }
        if let Err(error) = stop_legacy_manager_loops() {
            let _ = restore_legacy_tasks(&backup);
            return Err(format!("{error}；旧版计划任务已恢复"));
        }

        match start_managed_mode_inner(app.clone(), true) {
            Ok(mut report) if report.success => {
                let mut preferences = load_preferences(app.clone()).unwrap_or_default();
                preferences.mode = "managed".to_owned();
                preferences.auto_start = true;
                if let Err(error) = save_preferences(app.clone(), preferences) {
                    let _ = stop_managed_engine_process(&app);
                    let _ = restore_legacy_tasks(&backup);
                    return Err(format!("迁移未提交：{error}；旧版分流已恢复"));
                }
                backup.phase = "managed".to_owned();
                fs::write(
                    backup_path,
                    serde_json::to_vec_pretty(&backup)
                        .map_err(|error| format!("无法更新迁移恢复记录：{error}"))?,
                )
                .map_err(|error| format!("无法提交迁移恢复记录：{error}"))?;
                report.message =
                    "无中断迁移完成：新版托管已接管，旧版入口保留为可恢复后备".to_owned();
                Ok(report)
            }
            Ok(report) => {
                restore_legacy_tasks(&backup)?;
                Ok(ManagedActionReport {
                    message: format!("{}；旧版分流已恢复", report.message),
                    ..report
                })
            }
            Err(error) => {
                restore_legacy_tasks(&backup)?;
                Err(format!("{error}；旧版分流已恢复"))
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("当前平台没有检测到 Windows 旧版分流".to_owned())
    }
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn stop_managed_mode(app: tauri::AppHandle) -> Result<ManagedActionReport, String> {
    set_supervisor_suspended(&app, true)?;
    let result = stop_managed_mode_inner(app.clone());
    let resume_result = set_supervisor_suspended(&app, false);
    match (result, resume_result) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(format!("{first}；{second}")),
    }
}

#[cfg(feature = "desktop-runtime")]
fn stop_managed_mode_inner(app: tauri::AppHandle) -> Result<ManagedActionReport, String> {
    let before = managed_status(&app);
    if !before.running && !before.listener {
        let mut preferences = load_preferences(app.clone()).unwrap_or_default();
        preferences.mode = "preview".to_owned();
        save_preferences(app, preferences)?;
        return Ok(ManagedActionReport {
            success: true,
            message: "托管分流已经停止；没有改动当前网络".to_owned(),
            status: before,
            preview: None,
        });
    }
    stop_managed_engine_process(&app)?;
    #[cfg(target_os = "windows")]
    if let Some(mut backup) = load_migration_backup(&app)?
        && backup.phase == "managed"
    {
        restore_legacy_tasks(&backup)?;
        backup.phase = "restored".to_owned();
        fs::write(
            migration_backup_path(&app)?,
            serde_json::to_vec_pretty(&backup)
                .map_err(|error| format!("无法更新迁移恢复记录：{error}"))?,
        )
        .map_err(|error| format!("无法提交迁移恢复记录：{error}"))?;
    }
    let status = managed_status(&app);
    let mut preferences = load_preferences(app.clone()).unwrap_or_default();
    preferences.mode = "preview".to_owned();
    save_preferences(app, preferences)?;
    Ok(ManagedActionReport {
        success: !status.running && !status.listener,
        message: "托管分流已停止，原系统代理已经恢复，运行配置中的凭据已清除".to_owned(),
        status,
        preview: None,
    })
}

fn embedded_update_public_key() -> Result<minisign_verify::PublicKey, String> {
    use minisign_verify::PublicKey;
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|error| format!("内置签名配置格式错误：{error}"))?;
    let encoded = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or_else(|| "内置更新公钥缺失".to_owned())?;
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|error| format!("内置更新公钥编码错误：{error}"))?;
    PublicKey::decode(
        std::str::from_utf8(&decoded).map_err(|_| "内置更新公钥不是 UTF-8".to_owned())?,
    )
    .map_err(|error| format!("内置更新公钥无效：{error}"))
}

fn verify_referral_catalog_inner(
    envelope_text: &str,
) -> Result<PublisherReferralCatalogReport, String> {
    use minisign_verify::Signature;

    if envelope_text.len() > 2_000_000 {
        return Err("发行推荐目录超过 2 MB 限制".to_owned());
    }
    let envelope: ReferralCatalogEnvelope = serde_json::from_str(envelope_text)
        .map_err(|error| format!("发行推荐目录外层格式错误：{error}"))?;
    if envelope.schema_version != 1 {
        return Err(format!(
            "不支持的发行推荐目录版本：{}",
            envelope.schema_version
        ));
    }
    if envelope.payload.len() > 1_500_000 || envelope.signature.len() > 16_384 {
        return Err("发行推荐目录编码字段超过限制".to_owned());
    }
    let payload_bytes = STANDARD
        .decode(envelope.payload.trim())
        .map_err(|error| format!("发行推荐目录载荷编码错误：{error}"))?;
    if payload_bytes.len() > 1_000_000 {
        return Err("发行推荐目录载荷超过 1 MB 限制".to_owned());
    }
    let signature_text = STANDARD
        .decode(envelope.signature.trim())
        .map_err(|error| format!("发行推荐目录签名编码错误：{error}"))?;
    let signature = Signature::decode(
        std::str::from_utf8(&signature_text)
            .map_err(|_| "发行推荐目录签名不是 UTF-8".to_owned())?,
    )
    .map_err(|error| format!("发行推荐目录签名格式错误：{error}"))?;
    embedded_update_public_key()?
        .verify(&payload_bytes, &signature, false)
        .map_err(|_| "发行推荐目录签名校验失败".to_owned())?;

    let mut payload: ReferralCatalogPayload = serde_json::from_slice(&payload_bytes)
        .map_err(|error| format!("发行推荐目录载荷格式错误：{error}"))?;
    let version = payload.catalog_version.trim();
    if version.is_empty()
        || version.len() > 80
        || !version.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
    {
        return Err("发行推荐目录的 catalogVersion 无效".to_owned());
    }
    if payload.generated_at.trim().is_empty() || payload.generated_at.len() > 80 {
        return Err("发行推荐目录的 generatedAt 无效".to_owned());
    }
    for offer in &mut payload.offers {
        if !offer.id.starts_with("publisher-") {
            return Err(format!(
                "签名发行推荐位必须使用 publisher- 前缀：{}",
                offer.id
            ));
        }
        offer.origin = "publisher".to_owned();
        offer.catalog_version = Some(version.to_owned());
    }
    let validation = validate_referral_offers_inner(&payload.offers);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    Ok(PublisherReferralCatalogReport {
        catalog_version: version.to_owned(),
        generated_at: payload.generated_at,
        offers: payload.offers,
        signature_verified: true,
        message: "发行推荐目录已通过内置公钥签名验证；本机自定义推荐位保持独立".to_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn verify_referral_catalog(envelope: String) -> Result<PublisherReferralCatalogReport, String> {
    verify_referral_catalog_inner(&envelope)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn load_referral_offers(app: tauri::AppHandle) -> Result<Option<Vec<ReferralOffer>>, String> {
    let path = referral_offers_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| format!("无法读取推荐配置：{error}"))?;
    let envelope: ReferralOffersEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| format!("推荐配置格式错误：{error}"))?;
    if envelope.version != CONFIG_VERSION {
        return Err(format!("不支持的推荐配置版本：{}", envelope.version));
    }
    let validation = validate_referral_offers_inner(&envelope.offers);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    Ok(Some(envelope.offers))
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn save_referral_offers(app: tauri::AppHandle, offers: Vec<ReferralOffer>) -> Result<(), String> {
    let validation = validate_referral_offers_inner(&offers);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }

    let path = referral_offers_path(&app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "推荐配置路径无父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建推荐配置目录：{error}"))?;
    let envelope = ReferralOffersEnvelope {
        version: CONFIG_VERSION,
        offers,
    };
    let json = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| format!("无法序列化推荐配置：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    fs::write(&temporary, json).map_err(|error| format!("无法写入推荐临时配置：{error}"))?;

    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| format!("无法清理旧推荐备份：{error}"))?;
    }
    if path.exists() {
        fs::rename(&path, &backup).map_err(|error| format!("无法备份现有推荐配置：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交推荐配置：{error}"));
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("推荐配置已提交，但旧备份清理失败：{error}"))?;
    }
    Ok(())
}

#[cfg(feature = "desktop-runtime")]
fn load_gateway_policies_inner(app: &tauri::AppHandle) -> Result<Vec<GatewayDevicePolicy>, String> {
    let path = gateway_policies_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let payload = fs::read(&path).map_err(|error| format!("无法读取家庭网关策略：{error}"))?;
    let envelope: GatewayPolicyEnvelope = serde_json::from_slice(&payload)
        .map_err(|error| format!("家庭网关策略格式错误：{error}"))?;
    if envelope.version != CONFIG_VERSION {
        return Err(format!("不支持的家庭网关策略版本：{}", envelope.version));
    }
    validate_gateway_policies(&envelope.policies)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn load_gateway_policies(app: tauri::AppHandle) -> Result<Vec<GatewayDevicePolicy>, String> {
    load_gateway_policies_inner(&app)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn save_gateway_policies(
    app: tauri::AppHandle,
    policies: Vec<GatewayDevicePolicy>,
) -> Result<Vec<GatewayDevicePolicy>, String> {
    let policies = validate_gateway_policies(&policies)?;
    let path = gateway_policies_path(&app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "家庭网关策略路径无父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建家庭网关策略目录：{error}"))?;
    let payload = serde_json::to_vec_pretty(&GatewayPolicyEnvelope {
        version: CONFIG_VERSION,
        policies: policies.clone(),
    })
    .map_err(|error| format!("无法序列化家庭网关策略：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    fs::write(&temporary, payload)
        .map_err(|error| format!("无法写入家庭网关策略临时文件：{error}"))?;
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| format!("无法清理旧策略备份：{error}"))?;
    }
    if path.exists() {
        fs::rename(&path, &backup).map_err(|error| format!("无法备份现有设备策略：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交家庭网关策略：{error}"));
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("策略已提交，但旧备份清理失败：{error}"))?;
    }
    Ok(policies)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn save_preferences(app: tauri::AppHandle, preferences: Preferences) -> Result<(), String> {
    let validation = validate_preferences_inner(&preferences);
    if !validation.valid {
        return Err(validation.errors.join("；"));
    }
    let path = preferences_path(&app)?;
    let parent = path.parent().ok_or_else(|| "配置路径无父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建配置目录：{error}"))?;

    let requested_auto_start = preferences.auto_start;
    let previous_auto_start = if path.exists() {
        fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PreferencesEnvelope>(&bytes).ok())
            .map(|envelope| envelope.preferences.auto_start)
            .unwrap_or(false)
    } else {
        false
    };
    let payload = PreferencesEnvelope {
        version: CONFIG_VERSION,
        preferences,
    };
    let json =
        serde_json::to_vec_pretty(&payload).map_err(|error| format!("无法序列化配置：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    fs::write(&temporary, json).map_err(|error| format!("无法写入临时配置：{error}"))?;
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| format!("无法清理旧配置备份：{error}"))?;
    }
    if path.exists() {
        fs::rename(&path, &backup).map_err(|error| format!("无法备份现有配置：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交配置：{error}"));
    }
    if let Err(error) = configure_auto_start(requested_auto_start) {
        let _ = fs::remove_file(&path);
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
        }
        let rollback = configure_auto_start(previous_auto_start);
        return Err(match rollback {
            Ok(()) => format!("启动项配置失败，已恢复上一版设置：{error}"),
            Err(rollback_error) => {
                format!("启动项配置失败，且启动项回滚失败：{error}；{rollback_error}")
            }
        });
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("配置已提交，但旧备份清理失败：{error}"))?;
    }
    Ok(())
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn load_preferences(app: tauri::AppHandle) -> Result<Preferences, String> {
    let path = preferences_path(&app)?;
    if !path.exists() {
        return Ok(Preferences::default());
    }
    let payload = fs::read(&path).map_err(|error| format!("无法读取配置：{error}"))?;
    let mut envelope: PreferencesEnvelope =
        serde_json::from_slice(&payload).map_err(|error| format!("配置文件格式错误：{error}"))?;
    if envelope.version != CONFIG_VERSION {
        return Err(format!("不支持的配置版本：{}", envelope.version));
    }
    // v0.4.0 shipped before a production feed existed, so an empty value is the
    // old default rather than an intentional opt-out. Users can disable checks
    // with auto_check_updates while retaining a trustworthy manual endpoint.
    if envelope.preferences.update_endpoint.trim().is_empty() {
        envelope.preferences.update_endpoint = default_update_endpoint();
    }
    Ok(envelope.preferences)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn get_local_backup_status() -> LocalBackupStatus {
    match load_backup_envelope() {
        Ok(backup) => backup_status_from(Some(&backup)),
        Err(error) => LocalBackupStatus {
            message: error,
            ..backup_status_from(None)
        },
    }
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn create_local_backup(app: tauri::AppHandle) -> Result<LocalBackupReport, String> {
    let profile = load_profile()?;
    let preferences = load_preferences(app.clone())?;
    let offers = load_referral_offers(app.clone())?.unwrap_or_default();
    let gateway_policies = load_gateway_policies_inner(&app)?;
    let backup = LocalBackupEnvelope {
        version: CONFIG_VERSION,
        created_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("系统时间异常：{error}"))?
            .as_millis(),
        profile,
        preferences,
        offers,
        gateway_policies,
    };
    let secret =
        serde_json::to_string(&backup).map_err(|error| format!("无法序列化本机备份：{error}"))?;
    backup_entry()?
        .set_password(&secret)
        .map_err(|error| format!("无法写入本机凭据库备份：{error}"))?;
    Ok(LocalBackupReport {
        success: true,
        status: backup_status_from(Some(&backup)),
        message: "配置备份已写入系统凭据库；磁盘上没有生成明文 Token 或密码".to_owned(),
        requires_apply: false,
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn restore_local_backup(app: tauri::AppHandle) -> Result<LocalBackupReport, String> {
    let backup = load_backup_envelope()?;
    let profile_entry = profile_entry()?;
    let previous_profile = profile_entry.get_password().ok();
    let preferences_path = preferences_path(&app)?;
    let referrals_path = referral_offers_path(&app)?;
    let gateway_policies_path = gateway_policies_path(&app)?;
    let previous_preferences_bytes = fs::read(&preferences_path).ok();
    let previous_referrals_bytes = fs::read(&referrals_path).ok();
    let previous_gateway_policies_bytes = fs::read(&gateway_policies_path).ok();
    let previous_preferences = load_preferences(app.clone()).unwrap_or_default();

    let apply = (|| -> Result<(), String> {
        store_profile(&backup.profile)?;
        save_referral_offers(app.clone(), backup.offers.clone())?;
        save_gateway_policies(app.clone(), backup.gateway_policies.clone())?;
        save_preferences(app.clone(), backup.preferences.clone())?;
        Ok(())
    })();
    if let Err(error) = apply {
        let mut rollback_errors = Vec::new();
        let profile_rollback = match previous_profile {
            Some(secret) => profile_entry.set_password(&secret),
            None => profile_entry.delete_credential(),
        };
        if let Err(rollback) = profile_rollback {
            rollback_errors.push(format!("凭据库：{rollback}"));
        }
        if let Err(rollback) =
            restore_optional_file(&preferences_path, previous_preferences_bytes.as_deref())
        {
            rollback_errors.push(format!("应用设置：{rollback}"));
        }
        if let Err(rollback) =
            restore_optional_file(&referrals_path, previous_referrals_bytes.as_deref())
        {
            rollback_errors.push(format!("推荐配置：{rollback}"));
        }
        if let Err(rollback) = restore_optional_file(
            &gateway_policies_path,
            previous_gateway_policies_bytes.as_deref(),
        ) {
            rollback_errors.push(format!("家庭网关策略：{rollback}"));
        }
        if let Err(rollback) = configure_auto_start(previous_preferences.auto_start) {
            rollback_errors.push(format!("自动启动：{rollback}"));
        }
        return Err(if rollback_errors.is_empty() {
            format!("恢复失败，原配置已回滚：{error}")
        } else {
            format!(
                "恢复失败：{error}；回滚异常：{}",
                rollback_errors.join("；")
            )
        });
    }
    Ok(LocalBackupReport {
        success: true,
        status: backup_status_from(Some(&backup)),
        message: "配置已恢复到应用与系统凭据库；当前网络未切换，需主动点击验证并应用才会生效"
            .to_owned(),
        requires_apply: true,
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn export_diagnostics(app: tauri::AppHandle) -> Result<DiagnosticExportReport, String> {
    let preferences = load_preferences(app.clone()).unwrap_or_default();
    let offers = load_referral_offers(app.clone())?.unwrap_or_default();
    let profile = load_profile().ok();
    let snapshot = get_system_snapshot();
    let managed = managed_status(&app);
    let engine = get_engine_status();
    let payload = diagnostic_payload(
        profile.as_ref(),
        &preferences,
        &offers,
        &snapshot,
        &managed,
        &engine,
    );
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("无法序列化诊断信息：{error}"))?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| format!("无法确定下载目录：{error}"))?;
    let parent = downloads.join("AI-Route-Diagnostics");
    fs::create_dir_all(&parent).map_err(|error| format!("无法创建诊断导出根目录：{error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间异常：{error}"))?
        .as_millis();
    let directory = parent.join(format!("diagnostic-{stamp}-{}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| format!("无法创建诊断目录：{error}"))?;
    if let Err(error) = restrict_secret_path(&directory, true) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    let result = (|| -> Result<Vec<MobileExportFile>, String> {
        Ok(vec![
            write_mobile_export_file(&directory, "diagnostics.json", &json, "脱敏结构化诊断")?,
            write_mobile_export_file(
                &directory,
                "README.txt",
                "AI Route Manager 诊断包\r\n\r\n本包只包含结构化本机状态，不包含原始日志、订阅 URL、Token、密码、Cookie、私钥或完整节点地址。\r\n导出过程不会修改系统代理，也不会启动或停止任何核心。\r\n",
                "隐私说明",
            )?,
        ])
    })();
    let files = match result {
        Ok(files) => files,
        Err(error) => {
            let cleanup = fs::remove_dir_all(&directory).is_ok();
            return Err(format!(
                "{error}；失败诊断目录已{}",
                if cleanup {
                    "清理"
                } else {
                    "保留，请手动删除"
                }
            ));
        }
    };
    cleanup_old_diagnostics(&parent, &directory, preferences.diagnostics_retention_days);
    Ok(DiagnosticExportReport {
        success: true,
        directory: directory.to_string_lossy().into_owned(),
        files,
        message: "脱敏诊断包已生成；当前网络配置没有变化".to_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn get_remote_environment() -> RemoteEnvironmentReport {
    get_remote_environment_inner()
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn get_sunshine_config_status(app: tauri::AppHandle) -> Result<SunshineConfigStatus, String> {
    let supported = cfg!(any(target_os = "windows", target_os = "macos"));
    let config = sunshine_config_path()?;
    let running = remote_process_listing().contains("sunshine");
    let state = load_sunshine_state(&app)?;
    let current_hash = current_file_hash(&config)?;
    let rollback_available = state
        .as_ref()
        .filter(|value| matches!(value.phase.as_str(), "prepared" | "applied"))
        .is_some_and(|value| {
            validate_sunshine_state_paths(&app, value).is_ok()
                && current_hash.as_deref() == Some(value.applied_hash.as_str())
        });
    let applied_hash = state
        .as_ref()
        .and_then(|value| rollback_available.then_some(value.applied_hash.clone()));
    let message = if rollback_available {
        if running {
            "已检测到 AI Route Manager 配置；Sunshine 正在运行，重启 Sunshine 后读取新参数"
        } else {
            "已检测到 AI Route Manager 配置，可一键回滚到写入前版本"
        }
    } else if config.is_file() {
        "检测到 Sunshine 配置；尚未由 AI Route Manager 接管参数"
    } else {
        "未检测到 Sunshine 配置文件；安装并首次启动 Sunshine 后即可自动写入"
    };
    Ok(SunshineConfigStatus {
        supported,
        config_path: config.to_string_lossy().into_owned(),
        exists: config.is_file(),
        sunshine_running: running,
        rollback_available,
        applied_hash,
        message: message.to_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn configure_sunshine_streaming(
    app: tauri::AppHandle,
    request: SunshineConfigRequest,
) -> Result<SunshineConfigActionReport, String> {
    validate_sunshine_config_request(&request)?;
    let config = sunshine_config_path()?;
    let running = remote_process_listing().contains("sunshine");
    if request.action == "rollback" {
        let mut state = load_sunshine_state(&app)?
            .ok_or_else(|| "没有可用的 Sunshine 配置回滚点".to_owned())?;
        if !matches!(state.phase.as_str(), "prepared" | "applied") {
            return Err("Sunshine 配置回滚点已经使用或不完整".to_owned());
        }
        let (trusted_config, backup) = validate_sunshine_state_paths(&app, &state)?;
        #[cfg(target_os = "windows")]
        run_sunshine_elevated(
            "rollback",
            &trusted_config,
            &sunshine_state_path(&app)?,
            &backup,
            state.before_hash.as_deref(),
            &state.applied_hash,
        )?;
        #[cfg(not(target_os = "windows"))]
        commit_sunshine_direct(
            "rollback",
            &trusted_config,
            Path::new(""),
            &backup,
            state.before_hash.as_deref(),
            &state.applied_hash,
        )?;
        let restored_hash = current_file_hash(&trusted_config)?;
        if restored_hash.as_deref() != state.before_hash.as_deref() {
            return Err("Sunshine 回滚后的最终哈希不匹配".to_owned());
        }
        state.phase = "rolled_back".to_owned();
        write_sunshine_state(&app, &state)?;
        return Ok(SunshineConfigActionReport {
            success: true,
            action: "rollback".to_owned(),
            config_path: trusted_config.to_string_lossy().into_owned(),
            backup_path: Some(backup.to_string_lossy().into_owned()),
            before_hash: Some(state.applied_hash),
            after_hash: restored_hash,
            managed_values: Vec::new(),
            restart_required: running,
            message: if running {
                "Sunshine 配置已恢复；Sunshine 正在运行，请结束当前串流后重启 Sunshine"
            } else {
                "Sunshine 配置已恢复到写入前版本；本机代理和防火墙未改变"
            }
            .to_owned(),
        });
    }

    if let Some(state) = load_sunshine_state(&app)?
        && matches!(state.phase.as_str(), "prepared" | "applied")
        && current_file_hash(&config)?.as_deref() == Some(state.applied_hash.as_str())
    {
        return Err("已有可回滚的 Sunshine 配置；请先回滚，再应用新的实测参数".to_owned());
    }
    let environment = get_remote_environment_inner();
    if !environment
        .tools
        .iter()
        .any(|tool| tool.id == "sunshine" && tool.installed)
        && !config.is_file()
    {
        return Err("尚未安装 Sunshine；请先使用安装向导完成安装".to_owned());
    }
    let config_existed = config.is_file();
    let current_bytes = if config_existed {
        let bytes =
            fs::read(&config).map_err(|error| format!("无法读取 Sunshine 配置：{error}"))?;
        if bytes.len() > SUNSHINE_CONFIG_LIMIT {
            return Err("Sunshine 配置超过 1 MB 限制".to_owned());
        }
        bytes
    } else {
        Vec::new()
    };
    let current_text = std::str::from_utf8(&current_bytes)
        .map_err(|_| "Sunshine 配置不是 UTF-8，拒绝自动覆盖".to_owned())?;
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else {
        "macos"
    };
    let (candidate_text, managed_values) = merge_sunshine_config(current_text, &request, platform)?;
    let candidate_bytes = candidate_text.as_bytes();
    let before_hash = config_existed.then(|| sha256_hex(&current_bytes));
    let applied_hash = sha256_hex(candidate_bytes);
    if before_hash.as_deref() == Some(applied_hash.as_str()) {
        return Ok(SunshineConfigActionReport {
            success: true,
            action: "apply".to_owned(),
            config_path: config.to_string_lossy().into_owned(),
            backup_path: None,
            before_hash: before_hash.clone(),
            after_hash: before_hash,
            managed_values,
            restart_required: false,
            message: "Sunshine 已使用相同参数，无需重复写入".to_owned(),
        });
    }
    let root = sunshine_profile_root(&app)?;
    let backup_root = root.join("backups");
    fs::create_dir_all(&backup_root)
        .map_err(|error| format!("无法创建 Sunshine 事务目录：{error}"))?;
    let stamp = unix_time_ms();
    let backup = backup_root.join(format!("sunshine-{stamp}.conf"));
    let candidate = root.join(format!("candidate-{stamp}.conf"));
    fs::write(&candidate, candidate_bytes)
        .map_err(|error| format!("无法写入 Sunshine 候选配置：{error}"))?;
    let mut state = SunshineConfigState {
        version: SUNSHINE_CONFIG_STATE_VERSION,
        phase: "prepared".to_owned(),
        config_path: config.to_string_lossy().into_owned(),
        backup_path: backup.to_string_lossy().into_owned(),
        before_hash: before_hash.clone(),
        applied_hash: applied_hash.clone(),
        applied_at_ms: stamp,
    };
    write_sunshine_state(&app, &state)?;
    #[cfg(target_os = "windows")]
    let commit = run_sunshine_elevated(
        "apply",
        &config,
        &candidate,
        &backup,
        before_hash.as_deref(),
        &applied_hash,
    );
    #[cfg(not(target_os = "windows"))]
    let commit = commit_sunshine_direct(
        "apply",
        &config,
        &candidate,
        &backup,
        before_hash.as_deref(),
        &applied_hash,
    );
    let cleanup = fs::remove_file(&candidate);
    if let Err(error) = commit {
        if current_file_hash(&config)?.as_deref() != Some(applied_hash.as_str()) {
            let _ = fs::remove_file(sunshine_state_path(&app)?);
        }
        return Err(format!(
            "{error}；候选配置{}清理",
            if cleanup.is_ok() { "已" } else { "未" }
        ));
    }
    if current_file_hash(&config)?.as_deref() != Some(applied_hash.as_str()) {
        return Err("Sunshine 配置提交后最终哈希不匹配".to_owned());
    }
    state.phase = "applied".to_owned();
    write_sunshine_state(&app, &state)?;
    if let Err(error) = cleanup {
        return Err(format!("Sunshine 配置已提交，但候选文件清理失败：{error}"));
    }
    Ok(SunshineConfigActionReport {
        success: true,
        action: "apply".to_owned(),
        config_path: config.to_string_lossy().into_owned(),
        backup_path: Some(backup.to_string_lossy().into_owned()),
        before_hash,
        after_hash: Some(applied_hash),
        managed_values,
        restart_required: running,
        message: if running {
            "实测参数已写入并保留回滚点；Sunshine 正在运行，请结束当前串流后重启 Sunshine"
        } else {
            "实测参数已事务写入 Sunshine，并已创建可验证回滚点；本机代理和防火墙未改变"
        }
        .to_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn probe_remote_path(request: RemoteProbeRequest) -> Result<RemoteQualityReport, String> {
    probe_remote_path_inner(&request)
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn install_remote_tool(request: RemoteInstallRequest) -> Result<RemoteInstallReport, String> {
    if !request.confirmed {
        return Err("必须明确确认后才能安装远控工具".to_owned());
    }
    let package_id = remote_package_id(&request.tool_id)?.to_owned();
    #[cfg(not(target_os = "windows"))]
    {
        return Err("当前平台请使用页面中的官方下载入口".to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        let winget = executable_from_candidates(&["winget.exe", "winget"], &[])
            .ok_or_else(|| "未找到 winget，请使用官方下载入口".to_owned())?;
        let output = hidden_command(winget)
            .args([
                "install",
                "--id",
                package_id.as_str(),
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ])
            .output()
            .map_err(|error| format!("无法启动 winget：{error}"))?;
        let success = output.status.success();
        Ok(RemoteInstallReport {
            success,
            tool_id: request.tool_id,
            package_id,
            exit_code: output.status.code(),
            message: if success {
                "安装命令已成功完成；请刷新环境检测".to_owned()
            } else {
                format!(
                    "winget 安装失败（exit={}）；请使用官方下载入口",
                    output
                        .status
                        .code()
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_owned())
                )
            },
        })
    }
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn configure_local_streaming_firewall(
    request: LocalFirewallRequest,
) -> Result<FirewallActionReport, String> {
    validate_firewall_confirmation(&request.action, request.confirmed)?;
    let rules = sunshine_firewall_rules(&request.scope)?;
    let script = sunshine_firewall_script(&request.action, &request.scope)?;
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (rules, script);
        return Err("本机 Sunshine 防火墙向导当前仅支持 Windows".to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        let script_path = std::env::temp_dir().join(format!(
            "ai-route-firewall-{}-{}.ps1",
            std::process::id(),
            unix_time_ms()
        ));
        fs::write(&script_path, script.as_bytes())
            .map_err(|error| format!("无法写入临时防火墙脚本：{error}"))?;
        let command = "$script = $env:AI_ROUTE_FIREWALL_SCRIPT; $args = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('`\"' + $script + '`\"')); $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $args -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $process.ExitCode";
        let output = hidden_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .env("AI_ROUTE_FIREWALL_SCRIPT", &script_path)
            .output();
        let cleanup_result = fs::remove_file(&script_path);
        let output = output.map_err(|error| format!("无法启动防火墙提权流程：{error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Windows 防火墙操作未完成（exit={}）；临时脚本{}清理",
                output
                    .status
                    .code()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
                if cleanup_result.is_ok() { "已" } else { "未" }
            ));
        }
        if let Err(error) = cleanup_result {
            return Err(format!("防火墙已提交，但临时脚本清理失败：{error}"));
        }
        Ok(FirewallActionReport {
            success: true,
            target: "本机 Sunshine".to_owned(),
            action: request.action.clone(),
            rules,
            message: if request.action == "apply" {
                "Sunshine 最小端口已限制在所选专用网络范围；未修改路由器端口转发或系统代理"
                    .to_owned()
            } else {
                "AI Route Manager 创建的 Sunshine 防火墙规则已回滚".to_owned()
            },
        })
    }
}

#[cfg(feature = "desktop-runtime")]
fn run_ssh_program(
    program: &std::path::Path,
    args: &[String],
    script: &str,
) -> Result<SshRunResult, String> {
    let mut child = hidden_command(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 SSH：{error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "无法写入 SSH 标准输入".to_owned())?
        .write_all(script.as_bytes())
        .map_err(|error| format!("无法发送固定部署脚本：{error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 SSH 完成失败：{error}"))?;
    Ok(SshRunResult {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn inspect_home_gateway(request: GatewayInspectRequest) -> Result<GatewayInspectReport, String> {
    let identity = PathBuf::from(request.identity_file.trim());
    let identity_is_file = identity.is_file();
    let ssh_name = if cfg!(target_os = "windows") {
        "ssh.exe"
    } else {
        "ssh"
    };
    let ssh = executable_from_candidates(&[ssh_name, "ssh"], &[])
        .ok_or_else(|| "未找到系统 SSH 客户端".to_owned())?;
    inspect_home_gateway_with_runner(&request, identity_is_file, |args, script| {
        run_ssh_program(&ssh, args, script)
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn configure_home_gateway(
    app: tauri::AppHandle,
    request: GatewayDeployRequest,
) -> Result<GatewayDeployReport, String> {
    let identity = PathBuf::from(request.identity_file.trim());
    let identity_is_file = identity.is_file();
    let profile = if request.action == "rollback" {
        None
    } else {
        Some(load_profile()?)
    };
    let policies = if request.action == "rollback" {
        Vec::new()
    } else {
        load_gateway_policies_inner(&app)?
    };
    let ssh_name = if cfg!(target_os = "windows") {
        "ssh.exe"
    } else {
        "ssh"
    };
    let ssh = executable_from_candidates(&[ssh_name, "ssh"], &[])
        .ok_or_else(|| "未找到系统 SSH 客户端".to_owned())?;
    configure_home_gateway_with_runner(
        &request,
        identity_is_file,
        profile.as_ref(),
        &policies,
        |args, script| run_ssh_program(&ssh, args, script),
    )
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn deploy_rustdesk_server(request: RustDeskDeployRequest) -> Result<RustDeskDeployReport, String> {
    let identity = PathBuf::from(request.identity_file.trim());
    let identity_is_file = identity.is_file();
    let ssh_name = if cfg!(target_os = "windows") {
        "ssh.exe"
    } else {
        "ssh"
    };
    let ssh = executable_from_candidates(&[ssh_name, "ssh"], &[])
        .ok_or_else(|| "未找到系统 SSH 客户端".to_owned())?;
    deploy_rustdesk_with_runner(&request, identity_is_file, |args, script| {
        run_ssh_program(&ssh, args, script)
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn configure_rustdesk_firewall(
    request: RustDeskFirewallRequest,
) -> Result<FirewallActionReport, String> {
    let identity = PathBuf::from(request.identity_file.trim());
    let identity_is_file = identity.is_file();
    let ssh_name = if cfg!(target_os = "windows") {
        "ssh.exe"
    } else {
        "ssh"
    };
    let ssh = executable_from_candidates(&[ssh_name, "ssh"], &[])
        .ok_or_else(|| "未找到系统 SSH 客户端".to_owned())?;
    configure_rustdesk_firewall_with_runner(&request, identity_is_file, |args, script| {
        run_ssh_program(&ssh, args, script)
    })
}

#[cfg(feature = "desktop-runtime")]
#[tauri::command]
fn export_remote_profile(
    app: tauri::AppHandle,
    request: RemoteProfileExportRequest,
) -> Result<DiagnosticExportReport, String> {
    let host = validate_remote_host(&request.host)?;
    if request.port == 0
        || request.fps == 0
        || request.fps > 240
        || !(3..=150).contains(&request.bitrate_mbps)
    {
        return Err("远控预设中的端口、帧率或码率无效".to_owned());
    }
    let allowed_encoders = ["auto", "nvenc", "quicksync", "amdvce"];
    if !allowed_encoders.contains(&request.encoder.as_str()) {
        return Err("不支持的 Sunshine 编码器".to_owned());
    }
    let encoder_line = if request.encoder == "auto" {
        "# encoder = auto（由 Sunshine 探测）".to_owned()
    } else {
        format!("encoder = {}", request.encoder)
    };
    let sunshine = format!(
        "# AI Route Manager 推荐片段；合并前先在 Sunshine Web UI 备份现有配置。\ncapture = ddx\n{encoder_line}\nmax_bitrate = {}\nnvenc_preset = 1\nnvenc_twopass = quarter_res\nnvenc_spatial_aq = disabled\nnvenc_vbv_increase = 0\n",
        u32::from(request.bitrate_mbps) * 1000
    );
    let moonlight = format!(
        "Moonlight 客户端建议\r\n目标：{host}:{}\r\n预设：{}\r\n分辨率：{}\r\n帧率：{} FPS\r\n视频码率：{} Mbps\r\n编码：{}\r\n\r\n优先使用主机有线连接和客户端 5 GHz/Wi-Fi 6；分辨率尽量匹配主机显示器，出现丢帧先降低码率。\r\n",
        request.port,
        request.preset,
        request.resolution,
        request.fps,
        request.bitrate_mbps,
        request.codec
    );
    let rustdesk = format!(
        "RustDesk 建议\r\n目标主机：{host}\r\n优先级：局域网直连 > UDP 打洞直连 > 自建近端中继 > 用户选择的 VPS 最后中继。\r\n安全设置：启用永久密码时使用独立高强度密码；仅向所需账户开放键鼠/剪贴板权限。\r\n直连：需要时在 Security 中启用 Direct IP access；默认端口 21118。\r\n自建服务器：客户端填写 hbbs 地址与服务器公钥；Relay 可留空让客户端推导。\r\n"
    );
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| format!("无法确定下载目录：{error}"))?;
    let parent = downloads.join("AI-Route-Remote-Profiles");
    fs::create_dir_all(&parent).map_err(|error| format!("无法创建远控预设目录：{error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间异常：{error}"))?
        .as_millis();
    let directory = parent.join(format!("remote-{stamp}-{}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| format!("无法创建本次远控预设：{error}"))?;
    let result = (|| -> Result<Vec<MobileExportFile>, String> {
        Ok(vec![
            write_mobile_export_file(
                &directory,
                "Sunshine-recommended.conf",
                &sunshine,
                "Sunshine 配置片段",
            )?,
            write_mobile_export_file(
                &directory,
                "Moonlight-settings.txt",
                &moonlight,
                "Moonlight 客户端参数",
            )?,
            write_mobile_export_file(
                &directory,
                "RustDesk-settings.txt",
                &rustdesk,
                "RustDesk 路径与安全建议",
            )?,
        ])
    })();
    let files = match result {
        Ok(files) => files,
        Err(error) => {
            let cleanup = fs::remove_dir_all(&directory).is_ok();
            return Err(format!(
                "{error}；失败预设已{}",
                if cleanup {
                    "清理"
                } else {
                    "保留，请手动删除"
                }
            ));
        }
    };
    Ok(DiagnosticExportReport {
        success: true,
        directory: directory.to_string_lossy().into_owned(),
        files,
        message: "远控预设已生成；未启动服务、未修改防火墙或当前代理".to_owned(),
    })
}

#[cfg(feature = "desktop-runtime")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            app.manage(PendingUpdate(Mutex::new(None)));
            #[cfg(target_os = "windows")]
            if let Ok(root) = windows_app_data_root() {
                start_stable_frontdoor(root);
            }
            let background = background_start_requested_from(std::env::args());
            if background {
                let handle = app.handle().clone();
                let preferences = load_preferences(handle.clone()).ok();
                let supervise = preferences
                    .as_ref()
                    .is_some_and(|preferences| preferences.auto_start);
                let recover_managed = preferences
                    .as_ref()
                    .is_some_and(|preferences| preferences.mode == "managed");
                if supervise {
                    if recover_managed
                        && let Err(error) = start_managed_mode_inner(handle.clone(), false)
                        && let Ok(path) = handle.path().app_log_dir()
                    {
                        let _ = fs::create_dir_all(&path);
                        let _ = fs::write(path.join("background-start-error.txt"), error);
                    }
                    start_supervisor(handle);
                    if let Some(window) = app.get_webview_window("main") {
                        window.hide()?;
                    }
                } else if let Some(window) = app.get_webview_window("main") {
                    window.destroy()?;
                }
            } else if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                if load_preferences(app.handle().clone())
                    .is_ok_and(|preferences| preferences.auto_start)
                {
                    let _ = launch_headless_supervisor_process();
                }
                #[cfg(not(target_os = "windows"))]
                if !read_supervisor_status(app.handle()).active {
                    start_supervisor(app.handle().clone());
                }
                window.show()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_system_snapshot,
            get_engine_status,
            validate_connection_profile,
            save_connection_profile,
            get_connection_profile_summary,
            export_mobile_bundle,
            inspect_home_gateway,
            get_local_network_devices,
            configure_home_gateway,
            load_gateway_policies,
            save_gateway_policies,
            export_diagnostics,
            get_local_backup_status,
            create_local_backup,
            restore_local_backup,
            get_remote_environment,
            get_sunshine_config_status,
            configure_sunshine_streaming,
            probe_remote_path,
            install_remote_tool,
            configure_local_streaming_firewall,
            deploy_rustdesk_server,
            configure_rustdesk_firewall,
            export_remote_profile,
            import_existing_profile,
            run_engine_preview,
            get_managed_status,
            get_supervisor_status,
            repair_supervisor,
            check_for_updates,
            install_pending_update,
            start_managed_mode,
            migrate_legacy_mode,
            stop_managed_mode,
            load_referral_offers,
            save_referral_offers,
            verify_referral_catalog,
            validate_preferences,
            save_preferences,
            load_preferences
        ])
        .run(tauri::generate_context!())
        .expect("AI Route Manager failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe_preview_preferences() {
        let preferences = Preferences::default();
        assert_eq!(preferences.mode, "preview");
        assert!(preferences.strict_ai);
        assert!(!preferences.auto_start);
        assert!(preferences.auto_check_updates);
        assert_eq!(preferences.update_endpoint, default_update_endpoint());
        assert!(validate_preferences_inner(&preferences).valid);
    }

    #[test]
    fn public_legacy_import_checks_user_and_system_locations_before_personal_compatibility() {
        let local = PathBuf::from(r"C:\Users\example\AppData\Local");
        let program = PathBuf::from(r"C:\ProgramData");
        let compatibility = PathBuf::from(r"E:\Tools\DynamicProxyManager");
        let locations = legacy_profile_locations(
            Some(local.clone()),
            Some(program.clone()),
            Some(compatibility.clone()),
        );
        assert_eq!(locations.len(), 3);
        assert_eq!(
            locations[0].subscriptions,
            local.join("DynamicProxyManager/app/subscriptions.local.json")
        );
        assert_eq!(
            locations[1].fixed_egress,
            program.join("DynamicProxyManager/data/fixed-egress.json")
        );
        assert_eq!(
            locations[2].subscriptions,
            compatibility.join("app/subscriptions.local.json")
        );
    }

    #[test]
    fn update_endpoint_requires_https_without_credentials_or_fragments() {
        assert!(validate_update_endpoint_inner("https://updates.example.com/latest.json").is_ok());
        assert!(validate_update_endpoint_inner("http://updates.example.com/latest.json").is_err());
        assert!(
            validate_update_endpoint_inner("https://name:secret@updates.example.com/latest.json")
                .is_err()
        );
        assert!(
            validate_update_endpoint_inner("https://updates.example.com/latest.json#old").is_err()
        );
    }

    #[test]
    fn preferences_accept_an_https_update_manifest() {
        let preferences = Preferences {
            update_endpoint: "https://updates.example.com/latest.json".to_owned(),
            ..Preferences::default()
        };
        assert!(validate_preferences_inner(&preferences).valid);
    }

    #[test]
    fn updater_progress_event_matches_frontend_contract() {
        let value = serde_json::to_value(UpdateDownloadEvent::Started {
            content_length: Some(4096),
        })
        .unwrap();
        assert_eq!(value["event"], "started");
        assert_eq!(value["data"]["contentLength"], 4096);
    }

    #[test]
    fn release_updater_signature_verifies_with_embedded_public_key_when_requested() {
        if std::env::var("AI_ROUTE_VERIFY_RELEASE_SIGNATURE").as_deref() != Ok("1") {
            return;
        }
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use minisign_verify::{PublicKey, Signature};

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config: serde_json::Value = serde_json::from_slice(
            &fs::read(manifest_dir.join("tauri.conf.json")).expect("read Tauri config"),
        )
        .expect("parse Tauri config");
        let embedded = config["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("embedded updater public key");
        let decoded = STANDARD
            .decode(embedded)
            .expect("decode embedded public key");
        let public_key = PublicKey::decode(std::str::from_utf8(&decoded).expect("public key utf8"))
            .expect("parse embedded public key");
        let version = env!("CARGO_PKG_VERSION");
        let artifact_name = format!("AI-Route-Manager-{version}-Windows-x64-setup.exe");
        let release = manifest_dir
            .parent()
            .expect("project root")
            .join("release")
            .join(version);
        let artifact = release.join(&artifact_name);
        let encoded_signature = fs::read_to_string(release.join(format!("{artifact_name}.sig")))
            .expect("load updater signature");
        let decoded_signature = STANDARD
            .decode(encoded_signature.trim())
            .expect("decode updater signature envelope");
        let signature = Signature::decode(
            std::str::from_utf8(&decoded_signature).expect("updater signature utf8"),
        )
        .expect("parse updater signature");
        assert!(
            signature
                .trusted_comment()
                .contains(&format!("version:{version}"))
        );
        let mut artifact_bytes = fs::read(artifact).expect("read updater artifact");
        public_key
            .verify(&artifact_bytes, &signature, false)
            .expect("verify updater artifact signature");
        artifact_bytes[0] ^= 1;
        assert!(
            public_key
                .verify(&artifact_bytes, &signature, false)
                .is_err()
        );
    }

    #[test]
    fn desktop_csp_allows_only_the_official_referral_catalog_host() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse Tauri config");
        let csp = config["app"]["security"]["csp"]
            .as_str()
            .expect("desktop CSP");
        assert!(
            csp.contains("connect-src ipc: http://ipc.localhost https://139-59-246-27.sslip.io")
        );
        assert!(!csp.contains("connect-src *"));
    }

    #[test]
    fn rejects_unknown_mode_and_bad_retention() {
        let preferences = Preferences {
            mode: "unsafe".to_owned(),
            diagnostics_retention_days: 0,
            ..Preferences::default()
        };
        let result = validate_preferences_inner(&preferences);
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn disabling_strict_ai_is_visible_as_warning() {
        let preferences = Preferences {
            strict_ai: false,
            ..Preferences::default()
        };
        let result = validate_preferences_inner(&preferences);
        assert!(result.valid);
        assert!(
            result
                .warnings
                .iter()
                .any(|item| item.contains("AI 严格出口"))
        );
    }

    fn sample_profile() -> ConnectionProfile {
        ConnectionProfile {
            subscriptions: vec![SubscriptionInput {
                role: "stable".to_owned(),
                label: "稳定订阅".to_owned(),
                prefix: "稳定".to_owned(),
                url: "https://example.com/subscribe?token=secret".to_owned(),
            }],
            fixed_egress: Some(FixedEgressInput {
                name: "固定出口".to_owned(),
                server: "203.0.113.8".to_owned(),
                port: 443,
                cipher: "aes-128-gcm".to_owned(),
                password: "test-only".to_owned(),
            }),
            ai_region: "SG".to_owned(),
            ai_region_label: "新加坡".to_owned(),
            preferred_node_keywords: vec!["李家坡2-直连".to_owned()],
        }
    }

    #[test]
    fn legacy_profile_without_region_migrates_to_personal_singapore_policy() {
        let value = serde_json::json!({
            "subscriptions": [{
                "role": "stable",
                "label": "稳定订阅",
                "prefix": "稳定",
                "url": "https://example.com/subscribe"
            }],
            "fixedEgress": null
        });
        let profile: ConnectionProfile = serde_json::from_value(value).unwrap();
        assert_eq!(profile.ai_region, "SG");
        assert_eq!(profile.ai_region_label, "新加坡");
        assert_eq!(profile.preferred_node_keywords, vec!["李家坡2-直连"]);
    }

    #[test]
    fn public_profile_accepts_non_singapore_regions_and_rejects_bad_codes() {
        let mut profile = sample_profile();
        profile.ai_region = "US".to_owned();
        profile.ai_region_label = "美国".to_owned();
        profile.preferred_node_keywords = vec!["洛杉矶优化".to_owned()];
        assert!(validate_profile_inner(&profile).valid);
        profile.ai_region = "USA".to_owned();
        assert!(!validate_profile_inner(&profile).valid);
    }

    #[test]
    fn connection_profile_requires_https_and_stable_role() {
        let mut profile = sample_profile();
        assert!(validate_profile_inner(&profile).valid);
        profile.subscriptions[0].url = "http://example.com/subscribe".to_owned();
        assert!(!validate_profile_inner(&profile).valid);
        profile.subscriptions[0].url = "https://example.com/subscribe".to_owned();
        profile.subscriptions[0].role = "download1".to_owned();
        assert!(!validate_profile_inner(&profile).valid);
    }

    #[test]
    fn profile_summary_never_contains_subscription_token_or_password() {
        let summary = summarize_profile(&sample_profile());
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("example.com"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("test-only"));
    }

    #[test]
    fn preview_workspace_removes_secrets_when_scope_exits_early() {
        let path = std::env::temp_dir().join(format!(
            "ai-route-cleanup-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("secrets")).unwrap();
        fs::write(path.join("secrets/profile.json"), b"test-secret").unwrap();
        {
            let _workspace = PreviewWorkspace::new(path.clone());
        }
        assert!(!path.exists());
    }

    #[test]
    fn engine_profile_uses_stdin_schema_without_camel_case_secret_field() {
        let bytes = engine_profile_bytes(&sample_profile()).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.get("subscriptions").unwrap().is_array());
        assert!(json.get("fixed_egress").unwrap().is_object());
        assert!(json.get("fixedEgress").is_none());
    }

    #[test]
    fn mobile_android_config_keeps_ai_fail_closed_and_china_direct() {
        let bundle = mobile_bundle_for_profile(&sample_profile()).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&bundle.android_yaml).unwrap();
        assert!(parsed.get("proxy-providers").is_some());
        assert!(parsed.get("proxy-groups").is_some());
        let ai = bundle
            .android_yaml
            .find("DOMAIN-SUFFIX,openai.com,AI-STRICT")
            .unwrap();
        let china = bundle.android_yaml.find("GEOSITE,cn,DIRECT").unwrap();
        let fallback = bundle.android_yaml.find("MATCH,STABLE-AUTO").unwrap();
        assert!(ai < china && china < fallback);
        assert!(
            bundle
                .android_yaml
                .contains("name: AI-STRICT\n    type: select\n    proxies:\n      - FIXED-EGRESS")
        );
        assert!(
            !bundle
                .android_yaml
                .contains("AI-STRICT\n    type: select\n    proxies:\n      - DIRECT")
        );
        assert!(
            bundle
                .android_yaml
                .contains("https://example.com/subscribe?token=secret")
        );
    }

    #[test]
    fn gateway_config_enables_lan_without_weakening_ai_or_china_rules() {
        let yaml = gateway_yaml_for_profile(&sample_profile()).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed["allow-lan"], true);
        assert_eq!(parsed["bind-address"], "*");
        let ai = yaml.find("DOMAIN-SUFFIX,openai.com,AI-STRICT").unwrap();
        let china = yaml.find("GEOSITE,cn,DIRECT").unwrap();
        let fallback = yaml.find("MATCH,STABLE-AUTO").unwrap();
        assert!(ai < china && china < fallback);
        assert!(!yaml.contains("AI-STRICT\n    type: select\n    proxies:\n      - DIRECT"));
    }

    fn sample_gateway_policy(id: &str, ip: &str, scenario: &str) -> GatewayDevicePolicy {
        GatewayDevicePolicy {
            id: id.to_owned(),
            name: format!("Device {id}"),
            ip: ip.to_owned(),
            mac: "aa:bb:cc:dd:ee:ff".to_owned(),
            scenario: scenario.to_owned(),
            enabled: true,
        }
    }

    #[test]
    fn gateway_device_policies_are_local_unique_and_ordered_after_china_direct() {
        let policies = vec![
            sample_gateway_policy("apple-tv", "192.168.1.20", "streaming"),
            sample_gateway_policy("xbox", "fd00::20", "gaming"),
        ];
        let yaml = gateway_yaml_for_profile_and_policies(&sample_profile(), &policies).unwrap();
        let china = yaml.find("GEOIP,CN,DIRECT").unwrap();
        let apple = yaml
            .find("SRC-IP-CIDR,192.168.1.20/32,DOWNLOAD-AUTO")
            .unwrap();
        let xbox = yaml.find("SRC-IP-CIDR6,fd00::20/128,STABLE-AUTO").unwrap();
        let fallback = yaml.find("MATCH,STABLE-AUTO").unwrap();
        assert!(china < apple && apple < xbox && xbox < fallback);
        assert!(yaml.contains("enhanced-mode: fake-ip"));
        assert!(yaml.contains("nameserver-policy:"));

        let mut public = policies.clone();
        public[0].ip = "8.8.8.8".to_owned();
        assert!(validate_gateway_policies(&public).is_err());
        let mut duplicate = policies.clone();
        duplicate[1].ip = "192.168.1.20".to_owned();
        assert!(validate_gateway_policies(&duplicate).is_err());
        let mut invalid = policies;
        invalid[0].scenario = "arbitrary-shell".to_owned();
        assert!(validate_gateway_policies(&invalid).is_err());
    }

    fn sample_gateway_request(action: &str) -> GatewayDeployRequest {
        GatewayDeployRequest {
            host: "192.168.1.1".to_owned(),
            ssh_port: 22,
            user: "root".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            action: action.to_owned(),
            confirmed: true,
        }
    }

    #[test]
    fn gateway_deploy_rejects_unconfirmed_injected_or_non_root_inputs() {
        let mut request = sample_gateway_request("preview");
        request.confirmed = false;
        assert!(validate_gateway_deploy_request(&request, true).is_err());
        request.confirmed = true;
        request.host = "192.168.1.1; reboot".to_owned();
        assert!(validate_gateway_deploy_request(&request, true).is_err());
        request.host = "192.168.1.1".to_owned();
        request.user = "admin".to_owned();
        assert!(validate_gateway_deploy_request(&request, true).is_err());
        request.user = "root".to_owned();
        request.action = "shell".to_owned();
        assert!(validate_gateway_deploy_request(&request, true).is_err());
        request.action = "apply".to_owned();
        assert!(validate_gateway_deploy_request(&request, false).is_err());
    }

    #[test]
    fn gateway_inspection_is_fixed_read_only_and_merges_ipv4_ipv6_devices() {
        let request = GatewayInspectRequest {
            host: "192.168.1.1".to_owned(),
            ssh_port: 22,
            user: "root".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            confirmed: true,
        };
        let report = inspect_home_gateway_with_runner(&request, true, |args, script| {
            assert!(args.contains(&"root@192.168.1.1".to_owned()));
            assert!(script.contains("/tmp/dhcp.leases"));
            assert!(script.contains("ip -6 neigh show"));
            assert!(!script.contains("uci set"));
            assert!(!script.contains("/etc/init.d/openclash restart"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: concat!(
                    "AI_ROUTE_GATEWAY=openwrt\n",
                    "AI_ROUTE_HOSTNAME=Living-Router\n",
                    "AI_ROUTE_RELEASE=24.10.1\n",
                    "AI_ROUTE_ARCH=aarch64_cortex-a53\n",
                    "AI_ROUTE_MODEL=GL-MT6000\n",
                    "AI_ROUTE_OPENCLASH=1\n",
                    "AI_ROUTE_WIREGUARD=1\n",
                    "AI_ROUTE_OPENVPN=1\n",
                    "AI_ROUTE_IPV6=1\n",
                    "AI_ROUTE_UPNP=1\n",
                    "AI_ROUTE_DEVICE=aa:bb:cc:dd:ee:01|192.168.1.20|AppleTV-Living\n",
                    "AI_ROUTE_DEVICE=aa:bb:cc:dd:ee:01|fd00::20|\n",
                    "AI_ROUTE_DEVICE=aa:bb:cc:dd:ee:02|192.168.1.30|Xbox-Series-X\n",
                )
                .to_owned(),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert!(report.capabilities.openclash);
        assert!(report.capabilities.ipv6_default_route);
        assert_eq!(report.devices.len(), 2);
        assert_eq!(report.devices[0].kind, "apple-tv");
        assert_eq!(report.devices[0].ipv6, vec!["fd00::20"]);
        assert_eq!(report.devices[1].kind, "xbox");
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("fixture-key"));
    }

    #[test]
    fn gateway_preview_uses_fixed_hash_gated_script_without_plaintext_secrets() {
        let profile = sample_profile();
        let config = gateway_yaml_for_profile(&profile).unwrap();
        let expected_hash = sha256_hex(config.as_bytes());
        let request = sample_gateway_request("preview");
        let report = configure_home_gateway_with_runner(&request, true, Some(&profile), &[], |args, script| {
            assert!(args.contains(&"root@192.168.1.1".to_owned()));
            assert!(!args.iter().any(|item| item.contains("secret") || item.contains("test-only")));
            assert!(!script.contains("token=secret"));
            assert!(!script.contains("test-only"));
            assert!(script.contains("EXPECTED_SHA256='"));
            assert!(script.contains("/etc/openclash/config/ai-route-manager.yaml"));
            assert!(script.contains("\"$CORE\" -t -d /etc/openclash -f \"$CANDIDATE\""));
            assert!(!script.contains("curl "));
            assert!(!script.contains("wget "));
            assert!(!script.contains("iptables"));
            assert!(!script.contains("nft "));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: format!(
                    "AI_ROUTE_GATEWAY=previewed\nAI_ROUTE_CORE=clash_meta\nAI_ROUTE_TARGET=/etc/openclash/config/ai-route-manager.yaml\nAI_ROUTE_SERVICE=openclash\nAI_ROUTE_CONFIG_SHA256={expected_hash}\nAI_ROUTE_ROLLBACK=0\n"
                ),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert_eq!(report.action, "preview");
        assert!(!report.rollback_available);
        assert_eq!(
            report.config_sha256.as_deref(),
            Some(expected_hash.as_str())
        );
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("token=secret"));
        assert!(!serialized.contains("test-only"));
        assert!(!serialized.contains("fixture-key"));
    }

    #[test]
    fn gateway_apply_and_rollback_scripts_preserve_one_conflict_checked_restore_point() {
        let profile = sample_profile();
        let expected_hash = sha256_hex(gateway_yaml_for_profile(&profile).unwrap().as_bytes());
        let apply = sample_gateway_request("apply");
        let report = configure_home_gateway_with_runner(&apply, true, Some(&profile), &[], |_, script| {
            assert!(script.contains("printf 'prepared\\n' > \"$STATE/phase\""));
            assert!(script.contains("cp -p \"$TARGET\" \"$STATE/target.before\""));
            assert!(script.contains("uci -q set openclash.config.config_path=\"$TARGET\""));
            assert!(script.contains("/etc/init.d/openclash restart"));
            assert!(script.contains("apply_restart_failed_rolled_back"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: format!(
                    "AI_ROUTE_GATEWAY=applied\nAI_ROUTE_CORE=clash_meta\nAI_ROUTE_TARGET=/etc/openclash/config/ai-route-manager.yaml\nAI_ROUTE_SERVICE=openclash\nAI_ROUTE_CONFIG_SHA256={expected_hash}\nAI_ROUTE_ROLLBACK=1\n"
                ),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert!(report.rollback_available);

        let rollback = sample_gateway_request("rollback");
        let rolled_back = configure_home_gateway_with_runner(&rollback, true, None, &[], |_, script| {
            assert!(script.contains("rollback_conflict"));
            assert!(script.contains("restore_previous_target"));
            assert!(script.contains("rollback_restart_failed_reapplied"));
            assert!(!script.contains("token=secret"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: "AI_ROUTE_GATEWAY=rolled_back\nAI_ROUTE_CORE=clash_meta\nAI_ROUTE_TARGET=/etc/openclash/config/ai-route-manager.yaml\nAI_ROUTE_SERVICE=openclash\nAI_ROUTE_ROLLBACK=0\n".to_owned(),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert!(!rolled_back.rollback_available);
        assert!(rolled_back.config_sha256.is_none());
    }

    #[test]
    fn gateway_script_parses_with_a_real_posix_shell_when_available() {
        let config = gateway_yaml_for_profile(&sample_profile()).unwrap();
        let (script, _) = openclash_gateway_script("preview", Some(&config)).unwrap();
        let path = std::env::temp_dir().join(format!(
            "ai-route-openclash-{}-{}.sh",
            std::process::id(),
            unix_time_ms()
        ));
        fs::write(&path, script).unwrap();
        #[cfg(target_os = "windows")]
        let shell = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        #[cfg(not(target_os = "windows"))]
        let shell = PathBuf::from("sh");
        if shell.is_file() || cfg!(not(target_os = "windows")) {
            let status = hidden_command(&shell)
                .arg("-n")
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn mobile_shadowrocket_export_has_qr_ready_sip002_uri() {
        let bundle = mobile_bundle_for_profile(&sample_profile()).unwrap();
        assert!(bundle.ss_uri.starts_with("ss://"));
        assert!(bundle.ss_uri.contains("@203.0.113.8:443#"));
        assert!(!bundle.ss_uri.contains("test-only"));
        assert!(bundle.shadowrocket_conf.contains("GEOIP,CN,DIRECT"));
        assert!(bundle.shadowrocket_conf.contains("FINAL,STABLE"));
        let svg = qr_svg(&bundle.ss_uri).unwrap();
        assert!(svg.starts_with("<?xml"));
        assert!(!svg.contains("test-only"));
    }

    #[test]
    fn mobile_export_report_never_serializes_credentials() {
        let report = MobileExportReport {
            success: true,
            directory: "C:/Users/example/Downloads/AI-Route-Mobile-Exports/export-1".to_owned(),
            files: vec![MobileExportFile {
                name: "AI-Route-Android.yaml".to_owned(),
                path: "C:/Users/example/Downloads/AI-Route-Mobile-Exports/export-1/AI-Route-Android.yaml".to_owned(),
                kind: "Android / Mihomo".to_owned(),
            }],
            warning: "敏感文件".to_owned(),
            fixed_egress_host: "203.0.113.8".to_owned(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("token=secret"));
        assert!(!json.contains("test-only"));
        assert!(json.contains("203.0.113.8"));
    }

    #[test]
    fn diagnostic_payload_omits_urls_passwords_nodes_and_raw_engine_details() {
        let profile = sample_profile();
        let snapshot = SystemSnapshot {
            os: "windows",
            arch: "x86_64",
            process_id: 42,
            preview_only: true,
            ports: vec![PortStatus {
                label: "主入口",
                host: "127.0.0.1",
                port: 17901,
                reachable: true,
            }],
        };
        let managed = ManagedStatus {
            configured: true,
            running: false,
            listener: false,
            active_proxy_port: None,
            system_proxy: Some("198.51.100.9:8888".to_owned()),
            legacy_conflict: false,
            message: "token=managed-secret".to_owned(),
        };
        let engine = EngineReport {
            available: true,
            executable: Some("C:/secret/path/engine.exe".to_owned()),
            success: false,
            status: Some(serde_json::json!({"password": "engine-secret"})),
            message: "https://example.com/?token=engine-secret".to_owned(),
        };
        let payload = diagnostic_payload(
            Some(&profile),
            &Preferences::default(),
            &[sample_referral_offer()],
            &snapshot,
            &managed,
            &engine,
        );
        let json = serde_json::to_string(&payload).unwrap();
        for secret in [
            "token=secret",
            "test-only",
            "198.51.100.9",
            "managed-secret",
            "engine-secret",
            "C:/secret/path",
        ] {
            assert!(!json.contains(secret), "diagnostic leaked {secret}");
        }
        assert!(json.contains("<non-local-endpoint-redacted>"));
        assert!(json.contains("subscriptionCount"));
    }

    #[test]
    fn local_backup_status_is_secret_free() {
        let backup = LocalBackupEnvelope {
            version: CONFIG_VERSION,
            created_at_ms: 123,
            profile: sample_profile(),
            preferences: Preferences::default(),
            offers: vec![sample_referral_offer()],
            gateway_policies: Vec::new(),
        };
        let json = serde_json::to_string(&backup_status_from(Some(&backup))).unwrap();
        assert!(json.contains("subscriptionCount"));
        assert!(!json.contains("token=secret"));
        assert!(!json.contains("test-only"));
    }

    #[test]
    fn remote_quality_profiles_degrade_with_real_path_conditions() {
        let lan = recommend_remote_quality("192.168.1.10".to_owned(), 47984, 5, 5, 4.0, 0.5, 0.0);
        assert_eq!(lan.recommended_preset, "局域网极致");
        assert_eq!(lan.resolution, "3840×2160");
        let wan = recommend_remote_quality("example.net".to_owned(), 47984, 5, 5, 42.0, 7.0, 1.0);
        assert_eq!(wan.recommended_preset, "远程流畅");
        assert!(wan.score < lan.score);
        let failed =
            recommend_remote_quality("example.net".to_owned(), 47984, 5, 0, 0.0, 0.0, 100.0);
        assert_eq!(failed.score, 0);
        assert_eq!(failed.recommended_preset, "不可达");
    }

    fn sample_sunshine_request(encoder: &str) -> SunshineConfigRequest {
        SunshineConfigRequest {
            action: "apply".to_owned(),
            bitrate_mbps: 35,
            encoder: encoder.to_owned(),
            confirmed: true,
        }
    }

    #[test]
    fn sunshine_profile_merge_preserves_unknowns_and_replaces_managed_duplicates() {
        let current = "# user setting\r\nport = 48000\r\nmax_bitrate = 5000\r\nencoder = software\r\nmax_bitrate = 6000\r\n";
        let (merged, values) =
            merge_sunshine_config(current, &sample_sunshine_request("nvenc"), "windows").unwrap();
        assert!(merged.contains("port = 48000"));
        assert_eq!(merged.matches("max_bitrate =").count(), 1);
        assert_eq!(merged.matches("encoder =").count(), 1);
        assert!(merged.contains("max_bitrate = 35000"));
        assert!(merged.contains("encoder = nvenc"));
        assert!(merged.contains("capture = ddx"));
        assert!(merged.contains("upnp = disabled"));
        assert!(merged.contains("nvenc_twopass = quarter_res"));
        assert!(merged.contains("\r\n"));
        assert!(values.contains(&"max_bitrate=35000".to_owned()));
    }

    #[test]
    fn sunshine_auto_encoder_clears_stale_forcing_without_nvenc_tuning() {
        let current = "encoder = software\nnvenc_preset = 7\n";
        let (merged, _) =
            merge_sunshine_config(current, &sample_sunshine_request("auto"), "macos").unwrap();
        assert!(
            !merged
                .lines()
                .any(|line| line.trim() == "encoder = software")
        );
        assert!(!merged.contains("nvenc_preset ="));
        assert!(!merged.contains("capture = ddx"));
        assert!(merged.contains("# encoder = auto"));
    }

    #[test]
    fn sunshine_profile_transaction_roundtrips_and_rejects_external_changes() {
        let root = std::env::temp_dir().join(format!(
            "ai-route-sunshine-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        fs::create_dir_all(&root).unwrap();
        let config = root.join("sunshine.conf");
        let candidate = root.join("candidate.conf");
        let backup = root.join("backups/original.conf");
        let original = b"port = 48000\nmax_bitrate = 5000\n";
        fs::write(&config, original).unwrap();
        let request = sample_sunshine_request("nvenc");
        let (merged, _) =
            merge_sunshine_config(std::str::from_utf8(original).unwrap(), &request, "windows")
                .unwrap();
        fs::write(&candidate, merged.as_bytes()).unwrap();
        let before = sha256_hex(original);
        let after = sha256_hex(merged.as_bytes());
        commit_sunshine_direct("apply", &config, &candidate, &backup, Some(&before), &after)
            .unwrap();
        assert_eq!(
            current_file_hash(&config).unwrap().as_deref(),
            Some(after.as_str())
        );
        assert_eq!(fs::read(&backup).unwrap(), original);

        fs::write(&config, b"user changed this after apply\n").unwrap();
        assert!(
            commit_sunshine_direct(
                "rollback",
                &config,
                Path::new(""),
                &backup,
                Some(&before),
                &after,
            )
            .unwrap_err()
            .contains("拒绝覆盖")
        );
        fs::write(&config, merged.as_bytes()).unwrap();
        commit_sunshine_direct(
            "rollback",
            &config,
            Path::new(""),
            &backup,
            Some(&before),
            &after,
        )
        .unwrap();
        assert_eq!(fs::read(&config).unwrap(), original);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sunshine_elevated_script_is_hash_gated_and_network_neutral() {
        let script = sunshine_elevated_script();
        assert!(script.contains("Get-FileHash"));
        assert!(script.contains("AI_ROUTE_SUNSHINE_BEFORE"));
        assert!(script.contains("AI_ROUTE_SUNSHINE_AFTER"));
        assert!(script.contains("Copy-Item -LiteralPath $config -Destination $backup"));
        assert!(script.contains("Move-Item -LiteralPath $temporary -Destination $config -Force"));
        for forbidden in [
            "ProxyServer",
            "Internet Settings",
            "New-NetFirewallRule",
            "netsh",
            "route add",
        ] {
            assert!(
                !script.contains(forbidden),
                "unexpected side effect: {forbidden}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn sunshine_elevated_script_parses_in_windows_powershell() {
        let path = std::env::temp_dir().join(format!(
            "ai-route-sunshine-parse-{}-{}.ps1",
            std::process::id(),
            unix_time_ms()
        ));
        fs::write(&path, sunshine_elevated_script().as_bytes()).unwrap();
        let command = "$tokens=$null;$errors=$null;[System.Management.Automation.Language.Parser]::ParseFile($env:AI_ROUTE_PARSE_FILE,[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){$errors|ForEach-Object{Write-Error $_};exit 2}";
        let output = hidden_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .env("AI_ROUTE_PARSE_FILE", &path)
            .output()
            .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn remote_host_validation_rejects_urls_and_shell_syntax() {
        assert_eq!(
            validate_remote_host("192.168.1.20").unwrap(),
            "192.168.1.20"
        );
        assert!(validate_remote_host("https://example.com").is_err());
        assert!(validate_remote_host("example.com && calc.exe").is_err());
    }

    #[test]
    fn remote_installer_allowlist_rejects_arbitrary_packages() {
        assert_eq!(
            remote_package_id("sunshine").unwrap(),
            "LizardByte.Sunshine"
        );
        assert_eq!(
            remote_package_id("moonlight").unwrap(),
            "MoonlightGameStreamingProject.Moonlight"
        );
        assert!(remote_package_id("RustDesk.RustDesk").is_err());
        assert!(remote_package_id("../../arbitrary").is_err());
    }

    #[test]
    fn rustdesk_deploy_rejects_unconfirmed_or_injected_inputs() {
        let base = RustDeskDeployRequest {
            host: "vps.example.com".to_owned(),
            ssh_port: 22,
            user: "deploy".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            confirmed: false,
        };
        assert!(validate_rustdesk_deploy_request(&base, true).is_err());
        let mut injected = base;
        injected.confirmed = true;
        injected.host = "host;touch /tmp/pwned".to_owned();
        assert!(validate_rustdesk_deploy_request(&injected, true).is_err());
        injected.host = "vps.example.com".to_owned();
        injected.user = "root && id".to_owned();
        assert!(validate_rustdesk_deploy_request(&injected, true).is_err());
        injected.user = "root".to_owned();
        assert!(validate_rustdesk_deploy_request(&injected, false).is_err());
    }

    #[test]
    fn rustdesk_deploy_fake_runner_receives_fixed_script_and_returns_only_public_key() {
        let request = RustDeskDeployRequest {
            host: "203.0.113.9".to_owned(),
            ssh_port: 2222,
            user: "deploy".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            confirmed: true,
        };
        let report = deploy_rustdesk_with_runner(&request, true, |args, script| {
            assert!(args.contains(&"deploy@203.0.113.9".to_owned()));
            assert!(args.contains(&"2222".to_owned()));
            assert!(script.contains("rustdesk/rustdesk-server:1.1.16"));
            assert!(!script.contains("rustdesk/rustdesk-server:latest"));
            assert!(script.contains("command: hbbs"));
            assert!(script.contains("command: hbbr"));
            assert!(script.contains("21116/tcp+udp"));
            assert!(script.contains("data/id_ed25519.pub"));
            assert!(!script.contains("< data/id_ed25519\n"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: "AI_ROUTE_STATUS=running\nAI_ROUTE_PUBLIC_KEY=ABCDEFGHIJKLMNOPQRSTUVWX1234567890=\n".to_owned(),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert!(report.success);
        assert_eq!(report.id_server, "203.0.113.9:21116");
        assert_eq!(report.relay_server, "203.0.113.9:21117");
        assert_eq!(report.status, "running");
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("identity_file"));
        assert!(!serialized.contains("id_ed25519"));
    }

    #[test]
    fn sunshine_firewall_is_minimal_private_scope_and_reversible() {
        let rules = sunshine_firewall_rules("local-subnet").unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].ports, "47984,47989,47990,48010");
        assert_eq!(rules[1].ports, "47998-48000");
        assert!(rules.iter().all(|rule| rule.scope.contains("LocalSubnet")));
        let apply = sunshine_firewall_script("apply", "local-subnet").unwrap();
        assert!(apply.contains("-Profile Private"));
        assert!(apply.contains("-RemoteAddress 'LocalSubnet'"));
        assert!(!apply.contains("RemoteAddress 'Any'"));
        let tailnet = sunshine_firewall_script("apply", "tailnet").unwrap();
        assert!(tailnet.contains("100.64.0.0/10"));
        let rollback = sunshine_firewall_script("rollback", "local-subnet").unwrap();
        assert!(rollback.contains("Remove-NetFirewallRule"));
        assert!(!rollback.contains("New-NetFirewallRule"));
        assert!(sunshine_firewall_script("apply", "public").is_err());
        assert!(sunshine_firewall_script("preview", "local-subnet").is_err());
    }

    #[test]
    fn rustdesk_firewall_requires_confirmation_and_uses_only_minimum_ports() {
        let mut request = RustDeskFirewallRequest {
            host: "203.0.113.9".to_owned(),
            ssh_port: 22,
            user: "deploy".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            action: "apply".to_owned(),
            confirmed: false,
        };
        assert!(
            configure_rustdesk_firewall_with_runner(&request, true, |_, _| { unreachable!() })
                .is_err()
        );
        request.confirmed = true;
        let report = configure_rustdesk_firewall_with_runner(&request, true, |args, script| {
            assert!(args.contains(&"deploy@203.0.113.9".to_owned()));
            assert!(script.contains("21115/tcp"));
            assert!(script.contains("21116/tcp"));
            assert!(script.contains("21116/udp"));
            assert!(script.contains("21117/tcp"));
            assert!(!script.contains("21118"));
            assert!(!script.contains("21119"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: "AI_ROUTE_FIREWALL=applied\n".to_owned(),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert_eq!(report.rules.len(), 3);
        assert!(report.message.contains("21118/21119"));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("identity_file"));
        assert!(!serialized.contains("fixture-key"));
    }

    #[test]
    fn rustdesk_firewall_rollback_is_fixed_and_idempotent() {
        let request = RustDeskFirewallRequest {
            host: "vps.example.com".to_owned(),
            ssh_port: 2222,
            user: "root".to_owned(),
            identity_file: std::env::current_dir()
                .unwrap()
                .join("fixture-key")
                .to_string_lossy()
                .into_owned(),
            action: "rollback".to_owned(),
            confirmed: true,
        };
        let report = configure_rustdesk_firewall_with_runner(&request, true, |_, script| {
            assert!(script.contains("ufw --force delete allow 21115/tcp"));
            assert!(script.contains("|| true"));
            assert!(!script.contains("ufw allow 21115/tcp comment"));
            Ok(SshRunResult {
                success: true,
                exit_code: Some(0),
                stdout: "AI_ROUTE_FIREWALL=rolled_back\n".to_owned(),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert_eq!(report.action, "rollback");
    }

    #[test]
    fn remote_path_probe_measures_a_real_local_tcp_listener() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = thread::spawn(move || {
            for _ in 0..3 {
                let _ = listener.accept().unwrap();
            }
        });
        let report = probe_remote_path_inner(&RemoteProbeRequest {
            host: "127.0.0.1".to_owned(),
            port,
            samples: 3,
        })
        .unwrap();
        acceptor.join().unwrap();
        assert_eq!(report.successful_samples, 3);
        assert_eq!(report.loss_percent, 0.0);
        assert!(report.average_latency_ms >= 0.0);
    }

    #[test]
    fn supervisor_only_recovers_committed_managed_mode_with_backoff() {
        let mut preferences = Preferences::default();
        let mut status = ManagedStatus {
            configured: true,
            running: false,
            listener: false,
            active_proxy_port: None,
            system_proxy: None,
            legacy_conflict: false,
            message: "fixture".to_owned(),
        };
        assert!(!supervisor_should_recover(&preferences, &status));
        preferences.mode = "managed".to_owned();
        assert!(supervisor_should_recover(&preferences, &status));
        status.listener = true;
        assert!(!supervisor_should_recover(&preferences, &status));
        status.listener = false;
        status.legacy_conflict = true;
        assert!(!supervisor_should_recover(&preferences, &status));
        assert_eq!(supervisor_backoff_seconds(1), 15);
        assert_eq!(supervisor_backoff_seconds(3), 60);
        assert_eq!(supervisor_backoff_seconds(99), 300);
    }

    #[test]
    fn release_diagnostics_only_lists_supported_ports_and_exposes_repair() {
        let snapshot = get_system_snapshot();
        assert!(!snapshot.ports.iter().any(|port| port.port == 18443));
        assert!(
            snapshot
                .ports
                .iter()
                .any(|port| port.port == STABLE_FRONTEND_PORT)
        );
        let source = include_str!("lib.rs");
        assert!(source.contains("fn repair_supervisor("));
        assert!(source.contains("launch_headless_supervisor_process"));
        assert!(source.contains("error.raw_os_error() == Some(33)"));
    }

    #[test]
    fn committed_migration_treats_legacy_listener_as_recovery_backup() {
        assert!(legacy_conflict_for_state(true, None));
        assert!(legacy_conflict_for_state(true, Some("prepared")));
        assert!(!legacy_conflict_for_state(true, Some("managed")));
        assert!(!legacy_conflict_for_state(false, None));
    }

    #[test]
    fn managed_status_reads_state_pid_and_listener_without_spawning_engine_status() {
        use std::net::TcpListener;
        let temporary = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fs::write(
            temporary.path().join("state.json"),
            serde_json::to_vec(&serde_json::json!({
                "manager_pid": std::process::id(),
                "active_proxy_port": port,
            }))
            .unwrap(),
        )
        .unwrap();
        let status = managed_engine_status_value_from_home(temporary.path()).unwrap();
        assert_eq!(status["manager_running"], true);
        assert_eq!(status["listener"], true);
        assert_eq!(status["state"]["active_proxy_port"], port);
    }

    #[test]
    fn windows_children_and_background_entrypoint_are_windowless_by_construction() {
        let source = include_str!("lib.rs");
        let constructor = ["Command::", "new("].concat();
        assert_eq!(source.matches(&constructor).count(), 1);
        assert!(source.contains("command.creation_flags(CREATE_NO_WINDOW)"));
        assert!(source.contains("fn run_headless_supervisor()"));
        assert!(source.contains("try_lock_exclusive()"));
        let main = include_str!("main.rs");
        let headless = main.find("run_background_if_requested()").unwrap();
        let tauri = main.find("ai_route_manager_lib::run()").unwrap();
        assert!(headless < tauri);
    }

    #[test]
    fn software_owns_stable_frontdoor_and_windows_compatibility() {
        let source = include_str!("lib.rs");
        assert!(source.contains("const MANAGED_FRONTEND_PORT: &str = \"17901\";"));
        assert!(source.contains("fn run_stable_frontdoor(root: PathBuf)"));
        assert!(source.contains("client.set_nonblocking(false)"));
        assert!(source.contains("fn sync_windows_compatibility(root: &Path)"));
        assert!(source.contains("setting-scope=user"));
        assert!(source.contains("r\"HKCU\\Environment\""));
        assert!(!source.contains("const MANAGED_FRONTEND_PORT: &str = \"0\";"));
    }

    #[test]
    fn restart_definitions_are_least_privilege_and_escape_paths() {
        let windows = windows_supervisor_task_xml(
            std::path::Path::new(r"C:\Program Files\AI & Route\AI Route Manager.exe"),
            "S-1-5-21-123",
        );
        assert!(windows.contains("<RestartOnFailure><Interval>PT1M</Interval><Count>999</Count>"));
        assert!(windows.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(windows.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(windows.contains("<Hidden>true</Hidden>"));
        assert!(!windows.contains("<Hidden>false</Hidden>"));
        assert!(windows.contains("AI &amp; Route"));
        assert!(!windows.contains("HighestAvailable"));
        assert_eq!(&windows_task_xml_bytes(&windows)[..2], &[0xff, 0xfe]);
        let mac = macos_supervisor_plist(std::path::Path::new(
            "/Applications/AI & Route.app/AI Route",
        ));
        assert!(mac.contains("<key>KeepAlive</key>"));
        assert!(mac.contains("<key>SuccessfulExit</key><false/>"));
        assert!(mac.contains("<key>ThrottleInterval</key><integer>15</integer>"));
        assert!(mac.contains("AI &amp; Route.app"));
        let watchdog = windows_watchdog_vbs(
            std::path::Path::new(r"C:\Program Files\AI Route\app.exe"),
            std::path::Path::new(r"C:\Users\Fixture\supervisor.enabled"),
        );
        assert!(watchdog.contains("If Not running Then shell.Run command, 0, False"));
        assert!(watchdog.contains("WScript.Sleep 15000"));
        assert!(watchdog.contains("LCase(CStr(process.ExecutablePath)) = target"));
        assert!(watchdog.contains("--background"));
        assert!(watchdog.contains("supervisor.enabled"));
        assert_eq!(
            parse_windows_user_sid(r#""DESKTOP\User","S-1-5-21-123""#).as_deref(),
            Some("S-1-5-21-123")
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_task_scheduler_schema_is_valid_when_explicitly_requested() {
        if std::env::var_os("AI_ROUTE_VALIDATE_TASK_XML").is_none() {
            return;
        }
        let whoami = hidden_command("whoami.exe")
            .args(["/USER", "/FO", "CSV", "/NH"])
            .output()
            .unwrap();
        assert!(whoami.status.success());
        let sid = parse_windows_user_sid(&String::from_utf8_lossy(&whoami.stdout)).unwrap();
        let xml = windows_supervisor_task_xml(&std::env::current_exe().unwrap(), &sid);
        let path =
            std::env::temp_dir().join(format!("ai-route-task-test-{}.xml", std::process::id()));
        fs::write(&path, windows_task_xml_bytes(&xml)).unwrap();
        let task_name = format!("AI Route Manager XML Test {}", std::process::id());
        let created = hidden_command("schtasks.exe")
            .args(["/Create", "/TN", &task_name, "/XML"])
            .arg(&path)
            .arg("/F")
            .output()
            .unwrap();
        let _ = fs::remove_file(&path);
        if created.status.success() {
            let deleted = hidden_command("schtasks.exe")
                .args(["/Delete", "/TN", &task_name, "/F"])
                .output()
                .unwrap();
            assert!(deleted.status.success());
        }
        let detail = String::from_utf8_lossy(&created.stderr);
        assert!(!detail.contains("XML is malformed"), "{detail}");
        assert!(!detail.contains("无法切换编码"), "{detail}");
        assert!(
            created.status.success()
                || detail.contains("Access is denied")
                || detail.contains("拒绝访问"),
            "unexpected scheduler result: {detail}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_autostart_fallback_roundtrip_when_explicitly_requested() {
        if std::env::var_os("AI_ROUTE_VALIDATE_AUTOSTART").is_none() {
            return;
        }
        let existing = hidden_command("reg.exe")
            .args([
                "QUERY",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "AIRouteManager",
            ])
            .output()
            .unwrap();
        assert!(
            !existing.status.success(),
            "existing startup value must not be overwritten"
        );
        let executable = std::env::current_exe().unwrap();
        let enabled = configure_windows_auto_start(&executable, true);
        let (marker, watchdog, backend) = windows_supervisor_files().unwrap();
        let backend_value = fs::read_to_string(&backend).unwrap_or_default();
        let registry = hidden_command("reg.exe")
            .args([
                "QUERY",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "AIRouteManager",
            ])
            .output()
            .unwrap();
        let disabled = configure_windows_auto_start(&executable, false);
        assert!(enabled.is_ok(), "{enabled:?}");
        assert!(disabled.is_ok(), "{disabled:?}");
        assert!(backend_value == "scheduled-task\n" || backend_value == "run-key-watchdog\n");
        if backend_value == "run-key-watchdog\n" {
            assert!(registry.status.success());
            let value = decode_windows_text(&registry.stdout).to_ascii_lowercase();
            assert!(value.contains("wscript.exe"));
            assert!(value.contains("supervisor.vbs"));
        }
        assert!(!marker.exists());
        assert!(!watchdog.exists());
        assert!(!backend.exists());
        let after = hidden_command("reg.exe")
            .args([
                "QUERY",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "AIRouteManager",
            ])
            .output()
            .unwrap();
        assert!(!after.status.success());
    }

    #[test]
    fn scheduled_task_xml_defaults_to_enabled_and_preserves_explicit_disable() {
        assert!(scheduled_task_enabled_from_xml("<Task><Settings/></Task>"));
        assert!(scheduled_task_enabled_from_xml("<Enabled>true</Enabled>"));
        assert!(!scheduled_task_enabled_from_xml("<Enabled>false</Enabled>"));
    }

    #[test]
    fn local_network_report_filters_public_neighbors_and_keeps_private_devices() {
        let report = parse_local_network_report(br#"{"gateway":"192.168.50.1","localIp":"192.168.50.43","devices":[{"id":"local","name":"PC","mac":"aa","ipv4":"192.168.50.43","ipv6":[],"kind":"computer","online":true},{"id":"public","name":"public","mac":"bb","ipv4":"8.8.8.8","ipv6":[],"kind":"device","online":false}]}"#).unwrap();
        assert_eq!(report.devices.len(), 1);
        assert_eq!(report.devices[0].id, "local");
        assert!(
            parse_local_network_report(
                br#"{"gateway":"8.8.8.8","localIp":"192.168.50.43","devices":[]}"#
            )
            .is_err()
        );
    }
    fn sample_referral_offer() -> ReferralOffer {
        ReferralOffer {
            id: "sg-vps".to_owned(),
            category: "vps".to_owned(),
            placement: "vps".to_owned(),
            provider: "Example Cloud".to_owned(),
            title: "固定出口 VPS".to_owned(),
            summary: "测试推荐".to_owned(),
            badge: "推荐".to_owned(),
            price: "$5".to_owned(),
            url: "https://example.com/ref/code".to_owned(),
            coupon: "SAVE".to_owned(),
            disclosure: "此链接为推广链接，推荐人可能获得返利。".to_owned(),
            enabled: true,
            priority: 80,
            click_count: 0,
            origin: "local".to_owned(),
            catalog_version: None,
        }
    }

    #[test]
    fn referral_validation_accepts_disclosed_https_links() {
        assert!(validate_referral_offers_inner(&[sample_referral_offer()]).valid);
    }

    #[test]
    fn referral_validation_rejects_credentials_duplicates_and_missing_disclosure() {
        let mut invalid = sample_referral_offer();
        invalid.url = "https://name:password@example.com/ref".to_owned();
        invalid.disclosure = "普通购买链接".to_owned();
        let result = validate_referral_offers_inner(&[invalid.clone(), invalid]);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|item| item.contains("HTTPS")));
        assert!(result.errors.iter().any(|item| item.contains("披露")));
        assert!(result.errors.iter().any(|item| item.contains("重复")));
    }

    #[test]
    fn referral_validation_rejects_https_without_a_host() {
        let mut invalid = sample_referral_offer();
        invalid.url = "https://?missing-host".to_owned();
        let result = validate_referral_offers_inner(&[invalid]);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|item| item.contains("HTTPS")));
    }

    #[test]
    fn signed_publisher_referral_catalog_verifies_with_embedded_key() {
        let envelope = include_str!("../testdata/referral-catalog-valid.json");
        let report = verify_referral_catalog_inner(envelope).expect("signed fixture should verify");
        assert!(report.signature_verified);
        assert_eq!(report.catalog_version, "test-1");
        assert_eq!(report.offers.len(), 1);
        assert_eq!(report.offers[0].origin, "publisher");
        assert_eq!(report.offers[0].catalog_version.as_deref(), Some("test-1"));
    }

    #[test]
    fn release_publisher_referral_catalog_verifies_when_requested() {
        if std::env::var("AI_ROUTE_VERIFY_REFERRAL_RELEASE").as_deref() != Ok("1") {
            return;
        }
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let catalog = fs::read_to_string(
            manifest_dir
                .parent()
                .expect("project root")
                .join("release")
                .join("referrals")
                .join("latest.json"),
        )
        .expect("read release referral catalog");
        let report =
            verify_referral_catalog_inner(&catalog).expect("verify release referral catalog");
        assert!(report.signature_verified);
        assert!(!report.catalog_version.is_empty());
    }

    #[test]
    fn publisher_referral_catalog_rejects_tampering_and_unknown_schema() {
        let original = include_str!("../testdata/referral-catalog-valid.json");
        let mut envelope: serde_json::Value = serde_json::from_str(original).unwrap();
        let mut payload = STANDARD
            .decode(envelope["payload"].as_str().unwrap())
            .unwrap();
        let marker = payload
            .windows(b"Test Provider".len())
            .position(|window| window == b"Test Provider")
            .unwrap();
        payload[marker] = b'X';
        envelope["payload"] = serde_json::Value::String(STANDARD.encode(payload));
        let tampered = serde_json::to_string(&envelope).unwrap();
        assert!(
            verify_referral_catalog_inner(&tampered)
                .unwrap_err()
                .contains("签名校验失败")
        );

        envelope = serde_json::from_str(original).unwrap();
        envelope["schemaVersion"] = serde_json::Value::from(2);
        let unsupported = serde_json::to_string(&envelope).unwrap();
        assert!(
            verify_referral_catalog_inner(&unsupported)
                .unwrap_err()
                .contains("不支持")
        );
    }

    #[test]
    fn platform_engine_directory_is_explicit() {
        #[cfg(target_os = "windows")]
        assert_eq!(platform_engine_directory_name(), "engine");
        #[cfg(target_os = "macos")]
        assert_eq!(platform_engine_directory_name(), "engine-macos");
    }

    #[test]
    fn macos_platform_config_replaces_windows_engine_resources() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.macos.conf.json"))
                .expect("macOS Tauri config must be valid JSON");
        let resources = config["bundle"]["resources"]
            .as_array()
            .expect("macOS bundle.resources must be an array");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], "resources/engine-macos/**/*");
        assert_eq!(config["bundle"]["macOS"]["minimumSystemVersion"], "11.0");
        assert_eq!(config["bundle"]["macOS"]["hardenedRuntime"], true);
    }

    #[test]
    fn background_start_flag_is_exact() {
        assert!(background_start_requested_from(["app", "--background"]));
        assert!(!background_start_requested_from(["app", "background"]));
        assert!(managed_repair_requested_from(["app", "--repair-managed"]));
        assert!(!managed_repair_requested_from(["app", "repair-managed"]));
    }
}
