import fs from 'node:fs';

const backend = fs.readFileSync(new URL('./src/lib/backend.ts', import.meta.url), 'utf8');
const application = fs.readFileSync(new URL('./src-tauri/src/lib.rs', import.meta.url), 'utf8');
const invokedCommands = new Set(
  [...backend.matchAll(/invoke(?:<[^>]+>)?\(\s*'([^']+)'/g)].map((match) => match[1]),
);
const handlerBlock = application.match(/invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/);
if (!handlerBlock) {
  throw new Error('Could not find the Tauri invoke handler registration.');
}
const registeredCommands = new Set(
  [...handlerBlock[1].matchAll(/(?:[a-zA-Z0-9_]+::)*([a-zA-Z0-9_]+)/g)].map((match) => match[1]),
);

const missingCommands = [...invokedCommands].filter((command) => !registeredCommands.has(command));
if (missingCommands.length > 0) {
  throw new Error(`Frontend invokes unregistered Tauri commands: ${missingCommands.join(', ')}`);
}

console.log(
  `${invokedCommands.size} frontend Tauri commands match registered Rust command handlers.`,
);
