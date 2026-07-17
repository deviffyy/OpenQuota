import fs from 'node:fs';

const root = new URL('./', import.meta.url);
const rustConsumers = [
  'src-tauri/src/service.rs',
  'src-tauri/src/settings.rs',
  'src-tauri/src/tray_presentation.rs',
  'src-tauri/src/pacing.rs',
  'src-tauri/src/notifications.rs',
  'src-tauri/src/commands/bootstrap.rs',
  'src-tauri/src/commands/settings.rs',
];
const frontendConsumers = [
  'src/App.svelte',
  'src/lib/CustomizeProviderDetail.svelte',
  'src/lib/CustomizeProviderList.svelte',
  'src/lib/Dashboard.svelte',
  'src/lib/MetricRenderer.svelte',
  'src/lib/TotalSpend.svelte',
  'src/lib/metrics.ts',
  'src/lib/shareCard.ts',
];
const providerLiteral = /["'](?:claude|codex|cursor|antigravity|openrouter)["']/;

for (const file of rustConsumers) {
  const source = fs.readFileSync(new URL(file, root), 'utf8').split('#[cfg(test)]')[0];
  if (providerLiteral.test(source)) {
    throw new Error(`${file} contains provider identity outside the provider registry.`);
  }
  if (/ends_with\(\s*["']\./.test(source)) {
    throw new Error(`${file} infers metric behavior from an id suffix.`);
  }
}

for (const file of frontendConsumers) {
  const source = fs.readFileSync(new URL(file, root), 'utf8');
  if (providerLiteral.test(source)) {
    throw new Error(`${file} contains provider identity outside the visual asset registry.`);
  }
  if (/endsWith\(\s*["']\./.test(source)) {
    throw new Error(`${file} infers metric behavior from an id suffix.`);
  }
}

const providerRuntime = fs.readFileSync(new URL('src-tauri/src/providers/mod.rs', root), 'utf8');
if (!/fn definition\(&self\) -> ProviderDefinition;/.test(providerRuntime)) {
  throw new Error('UsageProvider does not own its ProviderDefinition.');
}

const bootstrap = fs.readFileSync(new URL('src-tauri/src/commands/bootstrap.rs', root), 'utf8');
if (!/pub catalog: ProviderCatalog/.test(bootstrap)) {
  throw new Error('BootstrapState does not expose the provider catalog.');
}

const metrics = fs.readFileSync(new URL('src/lib/metrics.ts', root), 'utf8');
if (/metricDefinitions\s*[:=]/.test(metrics)) {
  throw new Error('Frontend metrics must come from the backend provider catalog.');
}

const iconRegistry = fs.readFileSync(new URL('src/lib/providerIconPaths.ts', root), 'utf8');
if (/\?\?\s*codex/.test(iconRegistry)) {
  throw new Error('Unknown providers must not silently render the Codex icon.');
}

const compositionRoot = fs.readFileSync(new URL('src-tauri/src/lib.rs', root), 'utf8');
if (/\.has_local_credentials\(\)/.test(compositionRoot)) {
  throw new Error('Tauri setup must not synchronously probe provider credentials.');
}

const credentialDetection = fs.readFileSync(
  new URL('src-tauri/src/providers/detection.rs', root),
  'utf8',
);
if (!/spawn_blocking/.test(credentialDetection) || !/registry\.runtime/.test(credentialDetection)) {
  throw new Error('Credential detection must fan out registry runtimes on blocking workers.');
}

const providerCommands = fs.readFileSync(
  new URL('src-tauri/src/commands/provider.rs', root),
  'utf8',
);
if (!/registry\s*\.definition\(provider_id\)/s.test(providerCommands)) {
  throw new Error('Provider links must resolve from registry metadata.');
}
if (/pub fn open_provider_link[\s\S]*?url:\s*String/.test(providerCommands)) {
  throw new Error('The provider-link command must not accept an arbitrary URL.');
}

console.log(
  `${rustConsumers.length + frontendConsumers.length} provider consumers use registry metadata without provider-id or suffix inference.`,
);
