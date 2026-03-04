/**
 * add-platform-deps.ts — Add platform packages as optionalDependencies in main packages.
 *
 * Usage:
 *   pnpm tsx scripts/add-platform-deps.ts
 */

import { readFileSync, writeFileSync } from 'fs';
import { join, resolve } from 'path';
import config from '../publish.config';

const ROOT = resolve(__dirname, '..');
const rootPkg = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8'));
const version = rootPkg.version;

function main() {
  console.log(`📦 Adding platform optionalDependencies (version: ${version})`);

  for (const mainPkg of config.mainPackages) {
    const pkgPath = join(ROOT, mainPkg.path, 'package.json');
    const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));

    if (!pkg.optionalDependencies) {
      pkg.optionalDependencies = {};
    }

    for (const binary of config.binaries) {
      for (const platform of config.platforms) {
        const depName = `${config.scope}/${binary.scope}-${platform}`;
        pkg.optionalDependencies[depName] = version;
      }
    }

    writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
    console.log(`  ✅ ${mainPkg.name}: added ${config.platforms.length * config.binaries.length} optionalDependencies`);
  }
}

main();
