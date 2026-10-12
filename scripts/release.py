"""Validate release identity, package native builds, and publish verified assets."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-pc-windows-msvc": "windows-x64.zip",
    "x86_64-unknown-linux-gnu": "linux-x64.tar.gz",
    "aarch64-apple-darwin": "macos-arm64.tar.gz",
    "x86_64-apple-darwin": "macos-x64.tar.gz",
}
STABLE_VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")


def run(*args: str) -> str:
    """Fail on command errors; never include credentials in command arguments."""
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def version() -> str:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    value = manifest["package"]["version"]
    if not STABLE_VERSION.fullmatch(value):
        raise ValueError("Cargo.toml must contain a stable X.Y.Z release version")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    packages = [p for p in lock["package"] if p["name"] == "relix" and "source" not in p]
    if len(packages) != 1 or packages[0]["version"] != value:
        raise ValueError("Cargo.lock's Relix version must match Cargo.toml")
    notes = ROOT / "releases" / f"v{value}.md"
    if not notes.is_file() or not notes.read_text(encoding="utf-8").strip():
        raise ValueError(f"Add nonempty release notes at releases/v{value}.md")
    return value


def validate_version() -> None:
    value = version()
    current = tuple(map(int, value.split(".")))
    source = run("git", "rev-parse", "HEAD")
    for tag in run("git", "tag", "--list", "v*").splitlines():
        if not STABLE_VERSION.fullmatch(tag[1:]):
            continue
        previous = tuple(map(int, tag[1:].split(".")))
        if previous > current:
            raise ValueError(f"Version {value} must be newer than {tag}")
        if previous == current and run("git", "rev-parse", f"{tag}^{{commit}}") != source:
            raise ValueError(f"{tag} already belongs to another commit; bump the package version")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"version={value}\n")
    print(f"Validated Relix {value}")


def windows_runtime(destination: Path) -> None:
    """Redistribute only CRT DLLs from Visual Studio's licensed redist tree."""
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = Path(run(str(vswhere), "-latest", "-products", "*", "-requires",
                            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                            "-property", "installationPath"))
    candidates = list(installation.glob("VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT"))
    if not candidates:
        raise ValueError("Visual Studio x64 redistributable CRT directory was not found")
    runtime = max(candidates, key=lambda p: tuple(map(int, p.parents[1].name.split("."))))
    dlls = list(runtime.glob("*.dll"))
    if not dlls or not (runtime / "vcruntime140.dll").is_file():
        raise ValueError("Visual Studio x64 CRT runtime is incomplete")
    for dll in dlls:
        shutil.copy2(dll, destination / dll.name)


def package(target: str) -> None:
    value = version()
    binary_name = "relix.exe" if "windows" in target else "relix"
    binary = ROOT / "target" / target / "release" / binary_name
    if not binary.is_file():
        raise ValueError(f"Build the {target} release executable before packaging")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    stem = f"Relix-{value}-{TARGETS[target]}"
    with tempfile.TemporaryDirectory(prefix="relix-package-") as temporary:
        staging = Path(temporary) / "Relix"
        staging.mkdir()
        for source, name in [(ROOT / "docs/releases.md", "RELEASE-PROCESS.md"),
                             (ROOT / "releases" / f"v{value}.md", "RELEASE-NOTES.md"),
                             (ROOT / "README.md", "README.md")]:
            shutil.copy2(source, staging / name)
        if "apple" in target:
            app = staging / "Relix.app"
            executable = app / "Contents/MacOS/relix"
            executable.parent.mkdir(parents=True)
            shutil.copy2(binary, executable)
            executable.chmod(0o755)
            with (app / "Contents/Info.plist").open("wb") as stream:
                plistlib.dump({
                    "CFBundleName": "Relix", "CFBundleDisplayName": "Relix",
                    "CFBundleIdentifier": "io.github.zain-repo.relix",
                    "CFBundleExecutable": "relix", "CFBundlePackageType": "APPL",
                    "CFBundleShortVersionString": value, "CFBundleVersion": value,
                    "LSMinimumSystemVersion": "15.0", "NSHighResolutionCapable": True,
                }, stream)
            dependencies = run("otool", "-L", str(executable)).splitlines()[1:]
            if any(not line.strip().startswith(("/usr/lib/", "/System/Library/"))
                   for line in dependencies):
                raise ValueError("macOS build has unbundled non-system dynamic dependencies")
            run("codesign", "--force", "--sign", "-", str(app))
            run("codesign", "--verify", "--strict", str(app))
        else:
            executable = staging / binary_name
            shutil.copy2(binary, executable)
            if "windows" in target:
                windows_runtime(staging)
            else:
                executable.chmod(0o755)
                if "not found" in run("ldd", str(executable)):
                    raise ValueError("Linux executable has unresolved runtime libraries")
        if run(str(executable), "--version") != f"Relix {value}":
            raise ValueError("Packaged executable reports an unexpected version")
        archive = dist / stem
        if "windows" in target:
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
                for path in sorted(staging.rglob("*")):
                    if path.is_file():
                        output.write(path, path.relative_to(staging.parent))
        else:
            with tarfile.open(archive, "w:gz") as output:
                output.add(staging, arcname="Relix")
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        (dist / f"{stem}.sha256").write_text(f"{digest}  {stem}\n", encoding="utf-8")
        print(f"Packaged and smoke-checked {stem}")


def publish() -> None:
    validate_version()
    value = version()
    repository = os.environ["GITHUB_REPOSITORY"]
    source = run("git", "rev-parse", "HEAD")
    if source != os.environ["GITHUB_SHA"]:
        raise ValueError("Release checkout does not match the tested workflow commit")
    tag = f"v{value}"
    dist = ROOT / "dist"
    sums, assets = [], []
    for suffix in TARGETS.values():
        name = f"Relix-{value}-{suffix}"
        archive = dist / name
        expected = (dist / f"{name}.sha256").read_text(encoding="utf-8").strip()
        with archive.open("rb") as stream:
            actual = f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  {name}"
        if actual != expected:
            raise ValueError(f"Checksum mismatch for {name}")
        sums.append(actual)
        assets.append(str(archive))
    combined = dist / "SHA256SUMS.txt"
    combined.write_text("\n".join(sums) + "\n", encoding="utf-8")
    assets.append(str(combined))
    # API errors remain failures; they must not be treated as a missing release.
    pages = json.loads(run("gh", "api", "--paginate", "--slurp",
                          f"repos/{repository}/releases?per_page=100"))
    existing = next((item for page in pages for item in page if item["tag_name"] == tag), None)
    if existing:
        tagged = json.loads(run("gh", "api", f"repos/{repository}/commits/{tag}"))["sha"]
        if tagged != source:
            raise ValueError("Existing release tag does not point to the tested commit")
        if not existing["draft"]:
            print(f"{tag} is already published at this commit; leaving it unchanged")
            return
    else:
        # Create an immutable tag explicitly so a draft can be retried before publication.
        if not run("git", "tag", "--list", tag):
            run("gh", "api", "--method", "POST", f"repos/{repository}/git/refs",
                "-f", f"ref=refs/tags/{tag}", "-f", f"sha={source}")
        run("gh", "release", "create", tag, "--repo", repository, "--target", source,
            "--title", f"Relix {value}", "--notes-file",
            str(ROOT / "releases" / f"{tag}.md"), "--verify-tag", "--draft")
    run("gh", "release", "upload", tag, *assets, "--repo", repository, "--clobber")
    uploaded = json.loads(run("gh", "api", f"repos/{repository}/releases/tags/{tag}"))
    remote_assets = {item["name"]: item for item in uploaded["assets"]}
    for asset in assets:
        path = Path(asset)
        with path.open("rb") as stream:
            digest = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
        remote = remote_assets.get(path.name)
        if not remote or remote["size"] != path.stat().st_size or remote.get("digest") != digest:
            raise ValueError(f"Uploaded asset verification failed for {path.name}; draft retained")
    run("gh", "release", "edit", tag, "--repo", repository, "--draft=false", "--latest")
    print(f"Published Relix {value} at {source}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("version")
    packaging = commands.add_parser("package")
    packaging.add_argument("--target", required=True, choices=TARGETS)
    commands.add_parser("publish")
    arguments = parser.parse_args()
    try:
        if arguments.command == "version":
            validate_version()
        elif arguments.command == "package":
            package(arguments.target)
        else:
            publish()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
