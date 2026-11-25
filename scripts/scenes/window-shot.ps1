# Writes a PNG of the visible window of a process, even when it is covered. Windows PowerShell 5.1.
#   .\window-shot.ps1 -ProcessId 1234 -Path out.png [-Title Riff]
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][int]$ProcessId,
  [Parameter(Mandatory = $true)][string]$Path,
  [string]$Title = '',
  [int]$Index = 0
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class WindowShot {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr param);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr param);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int size);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public static List<IntPtr> Find(int processId) {
    var found = new List<IntPtr>();
    EnumWindows((hwnd, param) => {
      uint pid;
      GetWindowThreadProcessId(hwnd, out pid);
      if (pid == processId && IsWindowVisible(hwnd)) found.Add(hwnd);
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static string TextOf(IntPtr hwnd) {
    var text = new StringBuilder(256);
    GetWindowText(hwnd, text, text.Capacity);
    return text.ToString();
  }
}
'@
$windows = @([WindowShot]::Find($ProcessId))
if ($Title) { $windows = @($windows | Where-Object { [WindowShot]::TextOf($_) -eq $Title }) }
if ($windows.Count -le $Index) { throw "no visible window $Index for process $ProcessId" }
$hwnd = $windows[$Index]
$rect = New-Object WindowShot+RECT
[void][WindowShot]::GetWindowRect($hwnd, [ref]$rect)
$bitmap = New-Object System.Drawing.Bitmap ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$dc = $graphics.GetHdc()
# 2 = PW_RENDERFULLCONTENT: needed for a window drawn by the GPU.
if (-not [WindowShot]::PrintWindow($hwnd, $dc, 2)) { throw 'PrintWindow failed' }
$graphics.ReleaseHdc($dc)
$graphics.Dispose()
$bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
