$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$lib = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\lib.rs') -Raw
$app = Get-Content -LiteralPath (Join-Path $root 'src\App.tsx') -Raw
$gateway = Get-Content -LiteralPath (Join-Path $root 'src\lib\gateway.ts') -Raw
$backend = Get-Content -LiteralPath (Join-Path $root 'src\lib\backend.ts') -Raw
$requestMatch = [regex]::Match($backend, '(?s)export interface GatewayDeployRequest\s*\{(?<body>.*?)\}')
$gatewayYamlStart = $lib.IndexOf('fn gateway_yaml_for_profile_and_policies')
$gatewayYamlEnd = $lib.IndexOf('fn gateway_yaml_for_profile', $gatewayYamlStart + 1)
$gatewayYaml = if ($gatewayYamlStart -ge 0 -and $gatewayYamlEnd -gt $gatewayYamlStart) { $lib.Substring($gatewayYamlStart, $gatewayYamlEnd - $gatewayYamlStart) } else { '' }

$checks = [ordered]@{
    fixed_read_only_inspection_script = ($lib.Contains('const OPENWRT_INSPECT_SCRIPT') -and $lib.Contains('/tmp/dhcp.leases') -and -not $backend.Contains('inspectionScript'))
    no_frontend_shell_or_target_path = ($requestMatch.Success -and -not $requestMatch.Groups['body'].Value.Contains('command') -and -not $requestMatch.Groups['body'].Value.Contains('targetPath'))
    ipv4_ipv6_device_merge = ($lib.Contains('parse_gateway_devices') -and $lib.Contains('ip -6 neigh show'))
    local_private_ip_gate = ($lib.Contains('value.is_private()') -and $lib.Contains('value.is_unicast_link_local()') -and $lib.Contains('0xfe00 == 0xfc00'))
    stable_device_identity = ($gateway.Contains('new Map(policies.map((policy) => [policy.id, policy]))') -and $gateway.Contains('device.id'))
    explicit_scenarios = ($gateway.Contains('"direct"') -and $gateway.Contains('"ai"') -and $gateway.Contains('"streaming"') -and $gateway.Contains('"gaming"') -and $gateway.Contains('"download"'))
    china_direct_precedes_device_rules = ($gatewayYaml.Contains('gateway_device_rules(policies)') -and $gatewayYaml.Contains('MATCH,STABLE-AUTO') -and $lib.Contains('GEOIP,CN,DIRECT,no-resolve'))
    no_fake_default_devices = (-not $app.Contains('defaultDevices'))
    transactional_policy_store = ($lib.Contains('gateway-policies.json') -and $lib.Contains('path.with_extension("json.tmp")') -and $lib.Contains('path.with_extension("json.bak")'))
    backup_includes_gateway_policies = ($lib.Contains('gateway_policies: Vec<GatewayDevicePolicy>') -and $lib.Contains('validate_gateway_policies(&backup.gateway_policies)'))
}

$failed = @($checks.GetEnumerator() | Where-Object { -not $_.Value })
foreach ($entry in $checks.GetEnumerator()) {
    Write-Output ("{0}={1}" -f $entry.Key, $entry.Value)
}
if ($failed.Count -gt 0) {
    throw ('home gateway policy failed: ' + (($failed | ForEach-Object Key) -join ', '))
}
Write-Output ("HOME_GATEWAY_POLICY_OK={0}" -f $checks.Count)
