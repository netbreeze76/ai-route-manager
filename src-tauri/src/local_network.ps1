$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$config = Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway } | Select-Object -First 1
if (-not $config) { throw 'No active IPv4 default gateway found' }
$ip = [string]($config.IPv4Address | Select-Object -First 1).IPAddress
$gateway = [string]($config.IPv4DefaultGateway | Select-Object -First 1).NextHop
$adapter = Get-NetAdapter | Where-Object { $_.ifIndex -eq $config.InterfaceIndex } | Select-Object -First 1
$devices = @([ordered]@{
    id = "local-$ip"; name = $env:COMPUTERNAME; mac = [string]$adapter.MacAddress
    ipv4 = $ip; ipv6 = @(); kind = 'computer'; online = $true
})
$neighbors = @(Get-NetNeighbor -InterfaceIndex $config.InterfaceIndex -AddressFamily IPv4 | Where-Object {
    $_.State -in @('Reachable', 'Stale', 'Delay', 'Probe') -and
    $_.LinkLayerAddress -match '^(?:[0-9A-Fa-f]{2}-){5}[0-9A-Fa-f]{2}$' -and
    $_.LinkLayerAddress -notmatch '^(FF-FF|00-00-00-00-00-00)'
})
foreach ($neighbor in $neighbors) {
    $devices += [ordered]@{
        id = "lan-$($neighbor.LinkLayerAddress)-$($neighbor.IPAddress)"; name = $(if ($neighbor.IPAddress -eq $gateway) { 'Gateway' } else { "LAN device $($neighbor.IPAddress)" })
        mac = [string]$neighbor.LinkLayerAddress; ipv4 = [string]$neighbor.IPAddress
        ipv6 = @(); kind = 'device'; online = ($neighbor.State -eq 'Reachable')
    }
}
[ordered]@{ gateway = $gateway; localIp = $ip; devices = @($devices) } | ConvertTo-Json -Depth 8 -Compress
