import fs from 'node:fs';

const root = new URL('./', import.meta.url);
const read = (path) => fs.readFileSync(new URL(path, root), 'utf8');
const ci = read('.github/workflows/ci.yml');
const release = read('.github/workflows/release.yml');
const action = read('.github/actions/platform-smoke/action.yml');
const windows = read('.github/actions/platform-smoke/scripts/windows.ps1');
const macos = read('.github/actions/platform-smoke/scripts/macos.sh');
const linuxX11 = read('.github/actions/platform-smoke/scripts/linux-x11.sh');
const linuxWayland = read('.github/actions/platform-smoke/scripts/linux-wayland.sh');
const linuxPackages = read('.github/actions/platform-smoke/scripts/linux-packages.sh');
const releaseSigning = read('.github/scripts/validate-platform-signing.sh');
const windowsSigning = read('.github/scripts/configure-windows-signing.ps1');
const macosSigning = read('.github/scripts/configure-macos-signing.sh');
const releaseTagVerification = read('.github/scripts/verify-release-tag.sh');

const ciContracts = [
  'os: [windows-latest, macos-latest, ubuntu-22.04]',
  'Test Windows Credential Manager integration',
  'Test macOS Keychain integration',
  'Test Linux Secret Service integration',
  'Build Windows installer',
  'Build macOS DMG',
  'Build Linux packages',
  'uses: ./.github/actions/platform-smoke',
];
const releaseContracts = [
  'checks: read',
  'name: Windows x64',
  'name: Windows ARM64',
  'name: Linux x64',
  'name: Linux ARM64',
  'name: macOS Universal',
  'name: Build and smoke ${{ matrix.name }}',
  'Smoke test release artifact',
  'uses: ./.github/actions/platform-smoke',
  'needs: [validate, prepare-release, publish-artifacts]',
  'Verify release smoke checks',
  'Build and smoke Windows x64',
  'Build and smoke Windows ARM64',
  'Build and smoke Linux x64',
  'Build and smoke Linux ARM64',
  'Build and smoke macOS Universal',
  'Configure Windows Authenticode signing',
  'Configure Apple Developer ID signing and notarization',
  'Validate mandatory release-signing configuration',
  'Validate trusted release tag',
  'ref: ${{ github.sha }}',
  '+refs/tags/${RELEASE_TAG}:refs/tags/${RELEASE_TAG}',
  'git merge-base --is-ancestor "$tag_commit" refs/remotes/origin/main',
  'git checkout --detach "$tag_commit"',
  'release_commit: ${{ steps.trusted_tag.outputs.release_commit }}',
  'echo "release_commit=$tag_commit" >> "$GITHUB_OUTPUT"',
  'ref: ${{ needs.validate.outputs.release_commit }}',
  'verify-release-tag.sh',
  'validate-platform-signing.sh',
  'configure-windows-signing.ps1',
  'configure-macos-signing.sh',
  'artifact-root: ${{ matrix.smoke-artifact-root }}',
  "needs.validate.result == 'success' && (inputs.verify_only",
  "APPLE_ID: ${{ runner.os == 'macOS' && secrets.APPLE_ID || '' }}",
  "APPLE_PASSWORD: ${{ runner.os == 'macOS' && secrets.APPLE_PASSWORD || '' }}",
  "APPLE_TEAM_ID: ${{ runner.os == 'macOS' && secrets.APPLE_TEAM_ID || '' }}",
];
const actionContracts = [
  'Install, start, and uninstall the Windows NSIS package',
  'Smoke test macOS tray startup',
  'Exercise Linux AppImage and Debian packages',
  'artifact-root:',
  'scripts/windows.ps1',
  'scripts/macos.sh',
  'scripts/linux-packages.sh',
];

for (const [source, contracts] of [
  ['CI', ciContracts],
  ['release', releaseContracts],
  ['action', actionContracts],
]) {
  const content = source === 'CI' ? ci : source === 'release' ? release : action;
  for (const contract of contracts) {
    if (!content.includes(contract)) {
      throw new Error(`Platform ${source} smoke contract is missing: ${contract}`);
    }
  }
}

for (const [source, content, contracts] of [
  [
    'release signing',
    releaseSigning,
    [
      'TAURI_SIGNING_PRIVATE_KEY',
      'WINDOWS_CERTIFICATE',
      'WINDOWS_CERTIFICATE_PASSWORD',
      'APPLE_CERTIFICATE',
      'APPLE_CERTIFICATE_PASSWORD',
      'APPLE_ID',
      'APPLE_PASSWORD',
      'APPLE_TEAM_ID',
      'exit 1',
    ],
  ],
  [
    'Windows signing',
    windowsSigning,
    ['Import-PfxCertificate', 'certificateThumbprint', 'OPENQUOTA_REQUIRE_AUTHENTICODE=true'],
  ],
  [
    'macOS signing',
    macosSigning,
    ['Developer ID Application', 'APPLE_SIGNING_IDENTITY=', 'OPENQUOTA_REQUIRE_NOTARIZATION=true'],
  ],
  [
    'release tag verification',
    releaseTagVerification,
    ['git fetch --force --no-tags origin', 'Release tag moved after validation', 'exit 1'],
  ],
]) {
  for (const contract of contracts) {
    if (!content.includes(contract)) {
      throw new Error(`${source} configuration is missing: ${contract}`);
    }
  }
}

for (const [source, content, contracts] of [
  [
    'Windows',
    windows,
    [
      '*-setup.exe',
      'RUNNER_TEMP is required for the Windows installer smoke test',
      'Refusing to disturb an existing OpenQuota installation',
      '@(\'/S\', "/D=$installRoot")',
      'Start-Sleep -Seconds 8',
      'Expected Windows GUI subsystem (2)',
      'Get-AuthenticodeSignature',
      "-ArgumentList '/S'",
      'remained installed after the NSIS uninstall smoke test',
    ],
  ],
  [
    'macOS',
    macos,
    [
      'hdiutil attach',
      'OpenQuota.app/Contents/MacOS/openquota',
      'OPENQUOTA_REQUIRE_NOTARIZATION',
      'codesign --verify',
      'xcrun stapler validate',
      'sleep 8',
    ],
  ],
  [
    'Linux packages',
    linuxPackages,
    [
      'APPIMAGE_EXTRACT_AND_RUN=1',
      'dpkg-deb --field',
      'test "${package_name}" = \'open-quota\'',
      'sudo apt-get install --yes',
      'sudo apt-get remove --yes',
      'linux-x11.sh',
      'linux-wayland.sh',
    ],
  ],
  ['Linux X11', linuxX11, ['xvfb-run', 'openbox', 'xdotool search --name "^OpenQuota$"']],
  ['Linux Wayland', linuxWayland, ['weston --backend=headless-backend.so', 'sleep 8']],
]) {
  for (const contract of contracts) {
    if (!content.includes(contract)) {
      throw new Error(`${source} smoke harness is missing: ${contract}`);
    }
  }
}

for (const obsoleteContract of ['smoke-binary:', 'binary-path:', 'bundle-directory:']) {
  if (release.includes(obsoleteContract) || action.includes(obsoleteContract)) {
    throw new Error(
      `Packaged smoke configuration still uses raw-binary input: ${obsoleteContract}`,
    );
  }
}

for (const unsafeReleaseContract of ['REQUIRE_PLATFORM_SIGNING']) {
  if (release.includes(unsafeReleaseContract)) {
    throw new Error(
      `Release signing can be bypassed or exposed outside macOS: ${unsafeReleaseContract}`,
    );
  }
}
if (release.includes('ref: ${{ env.RELEASE_TAG }}')) {
  throw new Error('A downstream release checkout is not pinned to the validated commit SHA.');
}
const pinnedCheckoutCount =
  release.split('ref: ${{ needs.validate.outputs.release_commit }}').length - 1;
if (pinnedCheckoutCount !== 3) {
  throw new Error(`Expected 3 SHA-pinned downstream checkouts, found ${pinnedCheckoutCount}.`);
}

const tauriBuildStart = release.indexOf('      - name: Build, sign, and upload platform artifacts');
const tauriBuildEnd = release.indexOf('\n        with:', tauriBuildStart);
const tauriBuildEnvironment = release.slice(tauriBuildStart, tauriBuildEnd);
for (const unscopedSecret of [
  'APPLE_ID: ${{ secrets.APPLE_ID }}',
  'APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}',
  'APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}',
]) {
  if (tauriBuildEnvironment.includes(unscopedSecret)) {
    throw new Error(`Tauri matrix exposes an Apple secret outside macOS: ${unscopedSecret}`);
  }
}

const trustedTagGate = release.indexOf('      - name: Validate trusted release tag');
const releaseSecretsGate = release.indexOf(
  '      - name: Validate mandatory release-signing configuration',
);
if (trustedTagGate === -1 || releaseSecretsGate === -1 || trustedTagGate >= releaseSecretsGate) {
  throw new Error('Release signing secrets are exposed before the trusted-tag gate.');
}

console.log(
  'CI and release builds exercise packaged Windows, macOS and Linux artifacts with platform-signing gates.',
);
