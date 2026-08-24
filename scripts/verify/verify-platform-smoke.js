import fs from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (path) => fs.readFileSync(new URL(path, root), 'utf8');
const ci = read('.github/workflows/ci.yml');
const release = read('.github/workflows/release.yml');
const action = read('.github/actions/platform-smoke/action.yml');
const windows = read('.github/actions/platform-smoke/scripts/windows.ps1');
const macos = read('.github/actions/platform-smoke/scripts/macos.sh');
const linuxX11 = read('.github/actions/platform-smoke/scripts/linux-x11.sh');
const linuxWayland = read('.github/actions/platform-smoke/scripts/linux-wayland.sh');
const linuxPackages = read('.github/actions/platform-smoke/scripts/linux-packages.sh');
const linuxdeploySetup = read('.github/scripts/setup-linuxdeploy.sh');
const windowsSigningSetup = read('.github/scripts/setup-windows-signing.ps1');
const windowsSigner = read('.github/scripts/sign-windows.ps1');
const windowsSignShim = read('.github/scripts/openquota-sign-windows.cmd');
const windowsSigningConfig = JSON.parse(read('src-tauri/tauri.windows-signing.conf.json'));
const releaseTagVerification = read('.github/scripts/verify-release-tag.sh');
const readme = read('README.md');
const releasing = read('docs/releasing.md');

const requireContracts = (source, content, contracts) => {
  for (const contract of contracts) {
    if (!content.includes(contract)) {
      throw new Error(`${source} configuration is missing: ${contract}`);
    }
  }
};

const requireExactKeyLines = (source, content, expectations) => {
  const lines = content.split('\n').map((line) => line.trim());
  for (const [key, expected] of Object.entries(expectations)) {
    const actual = lines.filter((line) => line.startsWith(`${key}:`)).sort();
    const reviewed = [...expected].sort();
    if (JSON.stringify(actual) !== JSON.stringify(reviewed)) {
      throw new Error(
        `${source} ${key} bindings differ from the reviewed optional-signing policy.`,
      );
    }
  }
};

requireContracts('CI', ci, [
  'os: [windows-latest, macos-latest, ubuntu-22.04]',
  'Test Windows Credential Manager integration',
  'Test macOS Keychain integration',
  'Test Linux Secret Service integration',
  'Prepare Linux AppImage bundler',
  'Build Windows installer',
  'Build macOS DMG',
  'Build Linux packages',
  'uses: ./.github/actions/platform-smoke',
  'dbus-tests',
  "APPLE_SIGNING_IDENTITY: '-'",
]);

requireContracts('release', release, [
  'checks: read',
  'name: Windows x64',
  'name: Windows ARM64',
  'name: Linux x64',
  'name: Linux ARM64',
  'name: macOS Universal',
  'name: Build and smoke ${{ matrix.name }}',
  'Smoke test release artifact',
  'name: Check out release smoke tooling',
  'ref: ${{ github.workflow_sha }}',
  'path: .release-workflow',
  'sparse-checkout: .github/actions/platform-smoke',
  'uses: ./.release-workflow/.github/actions/platform-smoke',
  "ENABLE_WINDOWS_NATIVE_SIGNING: ${{ vars.ENABLE_WINDOWS_NATIVE_SIGNING || 'false' }}",
  "ENABLE_MACOS_NATIVE_SIGNING: ${{ vars.ENABLE_MACOS_NATIVE_SIGNING || 'false' }}",
  'windows_signing: ${{ steps.signing_policy.outputs.windows_signing }}',
  'macos_signing: ${{ steps.signing_policy.outputs.macos_signing }}',
  'if: ${{ !inputs.verify_only }}',
  'if [[ "$WINDOWS_SIGNING_ENABLED" = true ]]',
  'if [[ "$MACOS_SIGNING_ENABLED" = true ]]',
  "if: runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true'",
  "if: runner.os == 'macOS' && needs.validate.outputs.macos_signing != 'true'",
  "if: runner.os == 'macOS' && needs.validate.outputs.macos_signing == 'true'",
  'echo \'APPLE_SIGNING_IDENTITY=-\' >> "$GITHUB_ENV"',
  'name: Configure native macOS signing',
  'write_env APPLE_CERTIFICATE "$OPENQUOTA_APPLE_CERTIFICATE"',
  'write_env APPLE_CERTIFICATE_PASSWORD "$OPENQUOTA_APPLE_CERTIFICATE_PASSWORD"',
  'write_env APPLE_ID "$OPENQUOTA_APPLE_ID"',
  'write_env APPLE_PASSWORD "$OPENQUOTA_APPLE_PASSWORD"',
  'write_env APPLE_TEAM_ID "$OPENQUOTA_APPLE_TEAM_ID"',
  "needs.validate.outputs.windows_signing == 'true' && matrix.windows-signing-args",
  "ES_USERNAME: ${{ steps.signing_policy.outputs.windows_signing == 'true' && secrets.ES_USERNAME || '' }}",
  "ES_PASSWORD: ${{ steps.signing_policy.outputs.windows_signing == 'true' && secrets.ES_PASSWORD || '' }}",
  "ES_CREDENTIAL_ID: ${{ steps.signing_policy.outputs.windows_signing == 'true' && secrets.ES_CREDENTIAL_ID || '' }}",
  "ES_TOTP_SECRET: ${{ steps.signing_policy.outputs.windows_signing == 'true' && secrets.ES_TOTP_SECRET || '' }}",
  "WINDOWS_SIGNER_SUBJECT: ${{ steps.signing_policy.outputs.windows_signing == 'true' && vars.WINDOWS_SIGNER_SUBJECT || '' }}",
  "APPLE_CERTIFICATE: ${{ steps.signing_policy.outputs.macos_signing == 'true' && secrets.APPLE_CERTIFICATE || '' }}",
  "APPLE_CERTIFICATE_PASSWORD: ${{ steps.signing_policy.outputs.macos_signing == 'true' && secrets.APPLE_CERTIFICATE_PASSWORD || '' }}",
  "APPLE_ID: ${{ steps.signing_policy.outputs.macos_signing == 'true' && secrets.APPLE_ID || '' }}",
  "APPLE_PASSWORD: ${{ steps.signing_policy.outputs.macos_signing == 'true' && secrets.APPLE_PASSWORD || '' }}",
  "APPLE_TEAM_ID: ${{ steps.signing_policy.outputs.macos_signing == 'true' && secrets.APPLE_TEAM_ID || '' }}",
  "ES_USERNAME: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && secrets.ES_USERNAME || '' }}",
  "ES_PASSWORD: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && secrets.ES_PASSWORD || '' }}",
  "ES_CREDENTIAL_ID: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && secrets.ES_CREDENTIAL_ID || '' }}",
  "ES_TOTP_SECRET: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && secrets.ES_TOTP_SECRET || '' }}",
  "OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && vars.WINDOWS_SIGNER_SUBJECT || '' }}",
  "release-validation: ${{ (runner.os == 'Linux' || (runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true') || (runner.os == 'macOS' && needs.validate.outputs.macos_signing == 'true')) && 'true' || 'false' }}",
  "windows-signer-subject: ${{ runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && vars.WINDOWS_SIGNER_SUBJECT || '' }}",
  "apple-team-id: ${{ runner.os == 'macOS' && needs.validate.outputs.macos_signing == 'true' && secrets.APPLE_TEAM_ID || '' }}",
  'windows-signer-subject:',
  'apple-team-id:',
  'needs: [validate, prepare-release, publish-artifacts]',
  'Verify release smoke checks',
  'VERIFY_ONLY: ${{ inputs.verify_only }}',
  'repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/jobs?filter=latest&per_page=100',
  'repos/$GITHUB_REPOSITORY/commits/$commit/check-runs?filter=all&per_page=100',
  'jobs_key=check_runs',
  'jobs_key=jobs',
  '.[$key] | any(.name == $name and .conclusion == "success")',
  'Build and smoke Windows x64',
  'Build and smoke Windows ARM64',
  'Build and smoke Linux x64',
  'Build and smoke Linux ARM64',
  'Build and smoke macOS Universal',
  'Prepare Linux AppImage bundler',
  'Prepare Windows code signing',
  'Resolve release signing policy',
  'Validate release signing credentials',
  'required=(TAURI_SIGNING_PRIVATE_KEY)',
  'TAURI_SIGNING_PRIVATE_KEY',
  'ES_USERNAME',
  'ES_PASSWORD',
  'ES_CREDENTIAL_ID',
  'ES_TOTP_SECRET',
  'WINDOWS_SIGNER_SUBJECT',
  'APPLE_CERTIFICATE',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_ID',
  'APPLE_PASSWORD',
  'APPLE_TEAM_ID',
  'OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT',
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
  'dbus-tests',
  '--config src-tauri/tauri.windows-signing.conf.json',
]);

requireContracts('release smoke tooling checkout', release, [
  `      - name: Check out release smoke tooling
        uses: actions/checkout@v7
        with:
          ref: \${{ github.workflow_sha }}
          path: .release-workflow
          sparse-checkout: .github/actions/platform-smoke

      - name: Smoke test release artifact
        uses: ./.release-workflow/.github/actions/platform-smoke`,
]);

const expression = (value) => `\${{ ${value} }}`;
const exactSigningBindings = {
  ENABLE_WINDOWS_NATIVE_SIGNING: [
    `ENABLE_WINDOWS_NATIVE_SIGNING: ${expression("vars.ENABLE_WINDOWS_NATIVE_SIGNING || 'false'")}`,
  ],
  ENABLE_MACOS_NATIVE_SIGNING: [
    `ENABLE_MACOS_NATIVE_SIGNING: ${expression("vars.ENABLE_MACOS_NATIVE_SIGNING || 'false'")}`,
  ],
  windows_signing: [
    `windows_signing: ${expression('steps.signing_policy.outputs.windows_signing')}`,
  ],
  macos_signing: [`macos_signing: ${expression('steps.signing_policy.outputs.macos_signing')}`],
  WINDOWS_SIGNING_ENABLED: [
    `WINDOWS_SIGNING_ENABLED: ${expression('steps.signing_policy.outputs.windows_signing')}`,
  ],
  MACOS_SIGNING_ENABLED: [
    `MACOS_SIGNING_ENABLED: ${expression('steps.signing_policy.outputs.macos_signing')}`,
  ],
  TAURI_SIGNING_PRIVATE_KEY: [
    `TAURI_SIGNING_PRIVATE_KEY: ${expression('secrets.TAURI_SIGNING_PRIVATE_KEY')}`,
    `TAURI_SIGNING_PRIVATE_KEY: ${expression('secrets.TAURI_SIGNING_PRIVATE_KEY')}`,
  ],
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: [
    `TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${expression('secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD')}`,
  ],
  WINDOWS_SIGNER_SUBJECT: [
    `WINDOWS_SIGNER_SUBJECT: ${expression("steps.signing_policy.outputs.windows_signing == 'true' && vars.WINDOWS_SIGNER_SUBJECT || ''")}`,
  ],
  OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT: [
    `OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT: ${expression("runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && vars.WINDOWS_SIGNER_SUBJECT || ''")}`,
  ],
};

for (const name of ['ES_USERNAME', 'ES_PASSWORD', 'ES_CREDENTIAL_ID', 'ES_TOTP_SECRET']) {
  exactSigningBindings[name] = [
    `${name}: ${expression(`steps.signing_policy.outputs.windows_signing == 'true' && secrets.${name} || ''`)}`,
    `${name}: ${expression(`secrets.${name}`)}`,
    `${name}: ${expression(`runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true' && secrets.${name} || ''`)}`,
  ];
}

for (const name of [
  'APPLE_CERTIFICATE',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_ID',
  'APPLE_PASSWORD',
  'APPLE_TEAM_ID',
]) {
  exactSigningBindings[name] = [
    `${name}: ${expression(`steps.signing_policy.outputs.macos_signing == 'true' && secrets.${name} || ''`)}`,
  ];

  exactSigningBindings[`OPENQUOTA_${name}`] = [
    `OPENQUOTA_${name}: ${expression(`secrets.${name}`)}`,
  ];
}

requireExactKeyLines('release', release, exactSigningBindings);

requireContracts('release documentation', releasing, [
  'ENABLE_WINDOWS_NATIVE_SIGNING',
  'ENABLE_MACOS_NATIVE_SIGNING',
  'TAURI_SIGNING_PRIVATE_KEY',
  'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  'unsigned Windows installer',
  'ad-hoc-signed, unnotarized',
  'does not weaken updater verification',
]);

requireContracts('download documentation', readme, [
  'Update payloads are cryptographically signed',
  'Authenticode-signed',
  'ad-hoc',
  'SmartScreen',
  'Gatekeeper',
]);

requireContracts('platform smoke action', action, [
  'Install, start, and uninstall the Windows NSIS package',
  'Smoke test macOS tray startup',
  'Exercise Linux AppImage and Debian packages',
  'artifact-root:',
  'release-validation:',
  "default: 'false'",
  'windows-signer-subject:',
  'apple-team-id:',
  'OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT',
  'OPENQUOTA_EXPECTED_APPLE_TEAM_ID',
  'scripts/windows.ps1',
  'scripts/macos.sh',
  'scripts/linux-packages.sh',
]);

requireContracts('release tag verification', releaseTagVerification, [
  'git fetch --force --no-tags origin',
  'Release tag moved after validation',
  'exit 1',
]);

requireContracts('Windows package smoke', windows, [
  '*-setup.exe',
  'RUNNER_TEMP is required for the Windows installer smoke test',
  'Refusing to disturb an existing OpenQuota installation',
  '@(\'/S\', "/D=$installRoot")',
  'OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT',
  'Get-AuthenticodeSignature',
  'TimeStamperCertificate',
  'verify /pa /all /tw',
  'SignerCertificate.Thumbprint',
  'system tray integration ready',
  'OpenQuota startup completed',
  'for ($attempt = 0; $attempt -lt 60; $attempt++)',
  'Expected Windows GUI subsystem (2)',
  "-ArgumentList '/S'",
  'remained installed after the NSIS uninstall smoke test',
]);

requireContracts('macOS package smoke', macos, [
  'hdiutil attach',
  'ditto "${source_app}" "${app}"',
  'hdiutil detach "${mount_dir}"',
  'open -n "${app}"',
  'com.apple.quarantine',
  'xattr -p com.apple.quarantine "${app}"',
  'xattr -d com.apple.quarantine "${app}"',
  'codesign --verify --deep --strict',
  'verify_app_signature "${source_app}"',
  'verify_app_signature "${app}"',
  'Authority=Developer ID Application:',
  'TeamIdentifier=${OPENQUOTA_EXPECTED_APPLE_TEAM_ID}',
  'flags=0x[0-9a-fA-F]+\\([^)]*runtime[^)]*\\)',
  'Timestamp=',
  'spctl --assess --type execute',
  'xcrun stapler validate "${candidate}"',
  'syspolicy_check distribution',
  'system tray integration ready',
  'OpenQuota startup completed',
]);

for (const bundle of ['source_app', 'app']) {
  const signature = macos.indexOf(`verify_app_signature "\${${bundle}}"`);
  const trust = macos.indexOf(`verify_release_trust "\${${bundle}}"`);
  const trustBranch = macos.lastIndexOf('if test "${release_validation}" = true; then', trust);
  if (signature === -1 || trust === -1 || !(signature < trustBranch && trustBranch < trust)) {
    throw new Error(`macOS ${bundle} signature integrity check is no longer unconditional.`);
  }
}

requireContracts('Linux packages', linuxPackages, [
  'APPIMAGE_EXTRACT_AND_RUN=1',
  '--appimage-extract',
  "-name 'libwayland-client.so*'",
  'dpkg-deb --field',
  'test "${package_name}" = \'open-quota\'',
  'sudo apt-get install --yes',
  'sudo apt-get remove --yes',
  'linux-x11.sh" "${appimage}" unavailable',
  'linux-x11.sh" "${installed_binary}" available',
  'linux-wayland.sh',
]);

requireContracts('Linux AppImage bundler', linuxdeploySetup, [
  "linuxdeploy_release='1-alpha-20251107-1'",
  'c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d',
  '620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff',
  'sha256sum --check --strict',
]);

requireContracts('Linux X11 package smoke', linuxX11, [
  'xvfb-run',
  'openbox',
  'dbus-test-tool echo --session --name=org.kde.StatusNotifierWatcher',
  'org.freedesktop.DBus.NameHasOwner',
  'desktop integration detected (tray=true)',
  'system tray integration ready',
  'OpenQuota startup completed',
  'kill "${watcher_pid}"',
  'system tray became unavailable; using standalone window',
  'xdotool search --onlyvisible --limit 1 --pid "${app_pid}" --name "^OpenQuota$"',
  'xdotool windowclose',
  'close_attempted=false',
  'close_requested=false',
  'exited before its standalone window was closed',
  'did not keep a visible standalone window available for closing',
  'did not exit when its standalone window was closed',
]);

requireContracts('Linux Wayland package smoke', linuxWayland, [
  'weston --backend=headless-backend.so',
  'desktop integration detected (tray=false)',
  'OpenQuota startup completed',
  'system tray integration ready',
]);

requireContracts('Windows signing setup', windowsSigningSetup, [
  'https://ssl.com/wp-content/uploads/2024/06/CodeSignTool-v1.3.0-windows.zip',
  'E22094505DECBE622AFE5B0C27ABC618ED2BA179BD94F3450490352399D5EF2A',
  'ES_USERNAME',
  'ES_PASSWORD',
  'ES_CREDENTIAL_ID',
  'ES_TOTP_SECRET',
  'Get-FileHash',
  'OPENQUOTA_CODESIGNTOOL_JAVA',
  'OPENQUOTA_CODESIGNTOOL_JAR',
  '$env:GITHUB_ENV',
  '$env:GITHUB_PATH',
]);

requireContracts('Windows signer', windowsSigner, [
  "'sign'",
  '-override=true',
  'OPENQUOTA_EXPECTED_WINDOWS_SIGNER_SUBJECT',
  'Get-AuthenticodeSignature',
  'SignerCertificate.Subject -ne',
  'TimeStamperCertificate',
]);

requireContracts('Windows signing shim', windowsSignShim, [
  'sign-windows.ps1',
  '-FilePath "%~1"',
  'exit /b %openquota_exit_code%',
]);

const signCommand = windowsSigningConfig.bundle?.windows?.signCommand;
if (
  signCommand?.cmd !== 'cmd.exe' ||
  JSON.stringify(signCommand.args) !==
    JSON.stringify(['/d', '/s', '/c', 'openquota-sign-windows.cmd', '%1'])
) {
  throw new Error('Windows Tauri signing command does not use the reviewed signing shim.');
}

for (const obsoleteContract of ['smoke-binary:', 'binary-path:', 'bundle-directory:']) {
  if (release.includes(obsoleteContract) || action.includes(obsoleteContract)) {
    throw new Error(
      `Packaged smoke configuration still uses raw-binary input: ${obsoleteContract}`,
    );
  }
}

for (const removedReleaseNoteContract of [
  'openquota-native-trust:start',
  '## Native package trust',
  'Verify native trust release notes',
]) {
  if (release.includes(removedReleaseNoteContract)) {
    throw new Error(
      `Release notes still expose native trust status: ${removedReleaseNoteContract}`,
    );
  }
}

if (release.includes("release-validation: 'true'")) {
  throw new Error('Release smoke still requires native trust checks unconditionally.');
}
for (const defaultTruePolicy of [
  "ENABLE_WINDOWS_NATIVE_SIGNING: ${{ vars.ENABLE_WINDOWS_NATIVE_SIGNING || 'true' }}",
  "ENABLE_MACOS_NATIVE_SIGNING: ${{ vars.ENABLE_MACOS_NATIVE_SIGNING || 'true' }}",
]) {
  if (release.includes(defaultTruePolicy)) {
    throw new Error(`Native signing policy defaults to enabled: ${defaultTruePolicy}`);
  }
}

const expectedReleaseValidation =
  "release-validation: ${{ (runner.os == 'Linux' || (runner.os == 'Windows' && needs.validate.outputs.windows_signing == 'true') || (runner.os == 'macOS' && needs.validate.outputs.macos_signing == 'true')) && 'true' || 'false' }}";
const releaseValidationLines = release
  .split('\n')
  .map((line) => line.trim())
  .filter((line) => line.startsWith('release-validation:'));
if (
  releaseValidationLines.length !== 1 ||
  releaseValidationLines[0] !== expectedReleaseValidation
) {
  throw new Error('Release smoke native trust routing is not the reviewed opt-in expression.');
}
if (release.includes('ref: ${{ env.RELEASE_TAG }}')) {
  throw new Error('A downstream release checkout is not pinned to the validated commit SHA.');
}
if (macos.includes('xcrun stapler validate "${dmg}"')) {
  throw new Error('The smoke test incorrectly expects Tauri to staple the outer DMG.');
}

const windowsSigningConfigLines = release
  .split('\n')
  .filter((line) => line.includes('--config src-tauri/tauri.windows-signing.conf.json'));
if (
  windowsSigningConfigLines.length !== 2 ||
  windowsSigningConfigLines.some((line) => !line.includes('windows-signing-args:'))
) {
  throw new Error(
    'Windows native signing configuration must be declared only as the 2 opt-in matrix arguments.',
  );
}

const unconditionalSignedBuild = release
  .split('\n')
  .find(
    (line) =>
      line.trimStart().startsWith('args: ') &&
      line.includes('--config src-tauri/tauri.windows-signing.conf.json'),
  );
if (unconditionalSignedBuild) {
  throw new Error('A default Windows matrix build still requires native signing.');
}

const pinnedCheckoutCount =
  release.split('ref: ${{ needs.validate.outputs.release_commit }}').length - 1;
if (pinnedCheckoutCount !== 3) {
  throw new Error(`Expected 3 SHA-pinned downstream checkouts, found ${pinnedCheckoutCount}.`);
}

const trustedTagGate = release.indexOf('      - name: Validate trusted release tag');
const signingGate = release.indexOf('      - name: Resolve release signing policy');
const firstSigningSecret = release.indexOf('          ES_USERNAME:');
if (
  trustedTagGate === -1 ||
  signingGate === -1 ||
  firstSigningSecret === -1 ||
  trustedTagGate >= signingGate ||
  trustedTagGate >= firstSigningSecret
) {
  throw new Error('Release signing secrets are exposed before the trusted-tag gate.');
}

console.log(
  'CI and release builds exercise installed packages; optional native trust and mandatory updater signatures remain fail-closed.',
);
