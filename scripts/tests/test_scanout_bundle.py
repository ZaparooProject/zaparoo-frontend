import hashlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('scanout_bundle', Path(__file__).parents[1] / 'scanout_bundle.py')
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


def module_elf():
    names = b'\0.shstrtab\0.note.gnu.build-id\0.modinfo\0'
    note = struct.pack('<III', 4, 20, 3) + b'GNU\0' + bytes.fromhex('12' * 20)
    info = b'name=zaparoo_scanout\0vermagic=6.18.38-MiSTer SMP mod_unload ARMv7 p2v8 \0kernel_revision=' + b'a' * 40 + b'\0'
    header = bytearray(52)
    header[:7] = b'\x7fELF\x01\x01\x01'
    struct.pack_into('<HH', header, 16, 1, 40)
    struct.pack_into('<I', header, 32, 52)
    struct.pack_into('<HHH', header, 46, 40, 4, 1)
    table = bytearray(160)
    start = 212
    for index, (name, data) in enumerate([(1, names), (11, note), (30, info)], 1):
        struct.pack_into('<10I', table, 40 * index, name, 1, 0, 0, start, len(data), 0, 0, 1, 0)
        start += len(data)
    # Locate names programmatically to keep the fixture independent of spelling lengths.
    struct.pack_into('<I', table, 80, names.index(b'.note.gnu.build-id'))
    struct.pack_into('<I', table, 120, names.index(b'.modinfo'))
    return bytes(header + table) + names + note + info


def members(kernel_id='34' * 20):
    module = module_elf()
    digest = hashlib.sha256(module).hexdigest()
    prefix = f'modules/6.18.38-MiSTer/{kernel_id}/'
    sources = {name: b'source' for name in ('zaparoo_scanout.c', 'zaparoo_scanout_platform.h', 'zaparoo_scanout_uapi.h', 'Makefile')}
    provenance = dict(schema=1, kernel_revision='a' * 40, kernel_build_id=kernel_id,
                      module_build_id='12' * 20, module_sha256=digest,
                      kernel_config_sha256='b' * 64, kernel_symvers_sha256='c' * 64,
                      source_sha256={k: hashlib.sha256(v).hexdigest() for k, v in sources.items()})
    return {
        prefix + 'profile': ('\n'.join([bundle.MAGIC, '6.18.38-MiSTer', kernel_id,
                                       '12' * 20, digest, 'a' * 40, bundle.CONTRACT, ''])).encode(),
        prefix + 'zaparoo_scanout.ko': module,
        prefix + 'provenance.json': json.dumps(provenance).encode(),
        prefix + 'source/README.md': b'Attribution and qualification notes',
        **{prefix + 'source/' + k: v for k, v in sources.items()},
    }


def archive(files):
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w') as output:
        for name, content in files.items():
            output.writestr(name, content)
    data.seek(0)
    return data


class BundleTests(unittest.TestCase):
    def test_same_release_different_kernel_builds_coexist(self):
        files = members() | members('56' * 20)
        self.assertEqual(bundle.validated_files(archive(files)), files)

    def test_merge_keeps_distinct_builds_and_rejects_duplicates(self):
        output = io.BytesIO()
        bundle.merge([archive(members()), archive(members('56' * 20))], output)
        output.seek(0)
        self.assertEqual(bundle.validated_files(output), members() | members('56' * 20))
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            bundle.merge([archive(members()), archive(members())], io.BytesIO())

    def test_hash_and_loaded_identity_are_independent(self):
        files = members()
        profile = next(k for k in files if k.endswith('/profile'))
        files[profile] = files[profile].replace(b'12' * 20, b'78' * 20)
        with self.assertRaisesRegex(ValueError, 'metadata'):
            bundle.validated_files(archive(files))

    def test_corrupt_module_is_rejected_before_any_staging(self):
        files = members()
        module = next(k for k in files if k.endswith('.ko'))
        files[module] += b'corrupted'
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, 'SHA-256'):
                bundle.stage(archive(files), Path(directory))
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_rejects_path_traversal_and_unclaimed_files(self):
        for name in ('../escape', 'modules/../../escape', '/absolute', 'modules/a/b/extra'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                bundle.validated_files(archive(members() | {name: b'bad'}))

    def test_rejects_symlink(self):
        stream = archive(members())
        with zipfile.ZipFile(stream, 'a') as output:
            entry = zipfile.ZipInfo('modules/a/b/link')
            entry.external_attr = 0o120777 << 16
            output.writestr(entry, '/tmp')
        stream.seek(0)
        with self.assertRaises(ValueError):
            bundle.validated_files(stream)

    def test_profile_must_agree_with_directory(self):
        files = {k.replace('34' * 20, '56' * 20): v for k, v in members().items()}
        with self.assertRaisesRegex(ValueError, 'directory'):
            bundle.validated_files(archive(files))

    def test_malformed_profiles(self):
        valid = next(v for k, v in members().items() if k.endswith('/profile'))
        for data in (valid + b'extra\n', valid[:-1], valid.replace(b'6.18.38-MiSTer', b'..'),
                     valid.replace(b'34' * 20, b'34' * 19 + b'GG'),
                     valid.replace(bundle.CONTRACT.encode(), b'magik')):
            with self.subTest(data=data), self.assertRaises(ValueError):
                bundle.parse_profile(data)

    def test_provenance_and_source_tampering(self):
        for suffix in ('source/zaparoo_scanout.c', 'provenance.json'):
            files = members()
            path = next(k for k in files if k.endswith(suffix))
            files[path] = b'{}'
            with self.assertRaises(ValueError):
                bundle.validated_files(archive(files))

    def test_staging_preserves_other_profiles_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            bundle.stage(archive(members()), destination)
            bundle.stage(archive(members('56' * 20)), destination)
            with self.assertRaises(ValueError):
                bundle.stage(archive(members()), destination)

    def test_truncated_or_wrong_architecture_elf(self):
        valid = module_elf()
        for data in (valid[:51], valid[:160], valid.replace(b'\x28\x00', b'\x3e\x00', 1)):
            with self.assertRaises(ValueError):
                bundle.module_identity(data)


class PackagingTests(unittest.TestCase):
    def test_release_with_and_without_scanout_and_invalid_bundle(self):
        scripts = Path(__file__).resolve().parents[1]
        for mode in ('baseline', 'local', 'download', 'corrupt'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / 'scripts').mkdir()
                for name in ('package-mister-release.sh', 'scanout_bundle.py'):
                    shutil.copyfile(scripts / name, root / 'scripts' / name)
                inputs = {
                    'rust/Cargo.toml': '[workspace.package]\nversion = "0.0.1"\n',
                    'rust/target/docker/armv7-unknown-linux-musleabihf/release/frontend': 'frontend',
                    'rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt': 'notices',
                    'LICENSES/notice': 'attribution', 'COPYING': 'license',
                }
                for name, contents in inputs.items():
                    target = root / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(contents)
                bundle_path = root / 'input.zip'
                bundle_path.write_bytes(archive(members()).getvalue() if mode != 'corrupt' else b'broken')
                (root / 'bin').mkdir()
                gh = root / 'bin/gh'
                gh.write_text("""#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv
assert args[1:3] == ['release', 'download']
name = args[args.index('--pattern') + 1]
out = pathlib.Path(args[args.index('--dir') + 1]) / name
if name == 'zaparoo-scanout.zip':
    shutil.copyfile(os.environ['TEST_SCANOUT_BUNDLE'], out)
else:
    out.write_bytes(b'asset')
""")
                gh.chmod(0o755)
                env = dict(os.environ, PATH=str(root / 'bin') + os.pathsep + os.environ['PATH'],
                           ZAPAROO_SKIP_FRONTEND_BUILD='1', MENU_MISTER_TAG='menu-test',
                           MAIN_MISTER_TAG='main-test', ZAPAROO_INCLUDE_SCANOUT='0',
                           ZAPAROO_SCANOUT_BUNDLE='', TEST_SCANOUT_BUNDLE=str(bundle_path))
                if mode in ('local', 'corrupt'):
                    env['ZAPAROO_SCANOUT_BUNDLE'] = str(bundle_path)
                elif mode == 'download':
                    env['ZAPAROO_INCLUDE_SCANOUT'] = '1'
                result = subprocess.run(['bash', str(root / 'scripts/package-mister-release.sh'), 'v0.0.1'],
                                        env=env, capture_output=True, text=True)
                output = root / 'output/release/zaparoo-frontend-v0.0.1.zip'
                if mode == 'corrupt':
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(output.exists())
                    continue
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                with zipfile.ZipFile(output) as release:
                    modules = [n for n in release.namelist() if n.endswith('.ko')]
                    self.assertEqual(len(modules), 0 if mode == 'baseline' else 1)


if __name__ == '__main__':
    unittest.main()
