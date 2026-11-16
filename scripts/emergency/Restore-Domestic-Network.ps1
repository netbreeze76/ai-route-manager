[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$SelfTest
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$appDataRoot = Join-Path $env:APPDATA 'com.airoute.manager'
$preferencesPath = Join-Path $appDataRoot 'preferences.json'
$managedRuntime = Join-Path $appDataRoot 'managed-runtime'
$recordRoot = Join-Path $scriptRoot 'records'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$recordDir = Join-Path $recordRoot $stamp
$logPath = Join-Path $recordDir 'recovery.log'
$resultPath = Join-Path $recordDir 'result.json'

function Write-Log {
    param([string]$Message)
    $line = '{0} {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    Write-Host $line
    if (-not $SelfTest) { Add-Content -LiteralPath $logPath -Value $line -Encoding UTF8 }
}

function Write-Utf8NoBom {
    param([string]$Path, [string]$Text)
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($Path, $Text, $encoding)
}

function ConvertTo-DirectPreferences {
    param($Envelope)
    if (-not $Envelope.preferences) { throw 'preferences.json 缺少 preferences 字段' }
    $Envelope.preferences.mode = 'preview'
    $Envelope.preferences.autoStart = $false
    return $Envelope
}

function Get-ManagedProcessIds {
    param([object[]]$Processes)
    $programRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\AI Route Manager'))
    $legacyRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'AI Route Manager'))
    $supervisorScript = [IO.Path]::GetFullPath((Join-Path $appDataRoot 'supervisor.vbs'))
    $matched = New-Object 'System.Collections.Generic.HashSet[int]'

    foreach ($process in $Processes) {
        $name = [string]$process.Name
        $path = [string]$process.ExecutablePath
        $command = [string]$process.CommandLine
        $isAppBinary = $false
        if ($path) {
            try {
                $full = [IO.Path]::GetFullPath($path)
                $isAppBinary = $full.StartsWith($programRoot, [StringComparison]::OrdinalIgnoreCase) -or
                    $full.StartsWith($legacyRoot, [StringComparison]::OrdinalIgnoreCase)
            } catch { $isAppBinary = $false }
        }
        $isManagedCore = $name -ieq 'mihomo.exe' -and $command -like "*$managedRuntime*"
        $isSupervisor = ($name -ieq 'wscript.exe' -or $name -ieq 'cscript.exe') -and
            $command -like "*$supervisorScript*"
        if ($isAppBinary -or $isManagedCore -or $isSupervisor) {
            [void]$matched.Add([int]$process.ProcessId)
        }
    }

    do {
        $added = $false
        foreach ($process in $Processes) {
            if ($matched.Contains([int]$process.ParentProcessId) -and -not $matched.Contains([int]$process.ProcessId)) {
                [void]$matched.Add([int]$process.ProcessId)
                $added = $true
            }
        }
    } while ($added)
    return @($matched | Sort-Object -Descending)
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Save-Baseline {
    $internetKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $envKey = 'HKCU:\Environment'
    $internet = Get-ItemProperty -LiteralPath $internetKey
    $userEnv = Get-ItemProperty -LiteralPath $envKey
    $proxyEnableProperty = $internet.PSObject.Properties['ProxyEnable']
    $proxyServerProperty = $internet.PSObject.Properties['ProxyServer']
    $proxyOverrideProperty = $internet.PSObject.Properties['ProxyOverride']
    $autoConfigProperty = $internet.PSObject.Properties['AutoConfigURL']
    $runValue = $null
    try { $runValue = (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run').AIRouteManager } catch {}
    $payload = [ordered]@{
        capturedAt = (Get-Date).ToString('o')
        internet = [ordered]@{
            ProxyEnable = if ($proxyEnableProperty) { $proxyEnableProperty.Value } else { $null }
            ProxyServer = if ($proxyServerProperty) { $proxyServerProperty.Value } else { $null }
            ProxyOverride = if ($proxyOverrideProperty) { $proxyOverrideProperty.Value } else { $null }
            AutoConfigURL = if ($autoConfigProperty) { $autoConfigProperty.Value } else { $null }
        }
        environment = [ordered]@{}
        runValue = $runValue
        preferencesSha256 = if (Test-Path -LiteralPath $preferencesPath) { (Get-FileHash -LiteralPath $preferencesPath -Algorithm SHA256).Hash } else { $null }
    }
    foreach ($name in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy')) {
        $property = $userEnv.PSObject.Properties[$name]
        $payload.environment[$name] = if ($property) { $property.Value } else { $null }
    }
    Write-Utf8NoBom -Path (Join-Path $recordDir 'baseline.json') -Text ($payload | ConvertTo-Json -Depth 6)
    if (Test-Path -LiteralPath $preferencesPath) { Copy-Item -LiteralPath $preferencesPath -Destination (Join-Path $recordDir 'preferences.json.original') -Force }
    foreach ($marker in @('supervisor.enabled','stable-frontdoor.enabled')) {
        $path = Join-Path $appDataRoot $marker
        if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination (Join-Path $recordDir ($marker + '.original')) -Force }
    }
    $task = Get-ScheduledTask -TaskName 'AI Route Manager Supervisor' -ErrorAction SilentlyContinue
    if ($task) {
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $taskXml = & schtasks.exe /Query /TN 'AI Route Manager Supervisor' /XML 2>$null
        $taskExit = $LASTEXITCODE
        $ErrorActionPreference = $previousPreference
        if ($taskExit -eq 0 -and $taskXml) { Write-Utf8NoBom -Path (Join-Path $recordDir 'supervisor-task.xml') -Text ($taskXml -join [Environment]::NewLine) }
    }
}

function Disable-AutoRecovery {
    if (Test-Path -LiteralPath $preferencesPath) {
        $envelope = Get-Content -LiteralPath $preferencesPath -Raw | ConvertFrom-Json
        $envelope = ConvertTo-DirectPreferences -Envelope $envelope
        $temporary = $preferencesPath + '.direct.tmp'
        Write-Utf8NoBom -Path $temporary -Text ($envelope | ConvertTo-Json -Depth 8)
        Move-Item -LiteralPath $temporary -Destination $preferencesPath -Force
        Write-Log '已把 AI Route Manager 切换为预览模式并关闭自动启动。'
    }
    Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'AIRouteManager' -ErrorAction SilentlyContinue
    foreach ($marker in @('supervisor.enabled','stable-frontdoor.enabled')) {
        $path = Join-Path $appDataRoot $marker
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    }
    $task = Get-ScheduledTask -TaskName 'AI Route Manager Supervisor' -ErrorAction SilentlyContinue
    if ($task) {
        Stop-ScheduledTask -TaskName 'AI Route Manager Supervisor' -ErrorAction SilentlyContinue
        Disable-ScheduledTask -TaskName 'AI Route Manager Supervisor' -ErrorAction Stop | Out-Null
    }
    Write-Log '已阻止监督器再次自动写回代理。'
}

function Reset-UserProxy {
    $internetKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    Set-ItemProperty -LiteralPath $internetKey -Name ProxyEnable -Type DWord -Value 0
    Set-ItemProperty -LiteralPath $internetKey -Name ProxyServer -Type String -Value ''
    Remove-ItemProperty -LiteralPath $internetKey -Name AutoConfigURL -ErrorAction SilentlyContinue
    foreach ($name in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy')) {
        Remove-ItemProperty -LiteralPath 'HKCU:\Environment' -Name $name -ErrorAction SilentlyContinue
        Remove-Item -Path "Env:$name" -ErrorAction SilentlyContinue
    }
    $direct = [ordered]@{ Proxy = ''; ProxyBypass = ''; AutoconfigUrl = ''; AutoDetect = $false }
    $directPath = Join-Path $recordDir 'winhttp-direct.json'
    Write-Utf8NoBom -Path $directPath -Text ($direct | ConvertTo-Json -Compress)
    & netsh.exe winhttp set advproxy setting-scope=user settings-file=$directPath | Add-Content -LiteralPath $logPath -Encoding UTF8
    if ($LASTEXITCODE -ne 0) { throw '用户级 WinHTTP 直连设置失败' }
    if (Test-IsAdministrator) {
        & netsh.exe winhttp reset proxy | Add-Content -LiteralPath $logPath -Encoding UTF8
        Write-Log '已重置机器级 WinHTTP。'
    } else {
        Write-Log '当前不是管理员；浏览器和当前用户已恢复直连，机器级 WinHTTP 保持原状。'
    }
    if (-not ('Win32.NativeMethods' -as [type])) {
        Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition @'
[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint flags, uint timeout, out IntPtr result);
'@
    }
    $result = [IntPtr]::Zero
    [void][Win32.NativeMethods]::SendMessageTimeout([IntPtr]0xffff, 0x001A, [IntPtr]::Zero, 'Internet Settings', 2, 3000, [ref]$result)
    Write-Log '已关闭 WinINET、用户环境变量和用户级 WinHTTP 代理。'
}

function Stop-ManagedProcesses {
    $processes = @(Get-CimInstance Win32_Process)
    $ids = @(Get-ManagedProcessIds -Processes $processes)
    foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    Write-Log ('已停止 AI Route Manager 托管进程：' + (($ids | ForEach-Object { [string]$_ }) -join ','))
}

function Test-DomesticNetwork {
    & ipconfig.exe /flushdns | Add-Content -LiteralPath $logPath -Encoding UTF8
    $gateway = Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue |
        Sort-Object RouteMetric | Select-Object -First 1
    $checks = @()
    foreach ($url in @('https://www.baidu.com/','https://www.qq.com/','https://www.aliyun.com/')) {
        $code = & curl.exe --noproxy '*' -L -sS -o NUL --connect-timeout 5 --max-time 12 -w '%{http_code}' $url
        $checks += [pscustomobject]@{ url = $url; http = [string]$code; exit = $LASTEXITCODE }
    }
    $proxy = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $passed = @($checks | Where-Object { $_.exit -eq 0 -and $_.http -match '^(2|3)' }).Count -ge 1
    return [pscustomobject]@{
        success = ($passed -and [int]$proxy.ProxyEnable -eq 0)
        proxyEnable = [int]$proxy.ProxyEnable
        proxyServer = [string]$proxy.ProxyServer
        gateway = if ($gateway) { [string]$gateway.NextHop } else { '' }
        checks = $checks
    }
}

if ($SelfTest) {
    $fixture = [pscustomobject]@{ preferences = [pscustomobject]@{ mode = 'managed'; autoStart = $true } }
    $converted = ConvertTo-DirectPreferences -Envelope $fixture
    $direct = [ordered]@{ Proxy = ''; ProxyBypass = ''; AutoconfigUrl = ''; AutoDetect = $false }
    $processFixture = @(
        [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; Name = 'ai-route-manager.exe'; ExecutablePath = (Join-Path $env:LOCALAPPDATA 'Programs\AI Route Manager\1.0.6\ai-route-manager.exe'); CommandLine = '' },
        [pscustomobject]@{ ProcessId = 11; ParentProcessId = 1; Name = 'mihomo.exe'; ExecutablePath = 'E:\Telegram\mihomo.exe'; CommandLine = 'mihomo -d E:\Telegram\dedicated' },
        [pscustomobject]@{ ProcessId = 12; ParentProcessId = 1; Name = 'FlClashCore.exe'; ExecutablePath = 'C:\Program Files\FlClash\FlClashCore.exe'; CommandLine = '' },
        [pscustomobject]@{ ProcessId = 13; ParentProcessId = 1; Name = 'mihomo.exe'; ExecutablePath = 'C:\engine\mihomo.exe'; CommandLine = ('mihomo -d ' + $managedRuntime + '\slot-A') },
        [pscustomobject]@{ ProcessId = 14; ParentProcessId = 10; Name = 'child.exe'; ExecutablePath = 'C:\child.exe'; CommandLine = '' }
    )
    $managedIds = @(Get-ManagedProcessIds -Processes $processFixture)
    if ($converted.preferences.mode -ne 'preview' -or $converted.preferences.autoStart -ne $false) { throw 'SELFTEST_PREFERENCES_FAILED' }
    if (($direct | ConvertTo-Json -Compress) -notmatch '"Proxy":""') { throw 'SELFTEST_WINHTTP_FAILED' }
    if ($managedIds -notcontains 10 -or $managedIds -notcontains 13 -or $managedIds -notcontains 14) { throw 'SELFTEST_MANAGED_PROCESS_MISSING' }
    if ($managedIds -contains 11 -or $managedIds -contains 12) { throw 'SELFTEST_UNRELATED_PROCESS_SELECTED' }
    Write-Output 'SELFTEST_PREFERENCES=preview,false'
    Write-Output 'SELFTEST_WINHTTP=direct'
    Write-Output ('SELFTEST_MANAGED_IDS=' + ($managedIds -join ','))
    Write-Output 'SELFTEST_UNRELATED_IDS=preserved'
    Write-Output 'SELFTEST_EXIT=0'
    exit 0
}

New-Item -ItemType Directory -Path $recordDir -Force | Out-Null
Write-Log ('开始恢复国内直连。DryRun=' + [bool]$DryRun)
Save-Baseline
if ($DryRun) {
    Write-Log 'DRYRUN：未修改系统。计划依次关闭自动恢复、关闭用户代理、停止本软件托管进程并验证国内站点。'
    exit 0
}

Disable-AutoRecovery
Reset-UserProxy
Stop-ManagedProcesses
$result = Test-DomesticNetwork
Write-Utf8NoBom -Path $resultPath -Text ($result | ConvertTo-Json -Depth 6)
if (-not $result.success) {
    Write-Log '国内直连验证失败，请查看本目录 recovery.log 和 result.json。'
    exit 2
}
Write-Log ('国内直连已恢复；网关=' + $result.gateway)
Write-Host ''
Write-Host '已恢复为国内直连模式。AI Route Manager 不会自动重新接管。' -ForegroundColor Green
Write-Host ('记录目录：' + $recordDir)
exit 0
