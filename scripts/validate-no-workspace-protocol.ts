/**
 * validate-no-workspace-protocol.ts — Safety gate before publishing.
 *
 * Ensures no workspace:* references remain in publishable packages.
 * Exits with code 1 if any are found.
 *
 * Usage:
 *   pnpm tsx scripts/validate-no-workspace-protocol.ts
 */

import { readFileSync } from 'fs';
import { join, resolve } from 'path';
import { globSync } from 'glob';

const ROOT = resolve(__dirname, '..');

const publishableGlobs = ['packages/*/package.json'];

const DEP_TYPES = ['dependencies', 'devDependencies', 'peerDependencies'] as const;

function main() {
  console.log('🔍 Validating no workspace protocol references remain...');

  let violations = 0;

  for (const pattern of publishableGlobs) {
    const matches = globSync(join(ROOT, pattern));
    for (const pkgPath of matches) {
      const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));

      for (const depType of DEP_TYPES) {
        const deps = pkg[depType];
        if (!deps) continue;

        for (const [name, ver] of Object.entries(deps)) {
          if (String(ver).startsWith('workspace:')) {
            console.error(`  ❌ ${pkg.name} → ${depType}.${name}: ${ver}`);
            violations++;
          }
        }
      }
    }
  }

  if (violations > 0) {
    console.error(`\n❌ Found ${violations} workspace protocol reference(s). Run prepare-publish.ts first.`);
    process.exit(1);
  }

  console.log('✅ No workspace protocol references found. Safe to publish.');
}

main();
