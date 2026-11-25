# Measures the idle RAM of a program: Private Working Set summed over its process tree.
# Method: NOTES.md, section "Etat fin Phase 6". Compatible with Windows PowerShell 5.1.
#
#   .\scripts\measure.ps1 -Command notepad.exe
#   .\scripts\measure.ps1 -Command .\app.exe -Arguments '--scene idle' -CsvPath out\ram.csv

[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Command,
  [string]$Arguments = '',
  [int]$Runs = 5,
  [int]$WaitSeconds = 30,
  [int]$Samples = 3,
  [int]$SampleIntervalSeconds = 2,
  [string]$CsvPath = ''
)

$ErrorActionPreference = 'Stop'

function Get-Median([double[]]$Values) {
  $sorted = @($Values | Sort-Object)
  $middle = [math]::Floor($sorted.Count / 2)
  if ($sorted.Count % 2 -eq 1) { return $sorted[$middle] }
  return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

# Start PID plus recursive children through ParentProcessId. The start PID may be gone:
# launcher stubs (Windows 11 notepad.exe) exit and leave an orphan child. A process created
# before the launch is ignored: its ParentProcessId points to a recycled PID.
function Get-ProcessTree([int]$RootId, [datetime]$LaunchedAt) {
  $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate |
      Where-Object { $_.CreationDate -ge $LaunchedAt })

  $seen = New-Object 'System.Collections.Generic.HashSet[int]'
  [void]$seen.Add($RootId)
  $queue = New-Object 'System.Collections.Generic.Queue[int]'
  $queue.Enqueue($RootId)
  while ($queue.Count -gt 0) {
    $parentId = $queue.Dequeue()
    foreach ($child in $all) {
      if ($child.ParentProcessId -eq $parentId -and $seen.Add([int]$child.ProcessId)) {
        $queue.Enqueue([int]$child.ProcessId)
      }
    }
  }
  return @($all | Where-Object { $seen.Contains([int]$_.ProcessId) } | ForEach-Object { [int]$_.ProcessId })
}

function Get-TreePrivateWorkingSet([int[]]$Ids) {
  $counters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Property IDProcess, WorkingSetPrivate)
  $sum = [double]0
  foreach ($counter in $counters) {
    if ($Ids -contains [int]$counter.IDProcess) { $sum += [double]$counter.WorkingSetPrivate }
  }
  return $sum
}

function Stop-Tree([int[]]$Ids) {
  foreach ($id in $Ids) {
    try { Stop-Process -Id $id -Force -ErrorAction Stop } catch { Write-Verbose "pid $id already gone" }
  }
}

$results = New-Object 'System.Collections.Generic.List[object]'

for ($run = 1; $run -le $Runs; $run++) {
  $startArgs = @{ FilePath = $Command; PassThru = $true }
  if ($Arguments) { $startArgs.ArgumentList = $Arguments }
  # WMI CreationDate is truncated to the microsecond; a small margin avoids excluding the root.
  $launchedAt = (Get-Date).AddSeconds(-1)
  $process = Start-Process @startArgs
  $rootId = $process.Id

  try {
    Start-Sleep -Seconds $WaitSeconds

    $values = New-Object 'System.Collections.Generic.List[double]'
    $count = 0
    for ($sample = 0; $sample -lt $Samples; $sample++) {
      if ($sample -gt 0) { Start-Sleep -Seconds $SampleIntervalSeconds }
      $ids = @(Get-ProcessTree $rootId $launchedAt)
      if ($ids.Count -eq 0) { throw "process tree of $rootId exited before the measurement" }
      $count = $ids.Count
      $values.Add((Get-TreePrivateWorkingSet $ids) / 1MB)
    }

    $median = Get-Median $values.ToArray()
    $results.Add([pscustomobject]@{
        Run       = $run
        Processes = $count
        Samples   = (($values | ForEach-Object { '{0:N1}' -f $_ }) -join ' / ')
        MedianMB  = [math]::Round($median, 1)
      })
    Write-Host ("run {0}/{1}: {2:N1} MB ({3} process(es)), samples {4}" -f $run, $Runs, $median, $count, $results[-1].Samples)
  }
  finally {
    Stop-Tree @(Get-ProcessTree $rootId $launchedAt)
    Start-Sleep -Seconds 2
  }
}

$medians = [double[]]@($results | ForEach-Object { $_.MedianMB })
$stats = $medians | Measure-Object -Minimum -Maximum
Write-Host ("min {0:N1} MB, median {1:N1} MB, max {2:N1} MB" -f $stats.Minimum, (Get-Median $medians), $stats.Maximum)

if ($CsvPath) {
  $results | Export-Csv -Path $CsvPath -NoTypeInformation -Encoding UTF8
}
