#!/usr/bin/env python3
"""Dynamic subscription router with guarded A/B Mihomo cutover on Windows."""

from __future__ import annotations

import argparse
import base64
import ctypes
import hashlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import quote

import requests
import yaml

if os.name == "nt":
    import winreg


APP_NAME = "DynamicProxyManager"
DEFAULT_HOME = Path(os.environ.get("LOCALAPPDATA", Path.home())) / APP_NAME
try:
    SLOT_OFFSET = int(os.environ.get("DPM_SLOT_OFFSET", "0"))
except ValueError:
    SLOT_OFFSET = 0
if not 0 <= SLOT_OFFSET <= 20000:
    SLOT_OFFSET = 0
try:
    FRONTEND_PORT = int(os.environ.get("DPM_FRONTEND_PORT", "17901"))
except ValueError:
    FRONTEND_PORT = 17901
if not 0 <= FRONTEND_PORT <= 65535:
    FRONTEND_PORT = 17901
SLOTS = (
    {"name": "A", "proxy_port": 17898 + SLOT_OFFSET, "controller_port": 19098 + SLOT_OFFSET, "dns_port": 1054 + SLOT_OFFSET},
    {"name": "B", "proxy_port": 17899 + SLOT_OFFSET, "controller_port": 19099 + SLOT_OFFSET, "dns_port": 1055 + SLOT_OFFSET},
)
AI_TEST_URL = "https://api.openai.com/v1/models"
AI_EXPECTED_STATUS = 401
AI_REGION_TRACE_URL = "https://chatgpt.com/cdn-cgi/trace"
LEGACY_AI_REGION = "SG"
LEGACY_AI_REGION_LABEL = "新加坡"
LEGACY_PREFERRED_NODE_KEYWORDS = ("李家坡2-直连", "李家坡2-直連")
FIXED_EGRESS_GROUP = "AI-固定出口"
FIXED_EGRESS_DEFAULT_NAME = "AI-固定出口"
SSH_SOCKS_PORT = 18443
FULL_CHAIN_CANDIDATE_LIMIT = 6
TRANSPORT_TEST_URL = "https://www.google.com/generate_204"
TRANSPORT_EXPECTED_STATUS = 204
DOWNLOAD_TEST_URL = "https://telegram.org"
PROBES = (
    ("chatgpt", AI_TEST_URL, AI_EXPECTED_STATUS),
    ("claude", "https://claude.ai/cdn-cgi/trace", 200),
    ("google", "https://www.google.com/generate_204", 204),
    ("youtube", "https://www.youtube.com/generate_204", 204),
    ("telegram", "https://telegram.org", 200),
    ("baidu", "https://www.baidu.com", 200),
)

REGION_ALIASES: dict[str, tuple[str, ...]] = {
    "SG": ("新加坡", "李家坡", "加坡", "狮城", "singapore", "sg"),
    "US": ("美国", "美國", "洛杉矶", "洛杉磯", "西雅图", "纽约", "紐約", "united states", "usa", "us"),
    "JP": ("日本", "东京", "東京", "大阪", "japan", "tokyo", "osaka", "jp"),
    "TW": ("台湾", "台灣", "台北", "taiwan", "taipei", "tw"),
    "KR": ("韩国", "韓國", "首尔", "首爾", "korea", "seoul", "kr"),
    "GB": ("英国", "英國", "伦敦", "倫敦", "united kingdom", "britain", "london", "uk", "gb"),
    "DE": ("德国", "德國", "法兰克福", "法蘭克福", "germany", "frankfurt", "de"),
    "FR": ("法国", "法國", "巴黎", "france", "paris", "fr"),
    "CA": ("加拿大", "多伦多", "多倫多", "温哥华", "溫哥華", "canada", "toronto", "vancouver", "ca"),
    "AU": ("澳大利亚", "澳洲", "悉尼", "australia", "sydney", "au"),
    "NL": ("荷兰", "荷蘭", "阿姆斯特丹", "netherlands", "amsterdam", "nl"),
    "HK": ("香港", "hong kong", "hk"),
}

for stream in (sys.stdin, sys.stdout, sys.stderr):
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding="utf-8", errors="strict" if stream is sys.stdin else "replace")


class ManagerError(RuntimeError):
    pass


class RunLock:
    """Cross-process singleton lock scoped to one manager home directory."""

    def __init__(self, home: Path):
        self.home = home.resolve()
        self.handle: Any = None
        self.file: Any = None

    def __enter__(self) -> "RunLock":
        self.home.mkdir(parents=True, exist_ok=True)
        if os.name == "nt":
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
            kernel32.CreateMutexW.restype = ctypes.c_void_p
            name = "Local\\AIRouteManager-" + hashlib.sha256(str(self.home).lower().encode("utf-8")).hexdigest()[:24]
            handle = kernel32.CreateMutexW(None, 0, name)
            if not handle:
                raise ManagerError("failed to create manager singleton mutex")
            if ctypes.get_last_error() == 183:
                kernel32.CloseHandle(handle)
                raise ManagerError("another manager is already running for this home")
            self.handle = handle
        else:
            import fcntl

            self.file = (self.home / "run.lock").open("a+b")
            try:
                fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                self.file.close()
                self.file = None
                raise ManagerError("another manager is already running for this home") from exc
        return self

    def __exit__(self, *_: Any) -> None:
        if self.handle:
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.CloseHandle(self.handle)
            self.handle = None
        if self.file:
            import fcntl

            fcntl.flock(self.file.fileno(), fcntl.LOCK_UN)
            self.file.close()
            self.file = None


def pid_exists(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        kernel32.OpenProcess.restype = ctypes.c_void_p
        handle = kernel32.OpenProcess(0x1000, 0, pid)
        if not handle:
            return False
        kernel32.CloseHandle(handle)
        return True
    try:
        os.kill(pid, 0)
        return True
    except (ProcessLookupError, PermissionError):
        return False


def mac_network_services() -> list[str]:
    if sys.platform != "darwin":
        return []
    result = subprocess.run(["networksetup", "-listallnetworkservices"], capture_output=True, text=True, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        raise ManagerError("networksetup could not list network services")
    return [line.strip() for line in result.stdout.splitlines()[1:] if line.strip() and not line.lstrip().startswith("*")]


def mac_proxy_details(service: str, secure: bool) -> dict[str, Any]:
    flag = "-getsecurewebproxy" if secure else "-getwebproxy"
    result = subprocess.run(["networksetup", flag, service], capture_output=True, text=True, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        raise ManagerError(f"networksetup could not inspect {service}")
    values: dict[str, str] = {}
    for line in result.stdout.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            values[key.strip().lower()] = value.strip()
    try:
        port = int(values.get("port", "0"))
    except ValueError:
        port = 0
    return {"enabled": values.get("enabled", "no").lower() == "yes", "server": values.get("server", ""), "port": port}


def set_mac_proxy(service: str, secure: bool, enabled: bool, server: str = "", port: int = 0) -> None:
    kind = "securewebproxy" if secure else "webproxy"
    if enabled:
        result = subprocess.run(["networksetup", f"-set{kind}", service, server, str(port)], capture_output=True, text=True, encoding="utf-8", errors="replace")
        if result.returncode == 0:
            result = subprocess.run(["networksetup", f"-set{kind}state", service, "on"], capture_output=True, text=True, encoding="utf-8", errors="replace")
    else:
        result = subprocess.run(["networksetup", f"-set{kind}state", service, "off"], capture_output=True, text=True, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        raise ManagerError(f"networksetup could not update {service}")


def local_stamp() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def atomic_write(path: Path, data: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(data, encoding="utf-8", newline="\n")
    os.replace(tmp, path)


def redact_message(text: str) -> str:
    text = re.sub(r"https?://[^\s]+", "<url-redacted>", text)
    return re.sub(r"(?i)(token=)[^&\s]+", r"\1<redacted>", text)


@dataclass
class Logger:
    path: Path

    def write(self, event: str, **fields: Any) -> None:
        safe = {k: redact_message(str(v)) for k, v in fields.items()}
        row = {"time": local_stamp(), "event": event, **safe}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
        stream = sys.stderr if os.environ.get("DPM_LOG_STDERR") == "1" else sys.stdout
        if stream is not None:
            print(json.dumps(row, ensure_ascii=False), file=stream, flush=True)


def load_json(path: Path, default: Any = None) -> Any:
    if not path.exists():
        return default
    return json.loads(path.read_text(encoding="utf-8"))


def save_json(path: Path, value: Any) -> None:
    atomic_write(path, json.dumps(value, ensure_ascii=False, indent=2))


def port_open(port: int, timeout: float = 0.5) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        return s.connect_ex(("127.0.0.1", port)) == 0


def listener_owner(port: int) -> int | None:
    """Return the PID that owns a TCP listener, if one can be proven."""
    if os.name != "nt":
        result = subprocess.run(
            ["lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN", "-t"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if result.returncode != 0:
            return None
        try:
            return int(result.stdout.splitlines()[0].strip())
        except (ValueError, IndexError):
            return None
    result = subprocess.run(
        ["netstat.exe", "-ano", "-p", "tcp"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    if result.returncode != 0:
        return None
    suffix = f":{port}"
    for line in result.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 5 and parts[0].upper() == "TCP" and parts[1].endswith(suffix) and parts[3].upper() == "LISTENING":
            try:
                return int(parts[-1])
            except ValueError:
                return None
    return None


def decode_subscription(content: bytes) -> dict[str, Any]:
    attempts = [content]
    compact = re.sub(rb"\s+", b"", content)
    try:
        attempts.append(base64.b64decode(compact + b"=" * (-len(compact) % 4), validate=False))
    except Exception:
        pass
    for raw in attempts:
        try:
            parsed = yaml.safe_load(raw.decode("utf-8-sig"))
        except Exception:
            continue
        if isinstance(parsed, dict) and isinstance(parsed.get("proxies"), list):
            return parsed
    raise ManagerError("subscription did not contain a Clash proxies list")


def unique_proxy_names(proxies: list[dict[str, Any]], prefix: str) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    seen: dict[str, int] = {}
    for item in proxies:
        if not isinstance(item, dict) or not item.get("name") or not item.get("type"):
            continue
        p = dict(item)
        base = f"[{prefix}] {str(p['name']).strip()}"
        seen[base] = seen.get(base, 0) + 1
        p["name"] = base if seen[base] == 1 else f"{base} #{seen[base]}"
        result.append(p)
    return result


def normalize_region(value: Any, default: str = LEGACY_AI_REGION) -> str:
    region = str(value or default).strip().upper()
    if not re.fullmatch(r"[A-Z]{2}", region):
        raise ManagerError("AI target region must be a two-letter ISO country code")
    return region


def region_name_pattern(region: str, label: str = "") -> re.Pattern[str]:
    aliases = list(REGION_ALIASES.get(normalize_region(region), ()))
    if label.strip():
        aliases.append(label.strip())
    words = [re.escape(item) for item in aliases if item and len(item) > 2]
    short = [re.escape(item) for item in aliases if item and len(item) <= 2]
    parts = words + [rf"(?:^|[^a-z]){item}(?:[^a-z]|$)" for item in short]
    return re.compile("(?i)(" + "|".join(parts or [r"(?!)"]) + ")")


def classify_stable(
    names: list[str],
    ai_region: str = LEGACY_AI_REGION,
    ai_region_label: str = LEGACY_AI_REGION_LABEL,
    preferred_keywords: list[str] | None = None,
) -> dict[str, list[str]]:
    region = region_name_pattern(ai_region, ai_region_label)
    direct = re.compile(r"(?i)(直連|直连|direct|直通)")
    keyword_source = LEGACY_PREFERRED_NODE_KEYWORDS if preferred_keywords is None else preferred_keywords
    keywords = [item.strip().casefold() for item in keyword_source if item.strip()]
    exact_names = [n for n in names if any(keyword in n.casefold() for keyword in keywords)]
    region_direct = [n for n in names if region.search(n) and direct.search(n)]
    region_any = [n for n in names if region.search(n)]
    return {
        "exact": exact_names,
        "region_direct": region_direct,
        "region_any": region_any,
        "all": list(names),
        "transport": list(names),
    }


def is_region_name(name: str, ai_region: str, ai_region_label: str) -> bool:
    return bool(region_name_pattern(ai_region, ai_region_label).search(name))


def rank_transport_candidates(
    names: list[str],
    preferred_keywords: list[str] | None = None,
    limit: int = FULL_CHAIN_CANDIDATE_LIMIT,
) -> list[str]:
    """Return sticky full-chain transports without depending on fixed provider names."""
    classes = classify_stable(names, preferred_keywords=preferred_keywords)
    direct = re.compile(r"(?i)(直連|直连|direct|直通)")

    def score(item: tuple[int, str]) -> tuple[int, int]:
        index, name = item
        value = 0
        if name in classes["exact"]:
            value += 100
        if direct.search(name):
            value += 20
        return (-value, index)

    candidates = list(enumerate(names))
    return [name for _, name in sorted(candidates, key=score)[:limit]]


def make_fixed_chain_proxy(base: dict[str, Any], name: str, dialer_proxy: str) -> dict[str, Any]:
    clone = dict(base)
    clone["name"] = name
    clone["dialer-proxy"] = dialer_proxy
    return clone


def preferred_consumer_port(frontend_open: bool, active_port: int) -> int:
    return FRONTEND_PORT if frontend_open else active_port


def make_chained_region_exits(
    proxies: list[dict[str, Any]],
    dialer_proxy: str,
    ai_region: str,
    ai_region_label: str,
) -> list[dict[str, Any]]:
    chainable_types = {"ss", "vmess", "vless", "trojan", "http", "socks", "socks5"}
    result: list[dict[str, Any]] = []
    for item in proxies:
        if not is_region_name(str(item.get("name", "")), ai_region, ai_region_label):
            continue
        if str(item.get("type", "")).lower() not in chainable_types:
            continue
        clone = dict(item)
        clone["name"] = f"[AI目标地区出口] {item['name']}"
        clone["dialer-proxy"] = dialer_proxy
        result.append(clone)
    return result


def make_fixed_egress_proxy(
    settings: dict[str, Any],
    dialer_proxy: str,
) -> dict[str, Any]:
    required = ("server", "port", "cipher", "password")
    missing = [key for key in required if settings.get(key) in (None, "")]
    if missing:
        raise ManagerError(f"fixed egress is missing required fields: {', '.join(missing)}")
    try:
        port = int(settings["port"])
    except (TypeError, ValueError) as exc:
        raise ManagerError("fixed egress port must be an integer") from exc
    if not 1 <= port <= 65535:
        raise ManagerError("fixed egress port must be between 1 and 65535")
    return {
        "name": str(settings.get("name") or FIXED_EGRESS_DEFAULT_NAME),
        "type": "ss",
        "server": str(settings["server"]),
        "port": port,
        "cipher": str(settings["cipher"]),
        "password": str(settings["password"]),
        "udp": False,
        "dialer-proxy": str(settings.get("dialer_proxy") or dialer_proxy),
        "ip-version": "ipv4",
    }


def trace_region(text: str) -> str | None:
    for line in text.splitlines():
        if line.startswith("loc="):
            return line.partition("=")[2].strip().upper() or None
    return None


def trace_ip(text: str) -> str | None:
    for line in text.splitlines():
        if line.startswith("ip="):
            return line.partition("=")[2].strip() or None
    return None


def make_url_test(
    name: str,
    proxies: list[str],
    url: str,
    interval: int = 300,
    expected_status: int | str | None = None,
) -> dict[str, Any]:
    group = {
        "name": name,
        "type": "url-test",
        "proxies": proxies,
        "url": url,
        "interval": interval,
        "tolerance": 100,
        "lazy": False,
    }
    if expected_status is not None:
        group["expected-status"] = expected_status
    return group


def build_config(
    stable: list[dict[str, Any]],
    download1: list[dict[str, Any]],
    download2: list[dict[str, Any]],
    slot: dict[str, Any],
    fixed_egress: dict[str, Any] | None = None,
    ai_region: str = LEGACY_AI_REGION,
    ai_region_label: str = LEGACY_AI_REGION_LABEL,
    preferred_keywords: list[str] | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if not stable:
        raise ManagerError("stable subscription yielded zero usable nodes")
    if not download1 and not download2:
        raise ManagerError("both download subscriptions yielded zero usable nodes")

    ai_region = normalize_region(ai_region)
    ai_region_label = str(ai_region_label or ai_region).strip()
    keyword_source = LEGACY_PREFERRED_NODE_KEYWORDS if preferred_keywords is None else preferred_keywords
    preferred_keywords = [item.strip() for item in keyword_source if item.strip()]
    stable_names = [p["name"] for p in stable]
    d1_names = [p["name"] for p in download1]
    d2_names = [p["name"] for p in download2]
    classes = classify_stable(stable_names, ai_region, ai_region_label, preferred_keywords)
    transport_group = "AI-前置稳定"
    fixed_proxy = make_fixed_egress_proxy(fixed_egress, transport_group) if fixed_egress else None
    fixed_chain_proxies: list[dict[str, Any]] = []
    fixed_chain_names: list[str] = []
    chained_exits = [] if fixed_proxy else make_chained_region_exits(
        download1 + download2, transport_group, ai_region, ai_region_label
    )
    chained_exit_names = [p["name"] for p in chained_exits]
    groups: list[dict[str, Any]] = []
    ai_children: list[str] = []

    groups.append(
        make_url_test(
            transport_group,
            classes["transport"],
            TRANSPORT_TEST_URL,
            180,
            TRANSPORT_EXPECTED_STATUS,
        )
    )

    if fixed_proxy:
        # The SSH tunnel is a transport alternative only; every path still exits from
        # the configured fixed-egress VPS. Provider transports are discovered from the
        # current subscription on every rebuild, so renamed nodes do not strand AI.
        ssh_proxy = {
            "name": "AI-SSH直连固定出口",
            "type": "socks5",
            "server": "127.0.0.1",
            "port": SSH_SOCKS_PORT,
            "udp": False,
            "ip-version": "ipv4",
        }
        fixed_chain_proxies.append(ssh_proxy)
        fixed_chain_names.append(ssh_proxy["name"])
        for index, transport in enumerate(rank_transport_candidates(stable_names, preferred_keywords), start=1):
            chain_name = f"AI-固定出口链-{index:02d}"
            fixed_chain_proxies.append(make_fixed_chain_proxy(fixed_proxy, chain_name, transport))
            fixed_chain_names.append(chain_name)
        # Keep the generated automatic transport as a final compatibility fallback.
        fixed_chain_proxies.append(fixed_proxy)
        fixed_chain_names.append(fixed_proxy["name"])
        groups.append(
            {
                "name": FIXED_EGRESS_GROUP,
                "type": "fallback",
                "proxies": fixed_chain_names,
                "url": AI_TEST_URL,
                "expected-status": AI_EXPECTED_STATUS,
                "interval": 300,
                "lazy": False,
            }
        )
        ai_children.append(FIXED_EGRESS_GROUP)
    else:
        if classes["exact"]:
            groups.append(make_url_test("AI-指定节点", classes["exact"], AI_TEST_URL, 120, AI_EXPECTED_STATUS))
            ai_children.append("AI-指定节点")
        if classes["region_direct"] and set(classes["region_direct"]) != set(classes["exact"]):
            groups.append(make_url_test("AI-目标地区直连", classes["region_direct"], AI_TEST_URL, 180, AI_EXPECTED_STATUS))
            ai_children.append("AI-目标地区直连")
        if classes["region_any"] and not ai_children:
            groups.append(make_url_test("AI-目标地区自动", classes["region_any"], AI_TEST_URL, 180, AI_EXPECTED_STATUS))
            ai_children.append("AI-目标地区自动")
        if chained_exit_names:
            groups.append(make_url_test("AI-目标地区双层", chained_exit_names, AI_TEST_URL, 180, AI_EXPECTED_STATUS))
            ai_children.append("AI-目标地区双层")
    if not ai_children:
        raise ManagerError(f"no {ai_region} AI exit candidates were found")
    groups.insert(
        0,
        {
            "name": "AI-智能",
            "type": "fallback",
            "proxies": ai_children,
            "url": AI_TEST_URL,
            "expected-status": AI_EXPECTED_STATUS,
            "interval": 300,
            "lazy": False,
        },
    )

    download_children: list[str] = []
    if d1_names:
        groups.append(make_url_test("下载源1", d1_names, DOWNLOAD_TEST_URL, 120))
        download_children.append("下载源1")
    if d2_names:
        groups.append(make_url_test("下载源2", d2_names, DOWNLOAD_TEST_URL, 120))
        download_children.append("下载源2")
    # Non-AI traffic must never spill into the fixed AI egress.  The stable
    # subscription transport is the final compatibility path for ordinary
    # foreign traffic; it keeps the VPS identity isolated to explicit AI rules.
    download_children.append(transport_group)
    groups.append(
        {
            "name": "下载线路",
            "type": "fallback",
            "proxies": download_children,
            "url": DOWNLOAD_TEST_URL,
            "interval": 60,
            "lazy": False,
        }
    )
    rules = [
        "PROCESS-NAME,Telegram.exe,下载线路",
        "DOMAIN-SUFFIX,telegram.org,下载线路",
        "DOMAIN-SUFFIX,t.me,下载线路",
        "DOMAIN-SUFFIX,telegra.ph,下载线路",
        "IP-CIDR,91.108.4.0/22,下载线路,no-resolve",
        "IP-CIDR,91.108.8.0/22,下载线路,no-resolve",
        "IP-CIDR,91.108.12.0/22,下载线路,no-resolve",
        "IP-CIDR,91.108.16.0/22,下载线路,no-resolve",
        "IP-CIDR,91.108.56.0/22,下载线路,no-resolve",
        "IP-CIDR,149.154.160.0/20,下载线路,no-resolve",
        "DOMAIN-SUFFIX,youtube.com,下载线路",
        "DOMAIN-SUFFIX,youtu.be,下载线路",
        "DOMAIN-SUFFIX,youtube-nocookie.com,下载线路",
        "DOMAIN-SUFFIX,googlevideo.com,下载线路",
        "DOMAIN-SUFFIX,ytimg.com,下载线路",
        "DOMAIN,youtubei.googleapis.com,下载线路",
    ]
    # Steam cloud is intentionally direct and must appear before the broad
    # googleapis AI rule.
    rules += [
        "DOMAIN-SUFFIX,storage.googleapis.com,DIRECT",
        "DOMAIN-KEYWORD,steamcloud,DIRECT",
    ]
    for domain in (
        "chatgpt.com", "openai.com", "oaistatic.com", "oaiusercontent.com",
        "anthropic.com", "claude.ai", "claudeusercontent.com", "google.com",
        "googleapis.com", "gstatic.com", "googleusercontent.com", "ggpht.com",
        "googleai.com", "deepmind.com", "x.ai", "grok.com", "perplexity.ai",
        "poe.com", "cohere.com", "cohere.ai", "mistral.ai", "huggingface.co",
        "cursor.com", "cursor.sh",
    ):
        rules.append(f"DOMAIN-SUFFIX,{domain},AI-智能")
    rules += [
        "DOMAIN,api.githubcopilot.com,AI-智能",
        "GEOSITE,cn,DIRECT",
        "GEOIP,CN,DIRECT,no-resolve",
        "MATCH,下载线路",
    ]

    config = {
        "mixed-port": slot["proxy_port"],
        "allow-lan": False,
        "mode": "rule",
        "log-level": "warning",
        "ipv6": False,
        "unified-delay": True,
        "tcp-concurrent": True,
        "find-process-mode": "always",
        "keep-alive-interval": 30,
        "external-controller": f"127.0.0.1:{slot['controller_port']}",
        "profile": {"store-selected": False, "store-fake-ip": True},
        "dns": {
            "enable": True,
            "listen": f"127.0.0.1:{slot['dns_port']}",
            "ipv6": False,
            "enhanced-mode": "fake-ip",
            "fake-ip-range": "198.18.0.1/16",
            "fake-ip-filter": ["*.lan", "localhost.ptlogin2.qq.com"],
            "default-nameserver": ["223.5.5.5", "119.29.29.29"],
            "nameserver": ["https://doh.pub/dns-query", "https://dns.alidns.com/dns-query"],
            "proxy-server-nameserver": ["https://doh.pub/dns-query", "https://dns.alidns.com/dns-query"],
            "nameserver-policy": {"geosite:cn": ["https://doh.pub/dns-query", "https://dns.alidns.com/dns-query"]},
        },
        "proxies": stable + download1 + download2 + fixed_chain_proxies + chained_exits,
        "proxy-groups": groups,
        "rules": rules,
    }
    summary = {
        "manager_version": 2,
        "stable": len(stable),
        "download1": len(download1),
        "download2": len(download2),
        "exact_preferred": len(classes["exact"]),
        "ai_region": ai_region,
        "ai_region_label": ai_region_label,
        "region_direct": len(classes["region_direct"]),
        "region_any": len(classes["region_any"]),
        "region_chained": len(chained_exit_names),
        "fixed_egress": bool(fixed_proxy),
        "fixed_egress_server": fixed_proxy["server"] if fixed_proxy else None,
        "full_chain_candidates": fixed_chain_names,
        "stable_frontend_port": FRONTEND_PORT,
        "ai_chain": ai_children,
    }
    return config, summary


class DynamicProxyManager:
    def __init__(
        self,
        home: Path,
        app_dir: Path,
        subscription_file: Path,
        fixed_egress_file: Path | None = None,
        profile: dict[str, Any] | None = None,
    ):
        self.home = home
        self.app_dir = app_dir
        self.subscription_file = subscription_file
        self.fixed_egress_file = fixed_egress_file or (home / "fixed-egress.json")
        self.state_path = home / "state.json"
        self.stop_path = home / "stop.request"
        self.log = Logger(home / "manager.log.jsonl")
        self.core = app_dir / "bin" / ("mihomo.exe" if os.name == "nt" else "mihomo")
        self.profile = profile
        self.ai_region = normalize_region(profile.get("ai_region") if profile else None)
        self.ai_region_label = str(
            (profile.get("ai_region_label") if profile else None) or self.ai_region
        ).strip()
        raw_keywords = profile.get("preferred_node_keywords", list(LEGACY_PREFERRED_NODE_KEYWORDS)) if profile else list(LEGACY_PREFERRED_NODE_KEYWORDS)
        if not isinstance(raw_keywords, list):
            raise ManagerError("preferred_node_keywords must be a list")
        self.preferred_node_keywords = [str(item).strip() for item in raw_keywords if str(item).strip()]
        self.stop_requested = False
        home.mkdir(parents=True, exist_ok=True)
        signal.signal(signal.SIGINT, self._request_stop)
        signal.signal(signal.SIGTERM, self._request_stop)

    def _request_stop(self, *_: Any) -> None:
        self.stop_requested = True

    def state(self) -> dict[str, Any]:
        return load_json(self.state_path, {}) or {}

    def save_state(self, state: dict[str, Any]) -> None:
        save_json(self.state_path, state)

    def consumer_port(self, active_port: int) -> int:
        return preferred_consumer_port(FRONTEND_PORT > 0 and port_open(FRONTEND_PORT), active_port)

    def subscription_settings(self) -> dict[str, Any]:
        data = {"subscriptions": self.profile.get("subscriptions")} if self.profile is not None else load_json(self.subscription_file)
        if not isinstance(data, dict) or not isinstance(data.get("subscriptions"), list):
            raise ManagerError("invalid subscription settings")
        return data

    def fixed_egress_settings(self) -> dict[str, Any] | None:
        data = self.profile.get("fixed_egress") if self.profile is not None else load_json(self.fixed_egress_file)
        if data is None:
            return None
        if not isinstance(data, dict):
            raise ManagerError("invalid fixed egress settings")
        # Build once here to validate without logging the password.
        proxy = make_fixed_egress_proxy(data, "AI-前置稳定")
        self.log.write(
            "fixed_egress_loaded",
            name=proxy["name"],
            server=proxy["server"],
            port=proxy["port"],
            cipher=proxy["cipher"],
        )
        return data

    def fetch_subscription(self, label: str, url: str) -> list[dict[str, Any]]:
        state = self.state()
        attempts: list[str | None] = []
        active_port = state.get("active_proxy_port")
        if isinstance(active_port, int) and port_open(active_port):
            attempts.append(f"http://127.0.0.1:{active_port}")
        if port_open(7890):
            attempts.append("http://127.0.0.1:7890")
        attempts.append(None)
        seen: set[str | None] = set()
        for proxy in attempts:
            if proxy in seen:
                continue
            seen.add(proxy)
            session = requests.Session()
            session.trust_env = False
            try:
                response = session.get(
                    url,
                    proxies={"http": proxy, "https": proxy} if proxy else {},
                    timeout=(10, 45),
                    headers={"User-Agent": "clash.meta"},
                )
                response.raise_for_status()
                parsed = decode_subscription(response.content)
                self.log.write("subscription_fetched", label=label, route=(proxy or "direct"), bytes=len(response.content), nodes=len(parsed["proxies"]))
                return parsed["proxies"]
            except Exception as exc:
                self.log.write("subscription_fetch_failed", label=label, route=(proxy or "direct"), error=type(exc).__name__)
        raise ManagerError(f"all fetch routes failed for {label}")

    def fetch_all(self) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
        settings = self.subscription_settings()
        found: dict[str, list[dict[str, Any]]] = {"stable": [], "download1": [], "download2": []}
        for sub in settings["subscriptions"]:
            role = sub.get("role")
            if role not in found:
                continue
            raw = self.fetch_subscription(str(sub.get("label", role)), str(sub["url"]))
            found[role] = unique_proxy_names(raw, str(sub.get("prefix", role)))
        return found["stable"], found["download1"], found["download2"]

    def next_slot(self) -> dict[str, Any]:
        active = self.state().get("active_slot")
        return SLOTS[1] if active == "A" else SLOTS[0]

    def prepare_candidate(self) -> tuple[dict[str, Any], Path, dict[str, Any]]:
        slot = self.next_slot()
        stable, d1, d2 = self.fetch_all()
        config, summary = build_config(
            stable,
            d1,
            d2,
            slot,
            self.fixed_egress_settings(),
            self.ai_region,
            self.ai_region_label,
            self.preferred_node_keywords,
        )
        slot_home = self.home / f"slot-{slot['name']}"
        slot_home.mkdir(parents=True, exist_ok=True)
        resource_dir = self.app_dir / "resources"
        for name in ("GEOSITE.dat", "GEOIP.dat", "GEOIP.metadb", "ASN.mmdb"):
            source = resource_dir / name
            target = slot_home / name
            if source.exists() and (not target.exists() or source.stat().st_size != target.stat().st_size):
                shutil.copy2(source, target)
        config_path = slot_home / "config.yaml"
        atomic_write(config_path, yaml.safe_dump(config, allow_unicode=True, sort_keys=False, width=4096))
        self.log.write("candidate_built", slot=slot["name"], config_sha256=sha256_file(config_path), **summary)
        return slot, config_path, summary

    def syntax_test(self, config_path: Path, slot: dict[str, Any]) -> None:
        slot_home = config_path.parent
        result = subprocess.run(
            [str(self.core), "-t", "-d", str(slot_home), "-f", str(config_path)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        self.log.write("syntax_test", slot=slot["name"], exit=result.returncode, tail=(result.stdout + result.stderr)[-500:])
        if result.returncode != 0:
            raise ManagerError("candidate syntax test failed")

    def start_candidate(self, config_path: Path, slot: dict[str, Any]) -> int:
        if port_open(slot["proxy_port"]) or port_open(slot["controller_port"]):
            raise ManagerError(f"candidate slot {slot['name']} ports already occupied")
        out = (config_path.parent / "core.log").open("a", encoding="utf-8")
        proc = subprocess.Popen(
            [str(self.core), "-d", str(config_path.parent), "-f", str(config_path)],
            stdout=out,
            stderr=subprocess.STDOUT,
            creationflags=(subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP) if os.name == "nt" else 0,
        )
        deadline = time.time() + 45
        while time.time() < deadline:
            if proc.poll() is not None:
                raise ManagerError(f"candidate core exited with {proc.returncode}")
            if port_open(slot["proxy_port"]) and port_open(slot["controller_port"]):
                self.log.write("candidate_started", slot=slot["name"], pid=proc.pid, proxy_port=slot["proxy_port"])
                return proc.pid
            time.sleep(0.5)
        proc.terminate()
        raise ManagerError("candidate core did not become ready")

    def stop_candidate(self, pid: int, slot: dict[str, Any]) -> None:
        """Stop only the freshly-created candidate and wait until its ports close."""
        try:
            if os.name == "nt":
                subprocess.run(
                    ["taskkill.exe", "/PID", str(pid), "/T", "/F"],
                    capture_output=True,
                    creationflags=subprocess.CREATE_NO_WINDOW,
                    timeout=15,
                )
            else:
                os.kill(pid, signal.SIGTERM)
        except Exception as exc:
            self.log.write("candidate_stop_failed", pid=pid, error=type(exc).__name__)
        deadline = time.time() + 10
        while time.time() < deadline and (port_open(slot["proxy_port"]) or port_open(slot["controller_port"])):
            time.sleep(0.25)
        self.log.write(
            "candidate_stopped",
            pid=pid,
            proxy_port=slot["proxy_port"],
            listener_remaining=port_open(slot["proxy_port"]),
        )

    def use_hot_fallback(self, reason: str) -> bool:
        """Temporarily restore live FlClash while a managed slot is unavailable."""
        if not port_open(7890):
            self.log.write("hot_fallback_unavailable", reason=reason)
            return False
        current = self.current_wininet_proxy()
        target = dict(current)
        target.update({"ProxyEnable": 1, "ProxyServer": "127.0.0.1:7890"})
        if int(current.get("ProxyEnable", 0)) != 1 or str(current.get("ProxyServer", "")) != "127.0.0.1:7890":
            self.set_wininet_proxy(target)
        self.log.write("hot_fallback_active", reason=reason, proxy_server="127.0.0.1:7890")
        return True

    def probe(self, proxy_port: int, expected_ip: str | None = None) -> dict[str, int | str]:
        session = requests.Session()
        session.trust_env = False
        proxy = f"http://127.0.0.1:{proxy_port}"
        result: dict[str, int | str] = {}
        for name, url, expected in PROBES:
            last_error: Exception | None = None
            for attempt in range(1, 4):
                try:
                    response = session.get(url, proxies={"http": proxy, "https": proxy}, timeout=(10, 30), allow_redirects=True)
                    result[name] = response.status_code
                    if response.status_code == expected:
                        last_error = None
                        break
                    last_error = ManagerError(f"probe {name} returned {response.status_code}, expected {expected}")
                except Exception as exc:
                    last_error = exc
                self.log.write("probe_retry", name=name, attempt=attempt, error=type(last_error).__name__)
                time.sleep(attempt)
            if last_error is not None:
                raise ManagerError(f"probe {name} failed after three attempts: {type(last_error).__name__}")
        last_error = None
        for attempt in range(1, 4):
            try:
                response = session.get(
                    AI_REGION_TRACE_URL,
                    proxies={"http": proxy, "https": proxy},
                    timeout=(10, 30),
                    allow_redirects=True,
                )
                region = trace_region(response.text)
                exit_ip = trace_ip(response.text)
                if response.status_code == 200 and region == self.ai_region and (not expected_ip or exit_ip == expected_ip):
                    result["chatgpt_region"] = region
                    result["chatgpt_ip"] = exit_ip or "unknown"
                    last_error = None
                    break
                last_error = ManagerError(
                    f"AI exit was ip={exit_ip or 'unknown'} region={region or 'unknown'}, expected ip={expected_ip or 'any'} region={self.ai_region}"
                )
            except Exception as exc:
                last_error = exc
            self.log.write("probe_retry", name="chatgpt_region", attempt=attempt, error=type(last_error).__name__)
            time.sleep(attempt)
        if last_error is not None:
            raise ManagerError(f"probe chatgpt_region failed after three attempts: {type(last_error).__name__}")
        self.log.write("probe_passed", proxy_port=proxy_port, **result)
        return result

    def select_ai_exit(self, slot: dict[str, Any], expected_ip: str | None = None) -> dict[str, str]:
        controller = f"http://127.0.0.1:{slot['controller_port']}"
        proxy_url = f"http://127.0.0.1:{slot['proxy_port']}"
        response = requests.get(f"{controller}/proxies", timeout=10)
        response.raise_for_status()
        proxies = response.json().get("proxies", {})
        for group_name in (FIXED_EGRESS_GROUP, "AI-指定节点", "AI-目标地区直连", "AI-目标地区自动", "AI-目标地区双层"):
            group = proxies.get(group_name)
            if not isinstance(group, dict):
                continue
            for node_name in group.get("all", []):
                try:
                    selected = requests.put(
                        f"{controller}/proxies/{quote(group_name, safe='')}",
                        json={"name": node_name},
                        timeout=10,
                    )
                    selected.raise_for_status()
                    selected = requests.put(
                        f"{controller}/proxies/{quote('AI-智能', safe='')}",
                        json={"name": group_name},
                        timeout=10,
                    )
                    selected.raise_for_status()
                    time.sleep(0.3)
                    session = requests.Session()
                    session.trust_env = False
                    route = {"http": proxy_url, "https": proxy_url}
                    api = session.get(AI_TEST_URL, proxies=route, timeout=(10, 30), allow_redirects=True)
                    trace = session.get(AI_REGION_TRACE_URL, proxies=route, timeout=(10, 30), allow_redirects=True)
                    region = trace_region(trace.text)
                    exit_ip = trace_ip(trace.text)
                    if api.status_code == AI_EXPECTED_STATUS and trace.status_code == 200 and region == self.ai_region and (not expected_ip or exit_ip == expected_ip):
                        result = {"group": group_name, "node": str(node_name), "region": region, "ip": exit_ip or "unknown"}
                        self.log.write("ai_exit_selected", **result)
                        return result
                    self.log.write(
                        "ai_exit_rejected",
                        group=group_name,
                        node=node_name,
                        api_status=api.status_code,
                        region=(region or "unknown"),
                        exit_ip=(exit_ip or "unknown"),
                    )
                except Exception as exc:
                    self.log.write(
                        "ai_exit_rejected",
                        group=group_name,
                        node=node_name,
                        error=type(exc).__name__,
                    )
        raise ManagerError(f"no candidate produced a verified {self.ai_region} AI exit")

    def group_state(self, slot: dict[str, Any]) -> dict[str, Any]:
        response = requests.get(f"http://127.0.0.1:{slot['controller_port']}/proxies", timeout=10)
        response.raise_for_status()
        proxies = response.json().get("proxies", {})
        result = {}
        for name in ("AI-智能", FIXED_EGRESS_GROUP, "AI-指定节点", "AI-目标地区直连", "AI-目标地区自动", "AI-目标地区双层", "AI-前置稳定", "下载线路"):
            if name in proxies:
                result[name] = {"type": proxies[name].get("type"), "now": proxies[name].get("now"), "count": len(proxies[name].get("all", []))}
        self.log.write("group_state", groups=json.dumps(result, ensure_ascii=False))
        return result

    @staticmethod
    def current_wininet_proxy() -> dict[str, Any]:
        if sys.platform == "darwin":
            services = []
            effective_server = ""
            enabled = False
            for name in mac_network_services():
                web = mac_proxy_details(name, secure=False)
                secure = mac_proxy_details(name, secure=True)
                services.append({"name": name, "web": web, "secure": secure})
                candidate = secure if secure.get("enabled") else web
                if candidate.get("enabled") and candidate.get("server") and candidate.get("port"):
                    enabled = True
                    effective_server = f"{candidate['server']}:{candidate['port']}"
            return {"ProxyEnable": int(enabled), "ProxyServer": effective_server, "ProxyOverride": "", "mac_services": services}
        if os.name != "nt":
            return {}
        key_path = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings"
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path) as key:
            def get(name: str, default: Any) -> Any:
                try:
                    return winreg.QueryValueEx(key, name)[0]
                except FileNotFoundError:
                    return default
            return {"ProxyEnable": get("ProxyEnable", 0), "ProxyServer": get("ProxyServer", ""), "ProxyOverride": get("ProxyOverride", "")}

    @staticmethod
    def set_wininet_proxy(proxy: dict[str, Any]) -> None:
        if sys.platform == "darwin":
            services = proxy.get("mac_services")
            if not isinstance(services, list):
                services = [{"name": name} for name in mac_network_services()]
            if proxy.get("mac_restore_services"):
                for item in services:
                    if not isinstance(item, dict) or not item.get("name"):
                        continue
                    for secure, key in ((False, "web"), (True, "secure")):
                        details = item.get(key, {})
                        set_mac_proxy(
                            str(item["name"]),
                            secure,
                            bool(details.get("enabled")),
                            str(details.get("server", "")),
                            int(details.get("port", 0)),
                        )
                return
            enabled = int(proxy.get("ProxyEnable", 0)) == 1
            server_text = str(proxy.get("ProxyServer", ""))
            if enabled:
                host, separator, port_text = server_text.rpartition(":")
                if not separator or not host:
                    raise ManagerError("invalid macOS proxy server")
                port = int(port_text)
            else:
                host, port = "", 0
            for item in services:
                if not isinstance(item, dict) or not item.get("name"):
                    continue
                set_mac_proxy(str(item["name"]), False, enabled, host, port)
                set_mac_proxy(str(item["name"]), True, enabled, host, port)
            return
        if os.name != "nt":
            return
        key_path = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings"
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0, winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, "ProxyEnable", 0, winreg.REG_DWORD, int(proxy.get("ProxyEnable", 0)))
            winreg.SetValueEx(key, "ProxyServer", 0, winreg.REG_SZ, str(proxy.get("ProxyServer", "")))
            winreg.SetValueEx(key, "ProxyOverride", 0, winreg.REG_SZ, str(proxy.get("ProxyOverride", "")))
        internet_set_option = ctypes.windll.Wininet.InternetSetOptionW
        internet_set_option(0, 39, 0, 0)
        internet_set_option(0, 37, 0, 0)

    def cutover(self, slot: dict[str, Any], pid: int, summary: dict[str, Any]) -> None:
        state = self.state()
        state_before = dict(state)
        before = self.current_wininet_proxy()
        if "original_proxy" not in state:
            state["original_proxy"] = before
        previous = {k: state.get(k) for k in ("active_slot", "active_proxy_port", "active_controller_port", "active_pid")}
        state.update(
            {
                "active_slot": slot["name"],
                "active_proxy_port": slot["proxy_port"],
                "active_controller_port": slot["controller_port"],
                "active_pid": pid,
                "activated_at": local_stamp(),
                "summary": summary,
                "previous": previous,
            }
        )
        # Commit the verified backend before directing new frontend connections to
        # it. Existing frontend connections remain attached to the draining core.
        self.save_state(state)
        consumer_port = self.consumer_port(slot["proxy_port"])
        target = dict(before)
        target.update({"ProxyEnable": 1, "ProxyServer": f"127.0.0.1:{consumer_port}"})
        try:
            self.set_wininet_proxy(target)
            time.sleep(1)
            after = self.current_wininet_proxy()
            if int(after.get("ProxyEnable", 0)) != 1 or str(after.get("ProxyServer")) != target["ProxyServer"]:
                raise ManagerError("WinINet proxy did not retain the stable consumer port")
            self.probe(consumer_port, str(summary.get("fixed_egress_server") or "") or None)
        except Exception:
            self.save_state(state_before)
            restore_before = dict(before)
            if sys.platform == "darwin":
                restore_before["mac_restore_services"] = True
            self.set_wininet_proxy(restore_before)
            self.log.write("cutover_reverted", restored_server=before.get("ProxyServer", ""))
            raise
        self.log.write(
            "cutover_committed",
            slot=slot["name"],
            proxy_port=slot["proxy_port"],
            consumer_port=consumer_port,
            previous_port=previous.get("active_proxy_port"),
        )
        previous_pid = previous.get("active_pid")
        previous_port = previous.get("active_proxy_port")
        if isinstance(previous_pid, int) and isinstance(previous_port, int) and previous_pid != pid:
            def retire() -> None:
                time.sleep(300)
                current = self.state()
                if current.get("active_pid") == previous_pid:
                    return
                if listener_owner(previous_port) != previous_pid:
                    self.log.write("previous_core_retire_skipped", pid=previous_pid, reason="listener_owner_mismatch")
                    return
                try:
                    if os.name == "nt":
                        subprocess.run(["taskkill.exe", "/PID", str(previous_pid), "/T", "/F"], capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW, timeout=15)
                    else:
                        os.kill(previous_pid, signal.SIGTERM)
                    self.log.write("previous_core_retired", pid=previous_pid)
                except Exception as exc:
                    self.log.write("previous_core_retire_skipped", pid=previous_pid, error=type(exc).__name__)
            threading.Thread(target=retire, name="retire-previous-core", daemon=True).start()

    def refresh(self, takeover: bool) -> dict[str, Any]:
        slot, config_path, summary = self.prepare_candidate()
        self.syntax_test(config_path, slot)
        pid = self.start_candidate(config_path, slot)
        try:
            expected_ip = str(summary.get("fixed_egress_server") or "") or None
            selection = self.select_ai_exit(slot, expected_ip)
            probes = self.probe(slot["proxy_port"], expected_ip)
            groups = self.group_state(slot)
            if takeover:
                self.cutover(slot, pid, summary)
            else:
                self.log.write("shadow_validation_complete", slot=slot["name"], pid=pid)
            return {"slot": slot, "pid": pid, "summary": summary, "selection": selection, "probes": probes, "groups": groups, "takeover": takeover}
        except Exception:
            self.stop_candidate(pid, slot)
            raise

    def preview(self) -> dict[str, Any]:
        """Validate a fresh candidate and always tear it down before returning."""
        slot, config_path, summary = self.prepare_candidate()
        self.syntax_test(config_path, slot)
        pid = self.start_candidate(config_path, slot)
        try:
            expected_ip = str(summary.get("fixed_egress_server") or "") or None
            selection = self.select_ai_exit(slot, expected_ip)
            probes = self.probe(slot["proxy_port"], expected_ip)
            groups = self.group_state(slot)
            result = {
                "slot": slot,
                "summary": summary,
                "selection": selection,
                "probes": probes,
                "groups": groups,
                "takeover": False,
                "candidate_stopped": True,
            }
            self.log.write("preview_validation_complete", slot=slot["name"], pid=pid)
            return result
        finally:
            self.stop_candidate(pid, slot)

    def resume_existing(self) -> bool:
        state = self.state()
        slot_name = state.get("active_slot")
        slot = next((s for s in SLOTS if s["name"] == slot_name), None)
        if slot is None:
            return False
        config_path = self.home / f"slot-{slot_name}" / "config.yaml"
        if not config_path.exists():
            return False
        if port_open(slot["proxy_port"]) and port_open(slot["controller_port"]):
            return True
        self.syntax_test(config_path, slot)
        pid = self.start_candidate(config_path, slot)
        try:
            expected_ip = str(state.get("summary", {}).get("fixed_egress_server") or "") or None
            self.select_ai_exit(slot, expected_ip)
            self.probe(slot["proxy_port"], expected_ip)
        except Exception:
            self.stop_candidate(pid, slot)
            raise
        state["active_pid"] = pid
        state["resumed_at"] = local_stamp()
        self.save_state(state)
        self.log.write("existing_core_resumed", slot=slot_name, pid=pid, proxy_port=slot["proxy_port"])
        return True

    def rollback(self) -> None:
        state = self.state()
        original = state.get("original_proxy")
        if not isinstance(original, dict):
            raise ManagerError("no original proxy state recorded")
        if str(original.get("ProxyServer", "")).endswith(":7890") and not port_open(7890):
            flclash = Path(r"C:\Program Files\FlClash\FlClash.exe")
            if flclash.exists():
                subprocess.Popen([str(flclash)], creationflags=subprocess.CREATE_NO_WINDOW)
                deadline = time.time() + 30
                while time.time() < deadline and not port_open(7890):
                    time.sleep(0.5)
        if int(original.get("ProxyEnable", 0)) and str(original.get("ProxyServer", "")).endswith(":7890") and not port_open(7890):
            raise ManagerError("rollback target 127.0.0.1:7890 is not listening")
        restore_target = dict(original)
        if sys.platform == "darwin":
            restore_target["mac_restore_services"] = True
        self.set_wininet_proxy(restore_target)
        self.log.write("rollback_complete", proxy_enable=original.get("ProxyEnable"), proxy_server=original.get("ProxyServer"))

    def _stop_recorded_cores(self) -> list[int]:
        state = self.state()
        stopped: list[int] = []
        candidates = [
            (state.get("active_pid"), state.get("active_proxy_port")),
            (state.get("previous", {}).get("active_pid"), state.get("previous", {}).get("active_proxy_port")),
        ]
        for pid, port in candidates:
            if not isinstance(pid, int) or not isinstance(port, int) or pid in stopped:
                continue
            if listener_owner(port) != pid:
                self.log.write("managed_core_stop_skipped", pid=pid, proxy_port=port, reason="listener_owner_mismatch")
                continue
            slot = next((item for item in SLOTS if item["proxy_port"] == port), None)
            if slot is None:
                self.log.write("managed_core_stop_skipped", pid=pid, proxy_port=port, reason="unknown_slot")
                continue
            self.stop_candidate(pid, slot)
            stopped.append(pid)
        return stopped

    def stop_and_rollback(self, timeout: int = 15) -> dict[str, Any]:
        state = self.state()
        manager_pid = state.get("manager_pid")
        if isinstance(manager_pid, int) and manager_pid != os.getpid() and pid_exists(manager_pid):
            atomic_write(self.stop_path, json.dumps({"requested_at": local_stamp(), "requester": os.getpid()}))
            deadline = time.time() + timeout
            while time.time() < deadline and pid_exists(manager_pid):
                time.sleep(0.25)
            if pid_exists(manager_pid):
                raise ManagerError("managed loop did not stop before rollback")
        self.rollback()
        stopped = self._stop_recorded_cores()
        for slot in SLOTS:
            (self.home / f"slot-{slot['name']}" / "config.yaml").unlink(missing_ok=True)
        state = self.state()
        for key in ("active_slot", "active_proxy_port", "active_controller_port", "active_pid", "previous"):
            state.pop(key, None)
        state["managed_stopped_at"] = local_stamp()
        self.save_state(state)
        return {"rollback": True, "stopped_core_pids": stopped, "proxy": self.current_wininet_proxy()}

    def is_stop_requested(self) -> bool:
        if self.stop_requested:
            return True
        if self.stop_path.exists():
            self.stop_requested = True
            self.log.write("external_stop_requested")
            return True
        return False

    def run_loop(self, interval: int) -> None:
        self.stop_path.unlink(missing_ok=True)
        state = self.state()
        state["manager_pid"] = os.getpid()
        state["manager_started_at"] = local_stamp()
        self.save_state(state)
        self.log.write("manager_loop_started", pid=os.getpid(), interval=interval)
        try:
            self._run_loop(interval)
        finally:
            state = self.state()
            if state.get("manager_pid") == os.getpid():
                state.pop("manager_pid", None)
                state.pop("manager_started_at", None)
                state["manager_stopped_at"] = local_stamp()
                self.save_state(state)
            self.stop_path.unlink(missing_ok=True)
            self.log.write("manager_loop_stopped", pid=os.getpid())

    def _run_loop(self, interval: int) -> None:
        listener_failures = 0
        health_failures = 0
        state = self.state()
        active_port = state.get("active_proxy_port")
        if isinstance(active_port, int) and not port_open(active_port):
            self.use_hot_fallback("startup_listener_missing")
            try:
                resumed = self.resume_existing()
                if resumed:
                    state = self.state()
                    active_port = state.get("active_proxy_port")
                    current = self.current_wininet_proxy()
                    consumer_port = self.consumer_port(int(active_port))
                    target = dict(current)
                    target.update({"ProxyEnable": 1, "ProxyServer": f"127.0.0.1:{consumer_port}"})
                    self.set_wininet_proxy(target)
                    self.log.write("resumed_proxy_reasserted", proxy_server=target["ProxyServer"])
            except Exception as exc:
                self.log.write("existing_core_resume_failed", error=type(exc).__name__)
            state = self.state()
            active_port = state.get("active_proxy_port")
        wininet = self.current_wininet_proxy()
        already_active = (
            isinstance(active_port, int)
            and port_open(active_port)
            and int(state.get("summary", {}).get("manager_version", 0)) >= 2
            and int(wininet.get("ProxyEnable", 0)) == 1
            and str(wininet.get("ProxyServer", "")) == f"127.0.0.1:{self.consumer_port(active_port)}"
        )
        if already_active:
            self.log.write("monitor_attached", proxy_port=active_port, pid=state.get("active_pid"))
        else:
            self.refresh(takeover=True)
        next_deep_health = time.time() + 300
        while not self.is_stop_requested():
            deadline = time.time() + interval
            while time.time() < deadline and not self.is_stop_requested():
                state = self.state()
                port = state.get("active_proxy_port")
                if not isinstance(port, int) or not port_open(port):
                    listener_failures += 1
                    self.log.write("active_health_failed", failures=listener_failures, reason="listener_missing")
                    self.use_hot_fallback("active_listener_missing")
                    if listener_failures >= 3:
                        try:
                            self.refresh(takeover=True)
                            listener_failures = 0
                            health_failures = 0
                        except Exception as exc:
                            self.log.write("refresh_failed_kept_fallback", error=type(exc).__name__)
                            self.use_hot_fallback("listener_refresh_failed")
                            listener_failures = 0
                else:
                    listener_failures = 0
                    current_proxy = self.current_wininet_proxy()
                    expected_server = f"127.0.0.1:{self.consumer_port(port)}"
                    if int(current_proxy.get("ProxyEnable", 0)) != 1 or str(current_proxy.get("ProxyServer", "")) != expected_server:
                        corrected = dict(current_proxy)
                        corrected.update({"ProxyEnable": 1, "ProxyServer": expected_server})
                        self.set_wininet_proxy(corrected)
                        self.log.write("system_proxy_reasserted", proxy_server=expected_server)
                    if time.time() >= next_deep_health:
                        try:
                            expected_ip = str(state.get("summary", {}).get("fixed_egress_server") or "") or None
                            self.probe(self.consumer_port(port), expected_ip)
                            health_failures = 0
                        except Exception as exc:
                            health_failures += 1
                            self.log.write("active_health_failed", failures=health_failures, reason=type(exc).__name__)
                            if health_failures >= 2:
                                try:
                                    self.refresh(takeover=True)
                                    listener_failures = 0
                                    health_failures = 0
                                except Exception as refresh_exc:
                                    self.log.write("refresh_failed_kept_fallback", error=type(refresh_exc).__name__)
                                    self.use_hot_fallback("health_refresh_failed")
                                    health_failures = 0
                        next_deep_health = time.time() + 300
                time.sleep(min(1, max(0.1, deadline - time.time())))
            if not self.is_stop_requested():
                try:
                    self.refresh(takeover=True)
                except Exception as exc:
                    self.log.write("scheduled_refresh_failed_kept_active", error=type(exc).__name__)
                    state = self.state()
                    port = state.get("active_proxy_port")
                    if not isinstance(port, int) or not port_open(port):
                        self.use_hot_fallback("scheduled_refresh_failed_without_listener")


def default_app_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parent


def main() -> int:
    parser = argparse.ArgumentParser(description="Dynamic multi-subscription proxy manager")
    parser.add_argument("command", choices=("preview", "shadow", "takeover", "run", "status", "rollback", "stop"))
    parser.add_argument("--home", type=Path, default=DEFAULT_HOME)
    parser.add_argument("--subscriptions", type=Path)
    parser.add_argument("--fixed-egress", type=Path)
    parser.add_argument("--profile-stdin", action="store_true")
    parser.add_argument("--interval", type=int, default=1800)
    args = parser.parse_args()
    app_dir = default_app_dir()
    subscriptions = args.subscriptions or (app_dir / "subscriptions.local.json")
    profile = None
    if args.profile_stdin:
        profile = json.load(sys.stdin)
        if not isinstance(profile, dict) or not isinstance(profile.get("subscriptions"), list):
            raise ManagerError("stdin profile must contain a subscriptions list")
    manager = DynamicProxyManager(args.home, app_dir, subscriptions, args.fixed_egress, profile)
    try:
        if args.command == "status":
            state = manager.state()
            manager_pid = state.get("manager_pid")
            print(json.dumps({"state": state, "wininet": manager.current_wininet_proxy(), "listener": port_open(int(state.get("active_proxy_port", 0))) if state.get("active_proxy_port") else False, "manager_running": isinstance(manager_pid, int) and pid_exists(manager_pid)}, ensure_ascii=False, indent=2))
        elif args.command == "preview":
            print(json.dumps(manager.preview(), ensure_ascii=False, indent=2))
        elif args.command == "rollback":
            manager.rollback()
        elif args.command == "stop":
            print(json.dumps(manager.stop_and_rollback(), ensure_ascii=False, indent=2))
        elif args.command == "shadow":
            print(json.dumps(manager.refresh(takeover=False), ensure_ascii=False, indent=2))
        elif args.command == "takeover":
            print(json.dumps(manager.refresh(takeover=True), ensure_ascii=False, indent=2))
        elif args.command == "run":
            with RunLock(args.home):
                manager.run_loop(max(300, args.interval))
        return 0
    except Exception as exc:
        manager.log.write("fatal", error=type(exc).__name__, message=str(exc))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
