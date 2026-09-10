#!/usr/bin/env node
// Generate website/src/data/facts.json from docs/metrics.md — the single
// authoritative metrics source (same file that feeds the README badges).
//
// Usage:
//   node scripts/gen-facts.mjs          # (re)generate website/src/data/facts.json
//   node scripts/gen-facts.mjs --check  # CI gate: fail if the committed file is stale
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const metricsPath = join(root, 'docs', 'metrics.md');
const outPath = join(root, 'website', 'src', 'data', 'facts.json');

const md = readFileSync(metricsPath, 'utf8');

// Summary table rows look like: | Tests (nextest, runnable) | 11752 |
function summaryValue(labelPattern) {
  const re = new RegExp(`\\|\\s*${labelPattern}\\s*\\|\\s*([0-9,]+)\\s*\\|`, 'm');
  const m = md.match(re);
  if (!m) throw new Error(`docs/metrics.md: cannot find summary row matching /${labelPattern}/`);
  return Number(m[1].replace(/,/g, ''));
}

const facts = {
  generatedFrom: 'docs/metrics.md',
  tests: summaryValue('Tests \\(nextest, runnable\\)'),
  rustLoc: summaryValue('Rust LOC \\(code\\)'),
  sourceFiles: summaryValue('Rust source files'),
  workspaceMembers: summaryValue('Workspace members'),
};

facts.testsDisplay = `${facts.tests.toLocaleString('en-US')}+`;
facts.rustLocDisplay = `${Math.round(facts.rustLoc / 1000)}K+`;

const json = JSON.stringify(facts, null, 2) + '\n';

if (process.argv.includes('--check')) {
  const current = readFileSync(outPath, 'utf8');
  if (current !== json) {
    console.error(
      'facade facts are stale (docs/metrics.md changed without regenerating):\n' +
        '  run: node scripts/gen-facts.mjs && commit website/src/data/facts.json'
    );
    process.exit(1);
  }
  console.log('facade facts up to date');
} else {
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, json);
  console.log(`wrote ${outPath}`);
}
