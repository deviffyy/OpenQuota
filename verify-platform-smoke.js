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
];
const actionContracts = [
  'Smoke test Windows tray startup',
  'Smoke test macOS tray startup',
  'Smoke test Linux X11 fallback window',
  'Smoke test Linux Wayland startup',
  'scripts/windows.ps1',
  'scripts/macos.sh',
  'scripts/linux-x11.sh',
  'scripts/linux-wayland.sh',
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
  ['Windows', windows, ['Start-Sleep -Seconds 8', 'Expected Windows GUI subsystem (2)']],
  ['macOS', macos, ['hdiutil attach', 'OpenQuota.app/Contents/MacOS/openquota', 'sleep 8']],
  ['Linux X11', linuxX11, ['xvfb-run', 'openbox', 'xdotool search --name "^OpenQuota$"']],
  ['Linux Wayland', linuxWayland, ['weston --backend=headless-backend.so', 'sleep 8']],
]) {
  for (const contract of contracts) {
    if (!content.includes(contract)) {
      throw new Error(`${source} smoke harness is missing: ${contract}`);
    }
  }
}

console.log('CI and release builds share the Windows, macOS and Linux smoke-test contracts.');
