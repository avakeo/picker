import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUILD = Path(tempfile.gettempdir()) / "picker-t0-4-test"


class ImageIOBenchmarkTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        BUILD.mkdir(exist_ok=True)
        env = os.environ | {"CLANG_MODULE_CACHE_PATH": str(BUILD / "module-cache")}
        subprocess.run(["swiftc", str(ROOT / "tools/t0-4/generate_fixtures.swift"), "-o", str(BUILD / "fixtures")], check=True, env=env)
        subprocess.run(["swiftc", str(ROOT / "tools/t0-4/imageio_benchmark.swift"), "-o", str(BUILD / "benchmark")], check=True, env=env)

    def test_formats_orientation_srgb_and_invalid_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            fixtures = Path(directory) / "fixtures"
            output = Path(directory) / "report.json"
            subprocess.run([str(BUILD / "fixtures"), str(fixtures)], check=True)
            external_heic = os.environ.get("PICKER_T0_4_HEIC_FIXTURE")
            if external_heic and Path(external_heic).is_file():
                (fixtures / "fixture.heic").write_bytes(Path(external_heic).read_bytes())
            subprocess.run([str(BUILD / "benchmark"), "--input-root", str(fixtures), "--output", str(output),
                            "--iterations", "2", "--warmups", "1", "--max-pixel", "512"], check=True)
            report = json.loads(output.read_text())
            valid = [row for row in report["observations"] if row["success"]]
            invalid = [row for row in report["observations"] if not row["success"]]
            suffixes = {Path(row["path"]).suffix for row in valid}
            expected = {".jpg", ".png", ".webp"}
            if (fixtures / "fixture.heic").is_file():
                expected.add(".heic")
            self.assertEqual(suffixes, expected)
            self.assertTrue(all(row["orientationCorrect"] for row in valid))
            self.assertTrue(all(row["srgbCorrect"] for row in valid))
            # A platform may expose a decoder but not a destination encoder for
            # HEIC. The supplied external fixture is therefore best-effort;
            # the two deliberately invalid fixtures must fail in every mode.
            self.assertGreaterEqual(len(invalid), 6)
            self.assertTrue(all(row["error"] for row in invalid))


if __name__ == "__main__":
    unittest.main()
