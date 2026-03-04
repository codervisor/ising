/**
 * validate-platform-binaries.ts — Pre-publish safety check for binary files.
 *
 * Verifies all binary files exist and have correct format headers.
 *
 * Usage:
 *   pnpm tsx scripts/validate-platform-binaries.ts
 */

import { readFileSync, statSync, existsSync } from 'fs';
import { join, resolve } from 'path';
import config from '../publish.config';

const ROOT = resolve(__dirname, '..');
const PLATFORM_DIR = join(ROOT, 'platform-packages');

const HEADERS: Record<string, { bytes: number[]; name: string }[]> = {
  darwin: [
    { bytes: [0xcf, 0xfa, 0xed, 0xfe], name: 'Mach-O 64-bit (LE)' },
    { bytes: [0xfe, 0xed, 0xfa, 0xcf], name: 'Mach-O 64-bit (BE)' },
  ],
  linux: [
    { bytes: [0x7f, 0x45, 0x4c, 0x46], name: 'ELF' },
  ],
  windows: [
    { bytes: [0x4d, 0x5a], name: 'PE/MZ' },
  ],
};

function getOsFromPlatform(platform: string): string {
  if (platform.startsWith('darwin')) return 'darwin';
  if (platform.startsWith('linux')) return 'linux';
  if (platform.startsWith('windows')) return 'windows';
  return 'unknown';
}

function validateHeader(filePath: string, os: string): boolean {
  const expectedHeaders = HEADERS[os];
  if (!expectedHeaders) return true;

  const buffer = readFileSync(filePath);
  return expectedHeaders.some((header) =>
    header.bytes.every((byte, i) => buffer[i] === byte)
  );
}

function main() {
  console.log('🔍 Validating platform binaries...');

  let errors = 0;

  for (const binary of config.binaries) {
    for (const platform of config.platforms) {
      const os = getOsFromPlatform(platform);
      const ext = platform.startsWith('windows') ? '.exe' : '';
      const binaryFile = `${binary.name}${ext}`;
      const binaryPath = join(PLATFORM_DIR, `${binary.scope}-${platform}`, binaryFile);

      if (!existsSync(binaryPath)) {
        console.error(`  ❌ Missing: ${binaryPath}`);
        errors++;
        continue;
      }

      const stat = statSync(binaryPath);
      if (stat.size === 0) {
        console.error(`  ❌ Empty file: ${binaryPath}`);
        errors++;
        continue;
      }

      if (!validateHeader(binaryPath, os)) {
        console.error(`  ❌ Invalid binary header: ${binaryPath} (expected ${os} format)`);
        errors++;
        continue;
      }

      const sizeMB = (stat.size / 1024 / 1024).toFixed(1);
      console.log(`  ✅ ${binary.scope}-${platform}: ${binaryFile} (${sizeMB} MB)`);
    }
  }

  if (errors > 0) {
    console.error(`\n❌ ${errors} validation error(s)`);
    process.exit(1);
  }

  console.log('\n✅ All binaries validated');
}

main();
