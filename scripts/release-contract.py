#!/usr/bin/env python3
"""Build and verify an atomic Loom binary/Skill/contract release bundle."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
import re
import shutil
import stat
import sys
import tempfile
import tomllib
from pathlib import Path

MANIFEST = "contract-manifest.json"
SKILL_REL = Path("skills/loom-registry")
INVENTORY_REL = Path("contracts/agent-command-surfaces.toml")
RELEASE_SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
CONTRACT_SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def fail(message: str) -> None:
    raise ValueError(message)


def release_semver(value: str, field: str) -> tuple[int, int, int]:
    match = RELEASE_SEMVER.fullmatch(value)
    if not match:
        fail(f"{field} must be a valid SemVer: {value!r}")
    return tuple(int(match.group(index)) for index in range(1, 4))


def contract_semver(value: str, field: str) -> tuple[int, int, int]:
    match = CONTRACT_SEMVER.fullmatch(value)
    if not match:
        fail(f"{field} must be a canonical release SemVer: {value!r}")
    return tuple(int(match.group(index)) for index in range(1, 4))


def contract_range_matches(requirement: str, version: str) -> bool:
    current = contract_semver(version, "CLI contract version")
    if not requirement:
        fail("Skill CLI contract range must not be empty")
    matches = True
    for raw in requirement.split(","):
        comparator = raw.strip()
        operator = next((candidate for candidate in (">=", "<=", ">", "<", "=") if comparator.startswith(candidate)), None)
        if operator is None:
            fail(f"unsupported Skill CLI contract comparator: {comparator!r}")
        expected = contract_semver(comparator[len(operator):], "Skill CLI contract comparator")
        matches = matches and {
            ">=": current >= expected,
            "<=": current <= expected,
            ">": current > expected,
            "<": current < expected,
            "=": current == expected,
        }[operator]
    return matches


def file_digest(path: Path) -> str:
    if not path.is_file():
        fail(f"required file is missing: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def skill_tree_digest(root: Path) -> str:
    if not root.is_dir():
        fail(f"required Skill directory is missing: {root}")
    entries: list[tuple[str, str, int, bytes]] = []
    resolved_root = root.resolve()
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix().encode()):
        relative = path.relative_to(root).as_posix()
        metadata = path.lstat()
        if stat.S_ISDIR(metadata.st_mode):
            continue
        executable = 1 if metadata.st_mode & 0o111 else 0
        if stat.S_ISLNK(metadata.st_mode):
            target = os.readlink(path)
            resolved = (path.parent / target).resolve()
            if os.path.commonpath([resolved_root, resolved]) != str(resolved_root):
                fail(f"Skill symlink escapes root: {relative} -> {target}")
            entries.append(("symlink", relative, executable, os.fsencode(target)))
        elif stat.S_ISREG(metadata.st_mode):
            entries.append(("file", relative, executable, path.read_bytes()))
        else:
            fail(f"unsupported Skill entry type: {relative}")
    if not entries:
        fail(f"Skill directory is empty: {root}")
    digest = hashlib.sha256()
    for entry_type, relative, executable, payload in entries:
        for field in (entry_type.encode(), relative.encode(), str(executable).encode()):
            digest.update(len(field).to_bytes(8, "big"))
            digest.update(field)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return f"sha256:{digest.hexdigest()}"


def skill_range(skill_root: Path) -> str:
    metadata_path = skill_root / "loom.skill.toml"
    if not metadata_path.is_file():
        fail(f"Skill metadata is missing: {metadata_path}")
    with metadata_path.open("rb") as stream:
        metadata = tomllib.load(stream)
    value = metadata.get("compatibility", {}).get("cli_contract")
    if not isinstance(value, str) or not value:
        fail(f"Skill CLI contract range is missing: {metadata_path}")
    return value


def manifest_for(root: Path, release_version: str, contract_version: str, target: str) -> dict[str, str]:
    if not release_version or not contract_version or not target:
        fail("release version, contract version, and binary target are required")
    release_semver(release_version, "release version")
    contract_semver(contract_version, "CLI contract version")
    supported_range = skill_range(root / SKILL_REL)
    if not contract_range_matches(supported_range, contract_version):
        fail(
            f"Skill CLI contract range {supported_range!r} does not contain {contract_version!r}"
        )
    return {
        "schema_version": "1",
        "release_version": release_version,
        "cli_contract_version": contract_version,
        "skill_cli_contract_range": supported_range,
        "binary_target": target,
        "binary_sha256": file_digest(root / "loom"),
        "skill_tree_digest": skill_tree_digest(root / SKILL_REL),
        "inventory_sha256": file_digest(root / INVENTORY_REL),
    }


def write_synced(path: Path, payload: bytes, mode: int = 0o644) -> None:
    with path.open("wb") as stream:
        stream.write(payload)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(mode)


def verify(root: Path) -> dict[str, str]:
    manifest_path = root / MANIFEST
    if not manifest_path.is_file():
        fail(f"contract manifest is missing: {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict):
        fail("contract manifest must be a JSON object")
    for field in ("release_version", "cli_contract_version", "binary_target"):
        if not isinstance(manifest.get(field), str) or not manifest[field]:
            fail(f"contract manifest field {field!r} must be a non-empty string")
    expected = manifest_for(
        root,
        manifest["release_version"],
        manifest["cli_contract_version"],
        manifest["binary_target"],
    )
    if manifest != expected:
        fail(f"contract manifest mismatch: expected {expected}, found {manifest}")
    return expected


def copy_inputs(args: argparse.Namespace, staging: Path) -> None:
    binary = Path(args.binary)
    skill = Path(args.skill_dir)
    inventory = Path(args.inventory)
    file_digest(binary)
    skill_tree_digest(skill)
    file_digest(inventory)
    shutil.copy2(binary, staging / "loom")
    shutil.copytree(skill, staging / SKILL_REL, symlinks=True)
    (staging / INVENTORY_REL).parent.mkdir(parents=True)
    shutil.copy2(inventory, staging / INVENTORY_REL)
    for extra_raw in args.extra:
        extra = Path(extra_raw)
        if not extra.is_file():
            fail(f"extra release file is missing: {extra}")
        shutil.copy2(extra, staging / extra.name)


def publish(args: argparse.Namespace) -> dict[str, str]:
    output = Path(args.output_dir).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{output.name}.tmp-", dir=output.parent))
    lock_path = output.parent / f".{output.name}.lock"
    try:
        copy_inputs(args, staging)
        manifest = manifest_for(staging, args.release_version, args.contract_version, args.target)
        write_synced(
            staging / MANIFEST,
            (json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode(),
            0o444,
        )
        verify(staging)
        if os.environ.get("LOOM_RELEASE_CONTRACT_FAULT") == "before_publish":
            fail("fault injected before atomic publish")
        directory = os.open(staging, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        with lock_path.open("a+b") as lock:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            if output.exists():
                if verify(output) != manifest:
                    fail(f"release bundle already exists with different content: {output}")
                return manifest
            os.rename(staging, output)
            parent = os.open(output.parent, os.O_RDONLY)
            try:
                os.fsync(parent)
            finally:
                os.close(parent)
        return manifest
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser()
    subcommands = command.add_subparsers(dest="command", required=True)
    publish_parser = subcommands.add_parser("publish")
    publish_parser.add_argument("--binary", required=True)
    publish_parser.add_argument("--skill-dir", required=True)
    publish_parser.add_argument("--inventory", required=True)
    publish_parser.add_argument("--output-dir", required=True)
    publish_parser.add_argument("--release-version", required=True)
    publish_parser.add_argument("--contract-version", required=True)
    publish_parser.add_argument("--target", required=True)
    publish_parser.add_argument("--extra", action="append", default=[])
    verify_parser = subcommands.add_parser("verify")
    verify_parser.add_argument("--bundle", required=True)
    return command


def main() -> int:
    args = parser().parse_args()
    try:
        manifest = publish(args) if args.command == "publish" else verify(Path(args.bundle))
    except (OSError, ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"release-contract: {error}", file=sys.stderr)
        return 1
    print(json.dumps(manifest, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
