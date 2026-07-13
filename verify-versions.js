import fs from 'node:fs';

const packageJson = JSON.parse(fs.readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
const tauriConfig = JSON.parse(
  fs.readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
);
const cargoManifest = fs.readFileSync(new URL('./src-tauri/Cargo.toml', import.meta.url), 'utf8');
const cargoPackage = cargoManifest.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);

if (!cargoPackage) {
  throw new Error('Could not read the package version from src-tauri/Cargo.toml.');
}

const versions = {
  package: packageJson.version,
  tauri: tauriConfig.version,
  cargo: cargoPackage[1],
};
const uniqueVersions = new Set(Object.values(versions));

if (uniqueVersions.size !== 1) {
  throw new Error(
    `OpenQuota version mismatch: ${Object.entries(versions)
      .map(([source, version]) => `${source}=${version}`)
      .join(', ')}`,
  );
}

const expectedTag = process.argv[2];
if (expectedTag && expectedTag !== `v${versions.package}`) {
  throw new Error(
    `Release tag ${expectedTag} does not match application version ${versions.package}.`,
  );
}

console.log(`OpenQuota version ${versions.package} is consistent.`);
