import fs from 'node:fs';

const rustSource = [
  './src-tauri/src/models.rs',
  './src-tauri/src/service.rs',
  './src-tauri/src/commands/bootstrap.rs',
]
  .map((file) => fs.readFileSync(new URL(file, import.meta.url), 'utf8'))
  .join('\n');
const typeScriptSource = fs.readFileSync(new URL('./src/lib/types.ts', import.meta.url), 'utf8');
const contracts = [
  'QuotaWindow',
  'UsagePeriod',
  'ModelUsageEntry',
  'ModelUsageBreakdown',
  'DailyUsage',
  'UsageHistory',
  'ProviderSnapshot',
  'ProviderViewState',
  'UsageViewState',
  'MetricLayout',
  'ProviderLayout',
  'NotificationPreferences',
  'AppSettings',
  'SettingsViewState',
  'BootstrapState',
];

function camelCase(value) {
  return value.replace(/_([a-zA-Z0-9])/g, (_, character) => character.toUpperCase());
}

function rustFields(name) {
  const body = rustSource.match(new RegExp(`pub struct ${name}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1];
  if (!body) throw new Error(`Rust contract ${name} was not found.`);
  return new Set(
    [...body.matchAll(/^\s*pub\s+([a-zA-Z0-9_]+)\s*:/gm)].map((match) => camelCase(match[1])),
  );
}

const typeScriptInterfaces = new Map(
  [
    ...typeScriptSource.matchAll(
      /export interface\s+([a-zA-Z0-9_]+)(?:\s+extends\s+([a-zA-Z0-9_]+))?\s*\{([\s\S]*?)\n\}/g,
    ),
  ].map((match) => [match[1], { parent: match[2], body: match[3] }]),
);

function typeScriptFields(name, visiting = new Set()) {
  if (visiting.has(name)) throw new Error(`Circular TypeScript contract inheritance at ${name}.`);
  const contract = typeScriptInterfaces.get(name);
  if (!contract) throw new Error(`TypeScript contract ${name} was not found.`);
  const fields = new Set(
    [...contract.body.matchAll(/^\s*([a-zA-Z0-9_]+)\??\s*:/gm)].map((match) => match[1]),
  );
  if (contract.parent) {
    visiting.add(name);
    for (const field of typeScriptFields(contract.parent, visiting)) fields.add(field);
  }
  return fields;
}

for (const contract of contracts) {
  const rust = rustFields(contract);
  const typeScript = typeScriptFields(contract);
  const missing = [...rust].filter((field) => !typeScript.has(field));
  const extra = [...typeScript].filter((field) => !rust.has(field));
  if (missing.length > 0 || extra.length > 0) {
    throw new Error(
      `${contract} field mismatch; missing in TypeScript: ${missing.join(', ') || 'none'}; ` +
        `extra in TypeScript: ${extra.join(', ') || 'none'}`,
    );
  }
}

console.log(`${contracts.length} Rust/TypeScript model field contracts match.`);
