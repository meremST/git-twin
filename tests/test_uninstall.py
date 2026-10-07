"""Exercise the uninstaller without touching the real user's installation."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "uninstall.sh"


class UninstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="git-twin-uninstall-")
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name) / "home with spaces"
        self.bin = self.home / "bin"
        self.bin.mkdir(parents=True)
        self.env = {**os.environ, "HOME": str(self.home)}

    def run_script(self, *args, env=None):
        return subprocess.run(
            ["/bin/sh", str(SCRIPT), *args],
            env=self.env if env is None else env,
            capture_output=True, text=True,
        )

    def test_manual_preserves_other_files_and_is_repeatable(self):
        for name in ("gitt", "git-tui", "other"):
            (self.bin / name).write_text("keep unless a launcher")
        preserved = (".config/git-twin/config.toml", ".local/state/git-twin/draft",
                     ".cache/git-twin/item", "repo/.git/HEAD")
        for path in preserved:
            file = self.home / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text("preserve")
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(p.name for p in self.bin.iterdir()), ["other"])
        for path in preserved:
            self.assertEqual((self.home / path).read_text(), "preserve")
        self.assertEqual(self.run_script().returncode, 0)

    def test_symlinks_are_removed_without_following_targets(self):
        target = self.home / "keep"
        target.write_text("preserve")
        (self.bin / "gitt").symlink_to(target)
        (self.bin / "git-tui").symlink_to(self.home / "missing")
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(target.read_text(), "preserve")
        self.assertEqual(list(self.bin.iterdir()), [])

    def test_directory_is_refused_before_removing_either_launcher(self):
        (self.bin / "gitt").write_text("keep")
        (self.bin / "git-tui").mkdir()
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to remove directory", result.stderr)
        self.assertTrue((self.bin / "gitt").is_file())
        self.assertTrue((self.bin / "git-tui").is_dir())

    def test_invalid_arguments_and_home_do_not_remove_files(self):
        (self.bin / "gitt").write_text("keep")
        for args in [("--unknown",), ("--cargo", "--unknown")]:
            self.assertEqual(self.run_script(*args).returncode, 2)
        for home in ("", "/", "//", "relative"):
            self.assertNotEqual(self.run_script(env={**self.env, "HOME": home}).returncode, 0)
        self.assertTrue((self.bin / "gitt").exists())
        self.assertEqual(self.run_script("--help", env={}).returncode, 0)

    def test_cargo_mode_forwards_arguments_and_never_falls_back(self):
        cargo = self.bin / "cargo"
        cargo.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\nexit "${CARGO_EXIT:-0}"\n')
        cargo.chmod(0o755)
        (self.bin / "gitt").write_text("keep")
        env = {**self.env, "PATH": str(self.bin)}
        result = self.run_script("--cargo", env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[:4],
                         ["uninstall", "--root", str(self.home), "git-twin"])
        result = self.run_script("--cargo", env={**env, "CARGO_EXIT": "7"})
        self.assertEqual(result.returncode, 7)
        self.assertNotIn("were preserved", result.stdout)
        cargo.unlink()
        result = self.run_script("--cargo", env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Cargo is required", result.stderr)
        self.assertTrue((self.bin / "gitt").exists())


if __name__ == "__main__":
    unittest.main()
