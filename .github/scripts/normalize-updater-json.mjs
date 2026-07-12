import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function normalizeUpdaterMetadata(update, release, repository, tag) {
  if (!update?.platforms || typeof update.platforms !== 'object') {
    throw new Error('Updater metadata has no platforms object.');
  }
  const assetsById = new Map(release.assets.map((asset) => [Number(asset.id), asset]));
  const assetsByName = new Map(release.assets.map((asset) => [asset.name, asset]));
  const prefix = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/`;

  for (const [platform, entry] of Object.entries(update.platforms)) {
    if (!entry?.url || !entry.signature) {
      throw new Error(`Incomplete updater entry: ${platform}`);
    }
    const apiMatch = entry.url.match(/\/releases\/assets\/(\d+)$/);
    let asset = apiMatch ? assetsById.get(Number(apiMatch[1])) : undefined;
    if (!asset && entry.url.startsWith(prefix)) {
      asset = assetsByName.get(decodeURIComponent(entry.url.slice(prefix.length)));
    }
    if (!asset) throw new Error(`Unknown updater asset for ${platform}: ${entry.url}`);
    if (!assetsByName.has(`${asset.name}.sig`)) {
      throw new Error(`Missing updater signature asset for ${platform}: ${asset.name}.sig`);
    }
    entry.url = `${prefix}${encodeURIComponent(asset.name)}`;
  }
  return update;
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (invokedPath === fileURLToPath(import.meta.url)) {
  const [metadataPath, releasePath, repository, tag] = process.argv.slice(2);
  if (!metadataPath || !releasePath || !repository || !tag) {
    throw new Error(
      'Usage: normalize-updater-json.mjs <latest.json> <release.json> <repository> <tag>',
    );
  }
  const update = JSON.parse(fs.readFileSync(metadataPath, 'utf8'));
  const release = JSON.parse(fs.readFileSync(releasePath, 'utf8'));
  fs.writeFileSync(
    metadataPath,
    `${JSON.stringify(normalizeUpdaterMetadata(update, release, repository, tag), null, 2)}\n`,
  );
}
