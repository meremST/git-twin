#!/usr/bin/env python3
"""Build the native Linux x86_64 release archive and its SHA-256 checksum."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-gnu"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Require a tag matching the Cargo version (vX.Y.Z)")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("build on Linux x86_64; cross-compilation is not supported")

    os.chdir(ROOT)
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1",
        "--filter-platform", TARGET,
    ]))
    packages = {p["id"]: p for p in metadata["packages"]}
    root_id = metadata["resolve"]["root"]
    version = packages[root_id]["version"]
    if args.tag is not None and args.tag != f"v{version}":
        parser.error(f"tag {args.tag!r} does not match Cargo version v{version}")

    build_output = subprocess.check_output([
        "cargo", "build", "--locked", "--release", "--bins", "--target", TARGET,
        "--message-format=json-render-diagnostics",
    ], text=True)
    target_dir = Path(metadata["target_directory"])
    binaries = target_dir / TARGET / "release"
    for name, display_name in (("gitt", "gitt"), ("git-tui", "git tui")):
        actual = subprocess.check_output([binaries / name, "--version"], text=True).strip()
        if actual != f"{display_name} {version}":
            raise RuntimeError(f"unexpected binary version: {actual}")

    # Compiler artifacts exclude optional backends and dev-only dependencies.
    messages = [json.loads(line) for line in build_output.splitlines()]
    included = {
        m["package_id"] for m in messages if m["reason"] == "compiler-artifact"
    }
    if root_id not in included:
        raise RuntimeError("Cargo did not report artifacts for git-twin")

    dist = target_dir / "dist"
    dist.mkdir(exist_ok=True)
    name = f"git-twin-{version}-{TARGET}"
    with tempfile.TemporaryDirectory(prefix="git-twin-package-", dir=target_dir) as work:
        bundle = Path(work) / name
        bundle.mkdir()
        for binary in ("gitt", "git-tui"):
            shutil.copy2(binaries / binary, bundle / binary)
        for document in ("README.md", "LICENSE", "CHANGELOG.md", "design.md", "status.md"):
            shutil.copy2(ROOT / document, bundle / document)
        shutil.copytree(ROOT / "docs", bundle / "docs")
        (bundle / "scripts").mkdir()
        shutil.copy2(ROOT / "scripts" / "uninstall.sh", bundle / "scripts" / "uninstall.sh")

        licenses = bundle / "licenses"
        licenses.mkdir()
        index = []
        for package_id in sorted(included - {root_id}):
            package = packages[package_id]
            source = Path(package["manifest_path"]).parent
            files = {
                p for p in source.iterdir()
                if p.name.lower().startswith(("license", "licence", "copying", "copyright", "notice"))
            }
            if package.get("license_file"):
                files.add(source / package["license_file"])
            if not files:
                raise RuntimeError(f"missing license files for {package_id}")
            destination = licenses / f'{package["name"]}-{package["version"]}'
            destination.mkdir()
            for path in sorted(files):
                if path.is_dir():
                    shutil.copytree(path, destination / path.name)
                else:
                    shutil.copy2(path, destination / path.name)
            index.append(f'{destination.name}: {package["license"] or "see license files"}')
        (licenses / "INDEX.txt").write_text("\n".join(index) + "\n", encoding="utf-8")

        archive = dist / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            output.add(bundle, arcname=name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = dist / f"{archive.name}.sha256"
    checksum.write_text(f"{digest}  {archive.name}\n", encoding="ascii")
    print(archive)
    print(checksum)


if __name__ == "__main__":
    main()
