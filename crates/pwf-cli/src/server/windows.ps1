$ErrorActionPreference = 'Stop'
$action = $env:PWF_SERVER_ACTION
$program = $env:PWF_SERVER_PROGRAM
$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$taskName = 'pwf-server-' + $identity.User.Value
$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue

if ($action -eq 'Status') {
    if (-not $existing) { Write-Output 'not installed' }
    elseif ($existing.State -eq 'Running') { Write-Output 'running' }
    else { Write-Output 'stopped' }
    exit 0
}

if ($action -eq 'Stop' -or $action -eq 'Uninstall') {
    if ($existing) {
        Stop-ScheduledTask -TaskName $taskName
        $deadline = [DateTime]::UtcNow.AddSeconds(12)
        while ((Get-ScheduledTask -TaskName $taskName).State -eq 'Running') {
            if ([DateTime]::UtcNow -ge $deadline) { throw 'pwf-server did not stop within 12 seconds' }
            Start-Sleep -Milliseconds 100
        }
        if ($action -eq 'Uninstall') { Unregister-ScheduledTask -TaskName $taskName -Confirm:$false }
    }
    exit 0
}

if ($action -eq 'Start') {
    if (-not $existing) { throw 'pwf-server is not installed - run pwf server install' }
    Start-ScheduledTask -TaskName $taskName
    exit 0
}

if ($action -ne 'Install') { throw 'unknown server action' }
if (-not [System.IO.Path]::IsPathRooted($program) -or -not (Test-Path -LiteralPath $program -PathType Leaf)) {
    throw 'pwf-server installation requires an existing absolute executable path'
}

$launch = '$ErrorActionPreference = ''Stop''' + "`n"
$environment = ConvertFrom-Json $env:PWF_SERVER_ENVIRONMENT
foreach ($entry in $environment) {
    $name = ([string]$entry[0]).Replace("'", "''")
    $value = ([string]$entry[1]).Replace("'", "''")
    $launch += "[Environment]::SetEnvironmentVariable('$name', '$value', 'Process')`n"
}
$quotedProgram = $program.Replace("'", "''")
$launch += "& '$quotedProgram'`n" + 'exit $LASTEXITCODE'
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($launch))
$powershell = Join-Path $PSHOME 'powershell.exe'
$taskAction = New-ScheduledTaskAction -Execute $powershell -Argument "-NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand $encoded" -WorkingDirectory (Split-Path -Parent $program)
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity.Name
$principal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $taskAction -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
