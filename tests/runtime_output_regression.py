"""Output settlement regressions; fixtures are retained, never bulk-deleted."""
import os
import ctypes
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "runtime/coding-tools-mcp"))
sys.path.insert(0, str(ROOT / "runtime/coding-tools-mcp/site-packages"))
from coding_tools_mcp.processes import ExecSession, RetainedExecOutput, start_reader_threads
from coding_tools_mcp.server import Runtime


class FakeProcess:
    stdin = stdout = stderr = None

    def __init__(self, code=0):
        self.returncode = code
        self.calls = 0

    def poll(self):
        self.calls += 1
        return self.returncode


def runtime_for(session):
    runtime = Runtime.__new__(Runtime)
    runtime.sessions = {session.session_id: session}
    runtime.output_sessions = {}
    runtime.sessions_lock = threading.RLock()
    return runtime


class OutputSettlementTests(unittest.TestCase):
    def delayed_reader(self, session, stream, marker):
        release = threading.Event()
        ready = threading.Event()

        def read():
            ready.set()
            release.wait(10)
            getattr(session, "append_" + stream)(marker)

        reader = threading.Thread(target=read, daemon=True)
        session.reader_threads.append(reader)
        reader.start()
        self.assertTrue(ready.wait(2))
        self.addCleanup(reader.join, 2)
        self.addCleanup(release.set)
        return release, reader

    def test_exit_after_snapshot_observation_cannot_upgrade_old_bytes(self):
        process = FakeProcess(None)
        session = ExecSession("transition", process)
        with patch.object(session, "refresh_status", side_effect=lambda: None):
            # Exit occurs after status observation, before snapshot formatting.
            process.returncode = 0
            first = session.snapshot_since_cursor(65536)
        self.assertEqual(first["status"], "running")
        self.assertIsNone(first["exit_code"])
        self.assertEqual(process.calls, 0)
        session.append_stdout(b"final-marker")
        final = session.snapshot_since_cursor(65536)
        self.assertEqual((final["status"], final["stdout"], final["exit_code"]), ("exited", "final-marker", 0))

    def test_delayed_stdout_and_stderr_are_delivered_before_terminal(self):
        session = ExecSession("two-streams", FakeProcess())
        out, out_thread = self.delayed_reader(session, "stdout", b"OUT")
        err, err_thread = self.delayed_reader(session, "stderr", b"ERR")
        runtime = runtime_for(session)
        self.assertIsNone(runtime._complete_session(session))
        self.assertIn(session.session_id, runtime.sessions)
        first = session.snapshot_since_cursor(65536)
        self.assertEqual(first["status"], "running")
        out.set()
        out_thread.join(2)
        middle = session.snapshot_since_cursor(65536)
        self.assertEqual((middle["status"], middle["stdout"]), ("running", "OUT"))
        err.set()
        err_thread.join(2)
        last = session.snapshot_since_cursor(65536)
        self.assertEqual((last["status"], last["stdout"], last["stderr"]), ("exited", "", "ERR"))
        retained = runtime._complete_session(session)
        self.assertIsInstance(retained, RetainedExecOutput)
        self.assertNotIn(session.session_id, runtime.sessions)
        self.assertEqual(retained.snapshot_since_cursor(65536)["stderr"], "")

    def test_snapshot_and_archive_share_one_cursor_under_contention(self):
        session = ExecSession("concurrent", FakeProcess())
        session.append_stdout(b"ONLY_ONCE")
        runtime = runtime_for(session)
        barrier = threading.Barrier(9)
        responses, errors = [], []

        def consume(index):
            try:
                barrier.wait(timeout=3)
                target = runtime._complete_session(session) if index % 2 else session
                responses.append(target.snapshot_since_cursor(65536))
            except BaseException as error:
                errors.append(error)

        threads = [threading.Thread(target=consume, args=(i,), daemon=True) for i in range(8)]
        for thread in threads:
            thread.start()
        barrier.wait(timeout=3)
        for thread in threads:
            thread.join(3)
            self.assertFalse(thread.is_alive(), "snapshot/archive lock inversion")
        self.assertEqual(errors, [])
        self.assertEqual("".join(r["stdout"] for r in responses), "ONLY_ONCE")
        self.assertIs(runtime._complete_session(session), session.retained)
        self.assertEqual(session.snapshot_since_cursor(65536)["stdout"], "")

    def test_pipe_deadline_is_shared_and_truncation_survives_archival(self):
        session = ExecSession("deadline", FakeProcess())
        release, reader = self.delayed_reader(session, "stdout", b"TOO_LATE")
        session.append_stdout(b"KEPT")
        with patch("coding_tools_mcp.processes.time.monotonic", return_value=100), patch.object(session, "drain_readers"):
            first = session.snapshot_since_cursor(65536)
        self.assertEqual((first["status"], first["stdout"]), ("running", "KEPT"))
        with patch("coding_tools_mcp.processes.time.monotonic", return_value=104.9), patch.object(session, "drain_readers"):
            self.assertEqual(session.snapshot_since_cursor(65536)["status"], "running")
        with patch("coding_tools_mcp.processes.time.monotonic", return_value=105.0), patch.object(session, "drain_readers"):
            final = session.snapshot_since_cursor(65536)
        self.assertEqual(final["status"], "exited")
        self.assertTrue(final["truncated"])
        self.assertIn("Output incomplete", final["warnings"][0])
        start = time.monotonic()
        retained = RetainedExecOutput.capture(session, threading.RLock())
        self.assertLess(time.monotonic() - start, 0.5, "cleanup blocked on a live reader")
        self.assertTrue(retained.snapshot_since_cursor(65536)["truncated"])
        release.set()
        reader.join(2)
        self.assertEqual(retained.retained_stream_bytes("stdout")[0], b"KEPT")
        self.assertEqual(session.snapshot_since_cursor(65536)["stdout"], "")

    def test_empty_output_completes_without_truncation(self):
        session = ExecSession("empty", FakeProcess())
        result = session.snapshot_since_cursor(65536)
        self.assertEqual((result["status"], result["exit_code"], result["stdout"]), ("exited", 0, ""))
        self.assertFalse(result["truncated"])

    def test_open_inherited_pipe_reader_stops_at_the_drain_deadline(self):
        read_fd, write_fd = os.pipe()
        process = FakeProcess()
        process.stdout = os.fdopen(read_fd, "rb")
        stream = process.stdout
        session = ExecSession("inherited-pipe", process)
        try:
            start_reader_threads(session)
            session.exit_observed_at = time.monotonic() - 6
            response = session.snapshot_since_cursor(65536)
            self.assertTrue(response["truncated"])
            self.assertTrue(session.reader_stop.is_set())
            RetainedExecOutput.capture(session, threading.RLock())
            for reader in list(session.reader_threads):
                reader.join(1)
                self.assertFalse(reader.is_alive(), "reader ignored drain cancellation")
            self.assertTrue(stream.closed)
        finally:
            os.close(write_fd)

    @unittest.skipUnless(os.name == "nt", "Windows runtime publication regression")
    def test_exec_installs_readers_before_publishing_a_fast_session(self):
        workspace = Path(tempfile.mkdtemp(prefix="localbridge-output-publication-"))
        print(f"TEST_WORKSPACE_RETAINED path={workspace}", flush=True)
        runtime = Runtime(workspace, permission_mode="dangerous")
        published = []

        class Sessions(dict):
            def __setitem__(self, key, session):
                published.append(session)
                if len(session.reader_threads) != 2 or session.watchdog_thread is None:
                    raise AssertionError("session published before reader/watchdog setup")
                return super().__setitem__(key, session)

        runtime.sessions = Sessions()
        result = runtime.exec_command({"cmd": "echo PUBLICATION_OK", "yield_time_ms": 0, "timeout_ms": 30000})
        output = result["stdout"]
        deadline = time.monotonic() + 30
        while result["status"] == "running":
            self.assertLess(time.monotonic(), deadline)
            result = runtime.write_stdin({"session_id": result["session_id"], "chars": "", "yield_time_ms": 10})
            output += result["stdout"]
        self.assertEqual(output.count("PUBLICATION_OK"), 1)
        self.assertEqual(result["exit_code"], 0)
        self.assertEqual(len(published), 1)
        self.assertEqual(runtime.sessions, {})
        # Runtime.close removes its fixture tree. Retain it under repo policy.
        runtime.telemetry.finish()

    @unittest.skipUnless(os.name == "nt", "Windows handle accounting")
    def test_terminal_archives_do_not_accumulate_os_handles(self):
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.GetProcessHandleCount.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]

        def handles():
            count = ctypes.c_ulong()
            self.assertTrue(kernel.GetProcessHandleCount(kernel.GetCurrentProcess(), ctypes.byref(count)))
            return count.value

        workspace = Path(tempfile.mkdtemp(prefix="localbridge-output-handles-"))
        print(f"TEST_WORKSPACE_RETAINED path={workspace}", flush=True)
        runtime = Runtime(workspace, permission_mode="dangerous")

        def run(marker):
            result = runtime.exec_command({"cmd": f"echo {marker}", "yield_time_ms": 5000, "timeout_ms": 10000})
            output = result["stdout"]
            deadline = time.monotonic() + 15
            while result["status"] == "running":
                self.assertLess(time.monotonic(), deadline)
                result = runtime.write_stdin({"session_id": result["session_id"], "chars": "", "yield_time_ms": 10})
                output += result["stdout"]
            self.assertEqual(output.count(marker), 1)
            self.assertEqual(result["exit_code"], 0)
            return result

        run("WARMUP")
        time.sleep(0.25)
        baseline = handles()
        for index in range(24):
            last = run(f"RETAINED_{index}_END")
        retained = runtime.read_output({"output_ref": f"session:{last['session_id']}:stdout", "offset": 0, "limit": 4096})
        self.assertIn("RETAINED_23_END", retained["content"])
        time.sleep(0.25)
        after = handles()
        print(f"HANDLE_REGRESSION baseline={baseline} after={after} max_growth=8", flush=True)
        self.assertLessEqual(after, baseline + 8)
        self.assertEqual(runtime.sessions, {})
        runtime.telemetry.finish()

    def test_timeout_remains_running_until_output_settles(self):
        session = ExecSession("timeout", FakeProcess(), timed_out=True)
        release, reader = self.delayed_reader(session, "stderr", b"before-timeout")
        result = session.snapshot_since_cursor(65536)
        self.assertEqual(result["status"], "running")
        self.assertFalse(result["timed_out"])
        release.set()
        reader.join(2)
        result = session.snapshot_since_cursor(65536)
        self.assertEqual(result["status"], "timeout")
        self.assertTrue(result["timed_out"])
        self.assertEqual(result["stderr"], "before-timeout")

    def test_kill_does_not_evict_exited_session_with_pending_output(self):
        session = ExecSession("kill-drain", FakeProcess())
        release, reader = self.delayed_reader(session, "stdout", b"LAST")
        runtime = runtime_for(session)
        result = runtime.kill_session({"session_id": session.session_id, "wait_ms": 0})
        self.assertEqual(result["status"], "running")
        self.assertFalse(result["evicted"])
        self.assertIn(session.session_id, runtime.sessions)
        release.set()
        reader.join(2)
        final = runtime.write_stdin({"session_id": session.session_id, "chars": "", "yield_time_ms": 0})
        self.assertEqual((final["status"], final["stdout"]), ("exited", "LAST"))

    @unittest.skipUnless(os.name == "nt", "Windows shell regression")
    def test_real_windows_shells_ten_runs_each(self):
        workspace = Path(tempfile.mkdtemp(prefix="localbridge-output-regression-"))
        print(f"TEST_WORKSPACE_RETAINED path={workspace}", flush=True)
        for extension in ("cmd", "bat", "ps1"):
            for iteration in range(10):
                with self.subTest(shell=extension, iteration=iteration):
                    marker = f"OUTPUT_{extension}_{iteration}_OK"
                    script = workspace / f"probe-{iteration}.{extension}"
                    script.write_text((f"Write-Output '{marker}'\r\n" if extension == "ps1" else f"@echo {marker}\r\n"), encoding="ascii", newline="")
                    system = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32"
                    args = ([str(system / "WindowsPowerShell/v1.0/powershell.exe"), "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(script)] if extension == "ps1" else [str(system / "cmd.exe"), "/d", "/c", str(script)])
                    process = subprocess.Popen(args, cwd=workspace, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
                    session = ExecSession(f"real-{extension}-{iteration}", process)
                    start_reader_threads(session)
                    runtime = runtime_for(session)
                    output, trace = "", []
                    deadline = time.monotonic() + 60
                    try:
                        while True:
                            response = runtime.write_stdin({"session_id": session.session_id, "chars": "", "yield_time_ms": 10})
                            output += response["stdout"] + response["stderr"]
                            trace.append((response["status"], response["exit_code"], response["stdout"], response["stderr"]))
                            if response["status"] != "running":
                                break
                            self.assertLess(time.monotonic(), deadline, trace)
                        self.assertEqual(response["exit_code"], 0, trace)
                        self.assertFalse(response["truncated"], trace)
                        self.assertEqual(output.count(marker), 1, trace)
                        replay = runtime.write_stdin({"session_id": session.session_id, "chars": "", "yield_time_ms": 0})
                        self.assertEqual(replay["stdout"] + replay["stderr"], "", trace)
                    finally:
                        if process.poll() is None:
                            process.kill()
                        process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main(verbosity=2)
