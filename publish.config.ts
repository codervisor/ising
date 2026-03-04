/**
 * Publish configuration for ising
 *
 * Distributes the `ising` CLI binary via npm platform packages.
 */
import type { PublishConfig } from '@codervisor/forge';

export default {
  scope: '@codervisor',

  binaries: [
    { name: 'ising', scope: 'cli', cargoPackage: 'ising-cli' },
  ],

  platforms: ['darwin-x64', 'darwin-arm64', 'linux-x64', 'windows-x64'],

  mainPackages: [
    { path: 'packages/cli', name: '@codervisor/cli' },
  ],

  cargoWorkspace: 'Cargo.toml',

  repositoryUrl: 'https://github.com/codervisor/ising',
} satisfies PublishConfig;
