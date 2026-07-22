param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
  throw 'RUNNER_TEMP is required for the Windows installer smoke test.'
}

$originalLocalAppData = [Environment]::GetEnvironmentVariable('LOCALAPPDATA')
if (![string]::IsNullOrWhiteSpace($originalLocalAppData)) {
  $existingBinary = Join-Path $originalLocalAppData 'OpenQuota\OpenQuota.exe'
  if (Test-Path -LiteralPath $existingBinary -PathType Leaf) {
    throw "Refusing to disturb an existing OpenQuota installation: $existingBinary"
  }
}
$uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
if (Test-Path -LiteralPath $uninstallRoot -PathType Container) {
  $existingRegistration = Get-ChildItem -LiteralPath $uninstallRoot | Where-Object {
    $_.GetValue('DisplayName') -eq 'OpenQuota' -or
    $_.GetValue('InstallLocation') -like '*\OpenQuota*' -or
    $_.GetValue('UninstallString') -like '*\OpenQuota\uninstall.exe*'
  } | Select-Object -First 1
  if ($null -ne $existingRegistration) {
    throw "Refusing to replace an existing OpenQuota uninstall registration: $($existingRegistration.Name)"
  }
}

if (!(Test-Path -LiteralPath $InstallerDirectory -PathType Container)) {
  throw "OpenQuota NSIS directory was not found: $InstallerDirectory"
}
$installers = @(Get-ChildItem -LiteralPath $InstallerDirectory -Filter '*-setup.exe' -File)
if ($installers.Count -ne 1) {
  throw "Expected exactly one OpenQuota NSIS installer, found $($installers.Count)."
}
$installer = $installers[0].FullName

$smokeRoot = Join-Path $env:RUNNER_TEMP "openquota-windows-$PID"
$installRoot = Join-Path $smokeRoot 'install'
$env:APPDATA = Join-Path $smokeRoot 'roaming'
$env:LOCALAPPDATA = Join-Path $smokeRoot 'local'
New-Item -ItemType Directory -Force -Path $installRoot, $env:APPDATA, $env:LOCALAPPDATA | Out-Null
$stdout = Join-Path $smokeRoot 'stdout.log'
$stderr = Join-Path $smokeRoot 'stderr.log'

$process = $null
$uninstaller = $null
$uninstallComplete = $false
try {
  $installProcess = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$installRoot") -PassThru -Wait
  if ($installProcess.ExitCode -ne 0) {
    throw "OpenQuota NSIS installer exited with code $($installProcess.ExitCode)."
  }

  $binaries = @(
    Get-ChildItem -LiteralPath $installRoot -Filter 'openquota.exe' -File -Recurse |
      Where-Object { $_.Name -notlike 'uninstall*' }
  )
  if ($binaries.Count -ne 1) {
    throw "Expected exactly one installed OpenQuota binary, found $($binaries.Count)."
  }
  $binary = $binaries[0].FullName
  $uninstallers = @(Get-ChildItem -LiteralPath $installRoot -Filter 'uninstall*.exe' -File -Recurse)
  if ($uninstallers.Count -ne 1) {
    throw "Expected exactly one OpenQuota uninstaller, found $($uninstallers.Count)."
  }
  $uninstaller = $uninstallers[0].FullName

  $process = Start-Process -FilePath $binary -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  Start-Sleep -Seconds 8
  if ($process.HasExited) {
    Get-Content $stdout, $stderr -ErrorAction SilentlyContinue
    throw 'OpenQuota exited during the Windows tray startup smoke test.'
  }

  $bytes = [System.IO.File]::ReadAllBytes($binary)
  $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
  $optionalHeader = $peOffset + 24
  $subsystem = [BitConverter]::ToUInt16($bytes, $optionalHeader + 68)
  if ($subsystem -ne 2) {
    throw "Expected Windows GUI subsystem (2), found $subsystem."
  }

  Stop-Process -Id $process.Id -Force
  $process.WaitForExit(10000)
  $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -Wait
  if ($uninstallProcess.ExitCode -ne 0) {
    throw "OpenQuota NSIS uninstaller exited with code $($uninstallProcess.ExitCode)."
  }
  for ($attempt = 0; $attempt -lt 30 -and (Test-Path -LiteralPath $binary); $attempt++) {
    Start-Sleep -Milliseconds 500
  }
  if (Test-Path -LiteralPath $binary) {
    throw 'OpenQuota remained installed after the NSIS uninstall smoke test.'
  }
  $uninstallComplete = $true
} finally {
  if ($null -ne $process -and !$process.HasExited) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $process.WaitForExit(10000)
  }
  if (!$uninstallComplete) {
    if ($null -eq $uninstaller) {
      $uninstaller = Get-ChildItem -LiteralPath $installRoot -Filter 'uninstall*.exe' -File -Recurse |
        Select-Object -First 1 -ExpandProperty FullName
    }
    if ($null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
      Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -ErrorAction SilentlyContinue
    }
  }
}
