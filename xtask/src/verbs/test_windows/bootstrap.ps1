param([string] $Bundle)

$ErrorActionPreference = 'Stop'
$Bundle = [IO.Path]::GetFullPath($Bundle).TrimEnd('\')
$runId = Split-Path -Leaf $Bundle
$suffix = [Guid]::NewGuid().ToString('N').Substring(0, 12)
$userName = 'pwf' + $suffix
$root = Join-Path $env:ProgramData ('pwf-smoke-' + $suffix)
$output = Join-Path $Bundle 'output'
$user = $null
$worker = $null
$remoteSessionId = $null
$controllerSessionId = (Get-Process -Id $PID).SessionId
$desktopDisconnected = $false
$credentialsPath = Join-Path $Bundle 'connection\login.json'
$result = @{ run_id = $runId; passed = $false }

try {
    $null = New-Item -ItemType Directory -Path $root
    Get-ChildItem -LiteralPath $Bundle -File | Copy-Item -Destination $root
    $null = New-Item -ItemType Directory -Path (Join-Path $root 'output')
    $password = 'Pwf!9' + [Guid]::NewGuid().ToString('N')
    $user = New-LocalUser -Name $userName -Password (ConvertTo-SecureString $password -AsPlainText -Force) -AccountNeverExpires -PasswordNeverExpires
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $user
    Add-LocalGroupMember -SID 'S-1-5-32-555' -Member $user
    $acl = Get-Acl -LiteralPath $root
    $rule = [Security.AccessControl.FileSystemAccessRule]::new($user.SID, 'Modify', 'ContainerInherit, ObjectInherit', 'None', 'Allow')
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $root -AclObject $acl
    # Interactive-token tasks need a signed-in user, beyond a RunAs process.
    @{ username = $userName; password = $password; domain = $env:COMPUTERNAME } |
        ConvertTo-Json | Set-Content -Encoding UTF8 ($credentialsPath + '.tmp')
    Move-Item ($credentialsPath + '.tmp') $credentialsPath
    & tsdiscon.exe $controllerSessionId
    if ($LASTEXITCODE -ne 0) { throw 'Could not disconnect the controller desktop for the test user' }
    $desktopDisconnected = $true
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    while ($null -eq $remoteSessionId) {
        $sessions = & cmd.exe /c "quser.exe $userName 2>nul"
        foreach ($line in $sessions) {
            if ($line -match ('^\s*>?\s*' + [Regex]::Escape($userName) + '\s+(?:\S+\s+)?(\d+)\s')) {
                $remoteSessionId = [int]$Matches[1]
            }
        }
        if ($elapsed.Elapsed.TotalSeconds -ge 90) { throw 'Temporary user did not sign in within 90 seconds' }
        Start-Sleep -Milliseconds 500
    }
    $program = "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe"
    $arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + (Join-Path $root 'smoke.ps1') + '" -Root "' + $root + '" -RunId "' + $runId + '"'
    $credential = [Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$userName", (ConvertTo-SecureString $password -AsPlainText -Force))
    $worker = Start-Process -FilePath $program -ArgumentList $arguments -WorkingDirectory $root -Credential $credential -LoadUserProfile -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root 'output\worker.stdout.txt') -RedirectStandardError (Join-Path $root 'output\worker.stderr.txt')
    $password = $null
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    $resultPath = Join-Path $root 'output\result.json'
    do {
        Start-Sleep -Milliseconds 500
        if (Test-Path $resultPath) { break }
        if ($worker.HasExited) { throw "Guest process stopped without a result (exit $($worker.ExitCode))" }
        if ($elapsed.Elapsed.TotalMinutes -ge 5) { throw 'Guest tests timed out after five minutes' }
    } while ($true)
    $result = Get-Content $resultPath -Raw | ConvertFrom-Json
} catch {
    $result = @{ run_id = $runId; passed = $false; error = $_.ToString(); stack = $_.ScriptStackTrace }
} finally {
    $cleanupErrors = [System.Collections.Generic.List[string]]::new()
    $cleanupActions = @(
        { Remove-Item $credentialsPath, ($credentialsPath + '.tmp') -Force -ErrorAction SilentlyContinue },
        {
            if ($worker) {
                if (-not $worker.HasExited -and -not $worker.WaitForExit(5000)) { $worker.Kill() }
                if (-not $worker.WaitForExit(5000)) { throw 'Guest process did not stop' }
                $worker.Dispose()
            }
        },
        {
        $owned = Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase) }
        foreach ($process in $owned) {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
            Wait-Process -Id $process.ProcessId -Timeout 5 -ErrorAction SilentlyContinue
        }
        },
        { if (Test-Path (Join-Path $root 'output')) { Copy-Item (Join-Path $root 'output\*') $output -Recurse -Force } },
        {
            if ($null -ne $remoteSessionId) {
                & logoff.exe $remoteSessionId
            }
            if ($desktopDisconnected) {
                & tscon.exe $controllerSessionId /dest:console
                if ($LASTEXITCODE -ne 0) { throw 'Could not restore the original desktop session' }
            }
        },
        {
        if ($user) {
            $elapsed = [Diagnostics.Stopwatch]::StartNew()
            do {
                $profile = Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -eq $user.SID.Value }
                if (-not $profile -or -not $profile.Loaded) { break }
                if ($elapsed.Elapsed.TotalSeconds -ge 15) { throw 'Test user profile remained loaded' }
                Start-Sleep -Milliseconds 500
            } while ($true)
            $profile = Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -eq $user.SID.Value }
            if ($profile) { $profile | Remove-CimInstance }
        }
        },
        { if ($user) { Remove-LocalUser -SID $user.SID } },
        { if (Test-Path $root) { Remove-Item -LiteralPath $root -Recurse -Force } }
    )
    foreach ($cleanup in $cleanupActions) {
        try { & $cleanup } catch { $cleanupErrors.Add($_.ToString()) }
    }
    if ($cleanupErrors.Count -gt 0) {
        $result = @{ run_id = $runId; passed = $false; cleanup_errors = $cleanupErrors; test_result = $result; user_name = $userName; directory = $root }
    }
    $result | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 (Join-Path $output 'complete.tmp')
    Move-Item (Join-Path $output 'complete.tmp') (Join-Path $output 'complete.json')
}
