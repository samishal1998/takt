"""Offline installer/launcher acceptance checks; run with Python's stdlib only."""

import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
VERSION = "9.8.7"
TARGETS = (
    "x86_64-unknown-linux-musl",
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
)


def executable(path, body):
    path.write_text(body, encoding="utf-8")
    path.chmod(0o755)


def python_script(body):
    return f"#!{sys.executable}\n" + body


FIXTURE_BINARY = python_script(
    "import json, os, pathlib, sys, time\n"
    f"if sys.argv[1:] == ['--version']:\n    sys.stdin.read()\n    print('takt {VERSION}')\n"
    "else:\n"
    "    mask = os.umask(0)\n"
    "    os.umask(mask)\n"
    "    if 'TAKT_TEST_READY' in os.environ:\n"
    "        pathlib.Path(os.environ['TAKT_TEST_READY']).write_text(str(os.getpid()))\n"
    "        time.sleep(60)\n"
    "    print(json.dumps({'args': sys.argv[1:], 'stdin': sys.stdin.read(), "
    "'source': os.path.realpath(sys.argv[0]), 'umask': mask}))\n"
    "    sys.exit(int(os.environ.get('TAKT_TEST_EXIT', '0')))\n"
)

FAKE_CURL = python_script(
    "import json, os, pathlib, shutil, sys, time\n"
    "args = sys.argv[1:]\n"
    "with open(os.environ['TAKT_TEST_CURL_LOG'], 'a') as log:\n"
    "    log.write(json.dumps(args) + '\\n')\n"
    "if '--output' in args:\n"
    "    output = pathlib.Path(args[args.index('--output') + 1])\n"
    "elif '-o' in args:\n"
    "    output = pathlib.Path(args[args.index('-o') + 1])\n"
    "else:\n"
    "    sys.exit('fake curl expected an output filename')\n"
    "url = next((arg for arg in args if arg.startswith('https://')), '')\n"
    f"prefix = 'https://github.com/samishal1998/takt/releases/download/v{VERSION}/'\n"
    "if not url.startswith(prefix):\n"
    "    sys.exit('unexpected download URL: ' + url)\n"
    "mode = os.environ.get('TAKT_TEST_CURL_MODE', '')\n"
    "if mode == 'offline':\n"
    "    sys.exit(7)\n"
    "output.write_bytes(b'partial download')\n"
    "time.sleep(float(os.environ.get('TAKT_TEST_CURL_SLEEP', '0')))\n"
    "if mode == 'fail':\n"
    "    sys.exit(7)\n"
    "if mode == 'corrupt':\n"
    "    sys.exit(0)\n"
    "shutil.copyfile(pathlib.Path(os.environ['TAKT_TEST_ARCHIVES']) / "
    "url.rsplit('/', 1)[1], output)\n"
)

FAKE_UNAME = python_script(
    "import os, sys\n"
    "values = {'-s': os.environ.get('TAKT_TEST_OS', 'Linux'), "
    "'-m': os.environ.get('TAKT_TEST_ARCH', 'x86_64'), "
    "'-r': os.environ.get('TAKT_TEST_KERNEL', '5.15.0')}\n"
    "print(values.get(sys.argv[1] if len(sys.argv) > 1 else '-s', 'test-host'))\n"
)


class BootstrapTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixtures_temp = tempfile.TemporaryDirectory(prefix="takt-bootstrap-fixtures-")
        cls.addClassCleanup(cls.fixtures_temp.cleanup)
        cls.fixtures = Path(cls.fixtures_temp.name)
        cls.generated = cls.fixtures / "generated"
        cls.generated.mkdir()
        checksums = []
        for target in TARGETS:
            name = f"takt-{VERSION}-{target}"
            archive = cls.fixtures / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as stream:
                for member, content, mode in (
                    (f"{name}/takt", FIXTURE_BINARY.encode(), 0o755),
                    (f"{name}/README.md", b"fixture documentation\n", 0o644),
                    ("../unexpected-extraction", b"must not be extracted\n", 0o644),
                ):
                    info = tarfile.TarInfo(member)
                    info.size = len(content)
                    info.mode = mode
                    stream.addfile(info, io.BytesIO(content))
            checksums.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
        checksum_file = cls.fixtures / "SHA256SUMS"
        checksum_file.write_text("".join(checksums), encoding="utf-8")
        generated = subprocess.run(
            [sys.executable, str(ROOT / "scripts/generate-bootstrap.py"),
             "--version", VERSION, "--checksums", str(checksum_file),
             "--output-dir", str(cls.generated)],
            capture_output=True, text=True, timeout=15,
        )
        if generated.returncode:
            raise RuntimeError(f"fixture generation failed: {generated.stdout}{generated.stderr}")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="takt-bootstrap-test-")
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.home = self.work / "home with spaces"
        self.tools = self.work / "tools"
        self.cache = self.work / "cache with spaces"
        self.tmp = self.work / "tmp"
        for directory in (self.home, self.tools, self.tmp):
            directory.mkdir()
        # A small explicit PATH prevents an installed Takt, curl, or local user
        # configuration from changing the test's result or reaching the network.
        for name in (
            "awk", "basename", "bash", "cat", "chmod", "cmp", "cp", "cut",
            "date", "dirname", "env", "expr", "grep", "gzip", "head", "id",
            "ln", "ls", "mkdir", "mktemp", "mv", "readlink", "rm", "sed",
            "sh", "sha256sum", "shasum", "sleep", "sort", "tar", "test",
            "touch", "tr", "wc",
        ):
            source = shutil.which(name)
            if source:
                (self.tools / name).symlink_to(source)
        executable(self.tools / "curl", FAKE_CURL)
        executable(self.tools / "uname", FAKE_UNAME)
        executable(self.tools / "sw_vers", "#!/bin/sh\nprintf '%s\\n' \"${TAKT_TEST_MACOS:-15.0}\"\n")
        self.log = self.work / "curl.jsonl"
        self.env = {
            "PATH": str(self.tools), "HOME": str(self.home),
            "TMPDIR": str(self.tmp), "LC_ALL": "C",
            "TAKT_CACHE_DIR": str(self.cache),
            "TAKT_TEST_ARCHIVES": str(self.fixtures),
            "TAKT_TEST_CURL_LOG": str(self.log),
        }

    def command(self, script="taktw", *args, env=None, input="", timeout=15, shell="/bin/sh"):
        return subprocess.run(
            [shell, str(self.generated / script), *map(str, args)],
            env=self.env | (env or {}), input=input, capture_output=True,
            text=True, cwd=self.work, timeout=timeout, umask=0o027,
        )

    def successful(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def requests(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def cached(self, target=TARGETS[0], root=None):
        return (root or self.cache) / VERSION / target / "takt"

    def assert_no_staging(self):
        for pattern in ("**/.download.*", "**/.takt-install.*"):
            self.assertEqual(list(self.work.glob(pattern)), [], "bootstrap staging directory was left behind")

    def install_system_binary(self, directory=None):
        directory = directory or self.tools
        directory.mkdir(exist_ok=True)
        binary = directory / "takt"
        executable(binary, FIXTURE_BINARY)
        return binary

    def test_generated_files_match_checked_in_release(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/generate-bootstrap.py"), "--check"],
            capture_output=True, text=True, timeout=15,
        )
        self.successful(result)

    def test_installed_takt_is_used_without_network_or_cache(self):
        binary = self.install_system_binary()
        result = self.successful(self.command(env={"TAKT_USE_SYSTEM": "1", "TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(json.loads(result.stdout)["source"], str(binary.resolve()))
        self.assertEqual(self.requests(), [])
        self.assertFalse(self.cache.exists())

    def test_system_binary_preserves_arguments_stdin_and_exit_status(self):
        self.install_system_binary()
        arguments = ("exec", "a b", "", "$(touch should-not-exist)", "*", "line\nbreak", "--")
        result = self.command("taktw", *arguments, input="input\nwith spaces\n",
                              env={"TAKT_USE_SYSTEM": "1", "TAKT_TEST_EXIT": "23"})
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(json.loads(result.stdout)["args"], list(arguments))
        self.assertEqual(json.loads(result.stdout)["stdin"], "input\nwith spaces\n")
        self.assertFalse((self.work / "should-not-exist").exists())

    def test_unrelated_executable_named_takt_is_skipped(self):
        executable(self.tools / "takt", "#!/bin/sh\nprintf 'some other command\\n'\n")
        result = self.successful(self.command(env={"TAKT_USE_SYSTEM": "1"}))
        self.assertEqual(json.loads(result.stdout)["source"], str(self.cached().resolve()))
        self.assertEqual(len(self.requests()), 1)

    def test_search_continues_past_unrelated_takt(self):
        executable(self.tools / "takt", "#!/bin/sh\nprintf 'other command\\n'\n")
        later = self.work / "later tools"
        binary = self.install_system_binary(later)
        result = self.successful(self.command(env={"TAKT_USE_SYSTEM": "1", "PATH": f"{self.tools}:{later}",
                                                   "TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(json.loads(result.stdout)["source"], str(binary.resolve()))
        self.assertEqual(self.requests(), [])

    def test_force_cache_ignores_installed_binary(self):
        self.install_system_binary()
        result = self.successful(self.command(env={"TAKT_USE_SYSTEM": "0"}))
        self.assertEqual(json.loads(result.stdout)["source"], str(self.cached().resolve()))
        self.assertEqual(len(self.requests()), 1)

    def test_default_uses_pinned_cache_without_probing_installed_takt(self):
        probe = self.work / "system-probed"
        executable(self.tools / "takt", python_script(
            "from pathlib import Path\n"
            f"Path({str(probe)!r}).write_text('probed')\n"
            "print('takt 1.2.3')\n"
        ))
        for mode in ("", "offline"):
            result = self.successful(self.command(env={"TAKT_TEST_CURL_MODE": mode}))
            self.assertEqual(json.loads(result.stdout)["source"], str(self.cached().resolve()))
            self.assertFalse(probe.exists(), "default wrapper must not probe an installed binary")
        self.assertEqual(len(self.requests()), 1)

    def test_cold_cache_then_offline_reuse(self):
        result = self.successful(self.command("taktw", "--version"))
        self.assertEqual(result.stdout.strip(), f"takt {VERSION}")
        self.assertTrue(os.access(self.cached(), os.X_OK))
        self.successful(self.command("taktw", "--version", env={"TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(len(self.requests()), 1)
        self.assertEqual(list(self.tmp.iterdir()), [])
        self.assert_no_staging()
        self.assertFalse((self.work / "unexpected-extraction").exists())
        self.assertEqual([p.name for p in self.cached().parent.iterdir()], ["takt"])

    def test_download_url_uses_pinned_version_and_secure_curl(self):
        self.successful(self.command("taktw", "--version"))
        request = self.requests()[0]
        self.assertIn(f"https://github.com/samishal1998/takt/releases/download/v{VERSION}/takt-{VERSION}-{TARGETS[0]}.tar.gz", request)
        self.assertIn("--proto", request)
        self.assertIn("=https", request)
        self.assertIn("--fail", request)

    def test_cached_binary_preserves_arguments_stdin_and_exit_status(self):
        arguments = ("exec", "has spaces", "", "'quoted'", "$HOME", "; false")
        result = self.command("taktw", *arguments, input="hello\n", env={"TAKT_TEST_EXIT": "19"})
        self.assertEqual(result.returncode, 19, result.stderr)
        self.assertEqual(json.loads(result.stdout)["args"], list(arguments))
        self.assertEqual(json.loads(result.stdout)["stdin"], "hello\n")

    def test_download_does_not_change_child_umask(self):
        for mode in ("", "offline"):
            result = self.successful(self.command(env={"TAKT_TEST_CURL_MODE": mode}))
            self.assertEqual(json.loads(result.stdout)["umask"], 0o027)

    def test_bash_cold_download_and_offline_reuse(self):
        if not Path("/bin/bash").exists():
            self.skipTest("Bash is not available on this host")
        for mode in ("", "offline"):
            result = self.successful(self.command("taktw", "--version", shell="/bin/bash",
                                                  env={"TAKT_TEST_CURL_MODE": mode}))
            self.assertEqual(result.stdout.strip(), f"takt {VERSION}")
        self.assertEqual(len(self.requests()), 1)

    def test_wrapper_exec_preserves_pid_and_signal_delivery(self):
        ready = self.work / "child-pid"
        process = subprocess.Popen(
            ["/bin/sh", str(self.generated / "taktw")],
            env=self.env | {"TAKT_TEST_READY": str(ready)}, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, start_new_session=True,
        )
        try:
            deadline = time.monotonic() + 5
            while not (ready.exists() and ready.stat().st_size) and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(ready.exists(), "application did not start")
            self.assertEqual(int(ready.read_text()), process.pid)
            process.send_signal(signal.SIGTERM)
            process.communicate(timeout=5)
            self.assertEqual(process.returncode, -signal.SIGTERM)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=5)

    def test_renamed_and_symlinked_wrapper_do_not_recurse(self):
        for kind in ("copy", "symlink"):
            with self.subTest(kind=kind):
                candidate = self.tools / "takt"
                if kind == "copy":
                    shutil.copy2(self.generated / "taktw", candidate)
                else:
                    candidate.symlink_to(self.generated / "taktw")
                try:
                    result = self.successful(self.command("taktw", "--version", env={"TAKT_USE_SYSTEM": "1"}))
                    self.assertEqual(result.stdout.strip(), f"takt {VERSION}")
                finally:
                    candidate.unlink()
        self.assertEqual(len(self.requests()), 1)

    def test_wrapper_invoked_as_takt_does_not_recurse(self):
        wrapper = self.tools / "takt"
        shutil.copy2(self.generated / "taktw", wrapper)
        result = subprocess.run([str(wrapper), "--version"], env=self.env | {"TAKT_USE_SYSTEM": "1"},
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
        self.successful(result)
        self.assertEqual(result.stdout.strip(), f"takt {VERSION}")
        self.assertEqual(len(self.requests()), 1)

    def test_checksum_failure_cannot_populate_or_execute_cache(self):
        result = self.command("taktw", "--version", env={"TAKT_TEST_CURL_MODE": "corrupt"})
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.cached().exists())
        self.assertNotIn(f"takt {VERSION}", result.stdout)
        self.assertEqual(list(self.tmp.iterdir()), [])
        self.assert_no_staging()
        self.successful(self.command("taktw", "--version"))
        self.assertEqual(len(self.requests()), 2)

    def test_failed_download_is_cleaned_and_retry_succeeds(self):
        result = self.command("taktw", "--version", env={"TAKT_TEST_CURL_MODE": "fail"})
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.cached().exists())
        self.assertEqual(list(self.tmp.iterdir()), [])
        self.assert_no_staging()
        self.successful(self.command("taktw", "--version"))
        self.assertEqual(len(self.requests()), 2)

    def test_interrupted_download_can_be_retried(self):
        process = subprocess.Popen(
            ["/bin/sh", str(self.generated / "taktw"), "--version"],
            env=self.env | {"TAKT_TEST_CURL_SLEEP": "5"}, stdout=subprocess.PIPE,
            stdin=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, start_new_session=True,
        )
        try:
            deadline = time.monotonic() + 5
            while not self.log.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(self.log.exists(), "download did not start")
            os.killpg(process.pid, signal.SIGTERM)
            process.communicate(timeout=5)
            self.assertNotEqual(process.returncode, 0)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=5)
        self.assertFalse(self.cached().exists())
        self.assertEqual(list(self.tmp.iterdir()), [])
        self.assert_no_staging()
        self.successful(self.command("taktw", "--version"))

    def test_two_cold_callers_publish_complete_binary_atomically(self):
        processes = [subprocess.Popen(
            ["/bin/sh", str(self.generated / "taktw"), "--version"],
            env=self.env | {"TAKT_TEST_CURL_SLEEP": "0.2"},
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        ) for _ in range(2)]
        try:
            for process in processes:
                stdout, stderr = process.communicate(timeout=15)
                self.assertEqual(process.returncode, 0, stdout + stderr)
                self.assertEqual(stdout.strip(), f"takt {VERSION}")
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=5)
        self.assertEqual(self.cached().read_text(), FIXTURE_BINARY)
        self.assertEqual(list(self.tmp.iterdir()), [])
        self.assert_no_staging()
        self.successful(self.command("taktw", "--version", env={"TAKT_TEST_CURL_MODE": "offline"}))

    def test_xdg_cache_default(self):
        xdg = self.work / "xdg cache"
        self.env.pop("TAKT_CACHE_DIR")
        self.successful(self.command("taktw", "--version", env={"XDG_CACHE_HOME": str(xdg)}))
        self.assertTrue(self.cached(root=xdg / "takt").is_file())

    def test_home_cache_default(self):
        self.env.pop("TAKT_CACHE_DIR")
        self.successful(self.command("taktw", "--version"))
        self.assertTrue(self.cached(root=self.home / ".cache/takt").is_file())

    def test_cache_path_with_backslash_and_newline(self):
        cache = self.work / "back\\slash and\nnewline"
        self.successful(self.command("taktw", "--version", env={"TAKT_CACHE_DIR": str(cache)}))
        self.assertTrue(self.cached(root=cache).is_file())
        self.successful(self.command("taktw", "--version", env={"TAKT_CACHE_DIR": str(cache),
                                                              "TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(len(self.requests()), 1)

    def test_supported_platforms_choose_matching_archives(self):
        for system, architecture, target in (("Linux", "x86_64", TARGETS[0]),
                                               ("Darwin", "arm64", TARGETS[2]),
                                               ("Darwin", "aarch64", TARGETS[2])):
            with self.subTest(system=system, architecture=architecture):
                result = self.successful(self.command(env={"TAKT_TEST_OS": system, "TAKT_TEST_ARCH": architecture}))
                self.assertEqual(json.loads(result.stdout)["source"], str(self.cached(target).resolve()))

    def test_gnu_target_can_be_selected_on_linux(self):
        result = self.successful(self.command(env={"TAKT_TARGET": TARGETS[1]}))
        self.assertEqual(json.loads(result.stdout)["source"], str(self.cached(TARGETS[1]).resolve()))

    def test_unsupported_platforms_and_incompatible_overrides_fail_before_network(self):
        cases = ({"TAKT_TEST_OS": "Windows_NT"}, {"TAKT_TEST_ARCH": "aarch64"},
                 {"TAKT_TEST_OS": "Darwin", "TAKT_TEST_ARCH": "x86_64"},
                 {"TAKT_TARGET": TARGETS[2]}, {"TAKT_TARGET": "../../untrusted"},
                 {"TAKT_TEST_OS": "Darwin", "TAKT_TEST_ARCH": "arm64", "TAKT_TARGET": TARGETS[0]})
        for environment in cases:
            with self.subTest(environment=environment):
                result = self.command("taktw", "--version", env=environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stderr.strip())
        self.assertEqual(self.requests(), [])

    def test_older_platform_baselines_fail_before_network(self):
        for environment in ({"TAKT_TEST_KERNEL": "5.14.21"},
                            {"TAKT_TEST_OS": "Darwin", "TAKT_TEST_ARCH": "arm64", "TAKT_TEST_MACOS": "14.7"}):
            with self.subTest(environment=environment):
                result = self.command("taktw", "--version", env=environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stderr.strip())
        self.assertEqual(self.requests(), [])

    def test_installed_binary_can_run_on_platform_without_release_artifact(self):
        binary = self.install_system_binary()
        result = self.successful(self.command(env={"TAKT_USE_SYSTEM": "1", "TAKT_TEST_OS": "OtherOS",
                                                   "TAKT_TEST_ARCH": "other", "TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(json.loads(result.stdout)["source"], str(binary.resolve()))
        self.assertEqual(self.requests(), [])

    def test_installer_help_and_bad_flags_do_not_download(self):
        result = self.successful(self.command("install.sh", "--help"))
        self.assertIn("--bin-dir", result.stdout + result.stderr)
        for args in (("--bin-dir",), ("--unknown",)):
            self.assertNotEqual(self.command("install.sh", *args).returncode, 0)
        self.assertEqual(self.requests(), [])

    def test_installer_accepts_piped_script_and_arguments(self):
        destination = self.work / "piped bin"
        result = subprocess.run(
            ["/bin/sh", "-s", "--", "--bin-dir", str(destination)], env=self.env,
            input=(self.generated / "install.sh").read_text(), capture_output=True,
            text=True, cwd=self.work, timeout=15,
        )
        self.successful(result)
        self.assertEqual((destination / "takt").read_text(), FIXTURE_BINARY)

    def test_truncated_piped_installer_cannot_start_installation(self):
        script = (self.generated / "install.sh").read_text()
        for prefix in (script[:len(script) // 2], script.rstrip().rsplit("\n", 1)[0] + "\n"):
            subprocess.run(["/bin/sh"], env=self.env, input=prefix, capture_output=True,
                           text=True, cwd=self.work, timeout=5)
        self.assertEqual(self.requests(), [])
        self.assertFalse(self.cache.exists())
        self.assertFalse((self.home / ".local").exists())

    def test_installer_default_directory_and_repeat_reuse_cache(self):
        profile = self.home / ".profile"
        profile.write_text("# preserve this file\n")
        self.successful(self.command("install.sh"))
        installed = self.home / ".local/bin/takt"
        self.assertEqual(installed.read_text(), FIXTURE_BINARY)
        self.assertTrue(os.access(installed, os.X_OK))
        self.successful(self.command("install.sh", env={"TAKT_TEST_CURL_MODE": "offline"}))
        self.assertEqual(len(self.requests()), 1)
        self.assertEqual(profile.read_text(), "# preserve this file\n")
        self.assert_no_staging()

    def test_installer_bin_dir_flag_overrides_environment_with_spaces(self):
        destination = self.work / "chosen bin path"
        other = self.work / "unused bin"
        self.successful(self.command("install.sh", "--bin-dir", destination, env={"TAKT_INSTALL_DIR": str(other)}))
        self.assertEqual((destination / "takt").read_text(), FIXTURE_BINARY)
        self.assertFalse(other.exists())

    def test_installer_environment_directory(self):
        destination = self.work / "env bin"
        self.successful(self.command("install.sh", env={"TAKT_INSTALL_DIR": str(destination)}))
        self.assertEqual((destination / "takt").read_text(), FIXTURE_BINARY)

    def test_installer_replaces_existing_regular_file(self):
        destination = self.work / "bin"
        destination.mkdir()
        old = destination / "takt"
        old.write_text("old binary")
        self.successful(self.command("install.sh", "--bin-dir", destination))
        self.assertEqual(old.read_text(), FIXTURE_BINARY)
        self.assertEqual([p.name for p in destination.iterdir()], ["takt"])

    def test_installer_refuses_symlink_and_directory_destinations(self):
        destination = self.work / "bin"
        destination.mkdir()
        unrelated = self.work / "unrelated"
        unrelated.write_text("preserve me")
        link = destination / "takt"
        link.symlink_to(unrelated)
        result = self.command("install.sh", "--bin-dir", destination)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(link.is_symlink())
        self.assertEqual(unrelated.read_text(), "preserve me")
        link.unlink()
        link.mkdir()
        result = self.command("install.sh", "--bin-dir", destination)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(link.is_dir())

    def test_installer_failure_keeps_existing_binary(self):
        destination = self.work / "bin"
        destination.mkdir()
        old = destination / "takt"
        old.write_text("keep old binary")
        result = self.command("install.sh", "--bin-dir", destination, env={"TAKT_TEST_CURL_MODE": "corrupt"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(old.read_text(), "keep old binary")
        self.assert_no_staging()

    def test_shasum_fallback_verifies_archive(self):
        if not (self.tools / "shasum").exists():
            self.skipTest("shasum is not available on this host")
        (self.tools / "sha256sum").unlink(missing_ok=True)
        self.successful(self.command("taktw", "--version"))
        self.assertEqual(len(self.requests()), 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
