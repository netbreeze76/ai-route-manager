$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
$workspace = Split-Path $project -Parent
$roots = @(
    (Join-Path $project 'scripts'),
    (Join-Path $workspace 'dynamic_proxy_manager')
)
$files = foreach ($root in $roots) {
    if (Test-Path -LiteralPath $root) {
        Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.ps1'
    }
}

$parseFailures = foreach ($file in $files) {
    $tokens = $null
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile(
        $file.FullName,
        [ref]$tokens,
        [ref]$errors
    )
    foreach ($parseError in $errors) {
        [pscustomobject]@{
            File = $file.FullName
            Line = $parseError.Extent.StartLineNumber
            Message = $parseError.Message
        }
    }
}

if ($parseFailures) {
    $parseFailures | Format-Table -AutoSize | Out-String | Write-Error
    exit 1
}

Write-Host "POWERSHELL_PARSE_OK=$($files.Count)"
