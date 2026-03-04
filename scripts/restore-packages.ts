/**
 * restore-packages.ts — Restore workspace:* dependencies after publishing.
 *
 * Finds .backup files created by prepare-publish.ts and restores originals.
 *
 * Usage:
 *   pnpm tsx scripts/restore-packages.ts
 */

import { copyFileSync, unlinkSync } from 'fs';
import { resolve } from 'path';
import { globSync } from 'glob';

const ROOT = resolve(__dirname, '..');

const backupGlobs = ['packages/*/package.json.backup'];

function main() {
  console.log('🔄 Restoring package.json files from backups...');

  let restored = 0;

  for (const pattern of backupGlobs) {
    const matches = globSync(`${ROOT}/${pattern}`);
    for (const backupPath of matches) {
      const originalPath = backupPath.replace('.backup', '');
      copyFileSync(backupPath, originalPath);
      unlinkSync(backupPath);
      console.log(`  ✅ Restored: ${originalPath}`);
      restored++;
    }
  }

  if (restored === 0) {
    console.log('  ⏭️  No backup files found');
  } else {
    console.log(`\n📊 Restored ${restored} file(s)`);
  }
}

main();
