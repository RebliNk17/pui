#!/usr/bin/env python3
"""Select the CI suites a change actually affects.

Usage: affected.py BASE_SHA   pull requests (PR base) and master pushes (previous head)
       affected.py --release  release tags / manual builds: every suite plus the app bundle

The app bundle is built only with --release. Otherwise a suite runs only when
its inputs changed. A missing/unknown base, a failed diff, shared build inputs or
paths matching no rule run every test suite, so new directories fail safe.
"""
import fnmatch
import subprocess
import sys

SUITES = ("rust", "node", "python", "mobile", "daemon")

# Shared build/CI inputs: any change re-validates every suite and the bundle.
FULL = (
    "Cargo.toml", "Cargo.lock", "rust-toolchain*", ".cargo/*",
    ".github/workflows/macos.yml", "scripts/ci/*", "packaging/*",
)
# Documentation and other workflows' sources: nothing to run on macOS.
NONE = (
    "*.md", "docs/*", ".github/workflows/android.yml", ".github/dependabot.yml",
    ".gitignore", ".gitattributes", "LICENSE*", "dist/*",
)
RULES = {
    "rust": ("src/*", "vendor/*", "tests/*.rs", "build.rs", "assets/*", "scripts/test-rust.sh"),
    "node": ("scripts/*.ts", "scripts/*.mjs", "scripts/*.cjs", "scripts/*.swift", "tests/*.mjs", "tests/fixtures/*"),
    # Packaging scripts are covered by their unit tests; the bundle itself is built on release.
    "python": ("scripts/*.py", "scripts/*.sh", "tests/*.py", "tests/fixtures/*", "CHANGELOG.md"),
    # Validated on Linux by their own jobs; never wake the macOS runner.
    "mobile": ("mobile/*",),
    "daemon": ("daemon/*",),
}


def matches(path, patterns):
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def derive(selected, package=False):
    selected["package"] = package
    # Rust and the bundle need the toolchain, pinned CEF and compiled caches.
    selected["toolchain"] = selected["rust"] or package
    # Docs/mobile/daemon-only changes skip the macOS job entirely.
    selected["macos"] = any(selected[suite] for suite in ("rust", "node", "python")) or package
    return selected


def everything(package=False):
    return derive(dict.fromkeys(SUITES, True), package)


def select(paths):
    selected = dict.fromkeys(SUITES, False)
    for path in paths:
        if matches(path, FULL):
            return everything()
        hit = False
        for suite, patterns in RULES.items():
            if matches(path, patterns):
                selected[suite] = hit = True
        if not hit and not matches(path, NONE):
            return everything()
    return derive(selected)


def changed_files(base):
    output = subprocess.check_output(["git", "diff", "--name-only", f"{base}...HEAD"], text=True)
    return [line for line in output.splitlines() if line]


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else ""
    if base == "--release":
        selected = everything(package=True)
    elif not base or set(base) == {"0"}:  # new branch push: no previous head
        selected = everything()
    else:
        try:
            selected = select(changed_files(base))
        except (OSError, subprocess.CalledProcessError) as error:
            print(f"Could not diff against {base!r} ({error}); running every suite.", file=sys.stderr)
            selected = everything()
    for suite, value in selected.items():
        print(f"{suite}={'true' if value else 'false'}")


if __name__ == "__main__":
    main()
