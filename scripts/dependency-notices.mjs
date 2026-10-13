import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
const result = spawnSync('cargo', ['metadata', '--locked', '--format-version', '1'], {
  encoding: 'utf8',
  maxBuffer: 20 * 1024 * 1024,
});
if (result.status !== 0) throw new Error(result.stderr);
const rust = JSON.parse(result.stdout)
  .packages.filter((p) => p.source)
  .sort((a, b) => a.name.localeCompare(b.name));
const npm = Object.entries(JSON.parse(readFileSync('package-lock.json', 'utf8')).packages)
  .filter(([key]) => key)
  .sort(([a], [b]) => a.localeCompare(b));
let text =
  '# Third-party dependency notices\n\nPyxsis is MIT licensed. Dependencies retain their own licenses. This inventory includes runtime and build dependencies from the locked manifests. Source and full license texts are available from the linked package releases; this file is included with the installer.\n\n## Rust\n\n| Package | Version | License | Source |\n| --- | --- | --- | --- |\n';
for (const p of rust)
  text += `| ${p.name} | ${p.version} | ${(p.license ?? 'See package license').replaceAll('|', '/')} | [source](https://crates.io/crates/${p.name}/${p.version}) |\n`;
text += '\n## JavaScript\n\n| Package | Version | License | Source |\n| --- | --- | --- | --- |\n';
for (const [key, p] of npm) {
  const name = key.split('node_modules/').pop();
  text += `| ${name} | ${p.version} | ${p.license ?? 'See package license'} | [source](https://www.npmjs.com/package/${name}/v/${p.version}) |\n`;
}
const licenseTexts = new Map();
function collect(directory, label) {
  if (!existsSync(directory)) return;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isFile() || !/^(licen[cs]e|copying|notice)([._-].*)?$/i.test(entry.name)) continue;
    const content = readFileSync(join(directory, entry.name), 'utf8')
      .replace(/\r\n/g, '\n')
      .replace(/[ \t]+$/gm, '')
      .trim();
    const hash = createHash('sha256').update(content).digest('hex');
    if (!licenseTexts.has(hash)) licenseTexts.set(hash, { content, labels: new Set() });
    licenseTexts.get(hash).labels.add(`${label} (${entry.name})`);
  }
}
for (const p of rust) collect(dirname(p.manifest_path), `${p.name} ${p.version}`);
for (const [key, p] of npm) collect(key, `${key.split('node_modules/').pop()} ${p.version}`);
text += '\n## Included license and notice texts\n\n';
for (const { content, labels } of licenseTexts.values())
  text += `### ${[...labels].join(', ')}\n\n\`\`\`text\n${content.replaceAll('```', '\u0060 \u0060 \u0060')}\n\`\`\`\n\n`;
text +=
  '\n## Behavioral reference\n\nSharpShot, copyright 2026 Leonxlnx, MIT license. Used as a behavioral reference only; no source copied. https://github.com/Leonxlnx/SharpShot\n';
writeFileSync('THIRD_PARTY_NOTICES.md', text.trimEnd() + '\n');
console.log(`Inventoried ${rust.length} Rust packages and ${npm.length} JavaScript packages.`);
