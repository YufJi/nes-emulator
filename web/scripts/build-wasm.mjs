#!/usr/bin/env node
/**
 * Build the Rust NES core to WebAssembly and vendor the generated bindings
 * into `web/src/wasm`, where Vite picks them up like any other source file.
 */
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.resolve(scriptDir, '..');
const repoRoot = path.resolve(webDir, '..');
const crateDir = path.join(repoRoot, 'crates', 'nes-wasm');
const pkgDir = path.join(crateDir, 'pkg');
const outDir = path.join(webDir, 'src', 'wasm');

const profileArgs = process.env.NES_WASM_PROFILE === 'dev' ? ['--dev'] : ['--release'];

console.log(`[build-wasm] wasm-pack build ${profileArgs.join(' ')} (cwd: ${crateDir})`);

const result = spawnSync(
  'wasm-pack',
  ['build', '--target', 'web', '--out-name', 'nes_wasm', ...profileArgs],
  { cwd: crateDir, stdio: 'inherit', env: process.env },
);

if (result.error) {
  console.error('[build-wasm] 无法执行 wasm-pack，请先安装：cargo install wasm-pack');
  console.error(result.error.message);
  process.exit(1);
}

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

if (!existsSync(pkgDir)) {
  console.error(`[build-wasm] 未找到 wasm-pack 输出目录：${pkgDir}`);
  process.exit(1);
}

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

for (const file of ['nes_wasm.js', 'nes_wasm.d.ts', 'nes_wasm_bg.wasm', 'nes_wasm_bg.wasm.d.ts']) {
  cpSync(path.join(pkgDir, file), path.join(outDir, file));
}

console.log(`[build-wasm] 已输出到 ${path.relative(repoRoot, outDir)}`);
