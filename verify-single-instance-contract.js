import fs from 'node:fs';

const manifest = fs.readFileSync(new URL('./src-tauri/Cargo.toml', import.meta.url), 'utf8');
const application = fs.readFileSync(new URL('./src-tauri/src/lib.rs', import.meta.url), 'utf8');

if (!/^tauri-plugin-single-instance\s*=/m.test(manifest)) {
  throw new Error('The desktop manifest must include the Tauri single-instance plugin.');
}

const builderStart = application.indexOf('tauri::Builder::default()');
const singleInstance = application.indexOf(
  '.plugin(tauri_plugin_single_instance::init',
  builderStart,
);
if (builderStart < 0 || singleInstance < 0) {
  throw new Error('The Tauri builder must register the single-instance plugin.');
}

const firstPlugin = application.indexOf('.plugin(', builderStart);
if (firstPlugin !== singleInstance) {
  throw new Error('The single-instance plugin must remain the first registered Tauri plugin.');
}
if (!application.includes('window::activate_existing_instance(app);')) {
  throw new Error('A second launch must activate the existing OpenQuota window.');
}

console.log('Single-instance remains the first Tauri plugin and activates the existing window.');
