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
  'Validate updater signing configuration',
  'Validate trusted release tag',
  'ref: ${{ github.sha }}',
  '+refs/tags/${RELEASE_TAG}:refs/tags/${RELEASE_TAG}',
  'git merge-base --is-ancestor "$tag_commit" refs/remotes/origin/main',
  'git checkout --detach "$tag_commit"',
  'release_commit: ${{ steps.trusted_tag.outputs.release_commit }}',
  'echo "release_commit=$tag_commit" >> "$GITHUB_OUTPUT"',
  'ref: ${{ needs.validate.outputs.release_commit }}',
  'verify-release-tag.sh',
  'artifact-root: ${{ matrix.smoke-artifact-root }}',
  "needs.validate.result == 'success' && (inputs.verify_only",
  "APPLE_SIGNING_IDENTITY: ${{ runner.os == 'macOS' && '-' || '' }}",
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
      "-ArgumentList '/S'",
      'remained installed after the NSIS uninstall smoke test',
    ],
  ],
  ['macOS', macos, ['hdiutil attach', 'OpenQuota.app/Contents/MacOS/openquota', 'sleep 8']],
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

for (const deferredPlatformSigningContract of [
  'WINDOWS_CERTIFICATE',
  'APPLE_CERTIFICATE',
  'APPLE_ID',
  'OPENQUOTA_REQUIRE_AUTHENTICODE',
  'OPENQUOTA_REQUIRE_NOTARIZATION',
  'tauri.windows-signing.conf.json',
]) {
  if (
    release.includes(deferredPlatformSigningContract) ||
    windows.includes(deferredPlatformSigningContract) ||
    macos.includes(deferredPlatformSigningContract)
  ) {
    throw new Error(
      `Deferred platform signing is still configured: ${deferredPlatformSigningContract}`,
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

const trustedTagGate = release.indexOf('      - name: Validate trusted release tag');
const updaterSigningGate = release.indexOf('      - name: Validate updater signing configuration');
if (trustedTagGate === -1 || updaterSigningGate === -1 || trustedTagGate >= updaterSigningGate) {
  throw new Error('Updater signing secret is exposed before the trusted-tag gate.');
}

console.log(
  'CI and release builds exercise packaged Windows, macOS and Linux artifacts with pinned release tags.',
);
