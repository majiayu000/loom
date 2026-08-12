#!/usr/bin/env python3
"""Regression checks for deterministic release archives."""

from __future__ import annotations

import hashlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("create-reproducible-tar.py")


class ReproducibleTarTest(unittest.TestCase):
    def test_archives_are_identical_and_metadata_is_normalized(self) -> None:
        with tempfile.TemporaryDirectory(prefix="loom-repro-tar-") as tmp:
            root = Path(tmp)
            source = root / "bundle"
            source.mkdir()
            regular = source / "README.md"
            regular.write_text("stable payload\n", encoding="utf-8")
            executable = source / "loom"
            executable.write_text("#!/bin/sh\n", encoding="utf-8")
            executable.chmod(0o700)

            archives = [root / "first.tar.gz", root / "second.tar.gz"]
            for archive in archives:
                subprocess.run(
                    [sys.executable, str(SCRIPT), "--source", str(source), "--output", str(archive), "--mtime", "123456789"],
                    check=True,
                )

            digests = [hashlib.sha256(path.read_bytes()).digest() for path in archives]
            self.assertEqual(digests[0], digests[1])

            with tarfile.open(archives[0], "r:gz") as archive:
                members = {member.name: member for member in archive.getmembers()}
            self.assertEqual(list(members), ["bundle", "bundle/README.md", "bundle/loom"])
            for member in members.values():
                self.assertEqual((member.uid, member.gid, member.uname, member.gname, member.mtime), (0, 0, "", "", 123456789))
            self.assertEqual(members["bundle/README.md"].mode, 0o644)
            self.assertEqual(members["bundle/loom"].mode, 0o755)


if __name__ == "__main__":
    unittest.main()
