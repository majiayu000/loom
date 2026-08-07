#!/usr/bin/env python3
"""Create a deterministic gzip-compressed tar archive for one directory tree."""

from __future__ import annotations

import argparse
import gzip
import os
import tarfile
from pathlib import Path


def normalized_mode(path: Path, info: tarfile.TarInfo) -> int:
    if info.isdir() or info.issym():
        return 0o755
    return 0o755 if path.stat().st_mode & 0o111 else 0o644


def create_archive(source: Path, output: Path, mtime: int) -> None:
    source = source.resolve(strict=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    entries = [source, *sorted(source.rglob("*"), key=lambda path: path.relative_to(source).as_posix().encode())]

    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=mtime, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for path in entries:
                    arcname = source.name if path == source else f"{source.name}/{path.relative_to(source).as_posix()}"
                    info = archive.gettarinfo(str(path), arcname=arcname)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = mtime
                    info.mode = normalized_mode(path, info)
                    if info.isfile():
                        with path.open("rb") as stream:
                            archive.addfile(info, stream)
                    else:
                        archive.addfile(info)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mtime", type=int, required=True)
    args = parser.parse_args()
    if not args.source.is_dir():
        parser.error(f"source is not a directory: {args.source}")
    if args.mtime < 0:
        parser.error("mtime must be non-negative")
    create_archive(args.source, args.output, args.mtime)


if __name__ == "__main__":
    main()
