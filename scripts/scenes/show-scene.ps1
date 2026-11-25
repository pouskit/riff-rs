# Ouvre une scène de riff-app pour la regarder (isolée de tes données, se ferme seule).
#   .\scripts\scenes\show-scene.ps1 -Scene e
#   .\scripts\scenes\show-scene.ps1 -Scene b -Lyrics scene-arabic.lrc
# Prérequis : node scripts/scenes/fixture.mjs <dossier> (par défaut ..\fixture-scenes) et cargo build --release.
# Sans --scene-seconds l'application se ferme aussitôt : ce script le fixe.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Scene,
  [int]$Seconds = 90,
  [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\riff-app.exe'),
  [string]$Fixture = (Join-Path $PSScriptRoot '..\..\..\fixture-scenes'),
  # Lyrics file of the fixture folder.
  [string]$Lyrics = 'scene-bench.lrc'
)
$ErrorActionPreference = 'Stop'
$run = Join-Path $Fixture "..\scene-run\$Scene"
if (Test-Path $run) { Remove-Item -Recurse -Force $run -Confirm:$false }
$local = Join-Path $run 'local'
$dataDir = Join-Path $local 'Riff (natif)'
New-Item -ItemType Directory -Force (Join-Path $dataDir 'lyrics'), (Join-Path $run 'roaming') | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
# Même clé de cache de paroles que measure-scene.ps1.
$hash = [System.Security.Cryptography.SHA1]::Create().ComputeHash($utf8.GetBytes("riff bench$([char]0)scene bench"))
$key = (-join ($hash | ForEach-Object { $_.ToString('x2') })).Substring(0, 16)
Copy-Item (Join-Path $Fixture $Lyrics) (Join-Path $dataDir "lyrics\$key.lrc")
[System.IO.File]::WriteAllText((Join-Path $dataDir 'window-state.json'), '{"bounds":{"x":40,"y":40,"width":1280,"height":800},"isMaximized":false,"isFullScreen":false}', $utf8)
# Variables d'environnement restaurées en fin de script, même en cas d'erreur.
$oldLocal = $env:LOCALAPPDATA
$oldRoaming = $env:APPDATA
try {
  $env:LOCALAPPDATA = $local
  $env:APPDATA = Join-Path $run 'roaming'
  $mp3 = (Resolve-Path (Join-Path $Fixture 'scene-bench.mp3')).Path
  $log = Join-Path $run 'frames.csv'
  $p = Start-Process -FilePath (Resolve-Path $Exe).Path -ArgumentList '--scene', $Scene, '--scene-file', "`"$mp3`"", '--scene-log', "`"$log`"", '--scene-seconds', $Seconds -PassThru -RedirectStandardError (Join-Path $run 'stderr.txt') -RedirectStandardOutput (Join-Path $run 'stdout.txt')
  Write-Host "scène $Scene lancée (PID $($p.Id)), fermeture automatique dans $Seconds s. Journal d'erreurs : $(Join-Path $run 'stderr.txt')"
} finally {
  $env:LOCALAPPDATA = $oldLocal
  $env:APPDATA = $oldRoaming
}
