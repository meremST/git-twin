"""Check release note selection without building or publishing binaries."""

import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "package_release", ROOT / "scripts" / "package-release.py",
)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class ReleaseNotesTests(unittest.TestCase):
    def test_selects_only_exact_version_and_preserves_subsections(self):
        changelog = (
            "# Release notes\n\n## Unreleased\n\nFuture\n\n"
            "## 1.0.0-rc.1\n\nCandidate\n\n"
            "## 1.0.0 - 2026-10-16\n\n### Features\n\nPublic release\n\n"
            "## 0.3.1\n\nInternal draft\n"
        )
        self.assertEqual(
            PACKAGE.release_notes(changelog, "1.0.0"),
            "## 1.0.0 - 2026-10-16\n\n### Features\n\nPublic release\n",
        )

    def test_accepts_last_section_without_trailing_newline(self):
        self.assertEqual(
            PACKAGE.release_notes("## 1.0.0\n\nFirst release", "1.0.0"),
            "## 1.0.0\n\nFirst release\n",
        )

    def test_rejects_missing_duplicate_and_empty_sections(self):
        for changelog in (
            "## Unreleased\n\nFuture\n",
            "## 1.0.0-rc.1\n\nCandidate\n",
            "## 1.0.0\n\nFirst\n\n## 1.0.0\n\nDuplicate\n",
            "## 1.0.0\n \n## 0.3.1\n\nOld\n",
        ):
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                PACKAGE.release_notes(changelog, "1.0.0")


if __name__ == "__main__":
    unittest.main()
