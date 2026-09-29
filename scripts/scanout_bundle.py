#!/usr/bin/env python3
"""Validate and stage exact-build scanout profiles without executing bundle code."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import zipfile

MAGIC = 'ZAPAROO-SCANOUT-PROFILE-1'
CONTRACT = 'zaparoo-scanout-v1-1080p'


def parse_profile(data):
    lines = data.decode('ascii').split('\n')
    if len(lines) != 8 or lines[0] != MAGIC or lines[-1] or lines[6] != CONTRACT:
        raise ValueError('unsupported scanout profile')
    release, kernel_id, module_id, digest, revision = lines[1:6]
    if not re.fullmatch(r'[A-Za-z0-9._-]{1,64}', release) or release in ('.', '..'):
        raise ValueError('invalid kernel release')
    for value in (kernel_id, module_id):
        if not re.fullmatch(r'(?:[0-9a-f]{2}){16,64}', value):
            raise ValueError('invalid build ID')
    if not re.fullmatch(r'[0-9a-f]{64}', digest) or not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('invalid profile digest or revision')
    return release, kernel_id, module_id, digest, revision


def module_identity(data):
    if len(data) < 52 or data[:7] != b'\x7fELF\x01\x01\x01':
        raise ValueError('expected little-endian ELF32 module')
    kind, machine = struct.unpack_from('<HH', data, 16)
    if kind != 1 or machine != 40:
        raise ValueError('expected relocatable ARM module')
    offset = struct.unpack_from('<I', data, 32)[0]
    stride, count, names_index = struct.unpack_from('<HHH', data, 46)
    if stride != 40 or not count or names_index >= count or offset + count * stride > len(data):
        raise ValueError('invalid ELF section table')
    def section(index):
        fields = struct.unpack_from('<10I', data, offset + index * stride)
        start, size = fields[4:6]
        if start + size > len(data):
            raise ValueError('invalid ELF section extent')
        return fields[0], data[start:start + size]
    names = section(names_index)[1]
    sections = {}
    for index in range(count):
        name_index, payload = section(index)
        end = names.find(b'\0', name_index)
        if end < 0:
            raise ValueError('invalid ELF section name')
        name = names[name_index:end]
        if name in (b'.note.gnu.build-id', b'.modinfo'):
            if name in sections:
                raise ValueError('duplicate ELF identity section')
            sections[name] = payload
    note = sections.get(b'.note.gnu.build-id', b'')
    if len(note) < 16:
        raise ValueError('missing module build ID')
    namesize, size, kind = struct.unpack_from('<III', note)
    if namesize != 4 or kind != 3 or note[12:16] != b'GNU\0' or not 16 <= size <= 64 or len(note) != 16 + ((size + 3) & ~3):
        raise ValueError('invalid module build ID')
    fields = sections.get(b'.modinfo', b'').split(b'\0')
    def info(key):
        values = [field[len(key) + 1:] for field in fields if field.startswith(key + b'=')]
        if len(values) != 1:
            raise ValueError('missing or duplicate module metadata')
        return values[0].decode('ascii')
    return note[16:16 + size].hex(), info(b'name'), info(b'vermagic').strip(), info(b'kernel_revision')


def validated_files(bundle):
    with zipfile.ZipFile(bundle) as archive:
        infos = archive.infolist()
        if not infos or len(infos) > 256 or sum(i.file_size for i in infos) > 32 * 1024 * 1024:
            raise ValueError('scanout bundle exceeds limits')
        files = {}
        for info in infos:
            path = PurePosixPath(info.filename)
            mode = info.external_attr >> 16
            if (str(path) != info.filename or path.is_absolute() or '..' in path.parts or
                    len(path.parts) < 4 or path.parts[0] != 'modules' or
                    info.is_dir() or stat.S_ISLNK(mode) or
                    (stat.S_IFMT(mode) not in (0, stat.S_IFREG)) or info.filename in files):
                raise ValueError('unsafe or duplicate scanout bundle path')
            files[info.filename] = archive.read(info)
    profiles = [name for name in files if len(PurePosixPath(name).parts) == 4 and name.endswith('/profile')]
    if not profiles:
        raise ValueError('scanout bundle has no profiles')
    claimed = set()
    for name in profiles:
        release, kernel_id, module_id, digest, revision = parse_profile(files[name])
        prefix = f'modules/{release}/{kernel_id}/'
        if name != prefix + 'profile':
            raise ValueError('profile directory disagrees with identity')
        module = files[prefix + 'zaparoo_scanout.ko']
        if hashlib.sha256(module).hexdigest() != digest:
            raise ValueError('module SHA-256 mismatch')
        if module_identity(module) != (module_id, 'zaparoo_scanout', release + ' SMP mod_unload ARMv7 p2v8', revision):
            raise ValueError('module metadata disagrees with profile')
        provenance = json.loads(files[prefix + 'provenance.json'])
        if any(provenance.get(key) != value for key, value in {
            'schema': 1, 'kernel_revision': revision, 'kernel_build_id': kernel_id,
            'module_build_id': module_id, 'module_sha256': digest,
        }.items()):
            raise ValueError('provenance disagrees with profile')
        for key in ('kernel_config_sha256', 'kernel_symvers_sha256'):
            if not re.fullmatch(r'[0-9a-f]{64}', provenance.get(key, '')):
                raise ValueError('missing kernel build provenance')
        sources = provenance.get('source_sha256', {})
        if not {'zaparoo_scanout.c', 'zaparoo_scanout_platform.h', 'zaparoo_scanout_uapi.h', 'Makefile'} <= sources.keys():
            raise ValueError('missing matching module source')
        allowed = {prefix + p for p in ('profile', 'zaparoo_scanout.ko', 'provenance.json', 'source/README.md')}
        for source, expected in sources.items():
            if PurePosixPath(source).name != source:
                raise ValueError('invalid source path')
            source_path = prefix + 'source/' + source
            if hashlib.sha256(files[source_path]).hexdigest() != expected:
                raise ValueError('source checksum mismatch')
            allowed.add(source_path)
        if prefix + 'source/README.md' not in files:
            raise ValueError('missing source attribution and qualification notes')
        claimed.update(allowed)
    if set(files) != claimed:
        raise ValueError('unexpected files in scanout bundle')
    return files


def stage(bundle, destination):
    files = validated_files(bundle)
    # Validate the entire archive before writing any member. The caller supplies
    # a new staging directory, never a live installation.
    for name in files:
        target = destination / name
        if target.exists() or any(p.is_symlink() for p in (target, *target.parents)):
            raise ValueError('refusing to overwrite a profile or follow a staging symlink')
    for name, data in files.items():
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def merge(bundles, output):
    files = {}
    for source in bundles:
        incoming = validated_files(source)
        if files.keys() & incoming.keys():
            raise ValueError('duplicate kernel profile across scanout bundles')
        files.update(incoming)
    if not files:
        raise ValueError('no scanout bundles supplied')
    if len(files) > 256 or sum(map(len, files.values())) > 32 * 1024 * 1024:
        raise ValueError('merged scanout bundle exceeds limits')
    with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name, data in sorted(files.items()):
            archive.writestr(name, data)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path, nargs='+')
    action = parser.add_mutually_exclusive_group()
    action.add_argument('--destination', type=Path)
    action.add_argument('--merge', type=Path)
    args = parser.parse_args()
    if args.merge:
        merge(args.bundle, args.merge)
    elif args.destination:
        if len(args.bundle) != 1:
            parser.error('merge multiple bundles before staging')
        stage(args.bundle[0], args.destination)
    else:
        for source in args.bundle:
            validated_files(source)
    print('Scanout bundle validated')
