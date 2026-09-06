param([string] $Root, [string] $RunId)

$ErrorActionPreference = 'Stop'
$processes = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$checks = [System.Collections.Generic.List[string]]::new()
$result = @{ run_id = $RunId; passed = $false; checks = $checks }
$output = Join-Path $Root 'output'

function Invoke-Executable([string] $Program, [string[]] $Arguments, [string] $Label, [int] $TimeoutSeconds = 20) {
    $quoted = foreach ($argument in $Arguments) {
        '"' + [regex]::Replace([regex]::Replace($argument, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
    }
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Program
    $start.Arguments = $quoted -join ' '
    $start.WorkingDirectory = $Root
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($start)
    $processes.Add($process)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        throw "$Label timed out after $TimeoutSeconds seconds"
    }
    $text = $stdout.Result
    $errors = $stderr.Result
    [IO.File]::WriteAllText((Join-Path $output "$Label.stdout.txt"), $text)
    [IO.File]::WriteAllText((Join-Path $output "$Label.stderr.txt"), $errors)
    if ($process.ExitCode -ne 0) { throw "$Label exited $($process.ExitCode): $errors $text" }
    return $text
}

function Invoke-Pwf([string[]] $Arguments, [string] $Label) {
    $timeout = if ($Arguments.Count -gt 0 -and $Arguments[0] -eq 'server') { 120 } else { 20 }
    Invoke-Executable (Join-Path $Root 'pwf.exe') $Arguments $Label $timeout
}

function Start-Server([string] $Label) {
    $server = Start-Process -FilePath (Join-Path $Root 'pwf-server.exe') -WorkingDirectory $Root -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $output "$Label.stdout.txt") -RedirectStandardError (Join-Path $output "$Label.stderr.txt")
    $processes.Add($server)
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    do {
        if ($server.HasExited) { throw "$Label exited $($server.ExitCode): $(Get-Content (Join-Path $output "$Label.stderr.txt") -Raw)" }
        try {
            $null = Invoke-Pwf @('project', 'ls') "$Label-ready"
            return $server
        } catch {
            if ($elapsed.Elapsed.TotalSeconds -ge 30) { throw }
            Start-Sleep -Milliseconds 200
        }
    } while ($true)
}

try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $result.identity = $identity.Name
    $result.sid = $identity.User.Value
    $result.binaries = @{}
    foreach ($name in @('pwf.exe', 'pwf-server.exe')) {
        $result.binaries[$name] = (Get-FileHash -LiteralPath (Join-Path $Root $name) -Algorithm SHA256).Hash
    }
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Smoke tests must run as a standard user' }
    $null = Invoke-Executable "$env:SystemRoot\System32\whoami.exe" @('/all') 'identity'
    $checks.Add('standard user token')

    $env:PWF_DATABASE_PATH = Join-Path $output 'pwf.sqlite3'
    $env:PWF_RUNTIME_DIR = Join-Path $Root 'runtime'
    $env:RUST_LOG = 'warn'
    $env:NO_COLOR = '1'
    $env:TEMP = Join-Path $Root 'temp'
    $env:TMP = $env:TEMP
    $null = New-Item -ItemType Directory -Path $env:TEMP

    $manifest = Get-Content (Join-Path $Root 'tests.json') -Raw | ConvertFrom-Json
    foreach ($test in $manifest) {
        $null = Invoke-Executable (Join-Path $Root $test) @('--test-threads=1', '--nocapture') $test
        $checks.Add("native tests: $test")
    }

    $server = Start-Server 'server-first'
    $empty = Invoke-Pwf @('project', 'ls') 'empty-registry'
    if ($empty.Trim() -ne '[]') { throw "Fresh database is not empty: $empty" }
    if (-not (Test-Path $env:PWF_DATABASE_PATH)) { throw 'Server did not create the database' }
    $checks.Add('startup migrations create an empty database')

    $duplicateRejected = $false
    try { $null = Invoke-Executable (Join-Path $Root 'pwf-server.exe') @() 'duplicate-server' }
    catch {
        if ($_ -match 'exited' -and (Get-Content (Join-Path $output 'duplicate-server.stderr.txt') -Raw) -match 'pipe|denied|local IPC') { $duplicateRejected = $true }
        else { throw }
    }
    if (-not $duplicateRejected) { throw 'Duplicate server was accepted' }
    $checks.Add('duplicate server rejected')

    $source = Join-Path $Root 'source with spaces'
    $tasks = Join-Path $Root 'tasks with spaces'
    $null = New-Item -ItemType Directory -Path $source, $tasks
    $payload = @{ id = 'WIN'; title = 'windows-smoke'; source = @{ value = $source }; tasks = @{ kind = 'directory'; path = $tasks } } | ConvertTo-Json -Compress
    $project = Invoke-Pwf @('project', 'add', '--kind', 'directory', $payload) 'project-add' | ConvertFrom-Json
    if ($project.id -ne 'WIN' -or $project.source.value -ne $source) { throw 'Project creation did not retain the Windows source path' }
    $paused = Invoke-Pwf @('project', 'pause', 'WIN') 'project-pause' | ConvertFrom-Json
    if (-not $paused.changed) { throw 'Pause did not change the project' }
    $before = Invoke-Pwf @('project', 'get', 'WIN') 'project-before-restart'
    if (-not ($before | ConvertFrom-Json).is_paused) { throw 'Paused state was not persisted' }
    $checks.Add('CLI writes and reads project data over named pipes')

    $server.Kill()
    if (-not $server.WaitForExit(5000)) { throw 'Server did not stop' }
    $server = Start-Server 'server-restarted'
    $after = Invoke-Pwf @('project', 'get', 'WIN') 'project-after-restart'
    if ($before -cne $after) { throw 'Project data changed after server restart' }
    $checks.Add('committed data survives server termination and restart')
    $null = Invoke-Pwf @('project', 'resume', 'WIN') 'project-resume'
    $server.Kill()
    if (-not $server.WaitForExit(5000)) { throw 'Server did not stop before service installation' }
    $null = Invoke-Pwf @('server', 'install') 'service-install'
    $status = Invoke-Pwf @('server', 'status') 'service-status'
    $version = (Invoke-Pwf @('--version') 'cli-version').Trim() -replace '^pwf ', ''
    if ($status -notmatch 'service: running' -or $status -notmatch 'health: serving' -or -not $status.Contains("server version: $version")) { throw "Service is not healthy at the installed version: $status" }
    $null = Invoke-Pwf @('server', 'stop') 'service-stop'
    $executable = [IO.File]::Open((Join-Path $Root 'pwf-server.exe'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $executable.Dispose()
    $checks.Add('service stop releases the executable for upgrades')
    $null = Invoke-Pwf @('server', 'start') 'service-start'
    $null = Invoke-Pwf @('server', 'restart') 'service-restart'
    $null = Invoke-Pwf @('project', 'get', 'WIN') 'service-retained-data'
    $null = Invoke-Pwf @('server', 'uninstall') 'service-uninstall'
    if (-not (Test-Path $env:PWF_DATABASE_PATH)) { throw 'Uninstall deleted the database' }
    if (Get-ScheduledTask -TaskName ('pwf-server-' + $identity.User.Value) -ErrorAction SilentlyContinue) { throw 'Uninstall retained startup registration' }
    $checks.Add('native service install, status, stop, start, restart and uninstall')
    $result.passed = $true
} catch {
    $result.error = $_.ToString()
    $result.stack = $_.ScriptStackTrace
    try {
        $taskName = 'pwf-server-' + $identity.User.Value
        Get-ScheduledTask -TaskName $taskName -ErrorAction Stop | Export-ScheduledTask | Set-Content (Join-Path $output 'service-task.xml')
        Get-ScheduledTaskInfo -TaskName $taskName | ConvertTo-Json | Set-Content (Join-Path $output 'service-task-info.json')
        $null = Invoke-Pwf @('server', 'status') 'service-failure-status'
    } catch { $result.diagnostic_error = $_.ToString() }
} finally {
    try { $null = Invoke-Pwf @('server', 'uninstall') 'service-cleanup' }
    catch { $result.passed = $false; $result.cleanup_error = $_.ToString() }
    foreach ($process in $processes) {
        try {
            if (-not $process.HasExited) {
                $process.Kill()
                if (-not $process.WaitForExit(5000)) { throw "Process $($process.Id) did not stop" }
            }
        } catch { $result.passed = $false; $result.cleanup_error = $_.ToString() }
        $process.Dispose()
    }
    $result | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $output 'result.tmp')
    Move-Item (Join-Path $output 'result.tmp') (Join-Path $output 'result.json')
}
if (-not $result.passed) { exit 1 }
