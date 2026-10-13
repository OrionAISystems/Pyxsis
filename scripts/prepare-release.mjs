import { readdirSync, readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { verifyConfig, verifyUpdaterSignature } from './verify-release.mjs';
const version = verifyConfig();
const directory = 'target/release/bundle/nsis';
const installers = readdirSync(directory).filter(
  (name) => name.endsWith('-setup.exe') && name.includes(version),
);
assert.equal(installers.length, 1);
const name = installers[0];
const installer = readFileSync(join(directory, name));
const signature = readFileSync(join(directory, `${name}.sig`), 'utf8').trim();
verifyUpdaterSignature(installer, signature);
const tampered = Buffer.from(installer);
tampered[0] ^= 1;
assert.throws(() => verifyUpdaterSignature(tampered, signature));
mkdirSync('target/public-release', { recursive: true });
const out = 'target/public-release';
copyFileSync(join(directory, name), join(out, name));
copyFileSync(join(directory, `${name}.sig`), join(out, `${name}.sig`));
copyFileSync('target/release/pyxsis-cli.exe', join(out, `pyxsis-cli-${version}-windows-x64.exe`));
writeFileSync(
  join(out, 'latest.json'),
  JSON.stringify(
    {
      version,
      notes: `Pyxsis ${version}. See CHANGELOG.md in the public repository for changes and capture boundaries.`,
      pub_date: new Date().toISOString(),
      platforms: {
        'windows-x86_64': {
          signature,
          url: `https://github.com/OrionAISystems/Pyxsis/releases/download/v${version}/${encodeURIComponent(name)}`,
        },
      },
    },
    null,
    2,
  ) + '\n',
);
const names = readdirSync(out).filter((n) => n !== 'SHA256SUMS.txt');
writeFileSync(
  join(out, 'SHA256SUMS.txt'),
  names
    .map(
      (n) =>
        `${createHash('sha256')
          .update(readFileSync(join(out, n)))
          .digest('hex')}  ${n}`,
    )
    .join('\n') + '\n',
);
console.log(
  JSON.stringify({
    version,
    directory: out,
    installer: name,
    signatureValid: true,
    tamperingRejected: true,
  }),
);
