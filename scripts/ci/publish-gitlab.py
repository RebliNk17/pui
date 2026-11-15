#!/usr/bin/env python3
"""CircleCI-only GitLab publisher; no build caches, npm, or model providers.

The context must be restricted outside this repository before it holds a token.
Runtime checks cannot protect a credential from arbitrary malicious CI config.
"""
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.error import URLError
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("pui_release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

# Not configurable by pipeline env: do not send the context secret to another
# API host/project or allow fork pipelines to select a different destination.
API = "https://gitlab.com/api/v4"
PROJECT_ID = "88726373"
PROJECT_PATH = "RebliNk17/pui"
CIRCLE_PROJECT_ID = "7a9da67d-bf7f-406f-867e-af29eb280f8e"


def verify(api, root, tag, commit):
    release.check(root, tag)
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Invalid CircleCI revision.")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    if head != commit:
        raise ValueError("Publisher checkout does not match the build revision.")
    endpoint = f"/projects/{PROJECT_ID}"
    project = api.request("GET", endpoint)
    if (str(project.get("id")), project.get("path_with_namespace")) != (PROJECT_ID, PROJECT_PATH):
        raise ValueError("GitLab project identity mismatch.")
    remote = api.request("GET", endpoint + "/repository/tags/" + quote(tag, safe=""))
    if remote.get("name") != tag or remote.get("protected") is not True or remote.get("commit", {}).get("id") != commit:
        raise ValueError("GitLab tag must exist, be protected, and match the built revision.")
    # Refuse tags on unmerged side branches. GitLab's compare uses a merge base
    # by default; an empty diff does not prove ancestry. Query merge-base directly.
    branch = project.get("default_branch")
    if not branch:
        raise ValueError("GitLab default branch is unavailable.")
    default = api.request("GET", endpoint + "/repository/branches/" + quote(branch, safe=""))
    branch_commit = default.get("commit", {}).get("id", "")
    if default.get("name") != branch or not re.fullmatch(r"[0-9a-f]{40}", branch_commit):
        raise ValueError("GitLab default branch revision is unavailable.")
    # Use commit IDs, not an ambiguous branch name that a same-named tag could shadow.
    base = api.request("GET", endpoint + "/repository/merge_base?" +
                       "refs%5B%5D=" + quote(commit, safe="") + "&refs%5B%5D=" + branch_commit)
    if base.get("id") != commit:
        raise ValueError("Release revision must be merged into GitLab's default branch.")


def main(root=ROOT):
    try:
        if os.environ.get("CIRCLECI") != "true" or os.environ.get("CIRCLE_PROJECT_ID") != CIRCLE_PROJECT_ID:
            raise ValueError("Publishing requires the configured CircleCI project.")
        if os.environ.get("CIRCLE_BRANCH") or os.environ.get("CIRCLE_PULL_REQUEST"):
            raise ValueError("Branch and merge-request pipelines cannot publish releases.")
        tag, commit = os.environ["CIRCLE_TAG"], os.environ["CIRCLE_SHA1"]
        if not tag.startswith("v") or not release.STABLE.fullmatch(tag[1:]):
            raise ValueError("Publishing requires a stable vX.Y.Z tag.")
        # The token is injected only into this uncached publisher job, never the
        # macOS builder. Context restrictions also reject branches and SSH reruns.
        api = release.GitLab(API, os.environ["PUI_GITLAB_RELEASE_TOKEN"], "PRIVATE-TOKEN")
        verify(api, root, tag, commit)
        release.publish(root, api=api, project_id=PROJECT_ID, tag=tag, commit=commit)
    except (ValueError, KeyError, OSError, URLError, subprocess.SubprocessError):
        # Server errors/URLs/CLI stderr may contain sensitive information.
        print("GitLab publication failed. Check project/tag protection, default-branch ancestry, credential expiry and artifact checksums; nothing was overwritten.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
