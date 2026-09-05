param(
    [ValidateSet('Install', 'Stop', 'Remove')]
    [string] $Action = $env:PWF_SERVER_ACTION,
    [string] $Program = $env:PWF_SERVER_PROGRAM
)

$ErrorActionPreference = 'Stop'
if ($Action -eq 'Install' -and (-not [System.IO.Path]::IsPathRooted($Program) -or -not (Test-Path -LiteralPath $Program -PathType Leaf))) {
    throw 'pwf-server installation requires an existing absolute executable path'
}

$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$taskName = 'pwf-server-' + $identity.User.Value
$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($existing) {
    Stop-ScheduledTask -TaskName $taskName
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    while ((Get-ScheduledTask -TaskName $taskName).State -eq 'Running') {
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'pwf-server did not stop within 12 seconds'
        }
        Start-Sleep -Milliseconds 100
    }
}

if ($Action -eq 'Remove') {
    if ($existing) {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    }
    return
}
if ($Action -eq 'Stop') {
    return
}


$taskAction = New-ScheduledTaskAction -Execute $Program -WorkingDirectory (Split-Path -Parent $Program)
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity.Name
$principal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $taskAction -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName

$directory = Split-Path -Parent $Program
$userPath = [string][Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $directory) {
    $updatedPath = ($userPath.TrimEnd(';') + ';' + $directory).TrimStart(';')
    [Environment]::SetEnvironmentVariable('Path', $updatedPath, 'User')
    Write-Host 'Added pwf to the user PATH. Open a new terminal to use it.'
}
