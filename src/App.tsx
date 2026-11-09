import { useEffect, useMemo, useState } from "react";
import {
  Activity, ArchiveRestore, ArrowRight, Bot, Boxes, Cable, Check, ChevronRight, Coffee,
  CircleDollarSign, Cloud, Cpu, Database, Download, ExternalLink, Gamepad2, Gauge,
  FileKey2, Globe2, HeartPulse, Home, Laptop, LockKeyhole, MonitorSmartphone,
  Network, PackageOpen, Play, Plus, Radio, Router, Save, Server,
  Settings, ShieldCheck, ShoppingBag, Signal, Smartphone, Sparkles, TerminalSquare,
  Tv, WandSparkles, Wifi, X,
} from "lucide-react";
import "./App.css";
import { SupportQr } from "./SupportQr";
import { getLocalNetworkDevices, type LocalNetworkReport } from "./lib/backend";
import {
  defaultOffers, defaultPreferences, defaultServices, officialReferralCatalogEndpoint,
} from "./data";
import { gatewayScenarioOptions, mergeGatewayDiscovery } from "./lib/gateway";
import { buildRoutePlan, normalizeConnectionProfileForSave, normalizeReferralOffers, referralDestinationHost, referralsForPlacement, validateOffer } from "./lib/planner";
import { normalizeUpdateEndpoint, updateEndpointError, updateProgressLabel } from "./lib/updater";
import { settleTask } from "./lib/startup";
import {
  checkForUpdates, configureHomeGateway, configureLocalStreamingFirewall, configureRustDeskFirewall, configureSunshineStreaming, createLocalBackup, deployRustDeskServer, exportDiagnostics, exportMobileBundle, exportRemoteProfile, getConnectionProfileSummary, getEngineStatus, getLocalBackupStatus, getManagedStatus, getRemoteEnvironment, getSunshineConfigStatus, getSupervisorStatus, getSystemSnapshot, importExistingProfile, inspectHomeGateway, installPendingUpdate, installRemoteTool,
  loadGatewayPolicies, loadPreferences, loadPublisherReferralCatalog, loadReferralOffers, migrateLegacyMode, openExternalUrl, probeRemotePath, repairSupervisor, restoreLocalBackup, runEnginePreview, saveConnectionProfile, saveGatewayPolicies, savePreferences, saveReferralOffers, startManagedMode, stopManagedMode,
  type ConnectionProfile, type DiagnosticExportReport, type EngineReport, type FirewallActionReport, type GatewayDeployReport, type GatewayDevicePolicy, type GatewayInspectReport, type GatewayScenario, type LocalBackupStatus, type ManagedStatus, type MobileExportReport, type PreviewReport, type ProfileSummary, type PublisherReferralCatalogReport, type RemoteEnvironmentReport, type RemoteQualityReport, type RustDeskDeployReport, type SunshineConfigActionReport, type SunshineConfigStatus, type SupervisorStatus, type SystemSnapshot, type UpdateCheckReport,
} from "./lib/backend";
import type { AppPreferences, NavId, ReferralOffer, ServicePolicy } from "./types";

const navigation: Array<{ id: NavId; label: string; icon: typeof Home }> = [
  { id: "dashboard", label: "总览", icon: Home },
  { id: "sources", label: "线路与 VPS", icon: Server },
  { id: "services", label: "场景分流", icon: Boxes },
  { id: "devices", label: "设备中心", icon: MonitorSmartphone },
  { id: "remote", label: "远程控制", icon: Radio },
  { id: "marketplace", label: "推荐中心", icon: CircleDollarSign },
  { id: "diagnostics", label: "诊断中心", icon: Activity },
  { id: "settings", label: "设置", icon: Settings },
];

const scenarios = ["日常模式", "AI 工作", "Steam 游戏", "Apple TV 观影", "大文件下载"];

const placementLabels: Record<ReferralOffer["placement"], string> = {
  vps: "固定出口 VPS",
  "stable-vpn": "稳定路线出口",
  "download-vpn": "大流量线路",
  "remote-control": "远程控制",
  general: "通用推荐",
};

function usePersistentState<T>(key: string, initial: T) {
  const [value, setValue] = useState<T>(() => {
    try {
      const stored = window.localStorage.getItem(key);
      return stored ? JSON.parse(stored) as T : initial;
    } catch {
      return initial;
    }
  });
  useEffect(() => {
    try {
      window.localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // The desktop backend remains authoritative when WebView storage is unavailable.
    }
  }, [key, value]);
  return [value, setValue] as const;
}

function StatusDot({ status }: { status: string }) {
  return <span className={`status-dot status-${status}`} />;
}

function PageHeader({ eyebrow, title, description, action }: { eyebrow: string; title: string; description: string; action?: React.ReactNode }) {
  return <header className="page-header"><div><span className="eyebrow">{eyebrow}</span><h1>{title}</h1><p>{description}</p></div>{action && <div className="page-action">{action}</div>}</header>;
}

function Dashboard({ preferences, setPreferences, services, snapshot, managed, managedBusy, managedMessage, onManagedAction }: { preferences: AppPreferences; setPreferences: (value: AppPreferences) => void; services: ServicePolicy[]; snapshot: SystemSnapshot | null; managed: ManagedStatus | null; managedBusy: boolean; managedMessage: string; onManagedAction: () => void }) {
  const [profileSummary, setProfileSummary] = useState<ProfileSummary | null>(null);
  const [chainExpanded, setChainExpanded] = useState(false);
  useEffect(() => { void getConnectionProfileSummary().then(setProfileSummary); }, []);
  const routePlan = useMemo(() => buildRoutePlan(services, preferences), [services, preferences]);
  const entryPort = managed?.running && managed.activeProxyPort ? managed.activeProxyPort : 17901;
  const mainEntry = snapshot?.ports.find((item) => item.port === entryPort);
  const onlinePorts = snapshot?.ports.filter((item) => item.reachable).length;
  const configuredDownloads = profileSummary?.sources.filter((source) => source.role.startsWith("download")).length ?? 0;
  return <>
    <PageHeader eyebrow="控制中心" title="网络控制中心" description="先配置、再预演、最后接管；停止托管会恢复原系统代理。" action={<button className={managed?.running ? "button button-secondary" : "button button-primary"} onClick={onManagedAction} disabled={managedBusy}><Play size={16} fill="currentColor" /> {managedBusy ? "正在执行完整验证…" : managed?.running ? "停止并恢复" : managed?.legacyConflict ? "无中断迁移" : "验证并应用"}</button>} />
    <section className="safety-banner"><div className="safety-icon"><ShieldCheck size={22} /></div><div><strong>{managed?.running ? "托管自愈模式" : managed?.legacyConflict ? "等待无中断迁移" : "安全预览模式"}</strong><span>{managedMessage || managed?.message || "所有编辑都先在隔离端口验证，当前网络不会被直接重启。"}</span></div><span className={`soft-badge ${managed?.running ? "success" : ""}`}>{managed?.running ? "运行中" : "受保护"}</span></section>
    <section className="scenario-strip"><div className="scenario-label"><WandSparkles size={17} /> 当前场景</div>{scenarios.map((scenario) => <button key={scenario} className={preferences.activeScenario === scenario ? "scenario active" : "scenario"} onClick={() => setPreferences({ ...preferences, activeScenario: scenario })}>{scenario}</button>)}</section>
    <section className="metric-grid">
      <article className="metric-card accent-blue"><div className="metric-top"><Bot size={19} /><span>AI 目标出口</span></div><strong>{profileSummary?.aiRegionLabel ?? "待配置"}</strong><small><StatusDot status={profileSummary ? "healthy" : "idle"} /> {profileSummary ? `${profileSummary.aiRegion} · 由健康检查验证` : "选择适合自己的地区"}</small></article>
      <article className="metric-card accent-green"><div className="metric-top"><Gauge size={19} /><span>稳定入口</span></div><strong>{entryPort}</strong><small><StatusDot status={managed?.listener || mainEntry?.reachable ? "healthy" : "idle"} /> {managed?.running ? "托管核心正在监听" : snapshot ? (mainEntry?.reachable ? "旧版入口正在监听" : "本机端口未监听") : "等待桌面端实测"}</small></article>
      <article className="metric-card accent-purple"><div className="metric-top"><Download size={19} /><span>下载线路</span></div><strong>{profileSummary ? `${configuredDownloads} 个已配置` : "待配置"}</strong><small><StatusDot status={configuredDownloads > 0 ? "healthy" : "idle"} /> {configuredDownloads > 0 ? "自动选择最低成本" : "可选添加下载订阅"}</small></article>
      <article className="metric-card accent-orange"><div className="metric-top"><MonitorSmartphone size={19} /><span>本机入口</span></div><strong>{snapshot ? `${onlinePorts} / ${snapshot.ports.length}` : "待检测"}</strong><small><StatusDot status={snapshot ? "healthy" : "idle"} /> {snapshot ? `${snapshot.os} · ${snapshot.arch}` : "启动桌面端后读取"}</small></article>
    </section>
    <section className="content-grid two-one">
      <article className="panel"><div className="panel-heading"><div><span className="eyebrow">实时策略</span><h2>流量会走到哪里</h2></div></div><div className="route-list">{routePlan.slice(0, 6).map((item) => <div className="route-row" key={item.service}><span className="route-service">{item.service}</span><span className="route-line" /><span className="route-target">{item.target}</span><span className={item.failClosed ? "policy-chip strict" : "policy-chip"}>{item.failClosed ? "严格" : "自动"}</span></div>)}</div></article>
      <article className="panel dark-panel"><div className="panel-heading"><div><span className="eyebrow">出口链</span><h2>AI 当前路径</h2></div><Signal size={20} /></div><div className="path-stack"><div className="path-node"><Laptop size={17} /><span>本机应用</span><small>ChatGPT / Claude</small></div><div className="path-connector" /><div className="path-node"><Network size={17} /><span>稳定入口</span><small>{managed?.listener ? `127.0.0.1:${entryPort} · 正在监听` : "尚未接管"}</small></div><div className="path-connector" /><div className="path-node active"><Cloud size={17} /><span>{profileSummary?.fixedEgressConfigured ? "固定 VPS" : "线路策略出口"}</span><small>{profileSummary?.configured ? `${profileSummary.aiRegionLabel || profileSummary.aiRegion} · 已配置` : "尚未配置线路"}</small></div></div><button className="button button-ghost full" onClick={() => setChainExpanded((value) => !value)}>{chainExpanded ? "收起完整链路" : "查看完整链路"} <ArrowRight size={15} /></button></article>
    </section>
    {chainExpanded && <section className="panel chain-detail"><div className="panel-heading"><div><span className="eyebrow">完整链路</span><h2>当前配置与运行实测</h2></div><span className={`soft-badge ${managed?.listener ? "success" : ""}`}>{managed?.listener ? "入口可达" : "入口未监听"}</span></div><div className="route-list">{routePlan.map((item) => <div className="route-row" key={`full-${item.service}`}><span className="route-service">{item.service}</span><span className="route-line" /><span className="route-target">{item.target}</span><span className={item.failClosed ? "policy-chip strict" : "policy-chip"}>{item.failClosed ? "严格" : "自动"}</span></div>)}</div><p className="section-copy">本机入口：{managed?.listener ? `127.0.0.1:${entryPort}` : "未监听"} · 托管核心：{managed?.running ? "运行中" : "未运行"} · 固定出口：{profileSummary?.fixedEgressConfigured ? `${profileSummary.fixedEgressHost} / ${profileSummary.aiRegionLabel || profileSummary.aiRegion}` : "未配置"}</p></section>}
  </>;
}

function ReferralPurchaseCard({ placement, offers, onOpen, onConfigure }: { placement: ReferralOffer["placement"]; offers: ReferralOffer[]; onOpen: (offer: ReferralOffer) => Promise<void>; onConfigure: () => void }) {
  const matchingOffers = referralsForPlacement(offers, placement);
  return <article className="purchase-card"><div><span className="eyebrow">{placementLabels[placement]}</span><h3>{matchingOffers.length > 0 ? matchingOffers.map((offer) => offer.provider).join(" · ") : "尚未配置推荐服务"}</h3><p>{matchingOffers.length > 0 ? (placement === "download-vpn" ? "大流量线路可在八戒与魔戒之间按套餐需求选择。" : matchingOffers[0].summary) : "添加经过筛选的服务地址后，用户可以在需要时直接查看。"}</p>{matchingOffers[0] && <small>{matchingOffers[0].disclosure}</small>}</div>{matchingOffers.length > 0 ? <div className="purchase-offers">{matchingOffers.map((offer) => <button key={offer.id} className="button button-primary" onClick={() => void onOpen(offer)}><ExternalLink size={15} />查看{offer.provider}</button>)}</div> : <button className="button button-secondary" onClick={onConfigure}>配置推荐服务</button>}</article>;
}

function SourcesPage({ offers, onReferralOpen, onConfigureReferrals }: { offers: ReferralOffer[]; onReferralOpen: (offer: ReferralOffer) => Promise<void>; onConfigureReferrals: () => void }) {
  const [profile, setProfile] = useState<ConnectionProfile>({
    subscriptions: [
      { role: "stable", label: "稳定订阅", prefix: "稳定", url: "" },
      { role: "download1", label: "下载订阅 1", prefix: "下载1", url: "" },
      { role: "download2", label: "下载订阅 2", prefix: "下载2", url: "" },
    ],
    fixedEgress: { name: "固定出口", server: "", port: 443, cipher: "aes-128-gcm", password: "" },
    aiRegion: "",
    aiRegionLabel: "",
    preferredNodeKeywords: [],
  });
  const [summary, setSummary] = useState<ProfileSummary | null>(null);
  const [preview, setPreview] = useState<PreviewReport | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<"save" | "import" | "preview" | null>(null);

  useEffect(() => { void getConnectionProfileSummary().then(setSummary); }, []);
  const updateUrl = (role: string, url: string) => setProfile({ ...profile, subscriptions: profile.subscriptions.map((source) => source.role === role ? { ...source, url } : source) });
  const run = async (kind: "save" | "import" | "preview") => {
    setBusy(kind); setMessage(""); setPreview(null);
    try {
      if (kind === "save") {
        setSummary(await saveConnectionProfile(normalizeConnectionProfileForSave(profile)));
        setMessage("连接配置已写入系统凭据库；普通配置文件和日志中没有保存 Token 或密码。");
      } else if (kind === "import") {
        setSummary(await importExistingProfile());
        setMessage("现有线路已直接导入系统凭据库，订阅 Token 和 VPS 密码没有回显到界面。");
      } else {
        const result = await runEnginePreview();
        setPreview(result); setMessage(result.message);
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally { setBusy(null); }
  };

  return <>
    <PageHeader eyebrow="线路管理" title="VPS 与订阅源" description="订阅 Token 与 VPS 密码进入 Windows Credential Manager 或 macOS Keychain；安全预演会在临时端口启动候选核心，完成后自动停止和清除。" action={<button className="button button-secondary" onClick={() => void run("import")} disabled={busy !== null}><Download size={16} /> {busy === "import" ? "正在导入…" : "自动导入旧版配置"}</button>} />
    {summary && <section className="source-grid compact">{summary.sources.map((source) => <article className="source-card" key={source.role}><div className="source-card-top"><div className={`source-icon role-${source.role.startsWith("download") ? "download" : "stable"}`}><Globe2 /></div><div><h3>{source.label}</h3><span className="muted">{source.host}</span></div><StatusDot status="healthy" /></div><div className="device-details"><span>角色<strong>{source.role}</strong></span><span>凭据<strong>系统安全存储</strong></span><span>完整地址<strong>已隐藏</strong></span></div></article>)}{summary.fixedEgressConfigured && <article className="source-card"><div className="source-card-top"><div className="source-icon role-fixed-exit"><Server /></div><div><h3>固定 VPS 出口</h3><span className="muted">{summary.fixedEgressHost}</span></div><StatusDot status="healthy" /></div><div className="device-details"><span>密码<strong>系统安全存储</strong></span><span>预期地区<strong>{summary.aiRegionLabel} ({summary.aiRegion})</strong></span><span>用途<strong>AI 固定出口</strong></span></div></article>}</section>}
    <section className="panel source-wizard"><div className="wizard-copy"><span className="eyebrow">连接向导</span><h2>填写自己的线路与目标地区</h2><p>保存不会切换系统代理。稳定订阅必填，下载订阅和固定 VPS 可选；安全预演会验证实际出口地区。</p><ul className="check-list"><li><Check />稳定订阅负责 AI 前置</li><li><Check />下载源按需添加并自动健康选择</li><li><Check />VPS 固定最终出口，地区由用户决定</li></ul></div><div className="form-grid wizard-form"><label className="span-two">稳定订阅 URL<input value={profile.subscriptions[0].url} placeholder="https://…" type="password" onChange={(event) => updateUrl("stable", event.target.value)} /></label><label className="span-two">下载订阅 1 URL（可选）<input value={profile.subscriptions[1].url} placeholder="https://…" type="password" onChange={(event) => updateUrl("download1", event.target.value)} /></label><label className="span-two">下载订阅 2 URL（可选）<input value={profile.subscriptions[2].url} placeholder="https://…" type="password" onChange={(event) => updateUrl("download2", event.target.value)} /></label><label>目标地区代码<input value={profile.aiRegion} maxLength={2} placeholder="US / JP / DE" onChange={(event) => setProfile({ ...profile, aiRegion: event.target.value.toUpperCase() })} /></label><label>目标地区名称<input value={profile.aiRegionLabel} placeholder="美国 / 日本 / 德国" onChange={(event) => setProfile({ ...profile, aiRegionLabel: event.target.value })} /></label><label className="span-two">优先节点关键词（逗号分隔，可选）<input value={profile.preferredNodeKeywords.join(", ")} placeholder="例如：低延迟、专线、Premium" onChange={(event) => setProfile({ ...profile, preferredNodeKeywords: event.target.value.split(/[,，]/).map((item) => item.trim()).filter(Boolean) })} /></label><label>VPS 地址（可选）<input value={profile.fixedEgress?.server ?? ""} placeholder="vps.example.com" onChange={(event) => setProfile({ ...profile, fixedEgress: { ...profile.fixedEgress!, server: event.target.value } })} /></label><label>端口<input value={profile.fixedEgress?.port ?? 443} inputMode="numeric" onChange={(event) => setProfile({ ...profile, fixedEgress: { ...profile.fixedEgress!, port: Number(event.target.value) || 0 } })} /></label><label>加密方式<select value={profile.fixedEgress?.cipher} onChange={(event) => setProfile({ ...profile, fixedEgress: { ...profile.fixedEgress!, cipher: event.target.value } })}><option>aes-128-gcm</option><option>aes-256-gcm</option><option>chacha20-ietf-poly1305</option></select></label><label>VPS 密码<input type="password" value={profile.fixedEgress?.password ?? ""} placeholder="仅写入系统凭据库" onChange={(event) => setProfile({ ...profile, fixedEgress: { ...profile.fixedEgress!, password: event.target.value } })} /></label><div className="span-two wizard-actions"><button className="button button-secondary" disabled={busy !== null} onClick={() => void run("save")}><LockKeyhole size={15} />{busy === "save" ? "正在安全保存…" : "保存到系统凭据库"}</button><button className="button button-primary" disabled={busy !== null || !summary} onClick={() => void run("preview")}><ShieldCheck size={15} />{busy === "preview" ? "正在隔离验证…" : "运行安全预演"}</button></div>{message && <div className={`span-two result-banner ${preview?.success ? "success" : ""}`}><StatusDot status={preview?.success ? "healthy" : "warning"} /><span>{message}</span>{preview && <strong>临时凭据清理：{preview.cleanupComplete ? "完成" : "待处理"}</strong>}</div>}</div></section>
    <section className="purchase-grid"><ReferralPurchaseCard placement="stable-vpn" offers={offers} onOpen={onReferralOpen} onConfigure={onConfigureReferrals} /><ReferralPurchaseCard placement="download-vpn" offers={offers} onOpen={onReferralOpen} onConfigure={onConfigureReferrals} /></section>
  </>;
}

function ServicesPage({ services, setServices }: { services: ServicePolicy[]; setServices: (value: ServicePolicy[]) => void }) {
  const categories = [{ id: "ai", label: "境外 AI", icon: Sparkles }, { id: "game", label: "游戏与商店", icon: Gamepad2 }, { id: "stream", label: "流媒体", icon: Tv }, { id: "work", label: "通讯与办公", icon: TerminalSquare }];
  const [category, setCategory] = useState<string>("all");
  const visibleServices = category === "all" ? services : services.filter((service) => service.category === category);
  return <><PageHeader eyebrow="策略中心" title="按场景自动分流" description="选择分类后直接编辑每项策略；修改会保存在本机。" /><section className="category-row"><button className={category === "all" ? "category-chip active" : "category-chip"} onClick={() => setCategory("all")}>全部</button>{categories.map(({ id, label, icon: Icon }) => <button className={category === id ? "category-chip active" : "category-chip"} key={id} onClick={() => setCategory(id)}><Icon size={17} />{label}</button>)}</section><section className="service-list panel"><div className="table-header"><span>服务</span><span>策略</span><span>实际出口</span><span>严格模式</span><span /></div>{visibleServices.map((service) => <div className="service-row" key={service.id}><div className="service-name"><span className={`service-logo category-${service.category}`}>{service.icon}</span><div><strong>{service.name}</strong><small>{service.category.toUpperCase()}</small></div></div><select value={service.policy} onChange={(e) => setServices(services.map((s) => s.id === service.id ? { ...s, policy: e.target.value } : s))}><option>{service.policy}</option><option>严格目标地区</option><option>稳定线路</option><option>便宜线路</option><option>直连保护</option></select><div className="exit-cell"><StatusDot status={service.health} />{service.exit}</div><button className={service.strict ? "toggle on" : "toggle"} onClick={() => setServices(services.map((s) => s.id === service.id ? { ...s, strict: !s.strict } : s))} aria-label={`${service.name}严格模式`}><span /></button><span /></div>)}</section></>;
}

function GatewayPolicyCard({ policy, online, onChange }: { policy: GatewayDevicePolicy; online: boolean; onChange: (next: GatewayDevicePolicy) => void }) {
  return <article className="source-card device-card gateway-policy-card"><div className="source-card-top"><div className="source-icon role-stable"><MonitorSmartphone /></div><div><input className="device-name-input" value={policy.name} maxLength={80} onChange={(event) => onChange({ ...policy, name: event.target.value })} /><span className="muted">局域网设备 · {policy.mac}</span></div><StatusDot status={online ? "healthy" : "idle"} /></div><div className="device-policy-fields"><label>固定设备 IP<input value={policy.ip} readOnly /></label><label>使用场景<select value={policy.scenario} onChange={(event) => onChange({ ...policy, scenario: event.target.value as GatewayScenario })}>{gatewayScenarioOptions.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}</select></label></div><div className="device-policy-footer"><small>{gatewayScenarioOptions.find((option) => option.id === policy.scenario)?.detail}</small><label className="switch-label"><button className={policy.enabled ? "toggle on" : "toggle"} onClick={() => onChange({ ...policy, enabled: !policy.enabled })}><span /></button>{policy.enabled ? "参与分流" : "暂停策略"}</label></div></article>;
}

function DevicesPage() {
  const [includeAndroid, setIncludeAndroid] = useState(true);
  const [includeShadowrocket, setIncludeShadowrocket] = useState(true);
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [report, setReport] = useState<MobileExportReport | null>(null);
  const [gateway, setGateway] = usePersistentState("ai-route.gateway-connection.v1", { host: "", sshPort: 22, user: "root", identityFile: "" });
  const [localNetwork, setLocalNetwork] = useState<LocalNetworkReport | null>(null);
  const [localBusy, setLocalBusy] = useState(false);
  const [localMessage, setLocalMessage] = useState("");
  const readLocalNetwork = async () => {
    setLocalBusy(true); setLocalMessage("");
    try {
      const result = await getLocalNetworkDevices(); setLocalNetwork(result);
      setGateway((current) => ({ ...current, host: current.host || result.gateway }));
      setLocalMessage(`已读取实际网关与 ${result.devices.length} 条本机邻居记录。`);
    } catch (error) { setLocalMessage(error instanceof Error ? error.message : String(error)); }
    finally { setLocalBusy(false); }
  };
  useEffect(() => { void readLocalNetwork(); }, []);
  const [gatewayBusy, setGatewayBusy] = useState<"preview" | "apply" | "rollback" | null>(null);
  const [gatewayMessage, setGatewayMessage] = useState("");
  const [gatewayReport, setGatewayReport] = useState<GatewayDeployReport | null>(null);
  const [gatewayInspect, setGatewayInspect] = useState<GatewayInspectReport | null>(null);
  const [gatewayPolicies, setGatewayPolicies] = useState<GatewayDevicePolicy[]>([]);
  const [policyBusy, setPolicyBusy] = useState<"inspect" | "save" | null>(null);
  useEffect(() => { void loadGatewayPolicies().then(setGatewayPolicies).catch((error) => setGatewayMessage(error instanceof Error ? error.message : String(error))); }, []);
  const runExport = async () => {
    if (!acknowledged) { setMessage("请先确认敏感信息提示。"); return; }
    if (!includeAndroid && !includeShadowrocket) { setMessage("至少选择一种移动端格式。"); return; }
    if (!window.confirm("本次导出会把订阅 Token 与 VPS 密码写入下载目录，只用于你自己的设备。继续吗？")) return;
    setBusy(true); setMessage(""); setReport(null);
    try {
      const result = await exportMobileBundle({ confirmedSecretExport: true, includeAndroid, includeShadowrocket });
      setReport(result); setMessage("导出完成；电脑当前代理、FlClash 和托管核心均未改动。");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally { setBusy(false); }
  };
  const runGateway = async (action: "preview" | "apply" | "rollback") => {
    const question = action === "preview"
      ? `把含订阅 Token 和 VPS 密码的候选配置通过 SSH 加密发送到 ${gateway.host}，只在 /tmp 校验，不写入或重启。继续吗？`
      : action === "apply"
        ? `把已校验配置事务写入 ${gateway.host} 的 OpenClash 并重启服务？旧配置和 UCI 选择会保存为单一回滚点。`
        : `恢复 ${gateway.host} 上 AI Route Manager 应用前的 OpenClash 配置和 UCI 选择？检测到后续人工改动时会拒绝覆盖。`;
    if (!window.confirm(question)) return;
    setGatewayBusy(action); setGatewayMessage(""); setGatewayReport(null);
    try {
      if (action !== "rollback") setGatewayPolicies(await saveGatewayPolicies(gatewayPolicies));
      const result = await configureHomeGateway({ ...gateway, action, confirmed: true });
      setGatewayReport(result); setGatewayMessage(result.message);
    } catch (error) {
      setGatewayMessage(error instanceof Error ? error.message : String(error));
    } finally { setGatewayBusy(null); }
  };
  const inspectGateway = async () => {
    if (!window.confirm(`通过只读 SSH 检查 ${gateway.host} 的 OpenWrt 能力、DHCP 租约和 IPv6 邻居？不会写入路由器或修改本机代理。`)) return;
    setPolicyBusy("inspect"); setGatewayMessage(""); setGatewayInspect(null);
    try {
      const result = await inspectHomeGateway({ ...gateway, confirmed: true });
      setGatewayInspect(result);
      setGatewayPolicies((current) => mergeGatewayDiscovery(result.devices, current));
      setGatewayMessage(`${result.message}；发现 ${result.devices.length} 台设备，请确认场景后保存。`);
    } catch (error) { setGatewayMessage(error instanceof Error ? error.message : String(error)); }
    finally { setPolicyBusy(null); }
  };
  const persistGatewayPolicies = async () => {
    setPolicyBusy("save"); setGatewayMessage("");
    try { const result = await saveGatewayPolicies(gatewayPolicies); setGatewayPolicies(result); setGatewayMessage(`已保存 ${result.length} 条设备策略；下次预演或应用时自动写入候选配置。`); }
    catch (error) { setGatewayMessage(error instanceof Error ? error.message : String(error)); }
    finally { setPolicyBusy(null); }
  };
  const updateGatewayPolicy = (next: GatewayDevicePolicy) => setGatewayPolicies((current) => current.map((policy) => policy.id === next.id ? next : policy));
  const gatewayReady = Boolean(gateway.host.trim() && gateway.user.trim() && gateway.identityFile.trim() && Number.isInteger(gateway.sshPort) && gateway.sshPort > 0 && gateway.sshPort <= 65535);
  return <><PageHeader eyebrow="家庭网络" title="所有设备，一处管理" description="移动端可导出；OpenWrt / OpenClash 可先隔离预演，再事务下发并一键回滚。所有操作都不切换这台电脑的代理。" action={<button className="button button-primary" disabled={busy || !acknowledged || (!includeAndroid && !includeShadowrocket)} onClick={() => void runExport()}><FileKey2 size={16} />{busy ? "正在安全导出…" : "生成移动端配置"}</button>} />
    <section className="panel local-network-panel"><div className="panel-heading"><div><span className="eyebrow">真实读取</span><h2>本机与局域网邻居</h2></div><button className="button button-secondary" disabled={localBusy} onClick={() => void readLocalNetwork()}><Network size={14} />{localBusy ? "正在读取…" : "重新读取"}</button></div><p className="section-copy">这里自动读取 Windows 当前默认网关和邻居缓存，只展示系统返回的真实记录。OpenWrt SSH 扫描可以进一步读取 DHCP 租约和设备名称。</p>{localMessage && <p className="section-copy">{localMessage}</p>}{localNetwork && <><p className="section-copy">本机 {localNetwork.localIp} · 实际网关 {localNetwork.gateway}</p>{localNetwork.devices.length > 0 ? <div className="local-device-grid">{localNetwork.devices.map((device) => <div className="local-device" key={device.id}><strong>{device.name}</strong><span>{device.ipv4}</span><small>{device.mac} · {device.id.startsWith("local-") ? "本机" : device.online ? "当前可达" : "邻居缓存"}</small></div>)}</div> : <div className="gateway-empty-state"><MonitorSmartphone /><div><h3>没有其他邻居记录</h3><p>当前系统只返回了本机与默认网关信息。</p></div></div>}</>}</section>
    <section className="mobile-export-panel panel"><div className="mobile-export-copy"><div className="mobile-export-icon"><Smartphone /></div><div><span className="eyebrow">一次生成</span><h2>Android + Shadowrocket</h2><p>Android 配置包含动态订阅、AI 严格固定目标地区出口、中国大陆直连和下载分流；Shadowrocket 导出固定出口二维码、规则文件与订阅清单。</p><ul className="check-list"><li><Check />AI 不会回退到 DIRECT</li><li><Check />国内规则在最终兜底之前直连</li><li><Check />导出失败会清理本轮目录</li></ul></div></div><div className="mobile-export-controls"><label className="export-choice"><input type="checkbox" checked={includeAndroid} onChange={(event) => setIncludeAndroid(event.target.checked)} /><span><strong>Android / Mihomo</strong><small>FlClash、Mihomo Party 等兼容客户端</small></span></label><label className="export-choice"><input type="checkbox" checked={includeShadowrocket} onChange={(event) => setIncludeShadowrocket(event.target.checked)} /><span><strong>iPhone / iPad</strong><small>Shadowrocket 配置、SS URI 与本地二维码</small></span></label><label className="secret-ack"><input type="checkbox" checked={acknowledged} onChange={(event) => setAcknowledged(event.target.checked)} /><span>我确认导出文件含订阅 Token 和 VPS 密码，只传到自己的设备，并会在导入后删除。</span></label><button className="button button-primary full" disabled={busy || !acknowledged || (!includeAndroid && !includeShadowrocket)} onClick={() => void runExport()}><ShieldCheck size={15} />{busy ? "正在写入受限目录…" : "确认并导出"}</button>{message && <div className={report ? "export-message success" : "export-message"}>{message}</div>}</div></section>
    {report && <section className="panel export-result"><div className="panel-heading"><div><span className="eyebrow">已生成</span><h2>移动端导出包</h2></div><span className="soft-badge success">未改动本机代理</span></div><div className="export-directory"><LockKeyhole size={16} /><div><span>导出目录</span><strong>{report.directory}</strong></div></div><div className="export-file-list">{report.files.map((file) => <div key={file.path}><FileKey2 size={15} /><span><strong>{file.name}</strong><small>{file.kind}</small></span></div>)}</div><div className="export-warning">{report.warning}</div></section>}
    {!gatewayReady && <section className="sync-message">路由器策略尚未就绪：请先读取实际网关，并填写已启用 SSH 的 OpenWrt 路由器地址与私钥路径；普通路由器不支持此下发方式。移动端导出还需先保存线路配置并勾选凭据导出确认。</section>}
    <section className="panel gateway-deploy-panel"><div className="panel-heading"><div><span className="eyebrow">路由器自动下发</span><h2>OpenWrt / OpenClash 事务部署</h2></div><Router size={21} /></div><p className="section-copy">先只读发现设备与能力，再保存每台设备的场景。候选配置在 /tmp 由现有 clash_meta 校验；应用只接管 ai-route-manager.yaml，并保存原文件、UCI 选择和 SHA-256。</p><div className="form-grid gateway-form"><label>路由器 IP / 域名<input value={gateway.host} onChange={(event) => setGateway({ ...gateway, host: event.target.value })} /></label><label>SSH 端口<input type="number" min={1} max={65535} value={gateway.sshPort} onChange={(event) => setGateway({ ...gateway, sshPort: Number(event.target.value) })} /></label><label>SSH 用户<input value={gateway.user} readOnly /></label><label>SSH 私钥绝对路径<input value={gateway.identityFile} placeholder="C:\\Users\\你\\.ssh\\openwrt_ed25519" onChange={(event) => setGateway({ ...gateway, identityFile: event.target.value })} /></label></div><div className="gateway-safety"><span><ShieldCheck size={14} />DNS、IPv4 与 IPv6 共同参与策略</span><span><LockKeyhole size={14} />凭据只经 SSH 标准输入传输，不进报告</span></div><div className="remote-actions"><button className="button button-secondary" disabled={policyBusy !== null || gatewayBusy !== null || !gatewayReady} onClick={() => void inspectGateway()}><Network size={14} />{policyBusy === "inspect" ? "只读扫描中…" : "扫描设备与能力"}</button><button className="button button-secondary" disabled={policyBusy !== null || gatewayBusy !== null || gatewayPolicies.length === 0} onClick={() => void persistGatewayPolicies()}><Save size={14} />{policyBusy === "save" ? "正在保存…" : "保存设备策略"}</button><button className="button button-secondary" disabled={policyBusy !== null || gatewayBusy !== null || !gatewayReady} onClick={() => void runGateway("preview")}><Activity size={14} />{gatewayBusy === "preview" ? "远端校验中…" : "隔离预演"}</button><button className="button button-primary" disabled={policyBusy !== null || gatewayBusy !== null || !gatewayReady || gatewayReport?.rollbackAvailable === true} onClick={() => void runGateway("apply")}><Play size={14} />{gatewayBusy === "apply" ? "事务提交中…" : "应用并重载"}</button><button className="button button-secondary" disabled={policyBusy !== null || gatewayBusy !== null || !gatewayReady} onClick={() => void runGateway("rollback")}><ArchiveRestore size={14} />{gatewayBusy === "rollback" ? "正在恢复…" : "一键回滚"}</button></div>{gatewayMessage && <div className={gatewayReport || gatewayInspect ? "export-message success" : "export-message"}>{gatewayMessage}</div>}{gatewayInspect && <div className="gateway-capability-grid"><div><span>路由器</span><strong>{gatewayInspect.hostname}</strong><small>{gatewayInspect.capabilities.model} · OpenWrt {gatewayInspect.capabilities.release}</small></div><div><span>执行能力</span><strong>{gatewayInspect.capabilities.openclash ? "OpenClash 可用" : "OpenClash 未安装"}</strong><small>WireGuard {gatewayInspect.capabilities.wireguard ? "✓" : "—"} · OpenVPN {gatewayInspect.capabilities.openvpn ? "✓" : "—"}</small></div><div><span>家庭网络</span><strong>IPv6 {gatewayInspect.capabilities.ipv6DefaultRoute ? "可用" : "未检测"}</strong><small>UPnP {gatewayInspect.capabilities.upnp ? "已启用" : "未启用"} · {gatewayInspect.devices.length} 台设备</small></div></div>}{gatewayReport && <div className="gateway-result"><div><span>状态</span><strong>{gatewayReport.action === "preview" ? "预演通过" : gatewayReport.action === "apply" ? "已应用" : "已回滚"}</strong></div><div><span>核心 / 服务</span><strong>{gatewayReport.core} · {gatewayReport.service}</strong></div><div><span>固定目标</span><code>{gatewayReport.targetPath}</code></div><div><span>回滚点</span><strong>{gatewayReport.rollbackAvailable ? "可用" : "无"}</strong></div></div>}</section>
    {gatewayPolicies.length > 0 ? <section className="source-grid compact">{gatewayPolicies.map((policy) => <GatewayPolicyCard key={policy.id} policy={policy} online={gatewayInspect?.devices.some((device) => device.id === policy.id) ?? false} onChange={updateGatewayPolicy} />)}</section> : <section className="panel gateway-empty-state"><MonitorSmartphone /><div><h3>OpenWrt 设备策略为空</h3><p>上方 Windows 邻居记录已经自动读取；填写 OpenWrt SSH 信息后可继续读取 DHCP 租约并建立逐设备策略。</p></div></section>}</>;
}

function RemotePage({ offers, onReferralOpen, onConfigureReferrals }: { offers: ReferralOffer[]; onReferralOpen: (offer: ReferralOffer) => Promise<void>; onConfigureReferrals: () => void }) {
  const [environment, setEnvironment] = useState<RemoteEnvironmentReport | null>(null);
  const [sunshineStatus, setSunshineStatus] = useState<SunshineConfigStatus | null>(null);
  const [sunshineReport, setSunshineReport] = useState<SunshineConfigActionReport | null>(null);
  const [quality, setQuality] = useState<RemoteQualityReport | null>(null);
  const [host, setHost] = useState("192.168.1.2");
  const [mode, setMode] = useState<"sunshine" | "rustdesk">("sunshine");
  const [busy, setBusy] = useState<"detect" | "probe" | "export" | "install" | "deploy" | "copy" | "sunshineConfig" | "localFirewall" | "vpsFirewall" | null>(null);
  const [message, setMessage] = useState("");
  const [deploy, setDeploy] = useState({ host: "", sshPort: 22, user: "root", identityFile: "" });
  const [deployReport, setDeployReport] = useState<RustDeskDeployReport | null>(null);
  const [firewallScope, setFirewallScope] = useState<"local-subnet" | "tailnet">("local-subnet");
  const [firewallReport, setFirewallReport] = useState<FirewallActionReport | null>(null);
  const refreshEnvironment = async () => {
    const [result, configStatus] = await Promise.all([getRemoteEnvironment(), getSunshineConfigStatus()]);
    setEnvironment(result); setSunshineStatus(configStatus);
  };
  useEffect(() => { setBusy("detect"); void refreshEnvironment().catch((error) => setMessage(error instanceof Error ? error.message : String(error))).finally(() => setBusy(null)); }, []);
  const probe = async () => {
    setBusy("probe"); setMessage(""); setQuality(null);
    try { const result = await probeRemotePath(host, mode === "sunshine" ? 47984 : 21118, 5); setQuality(result); setMessage(result.message); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const exportProfile = async () => {
    if (!quality || !environment) return;
    setBusy("export"); setMessage("");
    try { const result = await exportRemoteProfile(quality, environment.encoder); setMessage(`${result.message}：${result.directory}`); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const configureSunshineProfile = async (action: "apply" | "rollback") => {
    if (action === "apply" && (!quality || !environment)) return;
    const bitrate = quality?.bitrateMbps ?? 20;
    const encoder = environment?.encoder ?? "auto";
    const prompt = action === "apply"
      ? `把实测建议 ${quality?.recommendedPreset}（${bitrate} Mbps、${encoder.toUpperCase()}）事务写入 Sunshine？将先校验原文件并创建回滚点；不会重启 Sunshine、修改防火墙或网络代理。`
      : "恢复 AI Route Manager 写入前的 Sunshine 配置？如果配置在应用后被手动改过，回滚会自动拒绝覆盖。";
    if (!window.confirm(prompt)) return;
    setBusy("sunshineConfig"); setMessage(""); setSunshineReport(null);
    try {
      const result = await configureSunshineStreaming(action, bitrate, encoder, true);
      setSunshineReport(result); setMessage(result.message);
      setSunshineStatus(await getSunshineConfigStatus());
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const installTool = async (toolId: "sunshine" | "moonlight", name: string) => {
    if (!window.confirm(`将通过 winget 静默安装 ${name}，不会启动软件，也不会修改 AI Route 的系统代理。确认安装吗？`)) return;
    setBusy("install"); setMessage("");
    try {
      const result = await installRemoteTool(toolId, true);
      setMessage(result.message);
      if (result.success) await refreshEnvironment();
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const deployServer = async () => {
    if (!window.confirm(`将通过 SSH 连接 ${deploy.user}@${deploy.host}:${deploy.sshPort}，在 ~/ai-route-rustdesk 写入 Docker Compose 并启动 hbbs/hbbr。不会修改本机代理，也不会自动开放 VPS 防火墙。确认部署吗？`)) return;
    setBusy("deploy"); setMessage(""); setDeployReport(null);
    try {
      const result = await deployRustDeskServer({ ...deploy, confirmed: true });
      setDeployReport(result); setMessage(result.message);
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const copyClientSettings = async () => {
    if (!deployReport) return;
    setBusy("copy");
    try {
      await navigator.clipboard.writeText(`ID Server: ${deployReport.idServer}\nRelay Server: ${deployReport.relayServer}\nKey: ${deployReport.publicKey}`);
      setMessage("RustDesk 客户端参数已复制；私钥路径和服务器私钥均未复制。");
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const configureLocalFirewall = async (action: "apply" | "rollback") => {
    const verb = action === "apply" ? "创建" : "删除";
    if (!window.confirm(`将通过 Windows UAC ${verb}由 AI Route Manager 管理的 Sunshine 最小入站规则。不会开放公网范围、不会改路由器或系统代理。继续吗？`)) return;
    setBusy("localFirewall"); setMessage("");
    try { const result = await configureLocalStreamingFirewall(action, firewallScope, true); setFirewallReport(result); setMessage(result.message); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const configureVpsFirewall = async (action: "apply" | "rollback") => {
    const verb = action === "apply" ? "开放 21115/TCP、21116/TCP+UDP、21117/TCP" : "删除上述四条规则";
    if (!window.confirm(`将通过 SSH 在 ${deploy.host} 的 UFW 中${verb}；21118/21119 仍保持关闭，不修改容器和本机代理。继续吗？`)) return;
    setBusy("vpsFirewall"); setMessage("");
    try { const result = await configureRustDeskFirewall({ ...deploy, action, confirmed: true }); setFirewallReport(result); setMessage(result.message); }
    catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const profiles = [{ name: "弱网稳定", detail: "720p60 · H.264 · 10 Mbps", icon: TerminalSquare }, { name: "远程流畅", detail: "1080p60 · HEVC · 20 Mbps", icon: MonitorSmartphone }, { name: "均衡高清", detail: "1440p60 · HEVC · 35 Mbps", icon: Gamepad2 }, { name: "局域网极致", detail: "4K60 · HEVC · 70 Mbps", icon: Tv }];
  return <><PageHeader eyebrow="远程控制" title="直连优先，画质按实测调优" description="办公使用 RustDesk，高画质串流使用 Sunshine + Moonlight；用户选择的 VPS 只做信令或最后中继。" action={<button className="button button-primary" disabled={busy !== null} onClick={() => void probe()}><Activity size={16} />{busy === "probe" ? "正在实测…" : "实测远控路径"}</button>} />
    <section className="remote-tool-grid">{environment?.tools.map((tool) => <article className="panel remote-tool-card" key={tool.id}><div><StatusDot status={tool.running ? "healthy" : tool.installed ? "idle" : "warning"} /><strong>{tool.name}</strong><span>{tool.running ? "运行中" : tool.installed ? "已安装，未运行" : "尚未安装"}</span></div><div className="remote-tool-actions">{!tool.installed && tool.installSupported && (tool.id === "sunshine" || tool.id === "moonlight") && <button className="button button-primary" disabled={busy !== null} onClick={() => void installTool(tool.id as "sunshine" | "moonlight", tool.name)}><Download size={14} />{busy === "install" ? "安装中…" : "一键安装"}</button>}<button className="button button-secondary" onClick={() => void openExternalUrl(tool.officialUrl)}><ExternalLink size={14} />{tool.installed ? "官方文档" : "官方下载"}</button></div></article>) ?? <article className="panel remote-tool-card">正在读取本机工具…</article>}</section>
    <section className="content-grid two-one"><article className="panel remote-configurator"><div className="panel-heading"><div><span className="eyebrow">路径实测</span><h2>填写要连接的主机</h2></div><Network size={20} /></div><div className="form-grid"><label>使用方式<select value={mode} onChange={(event) => setMode(event.target.value as "sunshine" | "rustdesk")}><option value="sunshine">Sunshine 串流</option><option value="rustdesk">RustDesk 直连</option></select></label><label>目标端口<input value={mode === "sunshine" ? 47984 : 21118} readOnly /></label><label className="span-two">目标 IP 或主机名<input value={host} placeholder="192.168.1.2" onChange={(event) => setHost(event.target.value)} /></label></div><div className="remote-actions"><button className="button button-primary" disabled={busy !== null || !host.trim()} onClick={() => void probe()}><Gauge size={15} />{busy === "probe" ? "5 次握手实测中…" : "开始实测"}</button><button className="button button-secondary" disabled={!quality || !environment || busy !== null} onClick={() => void exportProfile()}><Download size={15} />{busy === "export" ? "正在生成…" : "导出推荐配置"}</button></div>{message && <div className="export-message success">{message}</div>}</article><article className="panel"><div className="panel-heading"><div><span className="eyebrow">连接路径</span><h2>实测结果</h2></div><HeartPulse size={20} /></div><div className="quality-score"><strong>{quality?.score ?? "—"}</strong><span>{quality?.recommendedPreset ?? "等待实测"}</span></div><ul className="quality-facts"><li><span>平均握手</span><strong>{quality ? `${quality.averageLatencyMs.toFixed(1)} ms` : "—"}</strong></li><li><span>抖动</span><strong>{quality ? `${quality.jitterMs.toFixed(1)} ms` : "—"}</strong></li><li><span>失败率</span><strong>{quality ? `${quality.lossPercent.toFixed(0)}%` : "—"}</strong></li><li><span>GPU / 编码</span><strong>{environment ? `${environment.encoder.toUpperCase()} · ${environment.gpu}` : "检测中"}</strong></li></ul></article></section>
    <section className="panel sunshine-config-wizard"><div className="panel-heading"><div><span className="eyebrow">实测参数落地</span><h2>Sunshine 自动配置与回滚</h2></div><WandSparkles size={20} /></div><p className="section-copy">把刚才实测出的码率和本机编码器写入官方 Sunshine 配置文件。提交前校验原哈希并保存完整备份；检测到用户后续修改时拒绝强行回滚。</p><div className="sunshine-config-status"><div><span>配置文件</span><strong>{sunshineStatus?.configPath ?? "正在检测…"}</strong></div><div><span>当前状态</span><strong>{sunshineStatus?.message ?? "正在读取…"}</strong></div><div><span>待写参数</span><strong>{quality && environment ? `${quality.bitrateMbps} Mbps · ${environment.encoder.toUpperCase()} · UPNP 关闭` : "先完成 Sunshine 路径实测"}</strong></div></div><div className="remote-actions"><button className="button button-primary" disabled={busy !== null || !quality || !environment || mode !== "sunshine" || sunshineStatus?.rollbackAvailable === true} onClick={() => void configureSunshineProfile("apply")}><Save size={14} />{busy === "sunshineConfig" ? "事务处理中…" : "写入实测参数"}</button><button className="button button-secondary" disabled={busy !== null || !sunshineStatus?.rollbackAvailable} onClick={() => void configureSunshineProfile("rollback")}><ArchiveRestore size={14} />一键回滚配置</button><span className={sunshineStatus?.rollbackAvailable ? "soft-badge success" : "soft-badge"}>{sunshineStatus?.rollbackAvailable ? "回滚点可用" : "未接管配置"}</span></div>{sunshineReport && <div className="deploy-result sunshine-config-result"><div><strong>操作</strong><span>{sunshineReport.action === "apply" ? "已应用" : "已回滚"}</span></div><div><strong>重启 Sunshine</strong><span>{sunshineReport.restartRequired ? "当前串流结束后需要" : "下次启动自动读取"}</span></div><div className="span-two"><strong>已管理参数</strong><code>{sunshineReport.managedValues.join(" · ") || "已恢复完整备份"}</code></div></div>}</section>
    <section className="panel firewall-wizard"><div className="panel-heading"><div><span className="eyebrow">最小暴露面</span><h2>Sunshine 防火墙向导</h2></div><ShieldCheck size={20} /></div><p className="section-copy">只创建 Windows 专用网络入站规则，不开放任意公网来源；应用和回滚都需要 UAC 确认。</p><label>允许来源<select value={firewallScope} onChange={(event) => setFirewallScope(event.target.value as "local-subnet" | "tailnet")}><option value="local-subnet">本地子网 LocalSubnet</option><option value="tailnet">Tailscale / Headscale 100.64.0.0/10</option></select></label><div className="firewall-rules"><span><strong>TCP</strong>47984、47989、47990、48010</span><span><strong>UDP</strong>47998–48000</span></div><div className="remote-actions"><button className="button button-primary" disabled={busy !== null} onClick={() => void configureLocalFirewall("apply")}><ShieldCheck size={14} />{busy === "localFirewall" ? "等待 UAC…" : "确认应用"}</button><button className="button button-secondary" disabled={busy !== null} onClick={() => void configureLocalFirewall("rollback")}><ArchiveRestore size={14} />回滚本机规则</button></div></section>
    <section className="panel rustdesk-deploy"><div className="panel-heading"><div><span className="eyebrow">VPS 自建中继</span><h2>部署 RustDesk Server OSS</h2></div><Server size={20} /></div><p className="section-copy">只使用系统 SSH 和现有私钥，把固定且版本锁定的 Docker Compose 通过标准输入发送到 VPS；不会上传 VPS 密码或回传服务器私钥。</p><div className="form-grid"><label>VPS IP / 域名<input value={deploy.host} placeholder="vps.example.com" onChange={(event) => setDeploy({ ...deploy, host: event.target.value })} /></label><label>SSH 端口<input type="number" min={1} max={65535} value={deploy.sshPort} onChange={(event) => setDeploy({ ...deploy, sshPort: Number(event.target.value) })} /></label><label>SSH 用户<input value={deploy.user} onChange={(event) => setDeploy({ ...deploy, user: event.target.value })} /></label><label>SSH 私钥绝对路径<input value={deploy.identityFile} placeholder="C:\\Users\\你\\.ssh\\id_ed25519" onChange={(event) => setDeploy({ ...deploy, identityFile: event.target.value })} /></label></div><div className="remote-actions"><button className="button button-primary" disabled={busy !== null || !deploy.host.trim() || !deploy.user.trim() || !deploy.identityFile.trim()} onClick={() => void deployServer()}><Play size={15} />{busy === "deploy" ? "SSH 部署中…" : "确认并部署"}</button><span className="soft-badge">RustDesk Server 1.1.16</span></div>{deployReport && <div className="deploy-result"><div><strong>容器状态</strong><span>{deployReport.status}</span></div><div><strong>ID Server</strong><span>{deployReport.idServer}</span></div><div><strong>Relay Server</strong><span>{deployReport.relayServer}</span></div><div><strong>最小端口</strong><span>{deployReport.requiredPorts.join(" · ")}</span></div><div className="span-two"><strong>公钥</strong><code>{deployReport.publicKey}</code></div><div className="span-two remote-actions"><button className="button button-secondary" disabled={busy !== null} onClick={() => void copyClientSettings()}><Save size={14} />复制客户端参数</button><button className="button button-primary" disabled={busy !== null} onClick={() => void configureVpsFirewall("apply")}><ShieldCheck size={14} />{busy === "vpsFirewall" ? "SSH 提交中…" : "开放最小端口"}</button><button className="button button-secondary" disabled={busy !== null} onClick={() => void configureVpsFirewall("rollback")}><ArchiveRestore size={14} />回滚端口</button></div></div>}</section>{firewallReport && <section className="panel firewall-result"><div className="panel-heading"><div><span className="eyebrow">已验证提交</span><h2>{firewallReport.target}</h2></div><span className="soft-badge success">{firewallReport.action === "apply" ? "已应用" : "已回滚"}</span></div><div className="firewall-rule-grid">{firewallReport.rules.map((rule) => <div key={`${rule.protocol}-${rule.ports}`}><strong>{rule.label}</strong><span>{rule.protocol} · {rule.ports}</span><small>{rule.scope}</small></div>)}</div><p className="section-copy">{firewallReport.message}</p></section>}
    <section className="panel"><div className="panel-heading"><div><span className="eyebrow">画质参考</span><h2>实测后自动匹配档位</h2></div><span className="soft-badge">{quality ? `建议：${quality.resolution} ${quality.fps}FPS · ${quality.bitrateMbps}Mbps` : "尚未实测"}</span></div><div className="profile-grid">{profiles.map(({ name, detail, icon: Icon }) => <article className={quality?.recommendedPreset === name ? "profile-card active" : "profile-card"} key={name}><Icon /><strong>{name}</strong><span>{detail}</span></article>)}</div></section><section className="purchase-grid"><ReferralPurchaseCard placement="remote-control" offers={offers} onOpen={onReferralOpen} onConfigure={onConfigureReferrals} /></section></>;
}

function MarketplacePage({ offers, setOffers, onReferralOpen, syncMessage }: { offers: ReferralOffer[]; setOffers: (value: ReferralOffer[]) => void; onReferralOpen: (offer: ReferralOffer) => Promise<void>; syncMessage: string }) {
  const [editing, setEditing] = useState<ReferralOffer | null>(null);
  const [saved, setSaved] = useState(false);
  const [qrExpanded, setQrExpanded] = useState(false);
  const errors = editing ? validateOffer(editing) : [];
  const sortedOffers = [...offers].sort((left, right) => right.priority - left.priority || left.title.localeCompare(right.title));
  const marketplaceSections: Array<{ placement: ReferralOffer["placement"]; title: string; description: string }> = [
    { placement: "download-vpn", title: "大流量线路", description: "八戒与魔戒按套餐方式互补，适合下载、视频与日常境外流量。" },
    { placement: "stable-vpn", title: "稳定路线出口", description: "推荐 VPS 作为可控、长期使用的固定出口。" },
    { placement: "vps", title: "其他 VPS 服务", description: "用户自行配置的 VPS 推荐。" },
    { placement: "remote-control", title: "远程控制", description: "远程连接与自建中继服务。" },
    { placement: "general", title: "其他推荐", description: "未归入专门场景的服务。" },
  ];
  const createOffer = (): ReferralOffer => ({ id: crypto.randomUUID(), category: "vps", placement: "vps", provider: "", title: "", summary: "", badge: "推荐", price: "", url: "", coupon: "", disclosure: "此链接可能属于合作推广，购买可能产生佣金，用户价格不受影响。", enabled: false, priority: 50, clickCount: 0, origin: "local" });
  const saveEditing = () => {
    if (!editing || editing.origin === "publisher" || errors.length > 0) return;
    setOffers(offers.some((offer) => offer.id === editing.id) ? offers.map((offer) => offer.id === editing.id ? editing : offer) : [...offers, editing]);
    setSaved(true);
  };
  const deleteEditing = () => {
    if (!editing || editing.origin === "publisher" || !offers.some((offer) => offer.id === editing.id)) return;
    if (!window.confirm(`删除推荐项“${editing.title}”？`)) return;
    setOffers(offers.filter((offer) => offer.id !== editing.id));
    setEditing(null);
  };
  return <>
    <PageHeader eyebrow="支持独立开发" title="推荐与赞赏" description="赞赏和返利链接分开展示，关系透明，选择权始终在你。" action={<button className="button button-primary" onClick={() => { setEditing(createOffer()); setSaved(false); }}><Plus size={16} /> 新增推荐项</button>} />
    <section className="support-grid"><article className="panel donation-card"><div className="support-copy"><Coffee /><div><span className="eyebrow">直接赞赏</span><h2>请开发者喝杯咖啡</h2><p>如果 AI Route Manager 帮你省下了排查和配置的时间，欢迎自愿赞赏。每一份支持都会用于继续维护、修复问题和完善文档。感谢你只为真正有用的东西付费。</p></div></div><div className="donation-qr"><button className="donation-qr-button" onClick={() => setQrExpanded(true)} aria-label="放大赞赏收款码"><SupportQr /></button><span>点击放大 · 自愿赞赏</span></div></article><article className="panel referral-intro"><ShoppingBag /><div><span className="eyebrow">推荐返利</span><h2>正好需要时，也可以这样支持</h2><p>如果你确实需要下方服务，欢迎通过明确标注的推荐链接下单；你的价格不会因此增加，开发者可能获得平台返利。不为返利硬推，不需要就不买——站着把钱挣了，也把话说明白。</p><div className="support-metrics"><span><strong>{offers.filter((offer) => offer.enabled).length}</strong>个已启用推荐</span><span><strong>{offers.reduce((sum, offer) => sum + offer.clickCount, 0)}</strong>次本机打开</span></div></div></article></section>
    {syncMessage && <section className="sync-message"><StatusDot status={syncMessage.includes("失败") ? "warning" : "healthy"} />{syncMessage}</section>}
    {marketplaceSections.map((section) => {
      const sectionOffers = sortedOffers.filter((offer) => offer.placement === section.placement);
      if (sectionOffers.length === 0) return null;
      return <section className="marketplace-section" key={section.placement}><header className="marketplace-section-header"><div><span className="eyebrow">{placementLabels[section.placement]}</span><h2>{section.title}</h2><p>{section.description}</p></div><span className="soft-badge success">{sectionOffers.length} 项推荐</span></header><div className="offer-grid">{sectionOffers.map((offer) => <article className="offer-card" key={offer.id}><div className="offer-top"><span className="offer-badge">{offer.origin === "publisher" ? "签名发行" : offer.badge}</span><span className={offer.enabled ? "soft-badge success" : "soft-badge"}>{offer.enabled ? "展示中" : "已停用"}</span></div><h3>{offer.title}</h3><strong className="offer-provider">{offer.provider}</strong><span className="placement-chip">{placementLabels[offer.placement]} · 优先级 {offer.priority}</span>{offer.origin === "publisher" && <span className="signed-catalog-chip"><ShieldCheck size={12} />目录 {offer.catalogVersion}</span>}<p>{offer.summary}</p><div className="offer-price">{offer.price}</div>{offer.coupon && <div className="coupon">优惠码：{offer.coupon}</div>}<small>{offer.disclosure}</small><div className="offer-clicks">本次运行已打开 {offer.clickCount} 次</div><div className={offer.origin === "publisher" ? "offer-actions publisher" : "offer-actions"}>{offer.origin !== "publisher" && <button className="button button-secondary" onClick={() => { setEditing(offer); setSaved(false); }}>编辑</button>}<button className="button button-primary" disabled={!offer.enabled || validateOffer(offer).length > 0} onClick={() => void onReferralOpen(offer)}>查看服务</button></div></article>)}</div></section>;
    })}
    <section className="panel disclosure-panel"><ShieldCheck /><div><strong>推荐说明</strong><p>推荐排序综合服务质量、稳定性与价格；每个返利链接都必须单独披露关系。购买和售后由服务商负责，用户始终可以填写自己的服务商，模板默认停用。</p></div></section>
    {qrExpanded && <div className="modal-backdrop" onClick={() => setQrExpanded(false)}><div className="modal donation-qr-modal" role="dialog" aria-modal="true" aria-labelledby="donation-qr-title" onClick={(event) => event.stopPropagation()}><div className="modal-header"><h2 id="donation-qr-title">自愿赞赏</h2><button className="icon-button" onClick={() => setQrExpanded(false)} aria-label="关闭收款码"><X /></button></div><SupportQr /><p>请使用支付应用扫码，核对收款方和金额后再确认。感谢你的支持！</p></div></div>}
    {editing && <div className="modal-backdrop"><div className="modal"><div className="modal-header"><div><span className="eyebrow">推荐项编辑器</span><h2>编辑推荐</h2></div><button className="icon-button" onClick={() => setEditing(null)}><X /></button></div><div className="form-grid"><label>类型<select value={editing.category} onChange={(event) => setEditing({ ...editing, category: event.target.value as ReferralOffer["category"] })}><option value="vps">VPS</option><option value="vpn">VPN</option><option value="hardware">硬件</option><option value="software">软件</option></select></label><label>展示位置<select value={editing.placement} onChange={(event) => setEditing({ ...editing, placement: event.target.value as ReferralOffer["placement"] })}><option value="vps">固定出口 VPS</option><option value="stable-vpn">稳定路线出口</option><option value="download-vpn">大流量线路</option><option value="remote-control">远程控制</option><option value="general">通用推荐</option></select></label><label>服务商<input value={editing.provider} onChange={(event) => setEditing({ ...editing, provider: event.target.value })} /></label><label>优先级（0-100）<input type="number" min={0} max={100} step={1} value={editing.priority} onChange={(event) => setEditing({ ...editing, priority: Number(event.target.value) })} /></label><label className="span-two">标题<input value={editing.title} onChange={(event) => setEditing({ ...editing, title: event.target.value })} /></label><label className="span-two">服务地址<input value={editing.url} placeholder="https://服务商.example/ref/你的代码" onChange={(event) => setEditing({ ...editing, url: event.target.value })} /></label><label>价格说明<input value={editing.price} onChange={(event) => setEditing({ ...editing, price: event.target.value })} /></label><label>优惠码<input value={editing.coupon} onChange={(event) => setEditing({ ...editing, coupon: event.target.value })} /></label><label className="span-two">推荐说明<textarea value={editing.summary} onChange={(event) => setEditing({ ...editing, summary: event.target.value })} /></label><label className="span-two">链接关系说明<textarea value={editing.disclosure} onChange={(event) => setEditing({ ...editing, disclosure: event.target.value })} /></label></div>{errors.length > 0 && <div className="validation-box">{errors.map((error) => <span key={error}>{error}</span>)}</div>}{saved && <div className="success-box"><Check />推荐项已保存并等待本机持久化</div>}<div className="modal-actions">{offers.some((offer) => offer.id === editing.id) && <button className="button button-danger" onClick={deleteEditing}>删除</button>}<label className="switch-label"><button className={editing.enabled ? "toggle on" : "toggle"} onClick={() => setEditing({ ...editing, enabled: !editing.enabled })}><span /></button>启用推荐项</label><button className="button button-primary" disabled={errors.length > 0} onClick={saveEditing}>保存推荐</button></div></div></div>}
  </>;
}

function DiagnosticsPage({ snapshot, engine, supervisor, refresh, loading, onRestored, onRepairManaged, onSupervisorChange }: { snapshot: SystemSnapshot | null; engine: EngineReport | null; supervisor: SupervisorStatus | null; refresh: () => void; loading: boolean; onRestored: () => Promise<void>; onRepairManaged: () => Promise<boolean>; onSupervisorChange: (status: SupervisorStatus) => void }) {
  const [backup, setBackup] = useState<LocalBackupStatus | null>(null);
  const [exported, setExported] = useState<DiagnosticExportReport | null>(null);
  const [busy, setBusy] = useState<"export" | "backup" | "restore" | "managed" | "supervisor" | null>(null);
  const [message, setMessage] = useState("");
  useEffect(() => { void getLocalBackupStatus().then(setBackup); }, []);
  const run = async (kind: "export" | "backup" | "restore") => {
    if (kind === "restore" && !window.confirm("恢复会替换应用设置、线路凭据和推荐配置，但不会切换当前网络。确认恢复吗？")) return;
    setBusy(kind); setMessage("");
    try {
      if (kind === "export") {
        const report = await exportDiagnostics(); setExported(report); setMessage(report.message);
      } else if (kind === "backup") {
        const report = await createLocalBackup(); setBackup(report.status); setMessage(report.message);
      } else {
        const report = await restoreLocalBackup(); setBackup(report.status); setMessage(report.message); await onRestored();
      }
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const port = (value: number) => snapshot?.ports.find((item) => item.port === value);
  const portCheck = (label: string, value: number) => {
    const item = port(value);
    return [label, item ? `127.0.0.1:${value} · ${item.reachable ? "监听中" : "未监听"}` : `127.0.0.1:${value} · 待检测`, item ? (item.reachable ? "healthy" : "warning") : "idle"];
  };
  const mainEntry = portCheck("系统代理入口", 17901);
  const flClash = portCheck("FlClash 外部热备", 7890);
  const telegram = portCheck("Telegram 外部专线", 17896);
  const checks: Array<{ name: string; value: string; status: string; action?: "managed" | "supervisor"; actionLabel?: string }> = [
    { name: mainEntry[0], value: mainEntry[1], status: mainEntry[2], ...((port(17901)?.reachable ?? false) ? {} : { action: "managed" as const, actionLabel: "修复入口" }) },
    { name: flClash[0], value: `${flClash[1]} · 可选外部程序`, status: port(7890)?.reachable ? "healthy" : "idle" },
    { name: telegram[0], value: `${telegram[1]} · 可选外部程序`, status: port(17896)?.reachable ? "healthy" : "idle" },
    { name: "本机分流引擎", value: engine?.message ?? "启动桌面端后读取", status: engine?.success ? "healthy" : engine?.available ? "warning" : "idle", ...(engine?.success ? {} : { action: "managed" as const, actionLabel: "验证并修复" }) },
    { name: "运行模式", value: snapshot?.previewOnly ? "安全预览：未接管系统代理" : "托管模式", status: snapshot ? "healthy" : "idle" },
    { name: "崩溃恢复监督器", value: supervisor?.active ? `${supervisor.restartPolicy} · 已恢复 ${supervisor.restartCount} 次` : supervisor?.lastError ?? "心跳未运行", status: supervisor?.active ? "healthy" : "warning", ...(supervisor?.active ? {} : { action: "supervisor" as const, actionLabel: "恢复监督器" }) },
  ];
  const repair = async (action: "managed" | "supervisor") => {
    setBusy(action); setMessage("");
    try {
      if (action === "managed") {
        if (!await onRepairManaged()) return;
        setMessage("入口修复流程已完成，正在重新读取运行状态。");
      } else {
        const status = await repairSupervisor();
        onSupervisorChange(status);
        setMessage("监督器已重新注册并恢复心跳。");
      }
      refresh();
    } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(null); }
  };
  const lines = [
    `${new Date().toLocaleTimeString()}  CHECK  desktop_runtime  ${snapshot ? "PASS" : "WAIT"}`,
    ...snapshot?.ports.map((item) => `${new Date().toLocaleTimeString()}  CHECK  port_${item.port}  ${item.reachable ? "PASS" : "CLOSED"}`) ?? [],
    `${new Date().toLocaleTimeString()}  CHECK  routing_engine  ${engine?.success ? "PASS" : engine?.available ? "ERROR" : "WAIT"}`,
    `${new Date().toLocaleTimeString()}  CHECK  crash_supervisor  ${supervisor?.active ? "PASS" : "WAIT"}`,
    `${new Date().toLocaleTimeString()}  INFO   recovery_count  ${supervisor?.restartCount ?? 0}`,
    `${new Date().toLocaleTimeString()}  NOTE   no network configuration changed`,
  ];
  return <><PageHeader eyebrow="诊断中心" title="检测后直接修复" description="必需组件提供修复按钮；外部可选程序只显示实测状态，不把未运行当成故障。" action={<button className="button button-primary" onClick={refresh} disabled={loading || busy !== null}><HeartPulse size={16} /> {loading ? "正在读取…" : "刷新本机实测"}</button>} /><section className="diagnostic-grid">{checks.map((check) => <article className="diagnostic-card" key={check.name}><StatusDot status={check.status} /><div><span>{check.name}</span><strong>{check.value}</strong>{check.action && <button className="button button-secondary diagnostic-action" disabled={busy !== null} onClick={() => void repair(check.action!)}>{busy === check.action ? "修复中…" : check.actionLabel}</button>}</div></article>)}</section><section className="recovery-grid"><article className="panel recovery-card"><PackageOpen /><div><span className="eyebrow">隐私诊断</span><h3>导出脱敏诊断包</h3><p>只写结构化本机状态，不收集原始日志、订阅 URL、密码、Cookie 或远端节点地址。</p>{exported && <small>{exported.directory}</small>}</div><button className="button button-secondary" disabled={busy !== null} onClick={() => void run("export")}><Download size={15} />{busy === "export" ? "正在导出…" : "导出诊断"}</button></article><article className="panel recovery-card"><Save /><div><span className="eyebrow">本机保险点</span><h3>配置备份</h3><p>线路 Token 和 VPS 密码只保存到系统凭据库；磁盘不会生成明文密钥。</p><small>{backup?.available && backup.createdAtMs ? `最近备份：${new Date(backup.createdAtMs).toLocaleString()} · ${backup.subscriptionCount} 个订阅` : backup?.message ?? "正在读取备份状态…"}</small></div><button className="button button-secondary" disabled={busy !== null} onClick={() => void run("backup")}><Save size={15} />{busy === "backup" ? "正在备份…" : "创建备份"}</button></article><article className="panel recovery-card"><ArchiveRestore /><div><span className="eyebrow">可验证恢复</span><h3>恢复最近备份</h3><p>只恢复应用设置和凭据，不碰当前系统代理；之后仍需主动“验证并应用”。</p><small>{backup?.available ? "备份可用" : "需要先创建备份"}</small></div><button className="button button-secondary" disabled={busy !== null || !backup?.available} onClick={() => void run("restore")}><ArchiveRestore size={15} />{busy === "restore" ? "正在恢复…" : "恢复配置"}</button></article></section>{message && <section className="sync-message"><StatusDot status={message.includes("失败") || message.includes("无法") ? "warning" : "healthy"} />{message}</section>}<section className="panel terminal-panel"><div className="terminal-top"><span /><span /><span /><strong>最近验证 · 只读</strong></div><pre>{lines.join("\n")}</pre></section></>;
}

function SettingsPage({ preferences, setPreferences, syncMessage, updateReport, updateMessage, updateBusy, onCheckUpdate, onInstallUpdate }: { preferences: AppPreferences; setPreferences: (value: AppPreferences) => void; syncMessage: string; updateReport: UpdateCheckReport | null; updateMessage: string; updateBusy: "check" | "install" | null; onCheckUpdate: () => void; onInstallUpdate: () => void }) {
  const setting = (title: string, desc: string, key: "strictAi" | "autoStart" | "autoCheckUpdates" | "allowSameRegionFallback" | "showPublisherOffers") => <div className="setting-row"><div><strong>{title}</strong><span>{desc}</span></div><button className={preferences[key] ? "toggle on" : "toggle"} onClick={() => setPreferences({ ...preferences, [key]: !preferences[key] })}><span /></button></div>;
  const endpointError = updateEndpointError(preferences.updateEndpoint);
  return <><PageHeader eyebrow="应用设置" title="安全、启动与隐私" description="所有高风险操作都需要预演、确认和可验证回滚。" /><section className="legal-notice"><ShieldCheck /><div><strong>合法使用声明</strong><p>AI Route Manager 只是管理用户自行合法取得的网络连接、路由策略与远程工具的管理软件，不提供节点、账号、翻墙、破解、越狱或其他违法服务。使用者必须遵守所在地法律法规及第三方服务条款，并自行确认配置、访问与使用行为合法合规。因违规或不当使用产生的责任由使用者承担。</p></div></section>{syncMessage && <section className="sync-message"><StatusDot status={syncMessage.includes("失败") ? "warning" : "healthy"} />{syncMessage}</section>}<section className="settings-layout"><article className="panel"><h2>路由保护</h2>{setting("AI 严格出口", "固定出口失效时绝不回落到 DIRECT。", "strictAi")}{setting("允许同地区兜底", "仅使用已经批准的同地区出口。", "allowSameRegionFallback")}{setting("登录与崩溃自愈", "注册用户级监督任务；异常退出后自动重启并验证线路。", "autoStart")}</article><article className="panel"><h2>数据与隐私</h2>{setting("展示平台推荐", "只加载内置公钥签名的推荐目录；关闭后本机推荐项仍可使用。", "showPublisherOffers")}<div className="setting-row"><div><strong>诊断记录保留</strong><span>自动清理旧的脱敏运行记录。</span></div><select value={preferences.diagnosticsRetentionDays} onChange={(e) => setPreferences({ ...preferences, diagnosticsRetentionDays: Number(e.target.value) })}><option value={7}>7 天</option><option value={14}>14 天</option><option value={30}>30 天</option></select></div></article><article className="panel update-panel"><div className="panel-heading"><div><span className="eyebrow">签名更新</span><h2>版本与更新源</h2></div><ShieldCheck size={20} /></div>{setting("启动后自动检查", "只检查，不会未经确认自动安装。", "autoCheckUpdates")}<label>HTTPS 更新清单<input value={preferences.updateEndpoint} placeholder="https://你的域名/latest.json" onChange={(event) => setPreferences({ ...preferences, updateEndpoint: event.target.value })} /></label><small className={endpointError ? "field-hint warning" : "field-hint"}>{endpointError ?? "安装包必须通过内置公钥验证，更新源不能替换签名。"}</small><div className="update-actions"><button className="button button-secondary" disabled={updateBusy !== null || Boolean(endpointError)} onClick={onCheckUpdate}><Activity size={15} />{updateBusy === "check" ? "正在检查…" : "检查更新"}</button>{updateReport?.available && <button className="button button-primary" disabled={updateBusy !== null} onClick={onInstallUpdate}><Download size={15} />{updateBusy === "install" ? "正在下载…" : `安装 ${updateReport.version}`}</button>}</div>{updateMessage && <div className={updateMessage.includes("失败") || updateMessage.includes("错误") ? "update-result warning" : "update-result"}><strong>{updateReport?.available ? `当前 ${updateReport.currentVersion} → 新版 ${updateReport.version}` : "更新状态"}</strong><span>{updateMessage}</span>{updateReport?.notes && <p>{updateReport.notes}</p>}</div>}</article></section></>;
}

function App() {
  const [active, setActive] = useState<NavId>("dashboard");
  const [preferences, setPreferences] = useState(defaultPreferences);
  const [services, setServices] = usePersistentState("ai-route.services.v1", defaultServices);
  const [offers, setOffers] = useState<ReferralOffer[]>(defaultOffers);
  const [snapshot, setSnapshot] = useState<SystemSnapshot | null>(null);
  const [engine, setEngine] = useState<EngineReport | null>(null);
  const [managed, setManaged] = useState<ManagedStatus | null>(null);
  const [supervisor, setSupervisor] = useState<SupervisorStatus | null>(null);
  const [managedBusy, setManagedBusy] = useState(false);
  const [managedMessage, setManagedMessage] = useState("");
  const [preferenceSyncMessage, setPreferenceSyncMessage] = useState("");
  const [referralSyncMessage, setReferralSyncMessage] = useState("");
  const [publisherCatalog, setPublisherCatalog] = useState<PublisherReferralCatalogReport | null>(null);
  const [publisherCatalogMessage, setPublisherCatalogMessage] = useState("");
  const [pendingReferral, setPendingReferral] = useState<ReferralOffer | null>(null);
  const [referralOpenBusy, setReferralOpenBusy] = useState(false);
  const [updateReport, setUpdateReport] = useState<UpdateCheckReport | null>(null);
  const [updateMessage, setUpdateMessage] = useState("");
  const [updateBusy, setUpdateBusy] = useState<"check" | "install" | null>(null);
  const [autoUpdateChecked, setAutoUpdateChecked] = useState(false);
  const [diagnosticsLoading, setDiagnosticsLoading] = useState(false);
  const [nativeReady, setNativeReady] = useState(false);
  const [preferencePersistenceReady, setPreferencePersistenceReady] = useState(false);
  const [referralPersistenceReady, setReferralPersistenceReady] = useState(false);

  // The headless supervisor can recover or switch cores without a GUI action.
  // Never leave the dashboard displaying an obsolete migration failure.
  useEffect(() => {
    if (!nativeReady) return;
    let cancelled = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const [runtime, supervision] = await Promise.all([
          settleTask(getManagedStatus()), settleTask(getSupervisorStatus()),
        ]);
        if (cancelled) return;
        if (runtime.ok && runtime.value) {
          setManaged(runtime.value);
          if (runtime.value.running && runtime.value.listener) setManagedMessage("");
        }
        if (supervision.ok && supervision.value) setSupervisor(supervision.value);
      } finally {
        pending = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    window.addEventListener("focus", refresh);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, [nativeReady]);

  const refreshRuntime = () => {
    setDiagnosticsLoading(true);
    Promise.all([settleTask(getSystemSnapshot()), settleTask(getEngineStatus()), settleTask(getManagedStatus()), settleTask(getSupervisorStatus())])
      .then(([system, report, managedStatus, supervisorStatus]) => {
        if (system.ok && system.value) setSnapshot(system.value);
        if (report.ok && report.value) setEngine(report.value);
        if (managedStatus.ok && managedStatus.value) setManaged(managedStatus.value);
        if (supervisorStatus.ok && supervisorStatus.value) setSupervisor(supervisorStatus.value);
        const failures = [system, report, managedStatus, supervisorStatus].filter((result) => !result.ok);
        if (failures.length > 0) setManagedMessage(`状态刷新完成，${failures.length} 项读取失败`);
      })
      .finally(() => setDiagnosticsLoading(false));
  };

  useEffect(() => {
    Promise.all([
      settleTask(loadPreferences()),
      settleTask(getSystemSnapshot()),
      settleTask(getEngineStatus()),
      settleTask(getManagedStatus()),
      settleTask(getSupervisorStatus()),
      settleTask(loadReferralOffers()),
    ])
      .then(([stored, system, report, managedStatus, supervisorStatus, storedOffers]) => {
        if (stored.ok) {
          if (stored.value) setPreferences(stored.value);
          setPreferencePersistenceReady(true);
        } else {
          setPreferenceSyncMessage(`设置读取失败，已停止自动写回以保护原文件：${stored.error}`);
        }
        if (system.ok && system.value) setSnapshot(system.value);
        if (report.ok && report.value) setEngine(report.value);
        if (managedStatus.ok && managedStatus.value) setManaged(managedStatus.value);
        if (supervisorStatus.ok && supervisorStatus.value) setSupervisor(supervisorStatus.value);
        if (storedOffers.ok) {
          setOffers(normalizeReferralOffers(storedOffers.value ?? defaultOffers));
          setReferralPersistenceReady(true);
        } else {
          setReferralSyncMessage(`推荐配置读取失败，已停止自动写回以保护原文件：${storedOffers.error}`);
        }
      })
      .finally(() => setNativeReady(true));
  }, []);

  useEffect(() => {
    if (!nativeReady || !preferencePersistenceReady) return;
    setPreferenceSyncMessage("正在保存启动与路由设置…");
    const timer = window.setTimeout(() => {
      void savePreferences(preferences)
        .then(() => setPreferenceSyncMessage("启动与路由设置已安全保存"))
        .catch((error) => setPreferenceSyncMessage(`设置保存失败：${error instanceof Error ? error.message : String(error)}`));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [nativeReady, preferencePersistenceReady, preferences]);

  useEffect(() => {
    if (!nativeReady || !referralPersistenceReady) return;
    setReferralSyncMessage("正在保存推荐配置…");
    const timer = window.setTimeout(() => {
      const localOffers = normalizeReferralOffers(offers.filter((offer) => offer.origin !== "publisher"));
      void saveReferralOffers(localOffers)
        .then(() => setReferralSyncMessage("推荐配置已安全保存到本机"))
        .catch((error) => setReferralSyncMessage(`推荐配置保存失败：${error instanceof Error ? error.message : String(error)}`));
    }, 350);
    return () => window.clearTimeout(timer);
  }, [nativeReady, referralPersistenceReady, offers]);

  useEffect(() => {
    if (!nativeReady) return;
    let cancelled = false;
    if (!preferences.showPublisherOffers) {
      setOffers((current) => current.filter((offer) => offer.origin !== "publisher"));
      setPublisherCatalog(null);
      setPublisherCatalogMessage("平台推荐已由用户关闭；本机推荐项继续可用");
      return;
    }
    setPublisherCatalogMessage("正在下载并验证签名发行推荐目录…");
    void loadPublisherReferralCatalog(officialReferralCatalogEndpoint)
      .then((report) => {
        if (cancelled || !report) return;
        setPublisherCatalog(report);
        setOffers((current) => [
          ...current.filter((offer) => offer.origin !== "publisher"),
          ...normalizeReferralOffers(report.offers),
        ]);
        setPublisherCatalogMessage(`${report.message}；版本 ${report.catalogVersion}，${report.offers.length} 个推荐位`);
      })
      .catch((error) => {
        if (cancelled) return;
        setPublisherCatalog(null);
        setOffers((current) => current.filter((offer) => offer.origin !== "publisher"));
        setPublisherCatalogMessage(`发行推荐目录验证失败，已拒绝加载：${error instanceof Error ? error.message : String(error)}`);
      });
    return () => { cancelled = true; };
  }, [nativeReady, preferences.showPublisherOffers]);

  const handleReferralOpen = async (offer: ReferralOffer) => {
    const errors = validateOffer(offer);
    if (!offer.enabled || errors.length > 0) {
      setReferralSyncMessage(`推荐链接未通过校验：${errors.join("；") || "推荐位未启用"}`);
      return;
    }
    setPendingReferral(offer);
  };

  const confirmReferralOpen = async () => {
    if (!pendingReferral || referralOpenBusy) return;
    setReferralOpenBusy(true);
    try {
      await openExternalUrl(pendingReferral.url);
      setOffers((current) => current.map((item) => item.id === pendingReferral.id ? { ...item, clickCount: item.clickCount + 1 } : item));
      setReferralSyncMessage(`已打开 ${pendingReferral.provider} 服务页面，本机打开次数 +1`);
      setPendingReferral(null);
    } catch (error) {
      setReferralSyncMessage(`购买页打开失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setReferralOpenBusy(false);
    }
  };

  const runUpdateCheck = async (automatic = false) => {
    const error = updateEndpointError(preferences.updateEndpoint);
    if (error) {
      if (!automatic) setUpdateMessage(error);
      return;
    }
    setUpdateBusy("check");
    if (!automatic) setUpdateMessage("正在读取并验证更新清单…");
    try {
      const result = await checkForUpdates(normalizeUpdateEndpoint(preferences.updateEndpoint));
      setUpdateReport(result);
      setUpdateMessage(result.message);
    } catch (errorValue) {
      setUpdateReport(null);
      setUpdateMessage(`检查更新失败：${errorValue instanceof Error ? errorValue.message : String(errorValue)}`);
    } finally {
      setUpdateBusy(null);
    }
  };

  const runUpdateInstall = async () => {
    if (!updateReport?.available || !window.confirm(`安装签名版本 ${updateReport.version}？Windows 安装时应用会自动退出，现有独立代理不会被停止。`)) return;
    setUpdateBusy("install");
    let downloaded = 0;
    let total: number | null = null;
    try {
      await installPendingUpdate((event) => {
        if (event.event === "started") {
          total = event.data.contentLength;
          setUpdateMessage("已验证签名，开始下载更新…");
        } else if (event.event === "progress") {
          downloaded += event.data.chunkLength;
          setUpdateMessage(updateProgressLabel(downloaded, total));
        } else {
          setUpdateMessage("下载完成，正在启动受信任安装程序…");
        }
      });
      setUpdateMessage("更新安装完成；应用将在安装程序结束后重新启动");
    } catch (errorValue) {
      setUpdateMessage(`更新安装失败：${errorValue instanceof Error ? errorValue.message : String(errorValue)}`);
    } finally {
      setUpdateBusy(null);
    }
  };

  useEffect(() => {
    if (!nativeReady || autoUpdateChecked || !preferences.autoCheckUpdates || updateEndpointError(preferences.updateEndpoint)) return;
    setAutoUpdateChecked(true);
    const timer = window.setTimeout(() => void runUpdateCheck(true), 1200);
    return () => window.clearTimeout(timer);
  }, [nativeReady, autoUpdateChecked, preferences.autoCheckUpdates, preferences.updateEndpoint]);

  const handleManagedAction = async () => {
    const stopping = Boolean(managed?.running && managed?.listener);
    const migrating = !stopping && Boolean(managed?.legacyConflict);
    const confirmed = window.confirm(stopping
      ? "停止托管后会恢复接管前的系统代理，并清除运行配置中的凭据。继续吗？"
      : migrating
        ? "迁移会先完整预演，再暂停旧版守护；新版通过全部探测后才切换，失败会恢复旧版。继续吗？"
        : "应用前会先做完整隔离预演；只有全部探测通过才会接管系统代理。继续吗？");
    if (!confirmed) return;
    setManagedBusy(true); setManagedMessage("");
    try {
      const report = stopping ? await stopManagedMode() : migrating ? await migrateLegacyMode() : await startManagedMode();
      setManaged(report.status); setManagedMessage(report.message);
      if (report.success) setPreferences({ ...preferences, mode: report.status.running ? "managed" : "preview", autoStart: migrating ? true : preferences.autoStart });
      const [system, supervisorStatus] = await Promise.all([getSystemSnapshot(), getSupervisorStatus()]);
      if (system) setSnapshot(system);
      if (supervisorStatus) setSupervisor(supervisorStatus);
    } catch (error) {
      setManagedMessage(error instanceof Error ? error.message : String(error));
      const status = await getManagedStatus();
      if (status) setManaged(status);
    } finally { setManagedBusy(false); }
  };

  const reloadRestoredConfiguration = async () => {
    const [storedPreferences, storedOffers] = await Promise.all([loadPreferences(), loadReferralOffers()]);
    if (storedPreferences) setPreferences(storedPreferences);
    if (storedOffers) setOffers((current) => [...normalizeReferralOffers(storedOffers), ...current.filter((offer) => offer.origin === "publisher")]);
    refreshRuntime();
  };

  const repairManagedFromDiagnostics = async () => {
    if (!window.confirm("重新执行隔离预演并修复托管入口？验证通过后会恢复系统代理入口。")) return false;
    setManagedBusy(true); setManagedMessage("");
    try {
      const report = await startManagedMode();
      setManaged(report.status); setManagedMessage(report.message);
      const [system, engineStatus] = await Promise.all([getSystemSnapshot(), getEngineStatus()]);
      if (system) setSnapshot(system);
      if (engineStatus) setEngine(engineStatus);
      return report.success;
    } finally { setManagedBusy(false); }
  };

  const visibleOffers = offers.filter((offer) => offer.origin !== "publisher" || preferences.showPublisherOffers);
  const catalogStatus = publisherCatalog?.signatureVerified ? publisherCatalogMessage : publisherCatalogMessage;
  const content = active === "sources" ? <SourcesPage offers={visibleOffers} onReferralOpen={handleReferralOpen} onConfigureReferrals={() => setActive("marketplace")} /> : active === "services" ? <ServicesPage services={services} setServices={setServices} /> : active === "devices" ? <DevicesPage /> : active === "remote" ? <RemotePage offers={visibleOffers} onReferralOpen={handleReferralOpen} onConfigureReferrals={() => setActive("marketplace")} /> : active === "marketplace" ? <MarketplacePage offers={visibleOffers} setOffers={setOffers} onReferralOpen={handleReferralOpen} syncMessage={[referralSyncMessage, catalogStatus].filter(Boolean).join(" · ")} /> : active === "diagnostics" ? <DiagnosticsPage snapshot={snapshot} engine={engine} supervisor={supervisor} refresh={refreshRuntime} loading={diagnosticsLoading} onRestored={reloadRestoredConfiguration} onRepairManaged={repairManagedFromDiagnostics} onSupervisorChange={setSupervisor} /> : active === "settings" ? <SettingsPage preferences={preferences} setPreferences={setPreferences} syncMessage={preferenceSyncMessage} updateReport={updateReport} updateMessage={updateMessage} updateBusy={updateBusy} onCheckUpdate={() => void runUpdateCheck()} onInstallUpdate={() => void runUpdateInstall()} /> : <Dashboard preferences={preferences} setPreferences={setPreferences} services={services} snapshot={snapshot} managed={managed} managedBusy={managedBusy} managedMessage={managedMessage} onManagedAction={() => void handleManagedAction()} />;
  return <div className="app-shell"><aside className="sidebar"><div className="brand"><div className="brand-mark"><Cable size={22} /></div><div><strong>AI Route</strong><span>智能分流管家</span></div></div><nav>{navigation.map(({ id, label, icon: Icon }) => <button key={id} className={active === id ? "nav-item active" : "nav-item"} onClick={() => setActive(id)}><Icon size={18} /><span>{label}</span>{id === "marketplace" && <i>精选</i>}</button>)}</nav><div className="sidebar-bottom"><div className="core-status"><div><Cpu size={16} /><span>核心状态</span></div><strong><StatusDot status={managed?.running ? "healthy" : "idle"} />{managed?.running ? "托管运行中" : managed?.legacyConflict ? "旧版待迁移" : "安全预览"}</strong></div><button className="support-card" onClick={() => setActive("diagnostics")}><Database size={19} /><div><strong>备份与恢复</strong><span>系统凭据库保险点</span></div><ChevronRight size={16} /></button></div></aside><main className="main-content"><div className="topbar"><div className="breadcrumb"><span>AI Route</span><ChevronRight size={14} /><strong>{navigation.find((item) => item.id === active)?.label}</strong></div><div className="top-actions"><span className="preview-pill"><LockKeyhole size={13} />{managed?.running ? "托管模式" : "预览模式"}</span><span className="runtime-indicator" title={snapshot ? "桌面后端已连接" : "正在连接桌面后端"}><Wifi size={18} /><StatusDot status={snapshot ? "healthy" : "idle"} /></span><span className="avatar" aria-label="本机用户">X</span></div></div><div className="page-content">{content}</div></main>{pendingReferral && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !referralOpenBusy) setPendingReferral(null); }}><div className="modal purchase-confirm-modal" role="dialog" aria-modal="true" aria-labelledby="purchase-confirm-title"><div className="modal-header"><div><span className="eyebrow">离开前确认</span><h2 id="purchase-confirm-title">打开服务商页面</h2></div><button className="icon-button" disabled={referralOpenBusy} onClick={() => setPendingReferral(null)} aria-label="关闭"><X /></button></div><div className="purchase-confirm-summary"><div className="purchase-confirm-icon"><ShoppingBag /></div><div><strong>{pendingReferral.title}</strong><span>{pendingReferral.provider}</span></div></div><dl className="purchase-confirm-facts"><div><dt>价格说明</dt><dd>{pendingReferral.price || "以服务商页面为准"}</dd></div>{pendingReferral.coupon && <div><dt>优惠码</dt><dd><code>{pendingReferral.coupon}</code></dd></div>}<div><dt>目标网站</dt><dd>{referralDestinationHost(pendingReferral.url)}</dd></div></dl><div className="referral-disclosure"><ShieldCheck /><div><strong>链接关系说明</strong><p>{pendingReferral.disclosure}</p><span>购买、付款和售后由服务商完成；本软件只记录本机打开次数，不收集订单和支付信息。</span></div></div><div className="modal-actions purchase-confirm-actions"><button className="button button-secondary" disabled={referralOpenBusy} onClick={() => setPendingReferral(null)}>取消</button><button className="button button-primary" disabled={referralOpenBusy} onClick={() => void confirmReferralOpen()}><ExternalLink size={15} />{referralOpenBusy ? "正在打开…" : "确认并打开服务商页面"}</button></div></div></div>}</div>;
}

export default App;
