#!/usr/bin/env python3
"""Package an already-built native Opterm; no signing or release publishing."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def dependency_notices(destination, host):
    metadata = json.loads(command('cargo', 'metadata', '--locked',
                                  '--format-version', '1', '--filter-platform', host))
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    selected, pending = set(), [metadata['resolve']['root']]
    while pending:
        package = pending.pop()
        if package in selected:
            continue
        selected.add(package)
        pending.extend(dep['pkg'] for dep in nodes[package]['deps'])
    supplemental = json.loads((ROOT / 'assets/licenses/sources.json').read_text())
    inventory = []
    for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
        if package['id'] not in selected or not package['source']:
            continue
        source = Path(package['manifest_path']).parent
        target = destination / 'dependencies' / f"{package['name']}-{package['version']}"
        texts = [file for file in source.iterdir() if file.is_file() and
                 any(word in file.name.lower() for word in ('license', 'licence', 'copying', 'copyright', 'notice'))]
        if package['name'] == 'epaint_default_fonts':
            texts += list((source / 'fonts').glob('*.txt'))
        for file in texts:
            target.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(file, target / file.name)
        if not texts:
            target.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / 'Cargo.toml', target / 'SOURCE-METADATA.toml')
        upstream = []
        for item in supplemental:
            if {'name': package['name'], 'version': package['version']} in item['packages']:
                target.mkdir(parents=True, exist_ok=True)
                for file in (ROOT / item['directory']).iterdir():
                    if file.is_file():
                        shutil.copyfile(file, target / file.name)
                upstream += item['sources']
        inventory.append({'name': package['name'], 'version': package['version'],
                          'license': package.get('license'), 'authors': package.get('authors', []),
                          'repository': package.get('repository'),
                          'source': f"https://crates.io/crates/{package['name']}/{package['version']}",
                          'upstream_license_sources': upstream,
                          'included_texts': sorted(f.name for f in target.iterdir()) if target.exists() else []})
    (destination / 'DEPENDENCIES.json').write_text(json.dumps(inventory, indent=2) + '\n', encoding='utf-8')
    (destination / 'NOTICE.txt').write_text(
        'Opterm includes third-party Rust dependencies and embedded fonts.\n'
        'DEPENDENCIES.json records the host-filtered Cargo resolved build graph, including\n'
        'build/optional dependencies; this conservative inventory is not a linked-code claim.\n'
        'License expressions and source references come from locked crate metadata.\n'
        'Available original legal texts are copied unchanged under dependencies/.\n'
        'Bundled Noto Emoji uses SIL OFL 1.1; its full text is included here.\n'
        'egui fonts include Hack, Ubuntu, Noto Emoji and emoji-icon-font; their original\n'
        'OFL, Ubuntu Font License, Hack and MIT notices accompany epaint_default_fonts.\n', encoding='utf-8')


def archive_tree(root, archive, epoch, windows):
    if windows:
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as output:
            for path in sorted(root.rglob('*')):
                if not path.is_file():
                    continue
                info = zipfile.ZipInfo(str(path.relative_to(root.parent)).replace(os.sep, '/'))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o100755 if path.name in ('opterm', 'opterm.exe') else 0o100644) << 16
                output.writestr(info, path.read_bytes())
    else:
        with archive.open('wb') as file, gzip.GzipFile(filename='', mode='wb', fileobj=file, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode='w') as output:
                for path in sorted([root, *root.rglob('*')]):
                    info = output.gettarinfo(str(path), arcname=str(path.relative_to(root.parent)))
                    info.uid = info.gid = 0
                    info.uname = info.gname = ''
                    info.mtime = epoch
                    info.mode = 0o755 if path.is_dir() or path.name == 'opterm' else 0o644
                    if path.is_file():
                        with path.open('rb') as content:
                            output.addfile(info, content)
                    else:
                        output.addfile(info)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'target/dist')
    args = parser.parse_args()
    system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    if not system:
        parser.error('Package on Linux, macOS, or Windows using its native binary')
    binary = (args.binary or ROOT / 'target/release' / ('opterm.exe' if system == 'windows' else 'opterm')).resolve()
    if not binary.is_file():
        parser.error(f'Release binary missing: {binary}; run cargo build --release --locked first')
    version = json.loads(command('cargo', 'metadata', '--locked', '--offline', '--no-deps', '--format-version', '1'))['packages'][0]['version']
    host = next(line.split(': ', 1)[1] for line in command('rustc', '-vV').splitlines() if line.startswith('host: '))
    arch = host.split('-')[0]
    name = f'opterm-{version}-{system}-{arch}'
    destination = args.output_dir.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    epoch = int(command('git', 'show', '-s', '--format=%ct', 'HEAD'))
    with tempfile.TemporaryDirectory(prefix='package-', dir=destination) as staging:
        root = Path(staging) / name
        root.mkdir()
        if system == 'macos':
            contents = root / 'Opterm.app/Contents'
            executable = contents / 'MacOS/opterm'
            resources = contents / 'Resources'
            executable.parent.mkdir(parents=True)
            resources.mkdir()
            with (contents / 'Info.plist').open('wb') as file:
                plistlib.dump({'CFBundleName': 'Opterm', 'CFBundleDisplayName': 'Opterm',
                              'CFBundleIdentifier': 'dev.opterm.prototype', 'CFBundleExecutable': 'opterm',
                              'CFBundlePackageType': 'APPL', 'CFBundleIconFile': 'opterm.icns', 'CFBundleVersion': version,
                              'CFBundleShortVersionString': version, 'NSHighResolutionCapable': True}, file)
        else:
            executable = root / binary.name
            resources = root
        if system == 'macos':
            shutil.copyfile(ROOT / 'assets/icons/opterm.icns', resources / 'opterm.icns')
        elif system == 'linux':
            shutil.copyfile(ROOT / 'scripts/install_linux.py', root / 'install.py')
            shutil.copyfile(ROOT / 'assets/icons/window-icon.png', root / 'opterm.png')
        elif system == 'windows':
            shutil.copyfile(ROOT / 'scripts/install_windows.ps1', root / 'install.ps1')
        shutil.copyfile(binary, executable)
        executable.chmod(0o755)
        for filename in ('LICENSE-MIT', 'LICENSE-APACHE'):
            shutil.copyfile(ROOT / filename, resources / filename)
        guide = (ROOT / 'docs/user-guide.md').read_text()
        commit = command('git', 'rev-parse', 'HEAD')
        # Keep the runtime guide's local references working without historical design assets.
        for reference in set(re.findall(r'\]\(([^)]+)\)', guide)):
            source = (ROOT / 'docs' / reference).resolve()
            if source.is_relative_to(ROOT / 'docs') and source.is_file():
                if source.suffix == '.md' and source.name != 'delivery-progress.md':
                    shutil.copyfile(source, resources / source.name)
                elif reference.startswith('screenshots/prototype-') and source.suffix == '.png':
                    target = resources / reference
                    target.parent.mkdir(exist_ok=True)
                    shutil.copyfile(source, target)
                else:
                    guide = guide.replace(f']({reference})', f'](https://github.com/PopBot/opterm/blob/{commit}/docs/{reference})')
        (resources / 'USER-GUIDE.md').write_text(guide, encoding='utf-8')
        shutil.copyfile(ROOT / 'docs/distribution.md', resources / 'DISTRIBUTION.md')
        shutil.copyfile(ROOT / 'assets/fonts/NotoEmoji-OFL.txt', resources / 'NotoEmoji-OFL.txt')
        shutil.copyfile(ROOT / 'docs/fonts.md', resources / 'FONTS.md')
        shutil.copyfile(ROOT / 'assets/providers/LICENSE.lobe-icons', resources / 'Lobe-Icons-LICENSE.txt')
        shutil.copyfile(ROOT / 'assets/providers/README.md', resources / 'PROVIDER-LOGOS.md')
        for document in resources.glob('*.md'):
            content = document.read_text(encoding='utf-8')
            for reference in set(re.findall(r'\]\(([^)]+)\)', content)):
                if reference.startswith(('https://', 'http://', '#')) or (resources / reference).exists():
                    continue
                source = (ROOT / 'docs' / reference).resolve()
                if source.is_relative_to(ROOT) and source.is_file():
                    relative = source.relative_to(ROOT).as_posix()
                    content = content.replace(f']({reference})', f'](https://github.com/PopBot/opterm/blob/{commit}/{relative})')
            document.write_text(content, encoding='utf-8')
        dependency_notices(resources, host)
        info = {'name': 'opterm', 'version': version, 'host': host, 'commit': command('git', 'rev-parse', 'HEAD'),
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'working_tree_dirty': bool(command('git', 'status', '--porcelain')), 'unsigned_prototype': True}
        (resources / 'BUILD.json').write_text(json.dumps(info, indent=2) + '\n', encoding='utf-8')
        archive = destination / (name + ('.zip' if system == 'windows' else '.tar.gz'))
        archive_tree(root, archive, epoch, system == 'windows')
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n', encoding='ascii')
    print(archive)


if __name__ == '__main__':
    main()
