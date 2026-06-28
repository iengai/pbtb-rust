#!/usr/bin/env python3
"""Build + push the passivbot live image to ECR via the free ARM CodeBuild project.

The image is `linux/arm64` (the cluster is t4g/Graviton). We build it on a native
arm64 CodeBuild fleet — NOT locally under QEMU — using the one-time project
`pbtb-passivbot-image-builder` (see deploy/passivbot-image/README.md).

Our build files (Dockerfile.ecs, buildspec.yml, entrypoint.sh) live in this repo
under deploy/passivbot-image/ and are the source of truth. The upstream passivbot
code comes from --passivbot-dir. This script overlays the former onto a subset of
the latter, zips it, uploads it as the CodeBuild source, and starts the build.

Usage:
  python scripts/build_passivbot_image.py --tag v7.12.0-arm64
  python scripts/build_passivbot_image.py --tag v7.13.0-arm64 --passivbot-dir E:/projects/passivbot
"""

import argparse
import json
import os
import posixpath
import subprocess
import sys
import tempfile
import time
import zipfile

REPO_ROOT = posixpath.dirname(posixpath.dirname(__file__.replace("\\", "/")))
DEPLOY_DIR = posixpath.join(REPO_ROOT, "deploy", "passivbot-image")

# Our build files (overlaid from this repo) and the upstream subset we ship.
OUR_FILES = ["Dockerfile.ecs", "buildspec.yml", "entrypoint.sh"]
UPSTREAM_FLAT = [
    ".dockerignore", "broker_codes.hjson", "pyproject.toml", "setup.py",
    "requirements-dev.txt", "requirements-full.txt",
    "requirements-live.txt", "requirements-rust.txt",
]
UPSTREAM_TREES = ["src", "passivbot-rust/src"]
UPSTREAM_EXTRA = ["passivbot-rust/Cargo.toml"]

PROJECT = "pbtb-passivbot-image-builder"
SRC_BUCKET = "scalable-cluster-dev-lambda-code"
SRC_KEY = "passivbot-build/source.zip"


def _skip(rel: str) -> bool:
    parts = rel.split("/")
    if "__pycache__" in parts:
        return True
    if rel.endswith((".pyd", ".pyc", ".pyo")):
        return True
    if posixpath.basename(rel) == "api-keys.json":  # never ship live keys
        return True
    return False


def stage_zip(passivbot_dir: str, out_zip: str) -> int:
    added = 0
    with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
        for f in OUR_FILES:
            z.write(posixpath.join(DEPLOY_DIR, f), f)
            added += 1
        for f in UPSTREAM_FLAT + UPSTREAM_EXTRA:
            src = posixpath.join(passivbot_dir, f)
            if not os.path.isfile(src):
                print(f"MISSING upstream file: {f}", file=sys.stderr)
                continue
            z.write(src, f)
            added += 1
        for tree in UPSTREAM_TREES:
            base = posixpath.join(passivbot_dir, tree)
            for dirpath, dirnames, filenames in os.walk(base):
                dirnames[:] = [d for d in dirnames if d != "__pycache__"]
                for name in filenames:
                    full = posixpath.join(dirpath.replace("\\", "/"), name)
                    rel = posixpath.relpath(full, passivbot_dir).replace("\\", "/")
                    if _skip(rel):
                        continue
                    z.write(full, rel)
                    added += 1
    return added


def aws(args: list[str], profile: str, region: str, capture=True):
    cmd = ["aws"] + args + ["--region", region]
    if profile:
        cmd += ["--profile", profile]
    return subprocess.run(cmd, check=True, capture_output=capture, text=True)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--tag", required=True, help="image tag, dotted semver, e.g. v7.12.0-arm64")
    p.add_argument("--passivbot-dir", default="E:/projects/passivbot")
    p.add_argument("--profile", default="dev")
    p.add_argument("--region", default="ap-northeast-1")
    p.add_argument("--no-wait", action="store_true")
    args = p.parse_args()

    pdir = args.passivbot_dir.replace("\\", "/")
    with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as tmp:
        zip_path = tmp.name
    try:
        n = stage_zip(pdir, zip_path)
        mb = round(os.path.getsize(zip_path) / (1024 * 1024), 2)
        print(f"staged {n} files, {mb} MB")

        print(f"uploading s3://{SRC_BUCKET}/{SRC_KEY}")
        aws(["s3", "cp", zip_path, f"s3://{SRC_BUCKET}/{SRC_KEY}"], args.profile, args.region)

        print(f"starting build {PROJECT} (IMAGE_TAG={args.tag})")
        override = json.dumps([{"name": "IMAGE_TAG", "value": args.tag, "type": "PLAINTEXT"}])
        out = aws(["codebuild", "start-build", "--project-name", PROJECT,
                   "--environment-variables-override", override,
                   "--query", "build.id", "--output", "text"], args.profile, args.region)
        build_id = out.stdout.strip()
        print(f"build id: {build_id}")
    finally:
        os.unlink(zip_path)

    if args.no_wait:
        print("(--no-wait) not polling. Check the CodeBuild console.")
        return 0

    print("waiting for build to finish...")
    for _ in range(150):  # ~50 min cap
        out = aws(["codebuild", "batch-get-builds", "--ids", build_id,
                   "--query", "builds[0].buildStatus", "--output", "text"], args.profile, args.region)
        status = out.stdout.strip()
        if status != "IN_PROGRESS":
            print(f"FINAL: {status}")
            return 0 if status == "SUCCEEDED" else 1
        time.sleep(20)
    print("TIMEOUT still IN_PROGRESS")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
