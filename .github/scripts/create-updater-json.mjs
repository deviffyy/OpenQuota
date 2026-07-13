import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const signedArtifacts = [
  {
    label: 'Windows x64 NSIS installer',
    pattern: /_x64-setup\.exe$/i,
    platforms: ['windows-x86_64', 'windows-x86_64-nsis'],
  },
  {
    label: 'Windows ARM64 NSIS installer',
    pattern: /_arm64-setup\.exe$/i,
    platforms: ['windows-aarch64', 'windows-aarch64-nsis'],
  },
  {
    label: 'Linux x64 AppImage',
    pattern: /_amd64\.AppImage$/i,
    platforms: ['linux-x86_64', 'linux-x86_64-appimage'],
  },
  {
    label: 'Linux ARM64 AppImage',
    pattern: /_(?:arm64|aarch64)\.AppImage$/i,
    platforms: ['linux-aarch64', 'linux-aarch64-appimage'],
  },
  {
    label: 'Linux x64 Debian package',
    pattern: /_amd64\.deb$/i,
    platforms: ['linux-x86_64-deb'],
  },
  {
    label: 'Linux ARM64 Debian package',
    pattern: /_arm64\.deb$/i,
    platforms: ['linux-aarch64-deb'],
  },
  {
    label: 'macOS Universal app archive',
    pattern: /_universal\.app\.tar\.gz$/i,
    platforms: [
      'darwin-aarch64',
      'darwin-x86_64',
      'darwin-universal',
      'darwin-aarch64-app',
      'darwin-x86_64-app',
      'darwin-universal-app',
    ],
  },
];

const distributablePattern = /(-setup\.exe|\.AppImage|\.deb|\.app\.tar\.gz|\.dmg)$/i;

function findExactlyOne(assets, pattern, label) {
  const matches = assets.filter((asset) => pattern.test(asset.name));
  if (matches.length !== 1) {
    throw new Error(`Expected one ${label}, found ${matches.length}.`);
  }
  return matches[0];
}

export function createUpdaterMetadata(release, signatures, repository, tag, version) {
  if (!Array.isArray(release?.assets)) {
    throw new Error('GitHub release metadata has no assets array.');
  }
  if (!repository || !tag || !version) {
    throw new Error('Repository, tag, and version are required.');
  }

  const assets = release.assets;
  const assetNames = new Set(assets.map((asset) => asset.name));
  const recognized = new Set();
  const platforms = {};
  const prefix = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/`;

  for (const definition of signedArtifacts) {
    const asset = findExactlyOne(assets, definition.pattern, definition.label);
    if (!asset.name.includes(`_${version}_`)) {
      throw new Error(
        `${definition.label} does not match release version ${version}: ${asset.name}`,
      );
    }

    const signatureName = `${asset.name}.sig`;
    if (!assetNames.has(signatureName)) {
      throw new Error(`Missing updater signature asset: ${signatureName}`);
    }
    const signature = signatures[signatureName]?.trim();
    if (!signature) {
      throw new Error(`Missing downloaded updater signature: ${signatureName}`);
    }

    const entry = {
      signature,
      url: `${prefix}${encodeURIComponent(asset.name)}`,
    };
    for (const platform of definition.platforms) {
      platforms[platform] = entry;
    }
    recognized.add(asset.name);
  }

  const dmg = findExactlyOne(assets, /_universal\.dmg$/i, 'macOS Universal DMG');
  if (!dmg.name.includes(`_${version}_`)) {
    throw new Error(`macOS Universal DMG does not match release version ${version}: ${dmg.name}`);
  }
  recognized.add(dmg.name);

  const unexpected = assets
    .filter((asset) => distributablePattern.test(asset.name) && !recognized.has(asset.name))
    .map((asset) => asset.name);
  if (unexpected.length > 0) {
    throw new Error(`Unexpected release artifacts: ${unexpected.join(', ')}`);
  }

  return {
    version,
    notes: release.body ?? '',
    pub_date: release.created_at ?? new Date().toISOString(),
    platforms,
  };
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (invokedPath === fileURLToPath(import.meta.url)) {
  const [releasePath, artifactDirectory, repository, tag, version, outputPath] =
    process.argv.slice(2);
  if (!releasePath || !artifactDirectory || !repository || !tag || !version || !outputPath) {
    throw new Error(
      'Usage: create-updater-json.mjs <release.json> <artifact-dir> <repository> <tag> <version> <output.json>',
    );
  }

  const release = JSON.parse(fs.readFileSync(releasePath, 'utf8'));
  const signatures = Object.fromEntries(
    release.assets
      .filter((asset) => asset.name.endsWith('.sig'))
      .map((asset) => [
        asset.name,
        fs.readFileSync(path.join(artifactDirectory, asset.name), 'utf8'),
      ]),
  );
  const metadata = createUpdaterMetadata(release, signatures, repository, tag, version);
  fs.writeFileSync(outputPath, `${JSON.stringify(metadata, null, 2)}\n`);
}
