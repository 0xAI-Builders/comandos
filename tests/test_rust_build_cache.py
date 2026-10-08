"""Exercise build admission in private Git worktrees without starting a compiler."""
import shutil
import os
import mmap
import subprocess
import tempfile
import time
import unittest
from pathlib import Path


SOURCE = Path(__file__).resolve().parents[1]


class BuildCache(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="comandos-cache-guard-")
        self.root = Path(self.tmp.name)
        self.main = self.root / "main"
        self.main.mkdir()
        self.git("init", "-q")
        # A private process census includes the real test process, so open FDs
        # and mappings are exercised without unrelated host/container processes.
        self.proc = self.root / "proc"
        self.proc.mkdir()
        (self.proc / str(os.getpid())).symlink_to(Path("/proc") / str(os.getpid()))
        scripts = self.main / "scripts"
        scripts.mkdir()
        for name in ["rust-sandbox", "rust-build-cache", "rust-python-oracle", "rust-cache-gc"]:
            source = SOURCE / "scripts" / name
            if source.exists():
                shutil.copyfile(source, scripts / name)
                if name == "rust-cache-gc":
                    helper = scripts / name
                    helper.write_text(helper.read_text().replace('PROC_ROOT = Path("/proc")', 'PROC_ROOT = Path(' + repr(str(self.proc)) + ')'))
                (scripts / name).chmod(0o700)
        self.git("add", "scripts")
        self.git("-c", "user.name=fixture", "-c", "user.email=fixture@localhost", "commit", "-qm", "fixture")
        self.a, self.b = self.root / "a", self.root / "b"
        for path in [self.a, self.b]:
            self.git("worktree", "add", "-q", "--detach", str(path))
            (path / ".migration-build/cargo/registry").mkdir(parents=True)
        self.cache = self.main / ".build/target-integration-acp"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        rustup = self.bin / "rustup"
        rustup.write_text("#!/bin/sh\nprintf '%s\\n' '" + str(self.bin / "cargo") + "'\n")
        rustup.chmod(0o700)
        runner = self.bin / "systemd-run"
        runner.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$FIXTURE_RECEIPT\"\nif [ -n \"${FIXTURE_GROWTH:-}\" ] && [ -n \"${CARGO_TARGET_DIR:-}\" ]; then dd if=/dev/zero of=\"$CARGO_TARGET_DIR/growth\" bs=1048576 count=2 status=none; fi\nexit \"${FIXTURE_EXIT:-0}\"\n")
        runner.chmod(0o700)
        self.env = {"PATH": str(self.bin) + ":/usr/bin:/bin", "HOME": str(self.root), "LANG": "C", "FIXTURE_RECEIPT": str(self.root / "receipt")}

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.main), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def run_sandbox(self, workspace=None, command=("true",), **extra):
        workspace = workspace or self.a
        return subprocess.run([str(workspace / "scripts/rust-sandbox"), *command], cwd=workspace, env={**self.env, **extra}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)

    def test_two_worktrees_share_one_target_outside_their_sources(self):
        for workspace in [self.a, self.b]:
            result = self.run_sandbox(workspace)
            self.assertEqual(result.returncode, 0, result.stderr)
            args = (self.root / "receipt").read_text().splitlines()
            self.assertIn(str(self.cache), args)
            index = args.index("CARGO_TARGET_DIR")
            self.assertEqual(args[index + 1], "/build-cache")
            self.assertFalse((workspace / ".migration-build/target").exists())

    def test_full_cache_refuses_new_build_and_preserves_all_existing_files(self):
        self.cache.mkdir(parents=True)
        data = self.cache / "active-artifact"
        body = b"x" * (2 * 1024 * 1024)
        data.write_bytes(body)
        result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="1024")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "receipt").exists())
        self.assertEqual(data.read_bytes(), body)

    def test_crossing_budget_is_reported_after_build_without_deleting_output(self):
        result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="1024", FIXTURE_GROWTH="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "receipt").exists())
        self.assertEqual((self.cache / "growth").stat().st_size, 2 * 1024 * 1024)

    def test_child_exit_status_is_preserved(self):
        self.assertEqual(self.run_sandbox(FIXTURE_EXIT="7").returncode, 7)

    def stale_variants(self):
        deps = self.cache / "debug/deps"
        deps.mkdir(parents=True)
        variants = []
        for index in range(4):
            artifact = deps / ("libfixture-%016x.rlib" % index)
            artifact.write_bytes(b"x" * (1024 * 1024))
            age = time.time() - (12 - index) * 86400
            os.utime(artifact, (age, age))
            variants.append(artifact)
        return variants

    def test_pressure_recycles_only_old_superseded_hashes(self):
        variants = self.stale_variants()
        current = self.cache / "debug/fixture"
        current.write_text("keep current binary")
        result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="3072")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(variants[0].exists())
        self.assertFalse(variants[1].exists())
        self.assertTrue(all(path.exists() for path in variants[2:]))
        self.assertEqual(current.read_text(), "keep current binary")

    def test_gc_preserves_hardlinked_and_open_artifacts(self):
        variants = self.stale_variants()
        (self.cache / "debug/current").hardlink_to(variants[0])
        with variants[1].open("rb"):
            result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="3072")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(all(path.exists() for path in variants))
        self.assertFalse((self.root / "receipt").exists())

    def test_gc_preserves_recent_variants_even_under_pressure(self):
        variants = self.stale_variants()
        for path in variants:
            path.touch()
        result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="3072")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(all(path.exists() for path in variants))

    def test_gc_preserves_mapped_artifact_after_its_fd_is_closed(self):
        variants = self.stale_variants()
        (self.cache / "debug/current").hardlink_to(variants[0])
        with variants[1].open("rb") as source:
            mapped = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="3072")
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(all(path.exists() for path in variants))
        finally:
            mapped.close()

    def test_compiler_outside_repository_with_explicit_target_blocks_gc(self):
        variants = self.stale_variants()
        proc = self.proc / "999999999"
        proc.mkdir()
        (proc / "status").write_text("State:\tS (sleeping)\nPPid:\t1\n")
        (proc / "comm").write_text("cargo\n")
        (proc / "cwd").symlink_to(self.root)
        (proc / "environ").write_bytes(b"")
        (proc / "cmdline").write_bytes(b"cargo\0build\0--target-dir=" + os.fsencode(self.cache) + b"\0")
        result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB="3072")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"a compiler is using this repository", result.stdout)
        self.assertTrue(all(path.exists() for path in variants))

    def test_build_commands_from_two_worktrees_are_serialized(self):
        script = self.root / "exclusive"
        script.write_text("#!/bin/sh\nmkdir \"$HOME/build-running\" || exit 44\nsleep 0.2\nrmdir \"$HOME/build-running\"\n")
        script.chmod(0o700)
        processes = [subprocess.Popen([str(worktree / "scripts/rust-build-cache"), "--", str(script)], cwd=worktree, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for worktree in [self.a, self.b]]
        results = [(process, process.communicate(timeout=10)[1]) for process in processes]
        for process, error in results:
            self.assertEqual(process.returncode, 0, error)

    def test_malformed_budget_cannot_disable_admission(self):
        for budget in ("1:2", "16777217", "33554432"):
            with self.subTest(budget=budget):
                result = self.run_sandbox(COMANDOS_BUILD_CACHE_MAX_KIB=budget)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.root / "receipt").exists())

    def test_other_target_override_is_rejected(self):
        protected = self.root / "user-target"
        protected.mkdir()
        marker = protected / "keep"
        marker.write_text("active")
        result = self.run_sandbox(CARGO_TARGET_DIR=str(protected))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "receipt").exists())
        self.assertEqual(marker.read_text(), "active")

    def test_insufficient_free_space_refuses_new_build(self):
        result = self.run_sandbox(COMANDOS_BUILD_MIN_FREE_KIB="99999999999999")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "receipt").exists())

    def test_cargo_target_options_cannot_bypass_shared_cache(self):
        cases = [("--target-dir", str(self.root / "bypass")),
                 ("--target-dir=" + str(self.root / "bypass"),),
                 ("--config", 'build.target-dir="' + str(self.root / "bypass") + '"'),
                 ("--config=" + str(self.root / "override.toml"),)]
        for options in cases:
            with self.subTest(options=options):
                result = self.run_sandbox(command=("cargo", "build", *options))
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.root / "receipt").exists())

    def test_target_symlink_is_rejected_without_touching_destination(self):
        destination = self.root / "protected"
        destination.mkdir()
        marker = destination / "keep"
        marker.write_text("untouched")
        self.cache.parent.mkdir()
        self.cache.symlink_to(destination, target_is_directory=True)
        self.assertNotEqual(self.run_sandbox().returncode, 0)
        self.assertFalse((self.root / "receipt").exists())
        self.assertEqual(marker.read_text(), "untouched")

    def test_oracle_mounts_shared_artifacts_readonly_and_exports_target(self):
        venv = self.root / "venv"
        (venv / "bin").mkdir(parents=True)
        python = venv / "bin/python"
        python.write_text("#!/bin/sh\nexit 0\n")
        python.chmod(0o700)
        tokens = self.root / "tokens"
        tokens.mkdir()
        result = subprocess.run([str(self.a / "scripts/rust-python-oracle"), str(venv), str(tokens), "true"], cwd=self.a, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        args = (self.root / "receipt").read_text().splitlines()
        self.assertIn(str(self.cache), args)
        index = args.index(str(self.cache))
        self.assertEqual(args[index - 1], "--ro-bind")
        self.assertEqual(args[index + 1], "/build-cache")
        index = args.index("CARGO_TARGET_DIR")
        self.assertEqual(args[index + 1], "/build-cache")


if __name__ == "__main__":
    unittest.main()
