# CPU of one scene: sum of the process tree, one sample per second, the audio session stays muted.
# 100 % = one logical core. Windows PowerShell 5.1. Same fixture and window size for both programs.
#   .\measure-scene.ps1 -App riff -Scene b -Exe <copy of Riff.exe> -Fixture <dir> -Work <dir>
#   .\measure-scene.ps1 -App rs -Scene b -Exe target\release\riff-app.exe -Fixture <dir> -Work <dir>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateSet('riff', 'rs')][string]$App,
  [Parameter(Mandatory = $true)][string]$Scene,
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$Fixture,
  [Parameter(Mandatory = $true)][string]$Work,
  [int]$Seconds = 60,
  [int]$Port = 9333,
  # riff-rs: pause after this many seconds, to count the frames at rest.
  [int]$PauseAfter = 0,
  # riff-rs: frames at the screen rate instead of 60 Hz, to measure the capacity.
  [switch]$Unthrottled,
  # Lyrics file of the fixture folder.
  [string]$Lyrics = 'scene-bench.lrc',
  # Minimizes the window before sampling, without taking the focus.
  [switch]$Minimize,
  # riff-rs: contenu de native-settings.json écrit avant le lancement (par exemple '{"uiScale":130}').
  [string]$SettingsJson = ''
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class SessionMute {
  [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class Enumerator {}
  [Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceEnumerator {
    [PreserveSig] int EnumAudioEndpoints(int flow, int mask, out IntPtr devices);
    [PreserveSig] int GetDefaultAudioEndpoint(int flow, int role, out IMMDevice device);
  }
  [Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDevice {
    [PreserveSig] int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object o);
  }
  [Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionManager2 {
    [PreserveSig] int GetAudioSessionControl(IntPtr g, int f, out IntPtr c);
    [PreserveSig] int GetSimpleAudioVolume(IntPtr g, int f, out IntPtr v);
    [PreserveSig] int GetSessionEnumerator(out IAudioSessionEnumerator e);
  }
  [Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionEnumerator {
    [PreserveSig] int GetCount(out int n);
    [PreserveSig] int GetSession(int i, [MarshalAs(UnmanagedType.IUnknown)] out object s);
  }
  [Guid("BFB7FF88-7239-4FC9-8FA2-07C950BE9C6D"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionControl2 {
    [PreserveSig] int GetState(out int s);
    [PreserveSig] int GetDisplayName(out IntPtr s);
    [PreserveSig] int SetDisplayName(IntPtr s, IntPtr g);
    [PreserveSig] int GetIconPath(out IntPtr s);
    [PreserveSig] int SetIconPath(IntPtr s, IntPtr g);
    [PreserveSig] int GetGroupingParam(out Guid g);
    [PreserveSig] int SetGroupingParam(IntPtr g, IntPtr c);
    [PreserveSig] int Register(IntPtr n);
    [PreserveSig] int Unregister(IntPtr n);
    [PreserveSig] int GetSessionIdentifier(out IntPtr s);
    [PreserveSig] int GetSessionInstanceIdentifier(out IntPtr s);
    [PreserveSig] int GetProcessId(out uint pid);
  }
  [Guid("87CE5498-68D6-44E5-9215-6DA47EF883D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface ISimpleAudioVolume {
    [PreserveSig] int SetMasterVolume(float v, IntPtr c);
    [PreserveSig] int GetMasterVolume(out float v);
    [PreserveSig] int SetMute(int m, IntPtr c);
    [PreserveSig] int GetMute(out int m);
  }
  // Mutes the sessions of these processes on the default devices. Returns (sessions found, sessions muted).
  public static int[] Mute(int[] pids, bool mute) {
    var wanted = new HashSet<int>(pids);
    int found = 0, muted = 0;
    var enumerator = (IMMDeviceEnumerator)new Enumerator();
    var iid = typeof(IAudioSessionManager2).GUID;
    foreach (var role in new[] { 0, 1 }) {
      IMMDevice device;
      if (enumerator.GetDefaultAudioEndpoint(0, role, out device) != 0) continue;
      object managerObject;
      if (device.Activate(ref iid, 23, IntPtr.Zero, out managerObject) != 0) continue;
      IAudioSessionEnumerator sessions;
      if (((IAudioSessionManager2)managerObject).GetSessionEnumerator(out sessions) != 0) continue;
      int count;
      sessions.GetCount(out count);
      for (var i = 0; i < count; i++) {
        object session;
        if (sessions.GetSession(i, out session) != 0) continue;
        uint pid;
        if (((IAudioSessionControl2)session).GetProcessId(out pid) != 0 || !wanted.Contains((int)pid)) continue;
        found++;
        var volume = (ISimpleAudioVolume)session;
        int state;
        volume.SetMute(mute ? 1 : 0, IntPtr.Zero);
        if (volume.GetMute(out state) == 0 && (state != 0) == mute) muted++;
      }
    }
    return new[] { found, muted };
  }
}
'@

function Get-Tree([int]$RootId, [datetime]$LaunchedAt) {
  $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate | Where-Object { $_.CreationDate -ge $LaunchedAt })
  $seen = New-Object 'System.Collections.Generic.HashSet[int]'
  [void]$seen.Add($RootId)
  $grew = $true
  while ($grew) {
    $grew = $false
    foreach ($p in $all) { if ($seen.Contains([int]$p.ParentProcessId) -and $seen.Add([int]$p.ProcessId)) { $grew = $true } }
  }
  return [int[]]@($seen)
}
function Get-Cpu([int[]]$Ids) {
  $sum = [double]0
  foreach ($id in $Ids) {
    try { $sum += [System.Diagnostics.Process]::GetProcessById($id).TotalProcessorTime.TotalSeconds } catch { Write-Verbose "pid $id gone" }
  }
  return $sum
}
function Get-PrivateMb([int[]]$Ids) {
  $sum = [double]0
  foreach ($c in @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Property IDProcess, WorkingSetPrivate)) {
    if ($Ids -contains [int]$c.IDProcess) { $sum += [double]$c.WorkingSetPrivate }
  }
  return $sum / 1MB
}
function Get-Median([double[]]$Values) {
  $s = @($Values | Sort-Object)
  $m = [math]::Floor($s.Count / 2)
  if ($s.Count % 2 -eq 1) { return $s[$m] }
  return ($s[$m - 1] + $s[$m]) / 2
}

$run = Join-Path $Work "$App-$Scene"
if (Test-Path $run) { Remove-Item -Recurse -Force $run -Confirm:$false }
$local = Join-Path $run 'local'
$roaming = Join-Path $run 'roaming'
$dataDir = Join-Path $local $(if ($App -eq 'rs') { 'Riff (natif)' } else { 'Riff' })
New-Item -ItemType Directory -Force (Join-Path $dataDir 'lyrics'), $roaming | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
$mp3 = Join-Path $Fixture 'scene-bench.mp3'
$sha1 = [System.Security.Cryptography.SHA1]::Create()
$hash = $sha1.ComputeHash($utf8.GetBytes("riff bench$([char]0)scene bench"))
$key = (-join ($hash | ForEach-Object { $_.ToString('x2') })).Substring(0, 16)
# Lyrics cache on disk, not custom lyrics: Riff shifts custom lines by one and no word ever becomes active.
Copy-Item (Join-Path $Fixture $Lyrics) (Join-Path $dataDir "lyrics\$key.lrc")

$bounds = '{"bounds":{"x":40,"y":40,"width":1280,"height":800},"isMaximized":false,"isFullScreen":false}'
$scenes = @{
  a  = @{ riff_m3_cover = '0'; riff_bar_wave = '0' }
  b  = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_lyrics_word_style = 'words' }
  b2 = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_lyrics_word_style = 'gradient' }
  c  = @{ riff_m3_cover = '1'; riff_bar_wave = '1'; riff_bar_viz_pos = 'behind' }
  d  = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_adv_cover_bg = 'true' }
  e  = @{ riff_m3_cover = '1'; riff_bar_wave = '1'; riff_bar_viz_pos = 'behind'; riff_adv_cover_bg = 'true'; riff_lyrics_word_style = 'words' }
  mesh = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_adv_mesh_gradient = 'true'; riff_mesh_cfg = '{"p1":{"x":20,"y":0},"p2":{"x":85,"y":5},"speed":20,"blur":0}' }
  banner = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_adv_track_banners = 'true' }
  bg = @{ riff_m3_cover = '0'; riff_bar_wave = '0'; riff_bg_url = ('file:///' + ((Join-Path $Fixture 'cover.jpg') -replace '\\', '/')); riff_bg_opacity = '60'; riff_bg_blur = '0' }
  liquid = @{ riff_m3_cover = '0'; riff_bar_wave = '1'; riff_bar_viz_pos = 'behind'; riff_viz_connected = '1'; riff_bar_viz_opacity = '60' }
}
$settings = $scenes[$Scene]
if ($App -eq 'riff' -and -not $settings) { throw "unknown scene $Scene" }

$oldLocal = $env:LOCALAPPDATA
$oldRoaming = $env:APPDATA
$env:LOCALAPPDATA = $local
$env:APPDATA = $roaming
$launchedAt = (Get-Date).AddSeconds(-1)
$driver = $null
$process = $null
try {
  if ($App -eq 'riff') {
    $userData = Join-Path $run 'userdata'
    New-Item -ItemType Directory -Force $userData | Out-Null
    $storage = @{}
    foreach ($k in $settings.Keys) { $storage[$k] = $settings[$k] }
    $track = @{ id = 'local_scenebench01'; path = $mp3; title = 'Scene Bench'; artist = 'Unknown Artist'; platform = 'local'; source = 'local' }
    $tracks = '[' + (ConvertTo-Json -Compress $track) + ']'
    $storage['riff_local_files'] = $tracks
    $storage['riff_local_tracks'] = $tracks
    [System.IO.File]::WriteAllText((Join-Path $userData 'storage.json'), (ConvertTo-Json -Compress $storage), $utf8)
    [System.IO.File]::WriteAllText((Join-Path $userData 'window-state.json'), $bounds, $utf8)
    $process = Start-Process -FilePath $Exe -ArgumentList "--remote-debugging-port=$Port", "--user-data-dir=`"$userData`"" -PassThru
    $driver = Start-Process -FilePath node -ArgumentList "`"$(Join-Path $here 'riff-driver.mjs')`"", 'start', $Port, $Scene -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $run 'driver.out') -RedirectStandardError (Join-Path $run 'driver.err')
  } else {
    [System.IO.File]::WriteAllText((Join-Path $dataDir 'window-state.json'), $bounds, $utf8)
    if ($SettingsJson) { [System.IO.File]::WriteAllText((Join-Path $dataDir 'native-settings.json'), $SettingsJson, $utf8) }
    $rsArgs = @('--scene', $Scene, '--scene-file', "`"$mp3`"", '--scene-log', "`"$(Join-Path $run 'frames.csv')`"", '--scene-seconds', ($Seconds + 25))
    if ($PauseAfter -gt 0) { $rsArgs += @('--scene-pause', $PauseAfter) }
    if ($Unthrottled) { $rsArgs += '--scene-unthrottled' }
    $process = Start-Process -FilePath $Exe -ArgumentList $rsArgs -PassThru -RedirectStandardError (Join-Path $run 'stderr.txt') -RedirectStandardOutput (Join-Path $run 'stdout.txt')
  }

  # Muted before the end of the silent intro, or the program is stopped.
  $deadline = (Get-Date).AddSeconds(120)
  $muted = $false
  while ((Get-Date) -lt $deadline) {
    $ids = Get-Tree $process.Id $launchedAt
    $state = [SessionMute]::Mute($ids, $true)
    if ($state[0] -gt 0 -and $state[0] -eq $state[1]) { $muted = $true; break }
    if ($driver -and $driver.HasExited -and -not (Get-Content -Raw (Join-Path $run 'driver.out'))) { break }
    Start-Sleep -Milliseconds 80
  }
  if (-not $muted) { throw ('audio session not muted: ' + $(if ($driver) { Get-Content -Raw (Join-Path $run 'driver.err') })) }
  Write-Host 'session muted'

  if ($driver) {
    $driver.WaitForExit()
    $start = Get-Content -Raw (Join-Path $run 'driver.out')
    Write-Host "state: $start"
    if (-not $start) { throw ('driver failed: ' + (Get-Content -Raw (Join-Path $run 'driver.err'))) }
  } else {
    Start-Sleep -Seconds 8
  }

  if ($Minimize) {
    Add-Type -Namespace SceneWindow -Name Native -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);'
    # 7 = SW_SHOWMINNOACTIVE
    [void][SceneWindow.Native]::ShowWindow((Get-Process -Id $process.Id).MainWindowHandle, 7)
    Start-Sleep -Seconds 2
  }

  $samples = New-Object 'System.Collections.Generic.List[double]'
  $clock = [System.Diagnostics.Stopwatch]::StartNew()
  $ids = Get-Tree $process.Id $launchedAt
  $previous = Get-Cpu $ids
  $previousAt = $clock.Elapsed.TotalSeconds
  for ($i = 1; $i -le $Seconds; $i++) {
    while ($clock.Elapsed.TotalSeconds -lt $i) { Start-Sleep -Milliseconds 20 }
    $now = Get-Cpu $ids
    $at = $clock.Elapsed.TotalSeconds
    $samples.Add(100 * ($now - $previous) / ($at - $previousAt))
    $previous = $now
    $previousAt = $at
    if ($i % 5 -eq 0) { [void][SessionMute]::Mute($ids, $true) }
  }
  $ram = Get-PrivateMb $ids
  $values = $samples.ToArray()
  $avg = ($values | Measure-Object -Average).Average
  $sorted = @($values | Sort-Object)
  $culture = [System.Globalization.CultureInfo]::InvariantCulture
  $line = [string]::Format($culture, '{0},{1},{2:F1},{3:F1},{4:F1},{5:F1},{6:F1},{7}', $App, $Scene, (Get-Median $values), $avg, $sorted[0], $sorted[-1], $ram, $ids.Count)
  Write-Host 'app,scene,median%,mean%,min%,max%,privateMB,processes'
  Write-Host $line
  Add-Content -Path (Join-Path $Work 'results.csv') -Value $line
  $samples | Set-Content -Path (Join-Path $run 'cpu-samples.txt')
  if ($App -eq 'riff') {
    & node (Join-Path $here 'riff-driver.mjs') check $Port (Join-Path $run 'scene.png')
  } else {
    if (-not $Minimize) {
      & (Join-Path $here 'window-shot.ps1') -ProcessId $process.Id -Path (Join-Path $run 'scene.png')
      if ($Scene -eq 'mini') { & (Join-Path $here 'window-shot.ps1') -ProcessId $process.Id -Path (Join-Path $run 'scene-2.png') -Index 1 }
    }
    # It writes its frame statistics when it exits.
    [void]$process.WaitForExit(60000)
  }
}
finally {
  $env:LOCALAPPDATA = $oldLocal
  $env:APPDATA = $oldRoaming
  if ($process) {
    foreach ($id in (Get-Tree $process.Id $launchedAt)) { try { Stop-Process -Id $id -Force -ErrorAction Stop } catch { Write-Verbose "pid $id gone" } }
  }
  if ($driver -and -not $driver.HasExited) { $driver.Kill() }
}
