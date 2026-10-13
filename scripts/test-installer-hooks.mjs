// Exercise the actual NSIS location-selection function without installing an app.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

assert.equal(process.platform, 'win32', 'Installer hooks require Windows and bundled NSIS');
const compiler = join(process.env.LOCALAPPDATA, 'tauri', 'NSIS', 'makensis.exe');
const root = mkdtempSync(join(tmpdir(), 'pyxsis-path-hook-check-'));
try {
  for (const mode of ['legacy', 'legacy-update', 'custom', 'explicit']) {
    const initial =
      mode === 'custom' ? '$LOCALAPPDATA\\ChosenPyxsis' : '$TEMP\\pyxsis-installer-qa-legacy\\App';
    const expected = mode.startsWith('legacy') ? '$LOCALAPPDATA\\Pyxsis' : initial;
    const exe = join(root, `${mode}.exe`);
    const script = join(root, `${mode}.nsi`);
    writeFileSync(
      script,
      `Unicode true
!include MUI2.nsh
!include FileFunc.nsh
!include "${resolve('src-tauri/installer/hooks.nsh')}"
Name "Pyxsis hook check"
OutFile "${exe}"
RequestExecutionLevel user
SilentInstall silent
Function CompilePostInstallHook
 !insertmacro NSIS_HOOK_POSTINSTALL
FunctionEnd
Section "Uninstall"
 !insertmacro NSIS_HOOK_PREUNINSTALL
SectionEnd
Section
 StrCpy $INSTDIR "${initial}"
 Call PyxsisCheckInstallLocation
 StrCmp $INSTDIR "${expected}" +3
 SetErrorLevel 4
 Quit
 StrCpy $INSTDIR "$TEMP\\pyxsis-installer-qa-deliberately-chosen\\App"
 Call PyxsisCheckInstallLocation
 StrCmp $INSTDIR "$TEMP\\pyxsis-installer-qa-deliberately-chosen\\App" +3
 SetErrorLevel 5
 Quit
 SetErrorLevel 0
SectionEnd
`,
    );
    const compiled = spawnSync(compiler, ['/V2', script], { encoding: 'utf8' });
    assert.equal(compiled.status, 0, compiled.error?.message ?? compiled.stdout + compiled.stderr);
    const args =
      mode === 'explicit'
        ? [`/D=${join(tmpdir(), 'pyxsis-installer-qa-legacy', 'App')}`]
        : mode === 'legacy-update'
          ? ['/P', '/UPDATE']
          : [];
    const checked = spawnSync(exe, args, { timeout: 30_000 });
    assert.equal(
      checked.status,
      0,
      `${mode} hook failed: ${checked.error?.message ?? checked.status}`,
    );
    console.log(`${mode} install-location hook passed`);
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log(
  'Native NSIS checks passed without changing the installed application or user registry.',
);
