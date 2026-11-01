const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { Version } = require('release-please/build/src/version');
const { TagName } = require('release-please/build/src/util/tag-name');
const { parseConventionalCommits } = require('release-please/build/src/commit');
const { Manifest } = require('release-please');
const { Sakatsu, versionUpdates, trustedPullRequest } = require('./index.cjs');
const current = JSON.parse(readFileSync(resolve(__dirname, '../../package.json'), 'utf8')).version;
const parts = current.split('.').map(Number);
const next = `${parts[0]}.${parts[1]}.${parts[2] + 1}`;

test('actual library loads the custom manifest as a draft prerelease', async () => {
  const github = { repository: { owner: 'example', repo: 'reader' },
    getFileJson: async file => JSON.parse(readFileSync(resolve(__dirname, '../..', file), 'utf8')) };
  const manifest = await Manifest.fromManifest(github, 'main', '.github/release-please/config.json', '.github/release-please/manifest.json');
  assert.equal(manifest.repositoryConfig['.'].releaseType, 'sakatsu');
  assert.equal(manifest.repositoryConfig['.'].draft, true);
  assert.equal(manifest.repositoryConfig['.'].prerelease, true);
});

test('actual pinned release strategy builds a patch PR with reconciled versions', async () => {
  const strategy = new Sakatsu({
    github: { repository: { owner: 'example', repo: 'reader' } }, targetBranch: 'main',
    includeComponentInTag: false,
    changelogNotes: { buildNotes: async () => `## ${next}\n\n### Fixes\n\n* Release automation\n` },
  });
  const pr = await strategy.buildReleasePullRequest(
    parseConventionalCommits([{ sha: 'b'.repeat(40), message: 'fix(release): automate private previews', files: [] }]),
    { tag: TagName.parse('v' + current), sha: 'a'.repeat(40) },
  );
  assert.equal(String(pr.version), next);
  assert.equal(pr.updates.length, 7);
  assert(pr.updates.some(u => u.path === 'Cargo.lock'));
  assert(pr.updates.some(u => u.path === 'CHANGELOG.md'));
});

test('updaters preserve inherited versions and unrelated dependencies', () => {
  const updates = versionUpdates(Version.parse(next), `## ${next}\n\n### Fixes\n\n* Preview\n`);
  const root = resolve(__dirname, '../..');
  const cargo = updates.find(u => u.path === 'Cargo.toml').updater.updateContent('');
  assert(cargo.includes(`version = "${next}"`));
  for (const member of ['core', 'src-tauri']) {
    assert(!updates.some(u => u.path === `${member}/Cargo.toml`));
    assert.match(readFileSync(resolve(root, member, 'Cargo.toml'), 'utf8'), /version\.workspace = true/);
  }
  const log = updates.find(u => u.path === 'CHANGELOG.md').updater.updateContent('# Changelog\n\n## 1.0.0\n\nInitial.\n');
  assert(log.includes(next));
  assert.match(log, /Initial/);
});

test('explicit CI dispatch accepts only the same repository release PR branch', () => {
  const valid = { base: { ref: 'main' }, head: { repo: { full_name: 'a/b' }, ref: 'release-please--branches--main' }, state: 'open' };
  assert.equal(trustedPullRequest(valid, 'a/b'), valid.head.ref);
  for (const mutate of [p => p.base.ref = 'other', p => p.head.repo.full_name = 'fork/b',
    p => p.head.ref = 'main', p => p.state = 'closed']) {
    const bad = structuredClone(valid); mutate(bad);
    assert.throws(() => trustedPullRequest(bad, 'a/b'));
  }
});
