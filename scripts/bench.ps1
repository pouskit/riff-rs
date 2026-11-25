# Bench of riff-rs against the original Riff (Electron).
# Reuses scripts\measure.ps1 (idle RAM) and scripts\scenes\measure-scene.ps1 (playback RAM and CPU per scene).
# Writes bench-results.json and bench-table.md in -Out. A failed measure never stops the others: it is written
# as "not measured" with its reason. Only one of the two programs runs at a time, the window is never minimized.
# Every launch uses isolated data folders (LOCALAPPDATA, APPDATA, RIFF_DATA_DIR, --user-data-dir, RIFF_IPC_NAME).
# Compatible with Windows PowerShell 5.1.
#
#   .\scripts\bench.ps1 -RsExe .\target\release\riff-app.exe -RiffExe <copy of Riff.exe> -Fixture <dir> -Work <dir> -Out <dir>
#   .\scripts\bench.ps1 -RsExe ... -Only size,ram-idle -IdleRuns 1 -IdleWait 10       (short trial)
#
# Measures (-Only): size, ram-idle, startup, ram-play, cpu, search, scroll, latency.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RsExe,
  # Copy of the published Riff outside the real install and outside the Riff reference repository. Without it only riff-rs is measured.
  [string]$RiffExe = '',
  # Folder made by scripts\scenes\fixture.mjs (scene-bench.mp3, scene-bench.lrc, cover.jpg).
  [Parameter(Mandatory = $true)][string]$Fixture,
  [Parameter(Mandatory = $true)][string]$Work,
  [Parameter(Mandatory = $true)][string]$Out,
  [ValidateSet('size', 'ram-idle', 'startup', 'ram-play', 'cpu', 'search', 'scroll', 'latency', 'latency-online')]
  [string[]]$Only = @('size', 'ram-idle', 'startup', 'ram-play', 'cpu', 'search', 'scroll', 'latency', 'latency-online'),
  # Commit of riff-rs the exe was built from (the worktree HEAD does not match the build once it moves).
  [string]$RsCommit = '',
  [string]$Node = 'C:\Program Files\nodejs\node.exe',
  # Idle RAM: launches, wait before sampling, samples (measure.ps1 defaults: 5, 30 s).
  [int]$IdleRuns = 5,
  [int]$IdleWait = 30,
  # Startup to first content: launches per application.
  [int]$StartupRuns = 5,
  # Playback scenes (measure-scene.ps1): list, passes per scene, seconds of sampling.
  [string[]]$Scenes = @('a', 'b', 'b2', 'c', 'd', 'e'),
  [int]$SceneRuns = 2,
  [int]$PlaySeconds = 60,
  # Search: same query, trials.
  [string]$SearchQuery = 'daft punk one more time',
  [int]$SearchTrials = 10,
  # Distinct queries of the second series (a server cache would make the repeated query almost free); the first $SearchTrials are used.
  [string[]]$SearchOthers = @('radiohead creep', 'nirvana smells like teen spirit', 'adele hello', 'queen bohemian rhapsody', 'coldplay yellow', 'eminem lose yourself', 'michael jackson thriller', 'beatles hey jude', 'pink floyd money', 'amy winehouse rehab'),
  # Scroll (--bench-scroll, 33 s each) and playback latency (--latency-log).
  [int]$ScrollRuns = 3,
  [int]$ScrollTracks = 5000,
  [int]$LatencyTrials = 5,
  # Install folder of Riff and its installer, read for their size only.
  [string]$RiffInstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Riff'),
  [string]$RiffInstaller = (Join-Path $env:USERPROFILE 'Downloads\riff-44.1.1-win-x64.exe'),
  [int]$Port = 9344
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$scenesDir = Join-Path $here 'scenes'
$culture = [System.Globalization.CultureInfo]::InvariantCulture
$utf8 = New-Object System.Text.UTF8Encoding($false)
$scriptStart = Get-Date
$stamp = $scriptStart.ToString('yyyyMMdd-HHmmss')

# ---------------------------------------------------------------- helpers

function Get-Median([double[]]$Values) {
  $s = @($Values | Sort-Object)
  $m = [math]::Floor($s.Count / 2)
  if ($s.Count % 2 -eq 1) { return $s[$m] }
  return ($s[$m - 1] + $s[$m]) / 2
}

function Get-UnixMs { [long][math]::Floor(([DateTimeOffset]::UtcNow).ToUnixTimeMilliseconds()) }

# Descendants of the given PID (recursive through ParentProcessId), created since $Since. The root is included.
# PID reuse: a process created before $Since is ignored.
function Get-Descendants([int]$RootId, [datetime]$Since) {
  $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate | Where-Object { $_.CreationDate -ge $Since })
  $seen = New-Object 'System.Collections.Generic.HashSet[int]'
  [void]$seen.Add($RootId)
  $grew = $true
  while ($grew) {
    $grew = $false
    foreach ($p in $all) {
      if ($seen.Contains([int]$p.ParentProcessId) -and $seen.Add([int]$p.ProcessId)) { $grew = $true }
    }
  }
  $seen.Remove($RootId) | Out-Null
  if ($RootId -ne $PID) { $seen.Add($RootId) | Out-Null }
  return [int[]]@($seen)
}

function Stop-Ids([int[]]$Ids) {
  foreach ($id in $Ids) {
    try { Stop-Process -Id $id -Force -ErrorAction Stop } catch { Write-Verbose "pid $id already gone" }
  }
}

# Kills a process and its tree (children created since $Since), by PID only: never by image name.
function Stop-Tree([int]$RootId, [datetime]$Since) {
  Stop-Ids @(Get-Descendants $RootId $Since)
  Start-Sleep -Milliseconds 500
}

# End-of-measure check: anything still alive below this script is ours (launched by us or by a script we ran).
function Assert-NoLeftover([string]$After) {
  $left = @(Get-Descendants $PID $scriptStart)
  if ($left.Count -gt 0) {
    $names = ($left | ForEach-Object { try { (Get-Process -Id $_ -ErrorAction Stop).ProcessName + ':' + $_ } catch { "gone:$_" } }) -join ' '
    Write-Warning "leftover processes after ${After}: $names (killed)"
    Stop-Ids $left
    Start-Sleep -Seconds 1
  }
}

# Isolated profile for one launch: LOCALAPPDATA, APPDATA, RIFF_DATA_DIR and RIFF_IPC_NAME point to a work folder, so
# no real data is read and no real instance receives the "show" request of the single-instance channel.
$script:envSaved = $null
function Enter-Isolation([string]$Root) {
  $script:envSaved = @{}
  foreach ($name in 'LOCALAPPDATA', 'APPDATA', 'RIFF_DATA_DIR', 'RIFF_IPC_NAME') {
    $script:envSaved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
  }
  $local = Join-Path $Root 'local'
  $dataDir = Join-Path $local 'Riff'
  New-Item -ItemType Directory -Force $dataDir, (Join-Path $Root 'roaming'), (Join-Path $Root 'userdata') | Out-Null
  $env:LOCALAPPDATA = $local
  $env:APPDATA = Join-Path $Root 'roaming'
  $env:RIFF_DATA_DIR = $dataDir
  $env:RIFF_IPC_NAME = "riff-bench-$PID"
  return [pscustomobject]@{ Local = $local; Data = $dataDir; User = (Join-Path $Root 'userdata') }
}
function Exit-Isolation {
  if ($script:envSaved) {
    foreach ($name in $script:envSaved.Keys) { [Environment]::SetEnvironmentVariable($name, $script:envSaved[$name], 'Process') }
    $script:envSaved = $null
  }
}

$windowState = '{"bounds":{"x":40,"y":40,"width":1280,"height":800},"isMaximized":false,"isFullScreen":false}'

# Fresh profile of one application: same window (1280 x 800), empty library, no first-run screen.
function New-Profile([string]$App, [string]$Root) {
  if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -Confirm:$false }
  $iso = Enter-Isolation $Root
  if ($App -eq 'riff') {
    [System.IO.File]::WriteAllText((Join-Path $iso.User 'window-state.json'), $windowState, $utf8)
    # Without the marker of the same version, Riff opens welcome.html instead of index.html.
    [System.IO.File]::WriteAllText((Join-Path $iso.Data '.rifftutorial'), $script:meta.riff.version, $utf8)
  } else {
    [System.IO.File]::WriteAllText((Join-Path $iso.Data 'window-state.json'), $windowState, $utf8)
  }
  return $iso
}

# ---------------------------------------------------------------- machine and applications

function Get-FileSha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$gpus = @(Get-CimInstance Win32_VideoController | Where-Object { $_.CurrentRefreshRate })
$os = Get-CimInstance Win32_OperatingSystem
$cs = Get-CimInstance Win32_ComputerSystem
$machine = [ordered]@{
  cpu       = $cpu.Name.Trim()
  logical   = [int]$cpu.NumberOfLogicalProcessors
  ramGb     = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
  gpu       = (($gpus | ForEach-Object { "$($_.Name) (driver $($_.DriverVersion))" }) -join '; ')
  refreshHz = (($gpus | ForEach-Object { $_.CurrentRefreshRate }) -join '; ')
  os        = "$($os.Caption.Trim()) build $($os.BuildNumber)"
}
$machineText = "$($machine.cpu), $($machine.gpu), $($machine.os), $($machine.ramGb) GB RAM, $($machine.refreshHz) Hz"

$RsExe = (Resolve-Path $RsExe).Path
$script:meta = @{
  rs   = [ordered]@{
    name = 'riff-rs'; exe = $RsExe; version = $(if ($RsCommit) { "commit $RsCommit" } else { 'commit not given' })
    sha256 = (Get-FileSha256 $RsExe); built = (Get-Item $RsExe).LastWriteTime.ToString('s')
  }
  riff = $null
}
if ($RiffExe) {
  $RiffExe = (Resolve-Path $RiffExe).Path
  $info = (Get-Item $RiffExe).VersionInfo
  $script:meta.riff = [ordered]@{
    name = 'Riff (Electron)'; exe = $RiffExe; version = ($info.ProductVersion -replace '^\s+|\s+$', '' -replace '^(\d+\.\d+\.\d+)\.0$', '$1')
    sha256 = (Get-FileSha256 $RiffExe); built = (Get-Item $RiffExe).LastWriteTime.ToString('s')
  }
}
$apps = @('riff', 'rs') | Where-Object { $script:meta[$_] }

# ---------------------------------------------------------------- results

$script:results = New-Object 'System.Collections.Generic.List[object]'

function Add-Result([string]$Measure, [string]$App, [string]$Unit, [double[]]$Trials, [double]$Value, [string]$Method, [string]$Note = '') {
  $stats = $Trials | Measure-Object -Minimum -Maximum
  $script:results.Add([pscustomobject][ordered]@{
      measure = $Measure; app = $App; status = 'measured'
      value = [math]::Round($Value, 2); unit = $Unit
      min = [math]::Round($stats.Minimum, 2); max = [math]::Round($stats.Maximum, 2)
      trials = $Trials.Count; raw = @($Trials | ForEach-Object { [math]::Round($_, 2) })
      method = $Method; note = $Note; reason = $null
      date = (Get-Date).ToString('o'); machine = $machineText
      exe = $script:meta[$App].exe; version = $script:meta[$App].version
    })
  Write-Host ("[{0}] {1}: {2} {3} (min {4}, max {5}, {6} trial(s))" -f $Measure, $App, [math]::Round($Value, 2), $Unit, [math]::Round($stats.Minimum, 2), [math]::Round($stats.Maximum, 2), $Trials.Count)
}

function Add-Missing([string]$Measure, [string]$App, [string]$Unit, [string]$Method, [string]$Reason) {
  $script:results.Add([pscustomobject][ordered]@{
      measure = $Measure; app = $App; status = 'not-measured'
      value = $null; unit = $Unit; min = $null; max = $null; trials = 0; raw = @()
      method = $Method; note = ''; reason = $Reason
      date = (Get-Date).ToString('o'); machine = $machineText
      exe = $script:meta[$App].exe; version = $script:meta[$App].version
    })
  Write-Warning "[$Measure] $App not measured: $Reason"
}

# Runs one measure for one application: any error becomes a "not measured" entry.
function Invoke-Measure([string]$Measure, [string]$App, [string]$Unit, [string]$Method, [scriptblock]$Body) {
  try {
    & $Body
  } catch {
    Add-Missing $Measure $App $Unit $Method ($_.Exception.Message -replace '\s+', ' ')
  } finally {
    Exit-Isolation
    Assert-NoLeftover "$Measure/$App"
  }
}

function Write-Outputs {
  New-Item -ItemType Directory -Force $Out | Out-Null
  $doc = [ordered]@{
    generated = (Get-Date).ToString('o')
    machine   = $machine
    apps      = [ordered]@{ riff = $script:meta.riff; rs = $script:meta.rs }
    measures  = $script:results.ToArray()
  }
  [System.IO.File]::WriteAllText((Join-Path $Out 'bench-results.json'), (ConvertTo-Json $doc -Depth 8), $utf8)
  [System.IO.File]::WriteAllText((Join-Path $Out 'bench-table.md'), (New-MarkdownTable), $utf8)
}

$labels = [ordered]@{
  'size-install'   = 'Taille du dossier d''installation'
  'size-exe'       = 'Taille de l''ex' + [char]0x00E9 + 'cutable'
  'size-installer' = 'Taille de l''installeur'
  'ram-idle'       = 'RAM au repos (m' + [char]0x00E9 + 'diane)'
  'startup'        = 'D' + [char]0x00E9 + 'marrage jusqu''au premier rendu'
  'ram-play'       = 'RAM en lecture (' + $PlaySeconds + ' s)'
  'search'         = 'Temps de recherche (m' + [char]0x00E9 + 'diane)'
  'search-uncached' = 'Temps de recherche, requ' + [char]0x00EA + 'tes distinctes (m' + [char]0x00E9 + 'diane)'
  'scroll'         = 'D' + [char]0x00E9 + 'filement 5 000 pistes (p95)'
  'latency'        = 'Latence de d' + [char]0x00E9 + 'but de lecture, fichier local (m' + [char]0x00E9 + 'diane)'
  'latency-online' = 'Latence de d' + [char]0x00E9 + 'but de lecture, titre en ligne'
}

$sceneNames = @{
  a = 'rien d''affich' + [char]0x00E9; b = 'paroles mot ' + [char]0x00E0 + ' mot'; b2 = 'paroles en d' + [char]0x00E9 + 'grad' + [char]0x00E9
  c = 'barres + forme'; d = 'fond de pochette flou'; e = 'paroles + barres + forme + fond flou'
  mesh = 'mesh'; banner = 'banni' + [char]0x00E8 + 're'; bg = 'fond personnalis' + [char]0x00E9; liquid = 'vague li' + [char]0x00E9 + 'e'
}
function Get-CpuLabel([string]$Key) {
  $scene = $Key.Substring(4)
  $name = $(if ($sceneNames.ContainsKey($scene)) { $sceneNames[$scene] } else { '' })
  return "CPU moyen en lecture, sc$([char]0x00E8)ne $scene ($name), $PlaySeconds s"
}

function Format-Cell($Entry) {
  if (-not $Entry) { return 'non lanc' + [char]0x00E9 + 'e' }
  if ($Entry.status -ne 'measured') { return 'non mesur' + [char]0x00E9 + 'e : ' + $Entry.reason }
  $text = [string]::Format($culture, '{0:0.##} {1}', $Entry.value, $Entry.unit)
  if ($Entry.trials -gt 1) { $text += [string]::Format($culture, ' ({0:0.##} ' + [char]0x00E0 + ' {1:0.##}, {2} essais)', $Entry.min, $Entry.max, $Entry.trials) }
  return $text
}

function New-MarkdownTable {
  $rows = New-Object 'System.Collections.Generic.List[string]'
  $riffHead = $(if ($script:meta.riff) { "Riff $($script:meta.riff.version)" } else { 'Riff' })
  $rows.Add("| Mesure | $riffHead | riff-rs ($($script:meta.rs.version)) | M" + [char]0x00E9 + 'thode |')
  $rows.Add('|---|---|---|---|')
  $keys = @($script:results | ForEach-Object { $_.measure } | Select-Object -Unique)
  $first = @('size-install', 'size-exe', 'size-installer', 'ram-idle', 'startup', 'ram-play')
  $order = $first + @($keys | Where-Object { $_ -like 'cpu-*' }) + @('search', 'search-uncached', 'scroll', 'latency', 'latency-online') + @($keys | Where-Object { $first -notcontains $_ -and $_ -notlike 'cpu-*' -and @('search', 'search-uncached', 'scroll', 'latency', 'latency-online') -notcontains $_ })
  foreach ($key in $order) {
    $entries = @($script:results | Where-Object { $_.measure -eq $key })
    if ($entries.Count -eq 0) { continue }
    $riff = $entries | Where-Object { $_.app -eq 'riff' } | Select-Object -First 1
    $rs = $entries | Where-Object { $_.app -eq 'rs' } | Select-Object -First 1
    $label = $(if ($labels.Contains($key)) { $labels[$key] } elseif ($key -like 'cpu-*') { Get-CpuLabel $key } else { $key })
    $method = ($entries | Select-Object -First 1).method
    $rows.Add("| $label | $(Format-Cell $riff) | $(Format-Cell $rs) | $method |")
  }
  $rows.Add('')
  $rows.Add("Machine : $machineText. Date : $($scriptStart.ToString('s')).")
  return (($rows -join "`n") + "`n")
}

# ---------------------------------------------------------------- measures

function Get-DirectoryBytes([string]$Path) {
  $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force | Measure-Object -Property Length -Sum).Sum
  return [double]$sum
}

function Measure-Size {
  $method = 'Taille lue sur le disque (lecture seule), en Mo (1 Mo = 1 048 576 octets).'
  Invoke-Measure 'size-exe' 'rs' 'Mo' $method {
    $bytes = (Get-Item $RsExe).Length
    Add-Result 'size-exe' 'rs' 'Mo' @($bytes / 1MB) ($bytes / 1MB) $method
  }
  Invoke-Measure 'size-install' 'rs' 'Mo' $method {
    Add-Missing 'size-install' 'rs' 'Mo' $method 'pas de dossier d''installation : riff-rs est un executable seul (installeur : paquet cargo-packager)'
  }
  Invoke-Measure 'size-installer' 'rs' 'Mo' $method {
    Add-Missing 'size-installer' 'rs' 'Mo' $method 'installeur non produit par ce script'
  }
  if (-not $script:meta.riff) { return }
  Invoke-Measure 'size-install' 'riff' 'Mo' $method {
    if (-not (Test-Path -LiteralPath $RiffInstallDir)) { throw "dossier absent : $RiffInstallDir" }
    $bytes = Get-DirectoryBytes $RiffInstallDir
    Add-Result 'size-install' 'riff' 'Mo' @($bytes / 1MB) ($bytes / 1MB) $method
  }
  Invoke-Measure 'size-exe' 'riff' 'Mo' $method {
    $bytes = (Get-Item $RiffExe).Length
    Add-Result 'size-exe' 'riff' 'Mo' @($bytes / 1MB) ($bytes / 1MB) $method 'Riff.exe seul : le programme charge aussi ses DLL, .pak et resources.'
  }
  Invoke-Measure 'size-installer' 'riff' 'Mo' $method {
    if (-not (Test-Path -LiteralPath $RiffInstaller)) { throw "installeur absent : $RiffInstaller" }
    $bytes = (Get-Item -LiteralPath $RiffInstaller).Length
    Add-Result 'size-installer' 'riff' 'Mo' @($bytes / 1MB) ($bytes / 1MB) $method
  }
}

# Riff updater: with the first-run marker present Riff checks for updates and could download an installer.
# Only a copy outside the real install may be measured, and its app-update.yml is set aside.
function Initialize-RiffCopy {
  $real = Join-Path $env:LOCALAPPDATA 'Programs\Riff'
  if ($script:meta.riff.exe.StartsWith($real, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "refusing to run the real Riff install: $($script:meta.riff.exe)"
  }
  $yml = Join-Path (Split-Path -Parent $script:meta.riff.exe) 'resources\app-update.yml'
  if (Test-Path -LiteralPath $yml) {
    Rename-Item -LiteralPath $yml -NewName 'app-update.yml.off'
    Write-Host "updater disabled in the copy: $yml renamed"
  }
}

function Get-AppArgs([string]$App, $Iso) {
  if ($App -eq 'riff') { return "--user-data-dir=`"$($Iso.User)`"" }
  return ''
}

function Measure-RamIdle {
  $method = "Somme du Private Working Set de l'arbre de processus (measure.ps1), m$([char]0x00E9)diane de $IdleRuns lancements (attente $IdleWait s, 3 relev$([char]0x00E9)s), fen$([char]0xEA)tre 1280 x 800, accueil, profil vide isol$([char]0x00E9)."
  foreach ($app in $apps) {
    Invoke-Measure 'ram-idle' $app 'Mo' $method {
      $iso = New-Profile $app (Join-Path $Work "idle-$app")
      $csv = Join-Path $Work "idle-$app.csv"
      if (Test-Path $csv) { Remove-Item $csv }
      & (Join-Path $here 'measure.ps1') -Command $script:meta[$app].exe -Arguments (Get-AppArgs $app $iso) -Runs $IdleRuns -WaitSeconds $IdleWait -CsvPath $csv | Out-Host
      $rows = @(Import-Csv $csv)
      if ($rows.Count -eq 0) { throw 'measure.ps1 wrote no run' }
      $values = [double[]]@($rows | ForEach-Object { [double]::Parse($_.MedianMB, $culture) })
      $procs = ($rows | ForEach-Object { $_.Processes }) -join '/'
      Add-Result 'ram-idle' $app 'Mo' $values (Get-Median $values) $method "processus par lancement : $procs"
    }
  }
}

function Invoke-Node([string[]]$NodeArgs) {
  $text = & $Node @NodeArgs 2>&1 | Out-String
  if ($LASTEXITCODE -ne 0) { throw "node failed: $($text.Trim())" }
  return $text.Trim()
}

# Runs measure-scene.ps1 (own process: it compiles the SessionMute type) $SceneRuns times and keeps the valid runs.
# Cached per application and scene, so ram-play and cpu share the passes. The audio session stays muted.
$script:sceneCache = @{}
function Get-SceneRuns([string]$App, [string]$Scene) {
  $key = "$App/$Scene"
  if ($script:sceneCache.ContainsKey($key)) { return $script:sceneCache[$key] }
  $sceneWork = Join-Path $Work 'scenes'
  New-Item -ItemType Directory -Force $sceneWork | Out-Null
  $exe = $script:meta[$App].exe
  $runs = New-Object 'System.Collections.Generic.List[object]'
  $attempt = 0
  while ($runs.Count -lt $SceneRuns -and $attempt -lt ($SceneRuns + 2)) {
    $attempt++
    $logOut = Join-Path $sceneWork "$App-$Scene-$attempt.out.txt"
    $logErr = Join-Path $sceneWork "$App-$Scene-$attempt.err.txt"
    $argText = ('-NoProfile -ExecutionPolicy Bypass -File "{0}" -App {1} -Scene {2} -Exe "{3}" -Fixture "{4}" -Work "{5}" -Seconds {6} -Port {7}' -f
      (Join-Path $scenesDir 'measure-scene.ps1'), $App, $Scene, $exe, $Fixture, $sceneWork, $PlaySeconds, $Port)
    $launched = (Get-Date).AddSeconds(-1)
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argText -PassThru -NoNewWindow -RedirectStandardOutput $logOut -RedirectStandardError $logErr
    $null = $p.Handle   # caches the handle: without it ExitCode stays empty after WaitForExit(ms)
    try {
      if (-not $p.WaitForExit(($PlaySeconds + 240) * 1000)) { throw 'measure-scene.ps1 timed out' }
      if ($p.ExitCode -ne 0) { throw ("measure-scene.ps1 failed (exit code $($p.ExitCode)): " + ((Get-Content -Raw $logErr) -replace '\s+', ' ')) }
      $line = @(Get-Content (Join-Path $sceneWork 'results.csv') | Where-Object { $_.StartsWith("$App,$Scene,") }) | Select-Object -Last 1
      $f = $line.Split(',')
      $run = [pscustomobject]@{
        Median = [double]::Parse($f[2], $culture); Mean = [double]::Parse($f[3], $culture)
        Min = [double]::Parse($f[4], $culture); Max = [double]::Parse($f[5], $culture)
        PrivateMB = [double]::Parse($f[6], $culture); Processes = [int]$f[7]
      }
      $runDir = Join-Path $sceneWork "$App-$Scene"
      $count = @(Get-Content (Join-Path $runDir 'cpu-samples.txt')).Count
      if ($count -ne $PlaySeconds) { throw "$count CPU samples instead of $PlaySeconds" }
      if ($App -eq 'riff') {
        # Some Chromium-driven runs never start playing. A run counts only if audio plays after 8 s.
        $state = Get-Content -Raw (Join-Path $runDir 'driver.out') | ConvertFrom-Json
        if ($state.paused -or [double]$state.time -le 8) { throw "no playback: $($state | ConvertTo-Json -Compress)" }
      } else {
        $frames = Join-Path $runDir 'frames.csv'
        # At rest a scene draws no frame after warm-up (frames,0 is normal): the file only proves the scene ran to its end.
        if (-not (Test-Path $frames) -or -not (Select-String -Path $frames -Pattern '^views,' -Quiet)) { throw 'no scene log (the scene did not run to its end)' }
      }
      $runs.Add($run)
      Write-Host ("scene {0}/{1} pass {2}: mean {3:N1} %, median {4:N1} %, {5:N1} MB" -f $App, $Scene, $runs.Count, $run.Mean, $run.Median, $run.PrivateMB)
    } catch {
      Write-Warning "scene $App/$Scene attempt ${attempt} rejected: $($_.Exception.Message)"
    } finally {
      Stop-Tree $p.Id $launched
      Assert-NoLeftover "scene $App/$Scene"
    }
  }
  if ($runs.Count -eq 0) { throw "no valid run of scene $Scene in $attempt attempts" }
  $script:sceneCache[$key] = $runs.ToArray()
  return $script:sceneCache[$key]
}

function Measure-RamPlay {
  $method = "Private Working Set de l'arbre de processus, un relev$([char]0x00E9) apr$([char]0x00E8)s $PlaySeconds s de lecture de la sc$([char]0x00E8)ne a (measure-scene.ps1) : m$([char]0x00EA)me titre local de 5 min, son coup$([char]0x00E9), fen$([char]0x00EA)tre 1280 x 800, paroles ferm$([char]0x00E9)es. M$([char]0x00E9)diane des passes valides."
  foreach ($app in $apps) {
    Invoke-Measure 'ram-play' $app 'Mo' $method {
      $runs = Get-SceneRuns $app 'a'
      $values = [double[]]@($runs | ForEach-Object { $_.PrivateMB })
      Add-Result 'ram-play' $app 'Mo' $values (Get-Median $values) $method "processus : $(($runs | ForEach-Object { $_.Processes }) -join '/')"
    }
  }
}

function Measure-Cpu {
  $method = "Somme des temps CPU de l'arbre de processus, un relev$([char]0x00E9) par seconde pendant $PlaySeconds s (measure-scene.ps1), 100 % = un c$([char]0x0153)ur logique ; m$([char]0x00E9)diane des moyennes des passes valides. M$([char]0x00EA)me titre, son coup$([char]0x00E9), fen$([char]0x00EA)tre 1280 x 800. Le temps GPU n'est pas compt$([char]0x00E9)."
  foreach ($scene in $Scenes) {
    foreach ($app in $apps) {
      Invoke-Measure "cpu-$scene" $app '% d''un coeur' $method {
        $runs = Get-SceneRuns $app $scene
        $values = [double[]]@($runs | ForEach-Object { $_.Mean })
        Add-Result "cpu-$scene" $app '% d''un coeur' $values (Get-Median $values) $method ("m$([char]0x00E9)dianes des relev$([char]0x00E9)s par passe : " + (($runs | ForEach-Object { '{0:N1}' -f $_.Median }) -join ' / '))
      }
    }
  }
}

# Library for the scroll bench: user-data.json with $ScrollTracks liked tracks, each with its own 256 x 256 PNG
# (the original generator is not in the repository: this one is written for the bench, same sizes and counts).
function New-ScrollFixture([string]$Dir) {
  $marker = Join-Path $Dir "ready-$ScrollTracks.txt"
  if (Test-Path $marker) { return }
  if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir -Confirm:$false }
  $thumbs = Join-Path $Dir 'thumbs'
  New-Item -ItemType Directory -Force $thumbs | Out-Null
  Add-Type -AssemblyName System.Drawing
  $bitmap = New-Object System.Drawing.Bitmap 256, 256
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $items = New-Object 'System.Collections.Generic.List[string]'
  try {
    for ($i = 0; $i -lt $ScrollTracks; $i++) {
      # Distinct picture: color from the index, plus a bar whose position is the index.
      $color = [System.Drawing.Color]::FromArgb(40 + ($i * 7) % 200, 40 + ($i * 13) % 200, 40 + ($i * 29) % 200)
      $graphics.Clear($color)
      $graphics.FillRectangle([System.Drawing.Brushes]::White, ($i % 250), (($i / 250) % 250), 6, 6)
      $png = Join-Path $thumbs ('{0:D5}.png' -f $i)
      $bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
      $track = [ordered]@{
        id = ('bench{0:D5}' -f $i); title = ('Bench track {0:D5}' -f $i); artist = ('Bench artist {0}' -f ($i % 400))
        duration = 180 + ($i % 90); thumbnail = $png; url = ''; platform = 'ytmusic'; addedAt = 1700000000000 + $i
      }
      $items.Add((ConvertTo-Json $track -Compress))
    }
  } finally { $graphics.Dispose(); $bitmap.Dispose() }
  [System.IO.File]::WriteAllText((Join-Path $Dir 'user-data.json'), ('{"favorites":[' + ($items -join ',') + ']}'), $utf8)
  Set-Content -Path $marker -Value (Get-Date).ToString('o')
}

function Measure-Scroll {
  $method = "--bench-scroll : d$([char]0x00E9)filement automatique de Titres aim$([char]0x00E9)s ($ScrollTracks pistes, $ScrollTracks PNG distincts de 256 x 256, g$([char]0x00E9)n$([char]0x00E9)r$([char]0x00E9)s par le banc) $([char]0x00E0) 2 000 px/s pendant 30 s apr$([char]0x00E8)s 3 s de chauffe ; p95 de l'intervalle entre images, m$([char]0x00E9)diane de $ScrollRuns lancements. L'intervalle suit le rythme de l'$([char]0x00E9)cran ($($machine.refreshHz) Hz)."
  if ($script:meta.riff) {
    Invoke-Measure 'scroll' 'riff' 'ms' $method {
      Add-Missing 'scroll' 'riff' 'ms' $method 'pas de mesure equivalente dans Riff (aucun argument de defilement mesure, seul riff-rs expose --bench-scroll)'
    }
  }
  Invoke-Measure 'scroll' 'rs' 'ms' $method {
    $fixtureDir = Join-Path $Work 'scroll-fixture'
    New-ScrollFixture $fixtureDir
    $p95s = New-Object 'System.Collections.Generic.List[double]'
    $detail = New-Object 'System.Collections.Generic.List[string]'
    for ($run = 1; $run -le $ScrollRuns; $run++) {
      $root = Join-Path $Work 'scroll-rs'
      $iso = New-Profile 'rs' $root
      Copy-Item (Join-Path $fixtureDir 'user-data.json') (Join-Path $iso.Data 'user-data.json')
      $csv = Join-Path $root 'scroll.csv'
      $launched = (Get-Date).AddSeconds(-1)
      $p = Start-Process -FilePath $script:meta.rs.exe -ArgumentList '--bench-scroll', "`"$csv`"" -PassThru
      try {
        if (-not $p.WaitForExit(120000)) { throw "run $run : riff-app did not exit in 120 s" }
        if (-not (Test-Path $csv)) { throw "run $run : no result file" }
        $stats = @{}
        foreach ($line in Get-Content $csv) { $kv = $line.Split(','); $stats[$kv[0]] = [double]::Parse($kv[1], $culture) }
        if ($stats['frames'] -lt 500) { throw "run $run : only $($stats['frames']) frames measured (list not shown?)" }
        $p95s.Add($stats['p95_ms'])
        $detail.Add(('frames {0}, p50 {1}, p99 {2}, max {3}, > 16,67 ms : {4}, > 33,33 ms : {5}' -f $stats['frames'], $stats['p50_ms'], $stats['p99_ms'], $stats['max_ms'], $stats['missed_over_16_67ms'], $stats['severe_over_33_33ms']))
        Write-Host "scroll run ${run}: p95 $($stats['p95_ms']) ms ($($detail[-1]))"
      } finally {
        Stop-Tree $p.Id $launched
        Exit-Isolation
      }
    }
    Add-Result 'scroll' 'rs' 'ms' $p95s.ToArray() (Get-Median $p95s.ToArray()) $method ($detail -join ' | ')
  }
}

function Measure-Search {
  $method = "M$([char]0x00EA)me requ$([char]0x00EA)te ('$SearchQuery', YouTube Music, 12 r$([char]0x00E9)sultats), $SearchTrials essais $([char]0x00E0) 0,5 s d'intervalle, requ$([char]0x00EA)tes r$([char]0x00E9)seau r$([char]0x00E9)elles (le r$([char]0x00E9)seau varie). Riff : dur$([char]0x00E9)e du fetch /api/search lanc$([char]0x00E9) par la page (m$([char]0x00EA)me requ$([char]0x00EA)te que l'interface, sans le rendu), lue par le protocole de d$([char]0x00E9)bogage. M$([char]0x00E9)diane. S$([char]0x00E9)rie 'requ$([char]0x00EA)tes distinctes' : une requ$([char]0x00EA)te diff$([char]0x00E9)rente par essai (contourne le cache)."
  Invoke-Measure 'search' 'rs' 'ms' $method {
    foreach ($key in 'search', 'search-uncached') { Add-Missing $key 'rs' 'ms' $method 'aucun point d''entree sans modifier le code : pas d''argument de recherche, pas de serveur HTTP local, RIFF_NET_LOG ne journalise que les hotes (pas de duree) ; piloter l''interface ne donne aucune heure de fin observable' }
  }
  if (-not $script:meta.riff) { return }
  Invoke-Measure 'search' 'riff' 'ms' $method {
    $iso = New-Profile 'riff' (Join-Path $Work 'search-riff')
    $launched = (Get-Date).AddSeconds(-1)
    $p = Start-Process -FilePath $script:meta.riff.exe -ArgumentList "--remote-debugging-port=$Port", "--user-data-dir=`"$($iso.User)`"" -PassThru
    try {
      Start-Sleep -Seconds 8
      $others = $SearchOthers | Select-Object -First $SearchTrials
      $series = @(
        @{ Key = 'search'; Query = $SearchQuery },
        @{ Key = 'search-uncached'; Query = ($others -join '|') }
      )
      foreach ($s in $series) {
        $parsed = Invoke-Node @((Join-Path $here 'bench-riff.mjs'), 'search', $Port, $s.Query, $SearchTrials, 'ytmusic') | ConvertFrom-Json
        $trials = @($parsed | ForEach-Object { $_ })   # PowerShell 5.1 returns the JSON array as one object
        $bad = @($trials | Where-Object { -not $_.success -or $_.tracks -lt 1 })
        if ($bad.Count -gt 0) { Add-Missing $s.Key 'riff' 'ms' $method "$($bad.Count) of $($trials.Count) searches failed: $($bad[0] | ConvertTo-Json -Compress)"; continue }
        $values = [double[]]@($trials | ForEach-Object { [double]$_.ms })
        $note = "essai 1 : $('{0:N0}' -f $values[0]) ms"
        if ($values.Count -gt 1) { $note += ", m$([char]0x00E9)diane des essais 2 $([char]0x00E0) $($values.Count) : $('{0:N0}' -f (Get-Median ([double[]]@($values[1..($values.Count - 1)])))) ms" }
        if ($s.Key -eq 'search') { $note += ". Un cache du serveur de Riff rend les essais r$([char]0x00E9)p$([char]0x00E9)t$([char]0x00E9)s presque gratuits : voir la s$([char]0x00E9)rie 'requ$([char]0x00EA)tes distinctes'." }
        Add-Result $s.Key 'riff' 'ms' $values (Get-Median $values) $method $note
      }
    } finally {
      Stop-Tree $p.Id $launched
    }
  }
}

function Measure-Latency {
  $method = "--latency-log : d$([char]0x00E9)lai entre la demande du titre (activation de la premi$([char]0x00E8)re ligne des fichiers locaux, d$([char]0x00E9)clench$([char]0x00E9)e par l'application elle-m$([char]0x00EA)me en mode --scene) et la premi$([char]0x00E8)re position de lecture non nulle ; m$([char]0x00E9)diane de $LatencyTrials lancements, m$([char]0x00EA)me mp3 local que les sc$([char]0x00E8)nes, arr$([char]0x00EA)t apr$([char]0x00E8)s 4 s (intro silencieuse de 3 s : rien n'est audible)."
  if ($script:meta.riff) {
    Invoke-Measure 'latency' 'riff' 'ms' $method {
      Add-Missing 'latency' 'riff' 'ms' $method 'non scripte : la mesure par le protocole de debogage de Chromium est peu fiable (essais precedents : 4 sur 9 sans lecture, valeurs a 42 ms sans cause etablie), ordre de grandeur seulement'
    }
  }
  Invoke-Measure 'latency' 'rs' 'ms' $method {
    $mp3 = Join-Path $Fixture 'scene-bench.mp3'
    if (-not (Test-Path $mp3)) { throw "fixture absente : $mp3" }
    $values = New-Object 'System.Collections.Generic.List[double]'
    for ($run = 1; $run -le $LatencyTrials; $run++) {
      $root = Join-Path $Work 'latency-rs'
      [void](New-Profile 'rs' $root)
      $lat = Join-Path $root 'latency.csv'
      $launched = (Get-Date).AddSeconds(-1)
      $p = Start-Process -FilePath $script:meta.rs.exe -ArgumentList '--scene', 'a', '--scene-file', "`"$mp3`"", '--scene-log', "`"$(Join-Path $root 'frames.csv')`"", '--scene-seconds', '4', '--latency-log', "`"$lat`"" -PassThru
      try {
        if (-not $p.WaitForExit(60000)) { throw "run $run : riff-app did not exit in 60 s" }
        if (-not (Test-Path $lat)) { throw "run $run : no latency line (track never started?)" }
        $f = (Get-Content $lat | Select-Object -First 1).Split(',')
        $values.Add([double]::Parse($f[1], $culture))
        Write-Host "latency rs run ${run}: $($f[1]) ms"
      } finally {
        Stop-Tree $p.Id $launched
        Exit-Isolation
      }
    }
    Add-Result 'latency' 'rs' 'ms' $values.ToArray() (Get-Median $values.ToArray()) $method
  }
  foreach ($app in $apps) {
    Invoke-Measure 'latency-online' $app 'ms' $method {
      Add-Missing 'latency-online' $app 'ms' $method 'non mesuree : exige un titre en ligne (reseau) et, pour riff-rs, une activation de ligne par entree souris/clavier (--latency-log ne reagit qu a RowActivated, RowReleased et Next) ; aucune injection fiable sans voler le curseur ou le focus'
    }
  }
}

function Measure-Startup {
  $rsMethod = "Lancement avec --t0 (heure Unix en ms prise juste avant Start-Process) et --startup-log : l'application note le temps $([char]0x00E0) la deuxi$([char]0x00E8)me image pr$([char]0x00E9)sent$([char]0x00E9)e (borne haute du premier contenu), puis quitte. M$([char]0x00E9)diane de $StartupRuns lancements, profil vide, fen$([char]0x00EA)tre 1280 x 800."
  $riffMethod = "Riff n'a pas de deuxi$([char]0x00E8)me image mesurable : temps entre le lancement (m$([char]0x00EA)me horloge Unix) et le first-contentful-paint de index.html (timeOrigin + FCP lus par le protocole de d$([char]0x00E9)bogage, port ouvert), premier rendu de la coquille, pas des listes ni des pochettes. M$([char]0x00E9)diane de $StartupRuns lancements, profil vide, fen$([char]0x00EA)tre 1280 x 800."
  Invoke-Measure 'startup' 'rs' 'ms' $rsMethod {
    $values = New-Object 'System.Collections.Generic.List[double]'
    for ($run = 1; $run -le $StartupRuns; $run++) {
      $root = Join-Path $Work 'startup-rs'
      [void](New-Profile 'rs' $root)
      $log = Join-Path $root 'startup.log'
      $launched = (Get-Date).AddSeconds(-1)
      $t0 = Get-UnixMs
      $p = Start-Process -FilePath $script:meta.rs.exe -ArgumentList '--t0', $t0, '--startup-log', "`"$log`"" -PassThru
      try {
        if (-not $p.WaitForExit(60000)) { throw "run $run : riff-app did not exit in 60 s" }
        if (-not (Test-Path $log)) { throw "run $run : no startup log (exit code $($p.ExitCode))" }
        $ms = [double]::Parse((Get-Content $log | Select-Object -First 1), $culture)
        $values.Add($ms)
        Write-Host "startup rs run ${run}: $ms ms"
      } finally {
        Stop-Tree $p.Id $launched
        Exit-Isolation
      }
    }
    Add-Result 'startup' 'rs' 'ms' $values.ToArray() (Get-Median $values.ToArray()) $rsMethod
  }
  if (-not $script:meta.riff) { return }
  Invoke-Measure 'startup' 'riff' 'ms' $riffMethod {
    $values = New-Object 'System.Collections.Generic.List[double]'
    for ($run = 1; $run -le $StartupRuns; $run++) {
      $iso = New-Profile 'riff' (Join-Path $Work 'startup-riff')
      $launched = (Get-Date).AddSeconds(-1)
      $t0 = Get-UnixMs
      $p = Start-Process -FilePath $script:meta.riff.exe -ArgumentList "--remote-debugging-port=$Port", "--user-data-dir=`"$($iso.User)`"" -PassThru
      try {
        $json = Invoke-Node @((Join-Path $here 'bench-riff.mjs'), 'startup', $Port) | ConvertFrom-Json
        if ($json.url -ne 'index.html' -or $null -eq $json.fcp) { throw "run $run : unexpected page state $($json | ConvertTo-Json -Compress)" }
        $ms = [double]$json.origin + [double]$json.fcp - $t0
        $values.Add($ms)
        Write-Host ("startup riff run {0}: {1:N0} ms (fcp {2:N0} ms after navigation start)" -f $run, $ms, $json.fcp)
      } finally {
        Stop-Tree $p.Id $launched
        Exit-Isolation
      }
    }
    Add-Result 'startup' 'riff' 'ms' $values.ToArray() (Get-Median $values.ToArray()) $riffMethod
  }
}

# ---------------------------------------------------------------- main

New-Item -ItemType Directory -Force $Work, $Out | Out-Null
try {
  if ($script:meta.riff -and ($Only -ne 'size').Count -gt 0) {
    try { Initialize-RiffCopy } catch { Write-Warning $_.Exception.Message; $script:meta.riff = $null; $apps = @('rs') }
  }
  foreach ($name in $Only) {
    switch ($name) {
      'size' { Measure-Size }
      'ram-idle' { Measure-RamIdle }
      'startup' { Measure-Startup }
      'ram-play' { Measure-RamPlay }
      'cpu' { Measure-Cpu }
      'scroll' { Measure-Scroll }
      'search' { Measure-Search }
      'latency' { Measure-Latency }
      default { Write-Warning "measure '$name' not implemented yet" }
    }
  }
}
finally {
  Exit-Isolation
  Assert-NoLeftover 'bench'
  Write-Outputs
  Write-Host "results: $(Join-Path $Out 'bench-results.json')"
}
