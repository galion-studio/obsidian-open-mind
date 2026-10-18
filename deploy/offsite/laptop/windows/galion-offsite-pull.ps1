# galion-offsite-pull.ps1 (ws/offsite): pull the newest encrypted Galion backup from the VPS.
# Windows PowerShell 5.1 + built-in OpenSSH client. No admin rights. Ciphertext (.tar.gz.age) only;
# nothing is decrypted here (the age key lives in the admin's password manager).
#   1. ssh ... galion-offsite@VPS list  -> "<sha256>  <name>" lines, oldest -> newest
#   2. newest already in the backup dir with the right sha256 -> nothing to do
#   3. ssh ... get <name> -> temp file (raw stdout stream copied byte-for-byte: binary-safe,
#      unlike PowerShell 5.1 ">" redirection), Get-FileHash must equal the list entry
#   4. move into $env:GALION_BACKUP_DIR (default %USERPROFILE%\GalionBackups), keep newest 8
# Log: %USERPROFILE%\.galion-offsite\pull.log. Exit 0 = ok / up to date, 1 = error.
# Env overrides: GALION_OFFSITE_HOST, GALION_OFFSITE_PORT, GALION_OFFSITE_USER, GALION_OFFSITE_HOME,
#   GALION_OFFSITE_KEY, GALION_OFFSITE_KNOWN_HOSTS, GALION_BACKUP_DIR, GALION_OFFSITE_KEEP, GALION_OFFSITE_SSH.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function EnvOr([string]$name, [string]$default) {
    $v = [Environment]::GetEnvironmentVariable($name)
    if ([string]::IsNullOrEmpty($v)) { return $default } else { return $v }
}

$UserHome = EnvOr 'USERPROFILE' $HOME
$RHost  = EnvOr 'GALION_OFFSITE_HOST' '51.38.137.193'
$RPort  = EnvOr 'GALION_OFFSITE_PORT' '22'
$RUser  = EnvOr 'GALION_OFFSITE_USER' 'galion-offsite'
$Cfg    = EnvOr 'GALION_OFFSITE_HOME' (Join-Path $UserHome '.galion-offsite')
$Key    = EnvOr 'GALION_OFFSITE_KEY' (Join-Path $Cfg 'id_ed25519')
$KH     = EnvOr 'GALION_OFFSITE_KNOWN_HOSTS' (Join-Path $Cfg 'known_hosts')
$Dir    = EnvOr 'GALION_BACKUP_DIR' (Join-Path $UserHome 'GalionBackups')
$Keep   = [int](EnvOr 'GALION_OFFSITE_KEEP' '8')
$LogF   = Join-Path $Cfg 'pull.log'
$ListRe = '^[0-9a-f]{64}  (galion-[0-9T+-]+\.tar\.gz\.age)$'

if (-not (Test-Path -LiteralPath $Cfg)) { New-Item -ItemType Directory -Path $Cfg -Force | Out-Null }

function Log([string]$msg) {
    $line = (Get-Date -Format 'yyyy-MM-ddTHH:mm:sszzz') + ' galion-offsite-pull ' + $msg
    try {
        if ((Test-Path -LiteralPath $LogF) -and ((Get-Item -LiteralPath $LogF -Force).Length -gt 1MB)) {
            Move-Item -LiteralPath $LogF -Destination ($LogF + '.1') -Force
        }
        Add-Content -LiteralPath $LogF -Value $line -Encoding ASCII
    } catch { $null = $_ }
    Write-Host $line
}

# Quote one argument for the Windows C runtime command-line parser (CommandLineToArgvW rules).
function QuoteArg([string]$a) {
    if ($a -ne '' -and $a -notmatch '[\s"]') { return $a }
    $a = $a -replace '(\\*)"', '$1$1\"'
    $a = $a -replace '(\\+)$', '$1$1'
    return '"' + $a + '"'
}

function FindSsh {
    $explicit = EnvOr 'GALION_OFFSITE_SSH' ''
    if ($explicit -ne '') { return $explicit }
    $sr = EnvOr 'SystemRoot' ''
    if ($sr -ne '') {
        foreach ($p in @((Join-Path $sr 'System32\OpenSSH\ssh.exe'), (Join-Path $sr 'Sysnative\OpenSSH\ssh.exe'))) {
            if (Test-Path -LiteralPath $p) { return $p }
        }
    }
    $c = Get-Command ssh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($c) { return $c.Source }
    throw 'ssh.exe not found (Settings > Apps > Optional features > OpenSSH Client)'
}

# Run ssh with stdout copied byte-for-byte into $OutFile. Returns the exit code.
# Binary-safe: the raw stdout stream is copied (no PowerShell pipeline, no '>' which re-encodes
# text in 5.1, and no Start-Process -RedirectStandardOutput, which is line-based on non-Windows).
function InvokeSsh([string[]]$RemoteArgs, [string]$OutFile, [int]$TimeoutSec) {
    $khFwd = $KH -replace '\\', '/'
    $sshArgs = @('-i', $Key, '-p', $RPort,
        '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'IdentityAgent=none',
        '-o', ('UserKnownHostsFile="' + $khFwd + '"'), '-o', 'StrictHostKeyChecking=yes',
        '-o', 'ConnectTimeout=20', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=4',
        '-o', 'ClearAllForwardings=yes', '-T', ($RUser + '@' + $RHost)) + $RemoteArgs
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $script:Ssh
    $psi.Arguments = ($sshArgs | ForEach-Object { QuoteArg $_ }) -join ' '
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $psi
    $fs = $null
    try {
        $null = $p.Start()
        $p.StandardInput.Close()
        $errTask = $p.StandardError.ReadToEndAsync()
        $fs = New-Object System.IO.FileStream($OutFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        $copy = $p.StandardOutput.BaseStream.CopyToAsync($fs, 1048576)
        if (-not $copy.Wait($TimeoutSec * 1000)) {
            try { $p.Kill() } catch { $null = $_ }
            throw ('ssh ' + ($RemoteArgs -join ' ') + ' timed out after ' + $TimeoutSec + 's')
        }
        $fs.Close(); $fs = $null
        $p.WaitForExit()
        $code = $p.ExitCode
        $e = ($errTask.Result -split "`r?`n" | Where-Object { $_ -ne '' }) -join ' | '
        if ($code -ne 0 -and $e) { Log ('ssh stderr: ' + $e) }
        return $code
    } finally {
        if ($fs) { $fs.Close() }
        try { if (-not $p.HasExited) { $p.Kill() } } catch { $null = $_ }
        $p.Dispose()
    }
}

function Sha256([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }

$mutex = $null; $haveMutex = $false; $tmp = $null; $listF = $null
try {
    $mutex = New-Object System.Threading.Mutex($false, 'Local\GalionOffsitePull')
    try { $haveMutex = $mutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $haveMutex = $true }
    if (-not $haveMutex) { Log 'ERROR another pull is running'; exit 1 }

    if (-not (Test-Path -LiteralPath $Key)) { throw ('missing ssh key ' + $Key + ' (run install-galion-offsite.ps1)') }
    if (-not (Test-Path -LiteralPath $KH) -or (Get-Item -LiteralPath $KH -Force).Length -eq 0) { throw ('missing known_hosts ' + $KH) }
    if (-not (Test-Path -LiteralPath $Dir)) { New-Item -ItemType Directory -Path $Dir -Force | Out-Null }
    $script:Ssh = FindSsh

    # 1. list
    $listF = Join-Path $Dir ('.list-' + [guid]::NewGuid().ToString('N') + '.tmp')
    $rc = InvokeSsh @('list') $listF 120
    if ($rc -ne 0) { throw ('ssh list failed (exit ' + $rc + ')') }
    $entries = @()
    foreach ($l in @(Get-Content -LiteralPath $listF)) {
        $l = $l.TrimEnd("`r")
        if ($l -eq '') { continue }
        if ($l -match $ListRe) { $entries += ,@($l.Substring(0, 64), $Matches[1]) }
        else { throw ('unexpected list output: ' + $l) }
    }
    if ($entries.Count -eq 0) { throw 'remote list is empty' }
    $nsum = $entries[-1][0]; $name = $entries[-1][1]
    $dest = Join-Path $Dir $name

    # 2. already have it?
    if ((Test-Path -LiteralPath $dest) -and ((Sha256 $dest) -eq $nsum)) {
        Log ('up to date: ' + $name)
    } else {
        # 3. get <name> -> temp file, verify sha256
        $tmp = Join-Path $Dir ('.pull-' + [guid]::NewGuid().ToString('N') + '.tmp')
        $rc = InvokeSsh @('get', $name) $tmp 3600
        if ($rc -ne 0) { throw ('ssh get ' + $name + ' failed (exit ' + $rc + ')') }
        $size = (Get-Item -LiteralPath $tmp -Force).Length
        if ($size -eq 0) { throw ('downloaded ' + $name + ' is empty') }
        $got = Sha256 $tmp
        if ($got -ne $nsum) { throw ('sha256 mismatch for ' + $name + ': got ' + $got + ', list says ' + $nsum) }
        Move-Item -LiteralPath $tmp -Destination $dest -Force
        $tmp = $null
        Log ('pulled ' + $name + ' size=' + $size + ' sha256=' + $got + ' (matches remote list)')
    }

    # 4. keep newest $Keep (names sort by timestamp), refresh local SHA256SUMS
    $files = @(Get-ChildItem -LiteralPath $Dir -File -Filter 'galion-*.tar.gz.age' | Sort-Object Name)
    if ($files.Count -gt $Keep) {
        foreach ($old in $files[0..($files.Count - $Keep - 1)]) {
            Remove-Item -LiteralPath $old.FullName -Force
            Log ('pruned ' + $old.Name)
        }
        $files = @(Get-ChildItem -LiteralPath $Dir -File -Filter 'galion-*.tar.gz.age' | Sort-Object Name)
    }
    $sums = @($files | ForEach-Object { (Sha256 $_.FullName) + '  ' + $_.Name })
    Set-Content -LiteralPath (Join-Path $Dir 'SHA256SUMS') -Value $sums -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $Dir '.last-ok') -Value ((Get-Date -Format 'yyyy-MM-ddTHH:mm:sszzz') + ' ok ' + $name) -Encoding ASCII
    Log ('ok kept=' + $files.Count + ' dir=' + $Dir)
    exit 0
} catch {
    Log ('ERROR ' + $_.Exception.Message)
    exit 1
} finally {
    if ($tmp) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
    if ($listF) { Remove-Item -LiteralPath $listF -Force -ErrorAction SilentlyContinue }
    if ($haveMutex) { try { $mutex.ReleaseMutex() } catch { $null = $_ } }
    if ($mutex) { $mutex.Dispose() }
}
