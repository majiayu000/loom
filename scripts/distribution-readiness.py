#!/usr/bin/env python3
"""Verify that Loom's advertised install path resolves to complete live artifacts."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SUPPORTED_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
)
INSTALLER_URL = "https://raw.githubusercontent.com/majiayu000/loom/main/scripts/install.sh"
HOMEBREW_COMMAND = "brew install majiayu000/tap/loom"
HOMEBREW_FORMULA_URL = (
    "https://raw.githubusercontent.com/majiayu000/homebrew-tap/main/Formula/loom.rb"
)


def request_text(url: str) -> str:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "loom-release-readiness",
    }
    token = os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    last_error: Exception | None = None
    for attempt in range(3):
        try:
            request = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(request, timeout=20) as response:
                return response.read().decode("utf-8")
        except (urllib.error.URLError, TimeoutError) as error:
            last_error = error
            if attempt < 2:
                time.sleep(2)
    raise RuntimeError(f"could not fetch {url}: {last_error}")


def parse_payload(raw: str) -> dict[str, object]:
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise ValueError("release response must be a JSON object")
    return payload


def release_payload(
    repository: str, tag: str, fixture: Path | None
) -> tuple[dict[str, object], dict[str, object]]:
    if fixture:
        payload = parse_payload(fixture.read_text(encoding="utf-8"))
        return payload, payload
    tagged = parse_payload(
        request_text(f"https://api.github.com/repos/{repository}/releases/tags/{tag}")
    )
    latest = parse_payload(
        request_text(f"https://api.github.com/repos/{repository}/releases/latest")
    )
    return tagged, latest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", default="majiayu000/loom")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--readme", type=Path, default=Path("README.md"))
    parser.add_argument("--release-json", type=Path, help="offline release response fixture")
    parser.add_argument("--homebrew-formula", type=Path, help="offline formula fixture")
    args = parser.parse_args()

    match = re.fullmatch(
        r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
        r"(?:[-+][0-9A-Za-z.-]+)?",
        args.tag,
    )
    if not match:
        print(f"distribution readiness failed: invalid release tag {args.tag!r}", file=sys.stderr)
        return 1
    version = args.tag[1:]
    failures: list[str] = []

    try:
        payload, latest = release_payload(args.repository, args.tag, args.release_json)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        print(f"distribution readiness failed: {error}", file=sys.stderr)
        return 1

    if payload.get("tag_name") != args.tag:
        failures.append(
            f"release tag mismatch: expected {args.tag}, "
            f"found {payload.get('tag_name')!r}"
        )
    if latest.get("tag_name") != args.tag:
        failures.append(
            f"latest-release redirect does not resolve to {args.tag}: found {latest.get('tag_name')!r}"
        )
    if payload.get("draft") is True:
        failures.append("release is still a draft")
    assets = payload.get("assets")
    assets_by_name: dict[str, dict[str, object]] = {}
    if not isinstance(assets, list):
        failures.append("release assets are missing")
        asset_names: set[str] = set()
    else:
        assets_by_name = {
            asset["name"]: asset
            for asset in assets
            if isinstance(asset, dict) and isinstance(asset.get("name"), str)
        }
        asset_names = set(assets_by_name)
    expected_assets = {"SHA256SUMS"}
    expected_assets.update(f"skillloom-{version}-{target}.tar.gz" for target in SUPPORTED_TARGETS)
    for missing in sorted(expected_assets - asset_names):
        failures.append(f"release asset is missing: {missing}")
    for name in sorted(expected_assets & asset_names):
        asset = assets_by_name[name]
        size = asset.get("size")
        if asset.get("state") != "uploaded" or not isinstance(size, int) or size <= 0:
            failures.append(f"release asset is not fully uploaded: {name}")

    try:
        readme = args.readme.read_text(encoding="utf-8")
    except OSError as error:
        failures.append(f"could not read {args.readme}: {error}")
        readme = ""
    if f"curl -fsSL {INSTALLER_URL} | sh" not in readme:
        failures.append("README does not advertise the supported one-command installer")
    if HOMEBREW_COMMAND in readme:
        try:
            formula = (
                args.homebrew_formula.read_text(encoding="utf-8")
                if args.homebrew_formula
                else request_text(HOMEBREW_FORMULA_URL)
            )
            if "class Loom < Formula" not in formula:
                failures.append("advertised Homebrew formula is not a Loom formula")
        except (OSError, RuntimeError) as error:
            failures.append(f"README advertises Homebrew but its formula is unavailable: {error}")

    if failures:
        for failure in failures:
            print(f"distribution readiness failed: {failure}", file=sys.stderr)
        return 1
    print(
        f"distribution ready: {args.tag} has {len(expected_assets)} required assets; "
        "README installer is truthful"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
