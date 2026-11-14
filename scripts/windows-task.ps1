param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('install', 'uninstall')]
    [string]$Action,

    [Parameter(Mandatory = $true)]
    [string]$InstallDirectory
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
