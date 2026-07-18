param(
  [Parameter(Mandatory = $true)]
  [string]$BinaryPath
)

$candidates = @($BinaryPath, "$BinaryPath.exe")
$candidate = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (!$candidate) {
  throw "OpenQuota smoke binary was not found: $BinaryPath"
}
$binary = (Resolve-Path -LiteralPath $candidate).Path

$smokeRoot = Join-Path $env:RUNNER_TEMP "openquota-windows-$PID"
$env:APPDATA = Join-Path $smokeRoot 'roaming'
$env:LOCALAPPDATA = Join-Path $smokeRoot 'local'
New-Item -ItemType Directory -Force -Path $env:APPDATA, $env:LOCALAPPDATA | Out-Null
$stdout = Join-Path $smokeRoot 'stdout.log'
$stderr = Join-Path $smokeRoot 'stderr.log'

$process = Start-Process `
  -FilePath $binary `
  -PassThru `
  -WindowStyle Hidden `
  -RedirectStandardOutput $stdout `
  -RedirectStandardError $stderr
try {
  Start-Sleep -Seconds 8
  if ($process.HasExited) {
    Get-Content $stdout, $stderr -ErrorAction SilentlyContinue
    throw 'OpenQuota exited during the Windows tray startup smoke test.'
  }
} finally {
  if (!$process.HasExited) {
    Stop-Process -Id $process.Id -Force
  }
}

$bytes = [System.IO.File]::ReadAllBytes($binary)
$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
$optionalHeader = $peOffset + 24
$subsystem = [BitConverter]::ToUInt16($bytes, $optionalHeader + 68)
if ($subsystem -ne 2) {
  throw "Expected Windows GUI subsystem (2), found $subsystem."
}
