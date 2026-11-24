#!/usr/bin/env python3
"""Select the macOS CI suites a pull request actually affects.

Usage: affected.py BASE_SHA  (writes rust/node/python/package/toolchain=true|false lines)

Pushes, tags and any failure to compute the diff run everything. Pull requests
run a suite only when its inputs changed. Paths matching no rule are treated as
unknown and also run everything, so new directories fail safe rather than skip.
"""
import fnmatch
import subprocess
import sys

SUITES = ("rust", "node", "python", "package")

# Shared build/CI inputs: any change re-validates every suite and the bundle.
FULL = (
    "Cargo.toml", "Cargo.lock", "rust-toolchain*", ".cargo/*",
    ".github/workflows/macos.yml", "scripts/ci/*", "packaging/*",
)
# Documentation and other workflows' sources: nothing to run on macOS.
NONE = (
    "*.md", "docs/*", "mobile/*", "daemon/*", ".github/workflows/android.yml",
    ".gitignore", ".gitattributes", "LICENSE*", "dist/*",
)
RULES = {
    "rust": ("src/*", "vendor/*", "tests/*.rs", "build.rs", "assets/*", "scripts/test-rust.sh"),
    "node": ("scripts/*.ts", "scripts/*.mjs", "scripts/*.cjs", "scripts/*.swift", "tests/*.mjs", "tests/fixtures/*"),
    "python": ("scripts/*.py", "scripts/*.sh", "tests/*.py", "tests/fixtures/*", "CHANGELOG.md"),
    # Bundle/package/smoke only when the packaging pipeline itself changed.
    "package": ("scripts/bundle-*", "scripts/package-*", "scripts/smoke-*", "scripts/sign-*",
                "scripts/notarize-*", "scripts/fetch-chromium-*", "scripts/release.py", "assets/*"),
}


def matches(path, patterns):
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def select(paths):
    selected = dict.fromkeys(SUITES, False)
    for path in paths:
        if matches(path, FULL):
            return dict.fromkeys(SUITES, True)
        hit = False
        for suite, patterns in RULES.items():
            if matches(path, patterns):
                selected[suite] = hit = True
        if not hit and not matches(path, NONE):
            return dict.fromkeys(SUITES, True)
    return selected


def changed_files(base):
    output = subprocess.check_output(["git", "diff", "--name-only", f"{base}...HEAD"], text=True)
    return [line for line in output.splitlines() if line]


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else ""
    try:
        selected = select(changed_files(base)) if base else dict.fromkeys(SUITES, True)
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"Could not diff against {base!r} ({error}); running everything.", file=sys.stderr)
        selected = dict.fromkeys(SUITES, True)
    # Rust and the bundle need the toolchain, pinned CEF and compiled caches.
    selected["toolchain"] = selected["rust"] or selected["package"]
    for suite, value in selected.items():
        print(f"{suite}={'true' if value else 'false'}")


if __name__ == "__main__":
    main()
