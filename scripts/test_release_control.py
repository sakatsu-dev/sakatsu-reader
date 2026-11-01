import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


release = module('release-control')
runtime = module('prepare-webview')


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.files = {
            'Cargo.toml': '[workspace]\nmembers=["core","desktop"]\n[workspace.package]\nversion = "1.2.3"\nlicense="GPL-3.0-only"\n',
            'core/Cargo.toml': '[package]\nname="reader-core"\nversion.workspace=true\n',
            'desktop/Cargo.toml': '[package]\nname="reader-desktop"\nversion.workspace=true\n',
            'Cargo.lock': 'version=4\n[[package]]\nname="reader-core"\nversion = "1.2.3"\n[[package]]\nname="reader-desktop"\nversion = "1.2.3"\n[[package]]\nname="unrelated"\nversion = "1.2.3"\nsource="registry+https://example.org"\n',
        }
        for name in release.VERSION_JSON:
            self.files[name] = json.dumps({'version': '1.2.3', 'other': ['unchanged']})
        self.files['ui/package-lock.json'] = json.dumps({'version': '1.2.3', 'packages': {'': {'version': '1.2.3'}, 'node_modules/example': {'version': '1.2.3'}}})
        for path, value in self.files.items():
            file = self.root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(value, encoding='utf-8')

    def test_plan_reconciles_every_version_without_writing(self):
        plan = release.bump_plan(self.root, '1.2.4')
        updates = {u['path']: u['content'] for u in plan['updates']}
        self.assertEqual(len(updates), 6)
        self.assertEqual(release.state(lambda p: updates.get(p, self.files[p]))[0], '1.2.4')
        self.assertIn('name="unrelated"\nversion = "1.2.3"', updates['Cargo.lock'])
        self.assertEqual(json.loads(updates['ui/package-lock.json'])['packages']['node_modules/example']['version'], '1.2.3')
        for path, value in self.files.items():
            self.assertEqual((self.root / path).read_text(encoding='utf-8'), value)

    def test_refuses_stale_lock_and_member_override(self):
        for name in ['core/Cargo.toml', 'ui/package-lock.json', 'Cargo.lock', 'src-tauri/tauri.conf.json']:
            with self.subTest(name=name):
                file = self.root / name
                bad = self.files[name].replace('workspace=true', 'workspace=false').replace('1.2.3', '1.2.2')
                file.write_text(bad, encoding='utf-8')
                with self.assertRaises(ValueError):
                    release.bump_plan(self.root, '1.2.4')
                file.write_text(self.files[name], encoding='utf-8')

    def test_refuses_equal_older_or_nonstable_version(self):
        for target in ['1.2.3', '1.1.9', '1.3.0-beta', '01.3.0']:
            with self.subTest(target=target), self.assertRaises(ValueError):
                release.bump_plan(self.root, target)


class CiTests(unittest.TestCase):
    def setUp(self):
        self.sha = 'a' * 40
        self.repo = 'example/reader'
        self.run = dict(head_sha=self.sha, head_branch='main', head_repository={'full_name': self.repo},
                        event='push', path='.github/workflows/ci.yml', id=100, run_attempt=2,
                        status='completed', conclusion='success')
        self.jobs = [dict(name=n, status='completed', conclusion='success', run_id=100, run_attempt=2) for n in release.CI_JOBS]

    def test_all_required_jobs_on_exact_commit_and_attempt(self):
        self.assertTrue(release.check_ci(self.run, self.jobs, self.sha, self.repo))
        pending = dict(self.run, status='in_progress', conclusion=None)
        self.assertFalse(release.check_ci(pending, [], self.sha, self.repo))

    def test_refuses_wrong_identity_and_any_unsuccessful_job(self):
        changes = dict(head_sha='b'*40, head_branch='feature', head_repository={'full_name': 'other/reader'},
                       event='pull_request', path='.github/workflows/other.yml', conclusion='failure')
        for key, value in changes.items():
            with self.subTest(key=key), self.assertRaises(ValueError):
                release.check_ci(dict(self.run, **{key: value}), self.jobs, self.sha, self.repo)
        for change in [dict(conclusion='skipped'), dict(conclusion='cancelled'), dict(run_attempt=1), dict(run_id=99)]:
            jobs = copy.deepcopy(self.jobs)
            jobs[0].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.check_ci(self.run, jobs, self.sha, self.repo)
        with self.assertRaises(ValueError):
            release.check_ci(self.run, self.jobs[:-1], self.sha, self.repo)

    def test_latest_failure_cannot_use_older_success(self):
        older = dict(self.run, id=99, run_attempt=1)
        newer = dict(self.run, conclusion='failure')
        def api(repo, path):
            return {'workflow_runs': [older, newer]} if 'workflows/' in path else {'jobs': self.jobs}
        with patch.object(release, 'api', side_effect=api), self.assertRaisesRegex(ValueError, 'latest CI run'):
            release.wait_ci(self.repo, self.sha, timeout=0)


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        import hashlib
        self.body = b'verified archive'
        self.sha = hashlib.sha256(self.body).hexdigest()
        self.files = [dict(file='msedgewebview2.exe', bytes=len(self.body), sha256=self.sha)]
        self.spec = dict(version='1.2.3.4', directory='Microsoft.WebView2.FixedVersionRuntime.1.2.3.4.x64',
                         url='https://example.org/runtime.cab', bytes=len(self.body), sha256=self.sha,
                         executable_sha256=self.sha, max_unpacked_bytes=1024)
        self.save()

    def save(self):
        inventory = self.root / 'scripts/webview2-runtime-files.json'
        inventory.write_text(json.dumps(self.files), encoding='utf-8')
        self.spec['files_manifest_sha256'] = runtime.digest(inventory)
        (self.root / 'scripts/webview2-runtime.json').write_text(json.dumps(self.spec), encoding='utf-8')

    def test_inventory_rejects_traversal_case_collisions_and_stale_digest(self):
        runtime.specification(self.root)
        for name in ['../escape', '/absolute', 'C:/file', 'dir\\file']:
            self.files[0]['file'] = name
            self.save()
            with self.subTest(name=name), self.assertRaises(ValueError):
                runtime.specification(self.root)
        self.files[0]['file'] = 'msedgewebview2.exe'
        self.files.append(dict(self.files[0], file='MSEDGEWEBVIEW2.EXE'))
        self.save()
        with self.assertRaises(ValueError):
            runtime.specification(self.root)
        (self.root / 'scripts/webview2-runtime-files.json').write_text('[]')
        with self.assertRaisesRegex(ValueError, 'digest'):
            runtime.specification(self.root)

    def test_verified_existing_runtime_reused_and_corruption_preserved(self):
        destination = self.root / self.spec['directory']
        destination.mkdir()
        exe = destination / 'msedgewebview2.exe'
        exe.write_bytes(self.body)
        self.assertEqual(runtime.prepare(destination, self.root / 'cache', root=self.root), destination)
        exe.write_bytes(b'bad')
        with self.assertRaisesRegex(ValueError, 'left untouched'):
            runtime.prepare(destination, self.root / 'cache', root=self.root)
        self.assertEqual(exe.read_bytes(), b'bad')

    def test_inventory_pin_survives_windows_checkout_line_endings(self):
        import hashlib
        file = self.root / 'scripts/webview2-runtime-files.json'
        lf = (json.dumps(self.files, indent=2) + '\n').encode()
        self.spec['files_manifest_sha256'] = hashlib.sha256(lf).hexdigest()
        (self.root / 'scripts/webview2-runtime.json').write_text(json.dumps(self.spec))
        for data in [lf, lf.replace(b'\n', b'\r\n')]:
            file.write_bytes(data)
            runtime.specification(self.root)

    def test_cache_and_download_are_strict_and_incomplete_files_not_published(self):
        import io
        class Response(io.BytesIO):
            def geturl(self):
                return 'https://example.org/runtime.cab'
        cache = self.root / 'cache'
        with self.assertRaisesRegex(ValueError, 'missing'):
            runtime.archive(self.spec, cache)
        for body in [b'bad', self.body + b'overflow']:
            with self.subTest(body=body), self.assertRaises(ValueError):
                runtime.archive(self.spec, cache, True, lambda *a, **kw: Response(body))
            self.assertEqual(list(cache.iterdir()), [])
        found = runtime.archive(self.spec, cache, True, lambda *a, **kw: Response(self.body))
        self.assertEqual(found.read_bytes(), self.body)
        found.write_bytes(b'bad')
        with self.assertRaisesRegex(ValueError, 'corrupt'):
            runtime.archive(self.spec, cache, True)

    def test_http_redirect_refused_before_following(self):
        with self.assertRaisesRegex(ValueError, 'HTTPS'):
            runtime.HttpsRedirect().redirect_request(None, None, 302, '', {}, 'http://example.org/runtime.cab')


class EvidenceTests(unittest.TestCase):
    def test_ci_inventory_matches_the_actual_workflow(self):
        import re
        workflow = (release.ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8-sig')
        self.assertEqual(set(re.findall(r'^    name: (.+)$', workflow, re.MULTILINE)), release.CI_JOBS)

    def test_source_scan_permits_synthetic_ipc_path_and_refuses_real_paths(self):
        release.audit_source_text(r'C:\Users\someone\ichiran\ichiran-cli.exe')
        with self.assertRaises(ValueError):
            release.audit_source_text('C:' + r'\Users\actual-reader\private.txt')
        with self.assertRaises(ValueError):
            release.audit_source_text('gh' + 'p_' + 'a'*30)

    def test_source_guard_accepts_its_own_negative_test_fixture(self):
        release.audit_source_text(Path(__file__).read_text(encoding='utf-8'))

    def test_archive_guard_names_the_refused_member(self):
        import io
        import zipfile
        for content, refused in [(r'C:\Users\someone\example.txt', False),
                                 ('C:' + r'\Users\actual-reader\private.txt', True)]:
            archive = io.BytesIO()
            with zipfile.ZipFile(archive, 'w') as files:
                files.writestr('fixture.rs', content)
            archive.seek(0)
            if refused:
                with self.assertRaisesRegex(ValueError, 'fixture.rs'):
                    release.audit_source_archive(archive)
            else:
                release.audit_source_archive(archive)

    def test_existing_evidence_is_immutable(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'evidence.json'
            release.output(path, {'passed': True})
            release.output(path, {'passed': True})
            with self.assertRaises(ValueError):
                release.output(path, {'passed': False})
            self.assertTrue(json.loads(path.read_text())['passed'])


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.sha = 'c' * 40
        self.repo = 'example/reader'
        for name in ['SakatsuReader_1.2.3_x64-setup.exe', 'source.zip', 'BUILDING.txt']:
            (self.root / name).write_bytes(name.encode())
        (self.root / 'build-config.json').write_text(json.dumps({'bundle': {'createUpdaterArtifacts': False}}))
        self.info = dict(version='1.2.3', source_revision=self.sha, verification_only=False,
                         installer='SakatsuReader_1.2.3_x64-setup.exe',
                         installer_sha256=release.digest(self.root / 'SakatsuReader_1.2.3_x64-setup.exe'),
                         source_sha256=release.digest(self.root / 'source.zip'),
                         build_config_sha256=release.digest(self.root / 'build-config.json'),
                         authenticode_signed=False, application_updater_configured=False)
        (self.root / 'build-info.json').write_text(json.dumps(self.info))
        proof = dict(passed=True, installer_sha256=self.info['installer_sha256'])
        for name in ['installer-qa.json', 'verification.json']:
            (self.root / name).write_text(json.dumps(proof))
        (self.root / 'release-notes.md').write_text('Private unsigned preview.')
        self.names = [self.info['installer'], 'source.zip', 'build-info.json', 'build-config.json', 'BUILDING.txt', 'SHA256SUMS']
        self.checksums()
        self.draft = dict(id=3, draft=True, tag_name='v1.2.3', target_commitish=self.sha,
                          assets=[], html_url='https://example.org/draft')
        self.releases = []
        self.calls = []
        self.private = True
        for name, replacement in [('api', self.api), ('run', lambda *args, **kw: self.sha),
                                  ('state', lambda read: ('1.2.3', [])), ('wait_ci', lambda *a, **kw: {})]:
            handle = patch.object(release, name, side_effect=replacement)
            handle.start()
            self.addCleanup(handle.stop)
        handle = patch.object(release.subprocess, 'run', side_effect=self.upload)
        handle.start()
        self.addCleanup(handle.stop)

    def checksums(self):
        (self.root / 'SHA256SUMS').write_text(''.join(release.digest(self.root / n) + '  ' + n + '\n' for n in self.names[:-1]))

    def api(self, repo, path, method='GET', payload=None):
        self.assertEqual(repo, self.repo)
        self.calls.append((method, path))
        if path == '':
            return {'private': self.private}
        if method == 'POST':
            self.assertTrue(payload['draft'])
            self.releases.append(self.draft)
            return self.draft
        if method == 'PATCH':
            self.assertEqual(path, 'releases/3')
            self.assertNotIn('draft', payload)
            self.draft.update(payload)
            return self.draft
        if path.startswith('releases?'):
            return self.releases
        self.assertEqual(path, 'releases/3')
        return self.draft

    def upload(self, command, **kwargs):
        self.assertEqual(command[:3], ['gh', 'release', 'upload'])
        path = Path(command[4])
        self.draft['assets'].append(dict(name=path.name, state='uploaded', size=path.stat().st_size,
                                         digest='sha256:' + release.digest(path)))
        self.calls.append(('upload', path.name))

    def test_draft_upload_is_complete_and_identical_retry_writes_nothing(self):
        release.publish_draft(self.repo, self.root)
        self.assertEqual({a['name'] for a in self.draft['assets']}, set(self.names))
        self.calls.clear()
        release.publish_draft(self.repo, self.root)
        self.assertFalse(any(m in {'POST', 'PATCH', 'upload'} for m, p in self.calls))

    def test_published_or_different_source_release_never_modified(self):
        self.releases.append(self.draft)
        for change in [dict(draft=False), dict(target_commitish='d'*40)]:
            old = dict(self.draft)
            self.draft.update(change)
            self.calls.clear()
            with self.assertRaisesRegex(ValueError, 'Refusing to change'):
                release.publish_draft(self.repo, self.root)
            self.assertFalse(any(m in {'POST', 'upload'} for m, p in self.calls))
            self.draft.update(old)

    def test_corrupt_artifact_refused_before_draft_creation(self):
        for name in ['installer-qa.json', 'verification.json', 'source.zip', 'build-config.json', 'SHA256SUMS']:
            path = self.root / name
            old = path.read_bytes()
            path.write_bytes(b'{}')
            self.calls.clear()
            with self.subTest(name=name), self.assertRaises(ValueError):
                release.publish_draft(self.repo, self.root)
            self.assertFalse(any(m in {'POST', 'upload'} for m, p in self.calls))
            path.write_bytes(old)

    def test_public_repository_and_rehearsal_artifact_refused(self):
        self.private = False
        with self.assertRaisesRegex(ValueError, 'private'):
            release.publish_draft(self.repo, self.root)
        self.private = True
        self.info['verification_only'] = True
        (self.root / 'build-info.json').write_text(json.dumps(self.info))
        with self.assertRaisesRegex(ValueError, 'rehearsal'):
            release.publish_draft(self.repo, self.root)

    def test_existing_different_asset_never_overwritten(self):
        self.releases.append(self.draft)
        self.draft['assets'].append(dict(name=self.info['installer'], state='uploaded', size=1, digest='sha256:'+'0'*64))
        with self.assertRaisesRegex(ValueError, 'different bytes'):
            release.publish_draft(self.repo, self.root)
        self.assertFalse(any(m in {'POST', 'upload'} for m, p in self.calls))


if __name__ == '__main__':
    unittest.main()
