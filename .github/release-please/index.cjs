const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { appendFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { GitHub, Manifest, registerReleaseType } = require('release-please');
// Pinned internal adapters are exercised against the actual installed library in test.cjs.
const { BaseStrategy } = require('release-please/build/src/strategies/base');
const { Changelog } = require('release-please/build/src/updaters/changelog');

const ROOT = resolve(__dirname, '../..');

function versionUpdates(nextVersion, changelogEntry, root = ROOT) {
  const text = execFileSync(process.platform === 'win32' ? 'python' : 'python3',
    [resolve(ROOT, 'scripts/release-control.py'), 'bump-plan', '--root', root, '--version', String(nextVersion)],
    { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  const plan = JSON.parse(text);
  assert.equal(plan.version, String(nextVersion));
  return [
    ...plan.updates.map(({ path, content }) => ({
      path, createIfMissing: false, updater: { updateContent: () => content },
    })),
    { path: 'CHANGELOG.md', createIfMissing: true,
      updater: new Changelog({ version: nextVersion, changelogEntry }) },
  ];
}

class Sakatsu extends BaseStrategy {
  async getDefaultPackageName() { return 'sakatsu-reader'; }
  async buildUpdates({ newVersion, changelogEntry }) {
    return versionUpdates(newVersion, changelogEntry);
  }
}
registerReleaseType('sakatsu', (options) => new Sakatsu(options));

function trustedPullRequest(pr, repo) {
  assert.equal(pr.base.ref, 'main');
  assert.equal(pr.head.repo.full_name, repo);
  assert.match(pr.head.ref, /^release-please--branches--main(?:--[\w.-]+)?$/);
  assert.equal(pr.state, 'open');
  return pr.head.ref;
}

async function request(repo, token, path) {
  const response = await fetch(`https://api.github.com/repos/${repo}${path ? '/' + path : ''}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28' },
  });
  assert(response.ok, `GitHub request failed (${response.status})`);
  return response.json();
}

async function prepare({ repo, token, dryRun = false }) {
  assert.match(repo, /^[\w.-]+\/[\w.-]+$/);
  assert(token, 'A repository-scoped workflow token is required');
  const metadata = await request(repo, token, '');
  assert.equal(metadata.private, true, 'This workflow prepares private previews only');
  const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: ROOT, encoding: 'utf8' }).trim();
  const main = await request(repo, token, 'commits/main');
  assert.equal(head, main.sha, 'main moved; rerun from a fresh checkout');
  assert.equal(execFileSync('git', ['status', '--porcelain'], { cwd: ROOT, encoding: 'utf8' }).trim(), '',
    'Release preparation requires a clean checkout');
  const drafts = (await request(repo, token, 'releases?per_page=100')).filter(r => r.draft && /^v\d+\.\d+\.\d+$/.test(r.tag_name));
  if (drafts.length) {
    return { waiting: drafts.map(r => r.tag_name), prs: [] };
  }
  const [owner, name] = repo.split('/');
  const github = await GitHub.create({ owner, repo: name, token });
  const manifest = await Manifest.fromManifest(github, 'main', '.github/release-please/config.json',
    '.github/release-please/manifest.json');
  if (dryRun) return { proposed: await manifest.buildPullRequests(), prs: [] };
  const releases = (await manifest.createReleases()).filter(Boolean);
  assert(releases.length <= 1, 'Only one application release is supported');
  if (releases.length) {
    const release = releases[0];
    assert.match(release.tagName, /^v\d+\.\d+\.\d+$/);
    assert.match(release.sha, /^[0-9a-f]{40}$/);
    return { tag: release.tagName, sha: release.sha, prs: [] };
  }
  const prs = (await manifest.createPullRequests()).filter(Boolean);
  const branches = [];
  for (const pr of prs) {
    // GITHUB_TOKEN-created PRs do not trigger CI automatically. Explicit dispatch uses the
    // actual PR head in this repository; PR text cannot choose a different workflow or repo.
    const data = await request(repo, token, `pulls/${pr.number}`);
    branches.push(trustedPullRequest(data, repo));
  }
  return { prs: branches };
}

module.exports = { versionUpdates, Sakatsu, trustedPullRequest, prepare };

if (require.main === module) {
  prepare({ repo: process.env.GITHUB_REPOSITORY, token: process.env.GH_TOKEN,
    dryRun: process.argv.includes('--dry-run') }).then(result => {
    console.log(JSON.stringify(result));
    if (process.env.GITHUB_OUTPUT) {
      appendFileSync(process.env.GITHUB_OUTPUT,
        `branches=${JSON.stringify(result.prs)}\nrevision=${result.sha || ''}\n`, 'utf8');
    }
  }).catch(error => { console.error(error.message); process.exitCode = 1; });
}
