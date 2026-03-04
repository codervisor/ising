/**
 * Publish configuration for ising
 *
 * Distributes the `ising` CLI binary via npm platform packages.
 */

interface PublishConfig {
  scope: string;
  binaries: { name: string; scope: string; cargoPackage: string }[];
  platforms: string[];
  mainPackages: { path: string; name: string }[];
  cargoWorkspace: string;
  repositoryUrl: string;
}

export default {
  scope: '@codervisor',

  binaries: [
    { name: 'ising', scope: 'cli', cargoPackage: 'ising-cli' },
  ],

  platforms: ['darwin-x64', 'darwin-arm64', 'linux-x64', 'windows-x64'],

  mainPackages: [
    { path: 'packages/cli', name: '@codervisor/ising-cli' },
  ],

  cargoWorkspace: 'Cargo.toml',

  repositoryUrl: 'https://github.com/codervisor/ising',
} satisfies PublishConfig;
