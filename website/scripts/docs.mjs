import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const repo = fileURLToPath(new URL('../../', import.meta.url));
export const site = 'https://samishal1998.github.io';
export const base = '/takt';
export const github = 'https://github.com/samishal1998/takt';

export const pages = [
  ...readdirSync(new URL('../../docs/', import.meta.url))
    .filter((file) => file.endsWith('.md'))
    .sort()
    .map((file) => ({ source: `docs/${file}`, slug: file.slice(0, -3) })),
  { source: 'examples/README.md', slug: 'examples' },
];

export const descriptions = {
  'quick-start': 'Install Takt and run your first workflow with explicit CPU and memory budgets.',
  installation: 'Install Takt on Linux or macOS, or use the portable caching project wrapper.',
  configuration: 'Configure tasks, groups, dependencies, services, and resource policies.',
  cli: 'Takt commands, flags, environment variables, output modes, and exit codes.',
  examples: 'Runnable configurations for checks, branching, service readiness, and protected CI.',
  runtime: 'The scheduling, readiness, lifecycle, and resource-enforcement contract.',
  testing: 'Independent checks and evidence for platform compatibility and process protection.',
  release: 'Supported platforms, verified archives, compatibility gates, and release publication.',
  performance: 'Measured executable size, startup, memory use, and idle CPU.',
  decisions: 'Design decisions behind Takt’s command runner and resource policy.',
  'implementation-status': 'Implementation status and acceptance evidence for Takt v0.1.',
};
