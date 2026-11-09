$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$lib = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\lib.rs') -Raw
$main = Get-Content -LiteralPath (Join-Path $root 'src-tauri\src\main.rs') -Raw
$cargo = Get-Content -LiteralPath (Join-Path $root 'src-tauri\Cargo.toml') -Raw
$taskStart = $lib.IndexOf('fn windows_supervisor_task_xml')
$taskEnd = $lib.IndexOf('fn windows_task_xml_bytes', $taskStart)
$taskDefinition = if ($taskStart -ge 0 -and $taskEnd -gt $taskStart) { $lib.Substring($taskStart, $taskEnd - $taskStart) } else { '' }

$checks = [ordered]@{
    only_one_raw_command_constructor = ([regex]::Matches($lib, [regex]::Escape('Command::new(')).Count -eq 1)
    create_no_window_is_centralized = $lib.Contains('command.creation_flags(CREATE_NO_WINDOW)')
    child_processes_use_hidden_helper = ([regex]::Matches($lib, [regex]::Escape('hidden_command(')).Count -ge 30)
    background_intercepts_before_tauri = ($main.IndexOf('run_background_if_requested()') -ge 0 -and $main.IndexOf('run_background_if_requested()') -lt $main.IndexOf('ai_route_manager_lib::run()'))
    headless_supervisor_exists = $lib.Contains('fn run_headless_supervisor()')
    headless_single_instance_lock_exists = $lib.Contains('headless-supervisor.lock')
    gui_single_instance_plugin_exists = ($cargo.Contains('tauri-plugin-single-instance') -and $lib.Contains('tauri_plugin_single_instance::init'))
    scheduled_supervisor_is_hidden = ($taskDefinition.Contains('<Hidden>true</Hidden>') -and -not $taskDefinition.Contains('<Hidden>false</Hidden>'))
    managed_status_reads_state_directly = ($lib.Contains('fn managed_engine_status_value_from_home') -and $lib.Contains('home.join("state.json")'))
    committed_migration_suppresses_legacy_conflict = $lib.Contains('migration_phase != Some("managed")')
}

$failed = @($checks.GetEnumerator() | Where-Object { -not $_.Value })
foreach ($entry in $checks.GetEnumerator()) {
    Write-Output ("{0}={1}" -f $entry.Key, $entry.Value)
}
if ($failed.Count -gt 0) {
    throw ('runtime footprint policy failed: ' + (($failed | ForEach-Object Key) -join ', '))
}
Write-Output ("RUNTIME_FOOTPRINT_POLICY_OK={0}" -f $checks.Count)
