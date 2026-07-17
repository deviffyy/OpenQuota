import fs from 'node:fs';

const root = new URL('./', import.meta.url);
const workflow = fs.readFileSync(new URL('.github/workflows/ci.yml', root), 'utf8');

const workflowContracts = [
  'os: [windows-latest, macos-latest, ubuntu-22.04]',
  'Test Windows Credential Manager integration',
  'Test macOS Keychain integration',
  'Test Linux Secret Service integration',
  'Build Windows installer',
  'Build macOS DMG',
  'Build Linux packages',
  'Smoke test Windows tray startup',
  'Smoke test macOS tray startup',
  'Smoke test Linux X11 fallback window',
  'Smoke test Linux Wayland startup',
  'Verify Windows GUI subsystem',
];

for (const contract of workflowContracts) {
  if (!workflow.includes(contract)) {
    throw new Error(`Platform CI smoke contract is missing: ${contract}`);
  }
}

console.log('Windows, macOS and Linux smoke-test contracts are present.');
