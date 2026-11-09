$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
$files = @(
    (Join-Path $project 'src\App.tsx'),
    (Join-Path $project 'src\data.ts'),
    (Join-Path $project 'src-tauri\src\lib.rs'),
    (Join-Path (Split-Path $project -Parent) 'dynamic_proxy_manager\dynamic_proxy_manager.py')
)
$forbidden = @(
    'AI_REQUIRED_REGION',
    'FIXED-SINGAPORE',
    'Fixed-Singapore',
    'AI-新加坡直连',
    'AI-新加坡自动',
    'AI-新加坡双层',
    'select_singapore_exit',
    'no Singapore AI exit',
    'SG · 93 ms',
    '严格新加坡',
    '固定新加坡出口',
    '新加坡固定出口',
    'sg.example.com'
)

$violations = foreach ($file in $files) {
    $content = Get-Content -LiteralPath $file -Raw
    foreach ($pattern in $forbidden) {
        if ($content.Contains($pattern)) {
            [pscustomobject]@{ File = $file; Pattern = $pattern }
        }
    }
}

if ($violations) {
    $violations | Format-Table -AutoSize | Out-String | Write-Error
    exit 1
}

Write-Output 'PUBLIC_REGION_POLICY_OK=13'
