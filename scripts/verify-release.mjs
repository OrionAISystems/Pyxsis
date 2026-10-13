import { readFileSync } from 'node:fs';
import { createHash, createPublicKey, verify } from 'node:crypto';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

export const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
export function verifyUpdaterSignature(bytes, encodedSignature) {
  const publicLines = Buffer.from(config.plugins.updater.pubkey, 'base64')
    .toString('utf8')
    .trim()
    .split(/\r?\n/);
  const publicBytes = Buffer.from(publicLines[1] ?? '', 'base64');
  const lines = Buffer.from(encodedSignature, 'base64').toString('utf8').trim().split(/\r?\n/);
  const signature = Buffer.from(lines[1] ?? '', 'base64');
  assert.equal(publicBytes.length, 42, 'Invalid updater public key');
  assert.equal(signature.length, 74, 'Invalid updater signature');
  assert.equal(lines.length, 4, 'Invalid minisign envelope');
  assert.ok(lines[2].startsWith('trusted comment: '));
  assert.ok(
    publicBytes.subarray(2, 10).equals(signature.subarray(2, 10)),
    'Signature belongs to another key',
  );
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicBytes.subarray(10)]),
    format: 'der',
    type: 'spki',
  });
  const algorithm = signature.subarray(0, 2).toString();
  assert.ok(['Ed', 'ED'].includes(algorithm));
  const payload = algorithm === 'ED' ? createHash('blake2b512').update(bytes).digest() : bytes;
  assert.ok(
    verify(null, payload, key, signature.subarray(10)),
    'Installer signature does not verify',
  );
  assert.ok(
    verify(
      null,
      Buffer.concat([signature.subarray(10), Buffer.from(lines[2].slice(17))]),
      key,
      Buffer.from(lines[3], 'base64'),
    ),
    'Trusted comment signature does not verify',
  );
}
export function verifyConfig() {
  const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.equal(config.version, version);
  for (const path of [
    'src-tauri/Cargo.toml',
    'crates/cli/Cargo.toml',
    'crates/capture-core/Cargo.toml',
  ])
    assert.equal(
      readFileSync(path, 'utf8').match(/^version = "([^"]+)"/m)?.[1],
      version,
      `${path} version`,
    );
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.deepEqual(config.plugins.updater.endpoints, [
    'https://github.com/OrionAISystems/Pyxsis/releases/latest/download/latest.json',
  ]);
  assert.ok(!config.plugins.updater.pubkey.includes('REPLACE'));
  return version;
}
async function main() {
  const version = verifyConfig();
  if (process.argv.includes('--public')) {
    const url = 'https://github.com/OrionAISystems/Pyxsis/releases/latest/download/latest.json';
    const response = await fetch(url);
    assert.ok(response.ok, `Manifest HTTP ${response.status}`);
    const manifest = await response.json();
    assert.equal(manifest.version, version);
    const platform = manifest.platforms['windows-x86_64'];
    assert.ok(platform);
    assert.ok(
      platform.url.startsWith(
        `https://github.com/OrionAISystems/Pyxsis/releases/download/v${version}/`,
      ),
    );
    const installer = await fetch(platform.url);
    assert.ok(installer.ok, `Installer HTTP ${installer.status}`);
    const bytes = Buffer.from(await installer.arrayBuffer());
    verifyUpdaterSignature(bytes, platform.signature);
    const altered = Buffer.from(bytes);
    altered[0] ^= 1;
    assert.throws(
      () => verifyUpdaterSignature(altered, platform.signature),
      'Tampered installers must be rejected',
    );
    console.log(
      JSON.stringify({
        version,
        publicManifest: true,
        installerBytes: bytes.length,
        signatureValid: true,
        tamperingRejected: true,
      }),
    );
  } else console.log(JSON.stringify({ version, releaseConfiguration: true }));
}
if (import.meta.url === pathToFileURL(process.argv[1]).href) await main();
