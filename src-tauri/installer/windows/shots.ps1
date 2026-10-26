# installs, reinstalls and removes the built installer in english and danish, one png per page
param([string]$Out = "shots")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[W]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$setup = (Resolve-Path src-tauri/target/release/bundle/nsis/*-setup.exe).Path

function Window($p) {
  for ($i = 0; $i -lt 60; $i++) {
    $p.Refresh()
    if ($p.MainWindowHandle -ne 0) { return $p.MainWindowHandle }
    Start-Sleep -Milliseconds 500
  }
  throw "no window"
}

function Shot($p, $name, $wait = 2) {
  Start-Sleep $wait
  $r = New-Object W+RECT
  [W]::GetWindowRect((Window $p), [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
  $bmp.Save("$Out/$name.png")
}

function Key($p, $key) {
  [W]::SetForegroundWindow((Window $p)) | Out-Null
  [System.Windows.Forms.SendKeys]::SendWait($key)
}

foreach ($lang in 1033, 1030) {
  # escape on the finish page closes without starting the app, which would fetch the models
  $p = Start-Process $setup "/LANG=$lang" -PassThru
  Shot $p "$lang-1-welcome"; Key $p "{ENTER}"
  Shot $p "$lang-2-progress" 1
  Shot $p "$lang-3-finish" 8; Key $p "{ESC}"
  $p.WaitForExit()

  $p = Start-Process $setup "/LANG=$lang" -PassThru
  Start-Sleep 2; Key $p "{ENTER}"
  Shot $p "$lang-4-reinstall"; Key $p "{ESC}"
  $p.WaitForExit()

  $dir = "$env:LOCALAPPDATA\Eckermann"
  $p = Start-Process "$dir\uninstall.exe" "/LANG=$lang", "_?=$dir" -PassThru
  Shot $p "$lang-5-remove"; Key $p "{ENTER}"
  Shot $p "$lang-6-removed" 6; Key $p "{ENTER}"
  $p.WaitForExit()
}
