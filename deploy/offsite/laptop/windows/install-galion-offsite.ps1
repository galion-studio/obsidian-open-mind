# install-galion-offsite.ps1 (ws/offsite): set up the Galion offsite backup pull on a Windows laptop.
# Windows 10/11, Windows PowerShell 5.1, built-in OpenSSH client. No admin rights needed.
# Run it (as yourself, not elevated):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\install-galion-offsite.ps1
# What it does (idempotent; safe to re-run):
#   - creates %USERPROFILE%\.galion-offsite
#   - generates id_ed25519 there if missing (no passphrase; ACL = current user only, as OpenSSH requires)
#   - writes known_hosts with the pinned VPS host key (public data, embedded below)
#   - writes galion-offsite-pull.ps1 (embedded below)
#   - registers the scheduled task GalionOffsitePull (02:45, 08:45, 14:45, 20:45 local, catches up missed runs)
#   - prints the PUBLIC key line to send to Grok Bot. The private key never leaves this laptop.
# The backups stay age-encrypted; the age key is kept separately in the password manager.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$VpsHost   = '51.38.137.193'
$HostKey   = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINuUEX+qXmabIUUnNOc4l/Pj6uodQgS88AomCjxdPfDp'
$HostKeyFp = 'SHA256:35BjCqYWlUUOEZIos88Y6csdExE4Riql18I2Q+pRzRI'
$TaskName  = 'GalionOffsitePull'

$Cfg  = Join-Path $env:USERPROFILE '.galion-offsite'
$Key  = Join-Path $Cfg 'id_ed25519'
$KH   = Join-Path $Cfg 'known_hosts'
$Pull = Join-Path $Cfg 'galion-offsite-pull.ps1'

function Say([string]$m) { Write-Host ('install-galion-offsite: ' + $m) }

function FindTool([string]$exe) {
    foreach ($p in @((Join-Path $env:SystemRoot ('System32\OpenSSH\' + $exe)), (Join-Path $env:SystemRoot ('Sysnative\OpenSSH\' + $exe)))) {
        if (Test-Path -LiteralPath $p) { return $p }
    }
    $c = Get-Command $exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($c) { return $c.Source }
    throw ($exe + ' not found. Install the "OpenSSH Client" optional feature (Settings > Apps > Optional features).')
}

# Run a program with an explicit command line (avoids PowerShell 5.1 dropping empty arguments).
function RunExe([string]$exe, [string]$argLine) {
    $p = Start-Process -FilePath $exe -ArgumentList $argLine -NoNewWindow -PassThru -Wait
    return $p.ExitCode
}

$SshKeygen = FindTool 'ssh-keygen.exe'
$null = FindTool 'ssh.exe'

# 1. config dir
if (-not (Test-Path -LiteralPath $Cfg)) { New-Item -ItemType Directory -Path $Cfg -Force | Out-Null }
Say ('config dir: ' + $Cfg)

# 2. laptop key (generated here; only the .pub is ever sent anywhere)
$me = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
if (-not (Test-Path -LiteralPath $Key)) {
    $cmt = ('galion-offsite-' + $env:COMPUTERNAME + '-' + $env:USERNAME) -replace '[^A-Za-z0-9._-]', ''
    $rc = RunExe $SshKeygen ('-q -t ed25519 -N "" -C ' + $cmt + ' -f "' + $Key + '"')
    if ($rc -ne 0 -or -not (Test-Path -LiteralPath $Key)) { throw ('ssh-keygen failed (exit ' + $rc + ')') }
    Say ('generated new key ' + $Key)
} else {
    Say ('keeping existing key ' + $Key)
}
# private key readable by the current user only (no inheritance), otherwise ssh.exe refuses it
& icacls.exe $Key /inheritance:r /grant:r ('*' + $me + ':F') | Out-Null
if ($LASTEXITCODE -ne 0) { throw ('icacls on ' + $Key + ' failed (exit ' + $LASTEXITCODE + ')') }
if (-not (Test-Path -LiteralPath ($Key + '.pub'))) {
    $pubText = & $SshKeygen -y -f $Key
    if ($LASTEXITCODE -ne 0) { throw 'could not derive the public key' }
    Set-Content -LiteralPath ($Key + '.pub') -Value $pubText -Encoding ASCII
}

# 3. pinned VPS host key
Set-Content -LiteralPath $KH -Value ($VpsHost + ' ' + $HostKey) -Encoding ASCII
$fpOut = (& $SshKeygen -l -f $KH) -join ' '
if ($fpOut -notmatch [regex]::Escape($HostKeyFp)) { throw ('known_hosts fingerprint check failed: ' + $fpOut) }
Say ('pinned VPS host key ' + $HostKeyFp)

# 4. pull script
$PullScript = @'
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
'@
Set-Content -LiteralPath $Pull -Value $PullScript -Encoding ASCII
Say ('wrote ' + $Pull)

# 5. scheduled task for the current user (runs while logged on; no admin, no stored password)
$ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$action = New-ScheduledTaskAction -Execute $ps `
    -Argument ('-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $Pull + '"') `
    -WorkingDirectory $Cfg
$triggers = @('02:45', '08:45', '14:45', '20:45') | ForEach-Object { New-ScheduledTaskTrigger -Daily -At $_ }
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Hours 2)
$userId = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$principal = New-ScheduledTaskPrincipal -UserId $userId -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $triggers -Settings $settings `
    -Principal $principal -Description 'Pull the newest encrypted Galion backup from the VPS (galion-offsite).' -Force | Out-Null
Say ('scheduled task ' + $TaskName + ' registered: daily 02:45 08:45 14:45 20:45 local, missed runs start when available')

# 6. public key for the admin to send
$pub = (Get-Content -LiteralPath ($Key + '.pub') | Select-Object -First 1).Trim()
Write-Host ''
Write-Host '=================================================================='
Write-Host ' Send this public key line to Grok Bot (it is PUBLIC, safe to send):'
Write-Host ''
Write-Host $pub
Write-Host ''
Write-Host ' Do NOT send id_ed25519 (no .pub) - that private key stays here.'
Write-Host '=================================================================='
Write-Host ''
Say ('after Grok Bot confirms the key is authorised, test with:  Start-ScheduledTask -TaskName ' + $TaskName)
Say ('or run:  powershell -NoProfile -ExecutionPolicy Bypass -File "' + $Pull + '"')
Say ('log: ' + (Join-Path $Cfg 'pull.log') + '   backups: ' + (Join-Path $env:USERPROFILE 'GalionBackups'))
