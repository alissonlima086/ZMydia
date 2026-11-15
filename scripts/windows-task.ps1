param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('install', 'uninstall')]
    [string]$Action,

    [Parameter(Mandatory = $true)]
    [string]$InstallDirectory,

    [string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
$serviceTaskName = 'ZMydia'
$backupTaskName = 'ZMydia Backup'
$executable = Join-Path $InstallDirectory 'bin\zmydia-server.exe'

if ($Action -eq 'uninstall') {
    foreach ($taskName in @($serviceTaskName, $backupTaskName)) {
        $existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        if ($existing) {
            Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
        }
    }
    $deadline = (Get-Date).AddSeconds(30)
    do {
        $running = Get-CimInstance Win32_Process -Filter "Name = 'zmydia-server.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.ExecutablePath -eq $executable }
        if (-not $running) {
            break
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    if ($running) {
        throw 'ZMydia background service did not stop within 30 seconds'
    }
    exit 0
}

if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "ZMydia executable not found: $executable"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$serviceAction = New-ScheduledTaskAction `
    -Execute $executable `
    -Argument 'serve' `
    -WorkingDirectory $InstallDirectory
$serviceTrigger = New-ScheduledTaskTrigger -AtLogOn -User $identity
$backupAction = New-ScheduledTaskAction `
    -Execute $executable `
    -Argument 'backup' `
    -WorkingDirectory $InstallDirectory
$backupTrigger = New-ScheduledTaskTrigger -Daily -At '03:00'
$principal = New-ScheduledTaskPrincipal `
    -UserId $identity `
    -LogonType Interactive `
    -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -MultipleInstances IgnoreNew `
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 1)

foreach ($taskName in @($serviceTaskName, $backupTaskName)) {
    if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
        Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    }
}
Register-ScheduledTask `
    -TaskName $serviceTaskName `
    -Action $serviceAction `
    -Trigger $serviceTrigger `
    -Principal $principal `
    -Settings $settings `
    -Description 'ZMydia personal video library background service' `
    -Force | Out-Null
Register-ScheduledTask `
    -TaskName $backupTaskName `
    -Action $backupAction `
    -Trigger $backupTrigger `
    -Principal $principal `
    -Settings $settings `
    -Description 'Create a daily ZMydia library backup' `
    -Force | Out-Null
Start-ScheduledTask -TaskName $serviceTaskName

if ($ExpectedVersion) {
    $deadline = (Get-Date).AddSeconds(45)
    do {
        try {
            $health = Invoke-RestMethod -Uri 'http://127.0.0.1:4173/api/health/ready' -TimeoutSec 2
            if ($health.status -eq 'ready' -and $health.version -eq $ExpectedVersion) {
                exit 0
            }
        } catch {
            # The service is expected to be temporarily unavailable while starting.
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "ZMydia $ExpectedVersion did not become ready within 45 seconds"
}
