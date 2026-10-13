import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync } from 'node:fs';
const release = process.argv[2] === 'release';
const result = spawnSync(
  'cargo',
  ['build', '--locked', '-p', 'pyxsis-cli', ...(release ? ['--release'] : [])],
  { stdio: 'inherit' },
);
if (result.status !== 0) process.exit(result.status ?? 1);
mkdirSync('src-tauri/binaries', { recursive: true });
copyFileSync(
  `target/${release ? 'release' : 'debug'}/pyxsis-cli.exe`,
  'src-tauri/binaries/pyxsis-cli-x86_64-pc-windows-msvc.exe',
);
