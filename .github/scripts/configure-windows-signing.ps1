param(
  [Parameter(Mandatory = $true)]
  [string]$ConfigPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -or
    [string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
  throw 'RUNNER_TEMP and GITHUB_ENV are required for Windows release signing.'
}

if ([string]::IsNullOrWhiteSpace($env:WINDOWS_CERTIFICATE) -or
    [string]::IsNullOrWhiteSpace($env:WINDOWS_CERTIFICATE_PASSWORD)) {
  throw 'Windows Authenticode configuration is incomplete.'
}

$encodedPath = Join-Path $env:RUNNER_TEMP 'openquota-certificate.txt'
$pfxPath = Join-Path $env:RUNNER_TEMP 'openquota-certificate.pfx'
$imported = @()
$configurationWritten = $false
try {
  Set-Content -LiteralPath $encodedPath -Value $env:WINDOWS_CERTIFICATE
  & certutil.exe -decode $encodedPath $pfxPath | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw 'The Windows signing certificate could not be decoded.'
  }
  $password = ConvertTo-SecureString -String $env:WINDOWS_CERTIFICATE_PASSWORD -AsPlainText -Force
  $imported = @(
    Import-PfxCertificate `
      -FilePath $pfxPath `
      -CertStoreLocation Cert:\CurrentUser\My `
      -Password $password
  )
  $importedThumbprints = @($imported | Select-Object -ExpandProperty Thumbprint -Unique)
  "OPENQUOTA_WINDOWS_IMPORTED_CERTIFICATES=$($importedThumbprints -join ',')" |
    Out-File -FilePath $env:GITHUB_ENV -Append
  $certificate = @(
    $imported | Where-Object {
      $_.HasPrivateKey -and
      $_.NotAfter -gt (Get-Date) -and
      ($_.EnhancedKeyUsageList.ObjectId.Value -contains '1.3.6.1.5.5.7.3.3')
    }
  )
  if ($certificate.Count -ne 1) {
    throw "Expected one usable code-signing certificate, found $($certificate.Count)."
  }

  $thumbprint = $certificate[0].Thumbprint
  @{
    bundle = @{
      windows = @{
        certificateThumbprint = $thumbprint
        digestAlgorithm = 'sha256'
        timestampUrl = 'http://timestamp.digicert.com'
      }
    }
  } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $ConfigPath
  "OPENQUOTA_WINDOWS_CERTIFICATE_THUMBPRINT=$thumbprint" | Out-File -FilePath $env:GITHUB_ENV -Append
  'OPENQUOTA_REQUIRE_AUTHENTICODE=true' | Out-File -FilePath $env:GITHUB_ENV -Append
  $configurationWritten = $true
} finally {
  Remove-Item -LiteralPath $encodedPath, $pfxPath -Force -ErrorAction SilentlyContinue
  if (!$configurationWritten) {
    foreach ($certificateToRemove in $imported) {
      $certificatePath = "Cert:\CurrentUser\My\$($certificateToRemove.Thumbprint)"
      Remove-Item -LiteralPath $certificatePath -Force -ErrorAction SilentlyContinue
    }
  }
}
