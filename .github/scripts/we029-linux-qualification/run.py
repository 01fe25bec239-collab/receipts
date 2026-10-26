#!/usr/bin/env python3
"""Bounded, synthetic VM qualification. Never import this as product code."""
import base64
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import resource
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

BASELINE = "1811205f30f6993a373528f6cce67c489c19c6db"
BRANCH = "build/workspace-execution-a3-029-linux-qualification"
REPO = "01fe25bec239-collab/receipts"
QUALIFICATION_PR = 185
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
WORKFLOW = ".github/workflows/we029-linux-qualification.yml"
PREFIX = ".github/scripts/we029-linux-qualification/"
PUBLIC = ("provenance.json", "results.json", "raw.jsonl")
MAX_RAW = 1024 * 1024
MAX_STREAM = 128 * 1024
C, W = 61001, 61002
CASES = ("preflight", "worker", "lifecycle", "owned-failure", "controller")
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C", "HOME": "/nonexistent",
       "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0"}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def allowed(path):
    return path == WORKFLOW or (path.startswith(PREFIX) and ".." not in path.split("/"))


def dump(path, value):
    # Only these generated JSON files are ever published; no arbitrary input paths.
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    path.chmod(0o644)


def limit_child():
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_STREAM, MAX_STREAM))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


class Recorder:
    def __init__(self, out):
        self.out = out

    def run(self, argv, *, cwd=ROOT, timeout=30, expected=0, after_wait=None):
        require(all(isinstance(a, str) and "\0" not in a for a in argv), "invalid argv")
        started = time.monotonic()
        timed_out = False
        # Regular bounded files avoid pipe-holder deadlocks. A size limit is failure,
        # never silently truncated successful evidence. Probe separately tests EOF.
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            p = subprocess.Popen(argv, cwd=cwd, env=ENV, stdin=subprocess.DEVNULL,
                                 stdout=stdout, stderr=stderr, start_new_session=True,
                                 close_fds=True, preexec_fn=limit_child)
            try:
                code = p.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(p.pid, signal.SIGKILL)
                code = p.wait(timeout=5)
            cleanup_error = None
            if after_wait:
                try:
                    after_wait()
                except Exception as exc:
                    cleanup_error = str(exc)
            stdout.seek(0)
            stderr.seek(0)
            raw_out, raw_err = stdout.read(), stderr.read()
        item = {"argv": argv, "cwd": str(cwd), "exit": code, "timeout": timed_out,
                "seconds": round(time.monotonic() - started, 3),
                "cleanup_error": cleanup_error,
                "probe_writers_quiescent": cleanup_error is None if after_wait else None,
                "stdout_b64": base64.b64encode(raw_out).decode(),
                "stderr_b64": base64.b64encode(raw_err).decode(),
                "stdout_sha256": hashlib.sha256(raw_out).hexdigest(),
                "stderr_sha256": hashlib.sha256(raw_err).hexdigest(),
                "stdout_bytes": len(raw_out), "stderr_bytes": len(raw_err)}
        line = (json.dumps(item, sort_keys=True) + "\n").encode()
        log = self.out / "raw.jsonl"
        require(log.stat().st_size + len(line) <= MAX_RAW, "raw evidence bound exceeded")
        with log.open("ab") as f:
            f.write(line)
        require(cleanup_error is None, f"UNKNOWN CLEANUP: {cleanup_error}")
        require(not timed_out and len(raw_out) < MAX_STREAM and len(raw_err) < MAX_STREAM,
                f"timeout/output bound: {argv[0]}")
        require(code == expected, f"command refused: {argv} exit={code}")
        return raw_out


def git(rec, repo, *args):
    return rec.run(["/usr/bin/git", "-C", str(repo), *args])


def verify_source(rec):
    head = os.environ.get("QUALIFICATION_HEAD", "")
    require(re.fullmatch(r"[0-9a-f]{40}", head), "invalid exact PR head")
    require(os.environ.get("QUALIFICATION_BASELINE") == BASELINE, "baseline mismatch")
    require(git(rec, ROOT, "rev-parse", "HEAD").decode().strip() == head, "checkout differs from PR head")
    git(rec, ROOT, "cat-file", "-e", BASELINE + "^{commit}")
    git(rec, ROOT, "merge-base", "--is-ancestor", BASELINE, head)
    require(not git(rec, ROOT, "status", "--porcelain=v1", "--untracked-files=all"), "checkout dirty")
    paths = git(rec, ROOT, "diff", "--name-only", "--no-renames", "-z", BASELINE, head).decode().split("\0")[:-1]
    require(paths and all(allowed(p) for p in paths), "changed path outside qualification allowlist")
    require(WORKFLOW in paths and PREFIX + "probe.c" in paths, "incomplete qualification change")
    for path in paths:
        entry = git(rec, ROOT, "ls-tree", head, "--", path).decode()
        require(entry.startswith("100644 blob "), "only regular non-executable qualification sources admitted")
    git(rec, ROOT, "diff", "--check", BASELINE, head)
    return head, paths


def admit_event(event, env):
    require(env.get("GITHUB_EVENT_NAME") == "pull_request", "wrong event")
    require(event["action"] in ("opened", "reopened") and env.get("GITHUB_RUN_ATTEMPT") == "1",
            "requires opened/reopened attempt1")
    pr = event["pull_request"]
    require(event["number"] == QUALIFICATION_PR and pr["number"] == QUALIFICATION_PR, "requires PR185")
    require(env.get("GITHUB_REPOSITORY") == REPO and pr["head"]["repo"]["full_name"] == REPO and
            pr["base"]["repo"]["full_name"] == REPO, "foreign repository")
    require(pr["head"]["ref"] == BRANCH and pr["base"]["ref"] == "main", "wrong qualification branch")
    require(re.fullmatch(r"[0-9a-f]{40}", env.get("QUALIFICATION_HEAD", "")) and
            pr["head"]["sha"] == env.get("QUALIFICATION_HEAD"), "event/head mismatch")
    require(re.fullmatch(r"[1-9][0-9]*", env.get("GITHUB_RUN_ID", "")), "missing or invalid run ID")
    return {"event_name": env["GITHUB_EVENT_NAME"], "event_action": event["action"],
            "repository": env["GITHUB_REPOSITORY"], "pr_number": pr["number"],
            "run_id": env["GITHUB_RUN_ID"], "run_attempt": env["GITHUB_RUN_ATTEMPT"]}


def prepare(out):
    require(platform.system() == "Linux" and platform.machine() == "x86_64", "requires Linux x86_64")
    require(os.geteuid() != 0, "compile/provenance must run unprivileged")
    require(not out.exists(), "refuse reuse of evidence directory")
    out.mkdir(mode=0o755)
    (out / "raw.jsonl").touch(mode=0o644)
    dump(out / "provenance.json", {"checkout_sha": "UNVERIFIED", "qualification_baseline": BASELINE})
    dump(out / "results.json", {"status": "HOSTED CHECKS NOT RUN", "cases": [{"case": c, "outcome": "NOT RUN"} for c in CASES]})
    rec = Recorder(out)
    # Verify event, not just an attacker-controlled shell variable.
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    event_identity = admit_event(event, os.environ)
    dump(out / "provenance.json", {"checkout_sha": "UNVERIFIED", "qualification_baseline": BASELINE,
                                  **event_identity})
    head, paths = verify_source(rec)
    sources = [WORKFLOW] + sorted(str(p.relative_to(ROOT)) for p in HERE.iterdir() if p.is_file())
    provenance = {"checkout_sha": head, "harness_sha": head, "qualification_baseline": BASELINE,
                  "parent": git(rec, ROOT, "rev-parse", "HEAD^").decode().strip(),
                  "tree": git(rec, ROOT, "rev-parse", "HEAD^{tree}").decode().strip(),
                  "changed_paths": paths, "source_sha256": {p: sha(ROOT / p) for p in sources},
                  **event_identity,
                  "ImageOS": os.environ.get("ImageOS"), "ImageVersion": os.environ.get("ImageVersion"),
                  "platform": platform.uname()._asdict(), "scope": "synthetic qualification only"}
    require(provenance["ImageOS"] and provenance["ImageVersion"], "missing actual image identity")
    dump(out / "provenance.json", provenance)
    rec.run(["/usr/bin/gcc", "--version"])
    rec.run(["/usr/bin/git", "--version"])
    rec.run(["/usr/bin/python3", "--version"])
    rec.run(["/usr/bin/ldd", "--version"])
    rec.run(["/usr/bin/findmnt", "--version"])
    rec.run(["/usr/bin/stat", "--version"])
    rec.run(["/usr/bin/gcc", "-std=gnu11", "-O2", "-pipe", "-Wall", "-Wextra", "-Werror",
             "-Wformat=2", "-Wconversion", "-o", str(out / "probe"), str(HERE / "probe.c")])
    provenance["probe_sha256"] = sha(out / "probe")
    dump(out / "provenance.json", provenance)
    dump(out / "results.json", {"status": "HOSTED CHECKS NOT RUN", "cases": [{"case": c, "outcome": "NOT RUN"} for c in CASES]})


def copy_tool(rec, root, source, destination):
    dest = root / destination.lstrip("/")
    for parent in reversed(dest.relative_to(root).parents[:-1]):
        (root / parent).mkdir(exist_ok=True)
        (root / parent).chmod(0o755)
    shutil.copyfile(source, dest)
    dest.chmod(0o755)
    deps = rec.run(["/usr/bin/ldd", str(source)]).decode()
    require("not found" not in deps, "missing trusted tool dependency")
    for lib in re.findall(r"(?:=>\s+)?(/[^\s()]+)", deps):
        require(lib.startswith(("/lib/", "/lib64/", "/usr/lib/")), "unexpected dependency location")
        target = root / lib.lstrip("/")
        for parent in reversed(target.relative_to(root).parents[:-1]):
            (root / parent).mkdir(exist_ok=True)
            (root / parent).chmod(0o755)
        shutil.copyfile(lib, target)
        target.chmod(0o755)


def create_null(path, receipt):
    """Create and verify the sole fixture device, independent of inherited umask."""
    mask = os.umask(0o777)
    os.umask(mask)
    receipt["umask"] = oct(mask)
    os.mknod(path, stat.S_IFCHR | 0o666, os.makedev(1, 3))
    for stage in ("created", "final"):
        if stage == "final":
            os.chown(path, 0, 0)
            path.chmod(0o666)
        s = path.lstat()
        receipt[stage] = {"mode": oct(stat.S_IMODE(s.st_mode)), "uid": s.st_uid, "gid": s.st_gid,
                          "character": stat.S_ISCHR(s.st_mode),
                          "major": os.major(s.st_rdev), "minor": os.minor(s.st_rdev)}
        require(stat.S_ISCHR(s.st_mode) and s.st_rdev == os.makedev(1, 3), "wrong null device")
    require(stat.S_IMODE(s.st_mode) == 0o666 and s.st_uid == 0 and s.st_gid == 0,
            "wrong null device permissions/owner")


def fixture(rec, out, root, report):
    root.chmod(0o755)
    for directory in ("scope", "proc", "cgroups", "tool", "lib", "lib64", "dev"):
        (root / directory).mkdir(mode=0o755)
        (root / directory).chmod(0o755)
    # A regular synthetic /dev/null would be wrong; this fixed device is the only device.
    report["null_device"] = {}
    create_null(root / "dev/null", report["null_device"])
    rec.run(["/usr/bin/findmnt", "-n", "-T", str(root / "dev/null"), "-o", "TARGET,FSTYPE,OPTIONS"])
    (root / "outside").write_text("outside-synthetic\n")
    (root / "outside").chmod(0o600)
    copy_tool(rec, root, Path("/usr/bin/git"), "/tool/git")
    copy_tool(rec, root, out / "probe", "/tool/probe")
    repo = root / "scope"
    git(rec, repo, "init", "-b", "we029-task")
    for key, value in (("user.name", "Synthetic"), ("user.email", "synthetic@example.invalid"),
                       ("core.hooksPath", "/nonexistent"), ("core.fsmonitor", "false"),
                       ("maintenance.auto", "false"), ("gc.auto", "0"), ("commit.gpgSign", "false"),
                       ("extensions.worktreeConfig", "true"), ("include.path", "admitted")):
        git(rec, repo, "config", key, value)
    # Empty admitted sources are protected too; no executable filter or external include.
    (repo / ".git/admitted").write_text("[qualification]\n\tsynthetic = true\n")
    (repo / ".git/config.worktree").write_text("[qualification]\n\tworktree = true\n")
    for name in ("edit", "delete", "bait"):
        (repo / name).write_text("initial\n")
    git(rec, repo, "add", "--", "edit", "delete", "bait")
    git(rec, repo, "commit", "-m", "synthetic baseline")
    initial = git(rec, repo, "rev-parse", "HEAD").decode().strip()
    for path in (repo / ".git", *(repo / ".git").rglob("*")):
        require(not path.is_symlink(), "unexpected metadata symlink")
        os.chown(path, C, C)
        path.chmod(0o700 if path.is_dir() else 0o600)
    os.chown(root, C, C)
    os.chown(repo, C, W)
    repo.chmod(0o1770)
    for name in ("edit", "delete", "bait"):
        os.chown(repo / name, W, W)
        (repo / name).chmod(0o600)
    return initial


def metadata_snapshot(repo):
    result = {}
    for path in (repo, repo / ".git", *(repo / ".git").rglob("*")):
        s = path.lstat()
        result[str(path.relative_to(repo))] = [s.st_ino, s.st_mode, s.st_uid, s.st_gid,
                                             sha(path) if path.is_file() else None]
    return result


def reap_owned(results):
    end = time.monotonic() + 5
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            results.append({"all_adopted_children_reaped": True})
            return
        if pid:
            results.append({"adopted_pid": pid, "wait_status": status})
        else:
            require(time.monotonic() < end, "UNKNOWN CLEANUP: adopted child reap deadline")
            time.sleep(0.02)


def cleanup_cgroup(path, results):
    # Never reuse UIDs or erase fixture evidence when quiescence is unknown.
    (path / "cgroup.kill").write_text("1")
    end = time.monotonic() + 5
    while time.monotonic() < end:
        events = (path / "cgroup.events").read_text()
        if "populated 0\n" in events:
            results.append({"cgroup": path.name, "kill_written": True, "events": events})
            path.rmdir()
            return
        time.sleep(0.02)
    raise RuntimeError("UNKNOWN CLEANUP: retained cgroup and fixture; stop")


def check_process_identity(entry):
    """Ignore vanished processes; refuse malformed live status or reserved IDs."""
    try:
        status = (entry / "status").read_text()
    except (FileNotFoundError, ProcessLookupError):
        return
    fields = re.findall(r"^(Uid|Gid):[ \t]*(.*)$", status, re.M)
    ids = dict(fields)
    if len(fields) != 2 or set(ids) != {"Uid", "Gid"} or not all(
            re.fullmatch(r"[0-9]+(?:[ \t]+[0-9]+){3}[ \t]*", value) for value in ids.values()):
        try:
            entry.stat()
        except (FileNotFoundError, ProcessLookupError):
            return
        raise RuntimeError("malformed live process identity")
    require(not ({C, W} & {int(value) for row in ids.values() for value in row.split()}),
            "active fixture identity conflict")


def hosted(out):
    require(platform.system() == "Linux" and platform.machine() == "x86_64" and os.geteuid() == 0,
            "hosted Linux x64 administrator setup only")
    provenance = json.loads((out / "provenance.json").read_text())
    require(provenance["checkout_sha"] == os.environ.get("QUALIFICATION_HEAD"), "subject changed")
    require(os.environ.get("QUALIFICATION_BASELINE") == BASELINE, "baseline changed")
    require(sha(out / "probe") == provenance["probe_sha256"], "binary changed after prepare")
    require(all(sha(ROOT / p) == h for p, h in provenance["source_sha256"].items()), "source changed")
    require(platform.uname()._asdict() == provenance["platform"], "platform changed")
    require(os.environ.get("ImageOS") == provenance["ImageOS"] and
            os.environ.get("ImageVersion") == provenance["ImageVersion"], "image changed")
    require(Path("/sys/fs/cgroup/cgroup.controllers").is_file(), "cgroup v2 unavailable")
    require(Path("/proc/sys/fs/protected_hardlinks").read_text().strip() == "1", "unsafe hardlink policy")
    require(not any(f"{uid}:" in Path("/etc/passwd").read_text() for uid in (C, W)), "fixture UID conflict")
    require(not any(f"{gid}:" in Path("/etc/group").read_text() for gid in (C, W)), "fixture GID conflict")
    # Existing numeric owners could create a collision even without passwd entries.
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            check_process_identity(entry)
    libc = ctypes.CDLL(None, use_errno=True)
    require(libc.prctl(36, 1, 0, 0, 0) == 0, "cannot become owned-child subreaper")
    rec = Recorder(out)
    report = {"status": "REFUSAL", "cases": [{"case": c, "outcome": "NOT RUN"} for c in CASES], "cleanup": [],
              "ImageOS": provenance["ImageOS"], "ImageVersion": provenance["ImageVersion"],
              "limits": "fixture only; no product, compatibility or assurance verdict"}
    root = None
    parent = None
    groups = []
    cleanup_known = False
    cleanup_failed = []

    def release(group):
        groups.remove(group)  # Exactly one cleanup attempt per owned cgroup.
        try:
            cleanup_cgroup(group, report["cleanup"])
            reap_owned(report["cleanup"])
        except Exception:
            cleanup_failed.append(str(group))
            raise

    try:
        rec.run(["/usr/bin/findmnt", "-n", "-o", "FSTYPE,OPTIONS", "/sys/fs/cgroup"])
        root = Path(tempfile.mkdtemp(prefix="we029-", dir="/tmp"))
        initial = fixture(rec, out, root, report)
        report["fixture"] = str(root)
        report["initial_sha"] = initial
        repository_identity = [(root / "scope").stat().st_ino, (root / "scope/.git").stat().st_ino]
        report["repository_identity_before"] = repository_identity
        report["filesystem"] = rec.run(["/usr/bin/stat", "-f", "-c", "%T", str(root)]).decode().strip()
        candidate = Path("/sys/fs/cgroup") / root.name
        candidate.mkdir(mode=0o700)
        parent = candidate
        require((parent / "cgroup.type").read_text().strip() == "domain", "non-domain cgroup")
        # Keep a real empty sibling as an escape target. Never delegate cgroup FDs.
        escape = parent / "escape"
        escape.mkdir(mode=0o700)
        groups.append(escape)
        for index, case in enumerate(CASES):
            group = parent / case
            group.mkdir(mode=0o700)
            groups.append(group)
            before = metadata_snapshot(root / "scope") if case == "worker" else None
            try:
                raw = rec.run([str(out / "probe"), case, str(root), str(group)], timeout=40,
                              expected=72 if case == "owned-failure" else 0,
                              after_wait=lambda: release(group))
                require(f"CASE_COMPLETE {case}\n".encode() in raw, "missing complete case receipt")
                if case == "worker":
                    after = metadata_snapshot(root / "scope")
                    require(before == after, "worker modified protected metadata/ancestors")
                    report["protected_metadata_before_and_after"] = before
                report["cases"][index] = {"case": case, "outcome": "EXPECTED_FAILURE_CLEANED" if case == "owned-failure" else "DEMONSTRATED"}
            except Exception as exc:
                report["cases"][index] = {"case": case, "outcome": "REFUSAL", "reason": str(exc)}
                raise
        # Fixed host observer independently checks actual repository/branch/content.
        # safe.directory applies only to this synthetic path and is never persisted.
        def observe(*args):
            return git(rec, root / "scope", "-c", "safe.directory=" + str(root / "scope"), *args)
        final = observe("rev-parse", "HEAD").decode().strip()
        require(re.fullmatch(r"[0-9a-f]{40}", final) and final != initial, "no actual new SHA")
        require(observe("rev-parse", "HEAD^").decode().strip() == initial, "wrong parent")
        require(observe("symbolic-ref", "--short", "HEAD") == b"we029-task\n", "wrong branch")
        require(not observe("status", "--porcelain=v1", "-z"), "checkpoint is dirty")
        require(observe("diff-tree", "--no-commit-id", "--name-status", "-r", "HEAD") ==
                b"D\tdelete\nM\tedit\nA\tprivate\n", "wrong committed change set")
        require(observe("show", "HEAD:private") == b"worker0600\n", "wrong private content")
        report["checkpoint_sha"] = final
        report["repository_identity_after"] = [(root / "scope").stat().st_ino, (root / "scope/.git").stat().st_ino]
        require(report["repository_identity_after"] == repository_identity, "repository identity changed")
        report["status"] = "SYNTHETIC CASES DEMONSTRATED; NOT ASSURANCE PASS"
    except Exception as exc:
        report["failure"] = str(exc)
        raise
    finally:
        try:
            for group in list(reversed(groups)):
                release(group)
            require(not cleanup_failed, "UNKNOWN CLEANUP: earlier cleanup failed; no retry")
            reap_owned(report["cleanup"])
            if parent:
                parent.rmdir()
            cleanup_known = True
        except Exception as exc:
            report["cleanup_failure"] = str(exc)
            report["status"] = "REFUSAL: UNKNOWN CLEANUP"
        report["cleanup_known"] = cleanup_known
        report["retained_cgroups"] = cleanup_failed + [str(group) for group in groups]
        try:
            if root and cleanup_known:
                shutil.rmtree(root)
                report["synthetic_fixture_removed"] = True
        finally:
            dump(out / "results.json", report)
        require(cleanup_known, "unknown cleanup; retained fixture")


def seal(out):
    require(out.is_dir() and not out.is_symlink(), "no attributable evidence directory")
    # No glob upload, no fixture archive, no arbitrary host-file collection.
    for name in PUBLIC:
        path = out / name
        require(path.is_file() and not path.is_symlink(), f"missing evidence {name}")
        require(path.stat().st_size <= MAX_RAW, f"oversize evidence {name}")
    (out / "SHA256SUMS").write_text("".join(f"{sha(out / n)}  {n}\n" for n in PUBLIC))


def self_check():
    from types import SimpleNamespace
    from unittest import mock
    with tempfile.TemporaryDirectory(prefix="we029-setup-check-") as tmp:
        entry = Path(tmp) / "123"
        entry.mkdir()
        status = entry / "status"
        good = "Uid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n"
        status.write_text(good)
        check_process_identity(entry)
        for error in (FileNotFoundError(), ProcessLookupError()):
            with mock.patch.object(Path, "read_text", side_effect=error):
                check_process_identity(entry)
        for malformed in ("", "Uid:\nGid:\n", "Uid:\t1 1 1 1\n", good.replace("1000", "bad", 1),
                          good + "Uid:\t1 1 1 1\n", good.replace("1000\t1000\t1000\t1000", "1 1 1")):
            status.write_text(malformed)
            for error in (FileNotFoundError(), ProcessLookupError()):
                with mock.patch.object(Path, "stat", side_effect=error):
                    check_process_identity(entry)
            try:
                check_process_identity(entry)
            except RuntimeError as exc:
                require(str(exc) == "malformed live process identity", "wrong live-status refusal")
            else:
                raise RuntimeError("malformed live status accepted")
        for uid in (C, W):
            for field in ("Uid", "Gid"):
                status.write_text(good.replace(field + ":\t1000", field + ":\t" + str(uid)))
                try:
                    check_process_identity(entry)
                except RuntimeError as exc:
                    require(str(exc) == "active fixture identity conflict", "wrong conflict refusal")
                else:
                    raise RuntimeError("live identity conflict accepted")
        for operation in ("read_text", "stat"):
            status.write_text("")
            with mock.patch.object(Path, operation, side_effect=PermissionError("unexpected")):
                try:
                    check_process_identity(entry)
                except PermissionError:
                    pass
                else:
                    raise RuntimeError("unexpected proc error swallowed")
        # Safe FIFO exercises real umask/chmod; only device creation/identity and root chown are mocked.
        real_lstat = Path.lstat
        def device_stat(path):
            s = real_lstat(path)
            return SimpleNamespace(st_mode=stat.S_IFCHR | stat.S_IMODE(s.st_mode), st_uid=0,
                                   st_gid=0, st_rdev=os.makedev(1, 3))
        for mask in (0o022, 0o077, 0o777):
            path = Path(tmp) / ("null-" + str(mask))
            receipt = {}
            previous = os.umask(mask)
            try:
                with mock.patch.object(os, "mknod", side_effect=lambda p, m, d: os.mkfifo(p, stat.S_IMODE(m))) as node, \
                     mock.patch.object(os, "chown") as owner, \
                     mock.patch.object(Path, "lstat", autospec=True, side_effect=device_stat):
                    create_null(path, receipt)
                    node.assert_called_once_with(path, stat.S_IFCHR | 0o666, os.makedev(1, 3))
                    owner.assert_called_once_with(path, 0, 0)
                require(receipt["created"]["mode"] == oct(0o666 & ~mask) and
                        receipt["final"]["mode"] == "0o666" and stat.S_IMODE(path.stat().st_mode) == 0o666,
                        "null mode depends on umask")
                require(os.umask(mask) == mask, "fixture changed inherited umask")
                tool_root = Path(tmp) / ("tools-" + str(mask))
                tool_root.mkdir()
                tool_root.chmod(0o755)
                source = Path(tmp) / "source"
                source.write_bytes(b"synthetic")
                source.chmod(0o600)
                rec = mock.Mock()
                rec.run.return_value = b""
                copy_tool(rec, tool_root, source, "/lib/test/tool")
                require(all(stat.S_IMODE(p.stat().st_mode) == 0o755 for p in (
                    tool_root / "lib", tool_root / "lib/test", tool_root / "lib/test/tool")),
                    "tool access depends on umask")
            finally:
                os.umask(previous)
        with mock.patch.object(os, "mknod"), mock.patch.object(Path, "lstat", return_value=SimpleNamespace(
                st_mode=stat.S_IFREG | 0o666, st_uid=0, st_gid=0, st_rdev=0)):
            try:
                create_null(Path(tmp) / "wrong-device", {})
            except RuntimeError:
                pass
            else:
                raise RuntimeError("regular-file null accepted")
    print("Proc disappearance/live-conflict checks and mocked-device restrictive-umask checks passed")
    require(allowed(WORKFLOW) and allowed(PREFIX + "probe.c"), "allowlist positive")
    require(not any(allowed(p) for p in ("src/lib.rs", ".github/workflows/ci.yml", PREFIX + "../bad")),
            "allowlist negative")
    import copy
    event = {"action": "opened", "number": 185, "pull_request": {
        "number": 185, "head": {"repo": {"full_name": REPO}, "ref": BRANCH, "sha": "a" * 40},
        "base": {"repo": {"full_name": REPO}, "ref": "main"}}}
    env = {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": REPO,
           "GITHUB_RUN_ATTEMPT": "1", "GITHUB_RUN_ID": "12345", "QUALIFICATION_HEAD": "a" * 40}
    for action in ("opened", "reopened"):
        event["action"] = action
        identity = admit_event(event, env)
        require(identity["event_action"] == action and identity["pr_number"] == 185 and
                identity["run_id"] == "12345" and identity["run_attempt"] == "1", "event receipt mismatch")
    refusals = [
        ("event", ("action",), "synchronize"), ("event", ("action",), "closed"),
        ("event", ("number",), 186), ("event", ("pull_request", "number"), 186),
        ("event", ("pull_request", "head", "repo", "full_name"), "foreign/repo"),
        ("event", ("pull_request", "base", "repo", "full_name"), "foreign/repo"),
        ("event", ("pull_request", "head", "ref"), "other-branch"),
        ("event", ("pull_request", "base", "ref"), "other-target"),
        ("event", ("pull_request", "head", "sha"), "b" * 40),
        ("env", ("GITHUB_EVENT_NAME",), "workflow_dispatch"),
        ("env", ("GITHUB_RUN_ATTEMPT",), "2"), ("env", ("GITHUB_REPOSITORY",), "foreign/repo"),
        ("env", ("QUALIFICATION_HEAD",), "bad-head"), ("env", ("GITHUB_RUN_ID",), ""),
    ]
    for target, keys, value in refusals:
        candidate, candidate_env = copy.deepcopy(event), env.copy()
        mapping = candidate if target == "event" else candidate_env
        for key in keys[:-1]:
            mapping = mapping[key]
        mapping[keys[-1]] = value
        try:
            admit_event(candidate, candidate_env)
        except RuntimeError:
            pass
        else:
            raise RuntimeError(f"event gate admitted {target}:{keys}={value}")
    print(f"Event gate: 2 admissions, {len(refusals)} refusals passed")
    with tempfile.TemporaryDirectory(prefix="we029-static-") as tmp:
        out = Path(tmp)
        (out / "raw.jsonl").touch()
        rec = Recorder(out)
        require(rec.run([sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'a\\x00b')"]) == b"a\0b",
                "raw NUL preservation")
        rec.run([sys.executable, "-c", "raise SystemExit(7)"], expected=7)
        try:
            rec.run([sys.executable, "-c", "import time; time.sleep(10)"], timeout=0.05)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("timeout accepted")
        def unknown_cleanup():
            raise RuntimeError("synthetic cleanup refusal")
        try:
            rec.run([sys.executable, "-c", "print('failure evidence')"], after_wait=unknown_cleanup)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("unknown cleanup accepted")
        try:
            rec.run([sys.executable, "-c", "import os; os.write(1, b'x' * 262144)"])
        except RuntimeError:
            pass
        else:
            raise RuntimeError("oversize output accepted")
        records = [json.loads(line) for line in (out / "raw.jsonl").read_text().splitlines()]
        require(len(records) == 5 and records[2]["timeout"], "failure evidence not retained")
        require(records[3]["probe_writers_quiescent"] is False and records[3]["cleanup_error"],
                "unknown cleanup receipt missing")
        require(records[4]["stdout_bytes"] == MAX_STREAM, "output-bound witness missing")
        dump(out / "provenance.json", {"static": True})
        dump(out / "results.json", {"linux": "NOT RUN"})
        seal(out)
        require(len((out / "SHA256SUMS").read_text().splitlines()) == 3, "seal incomplete")
    print("Static self-check passed; Linux probes NOT RUN")


def main():
    require(len(sys.argv) == 2 and sys.argv[1] in ("prepare", "hosted", "seal", "self-check"), "fixed mode required")
    if sys.argv[1] == "self-check":
        self_check()
        return
    out = Path(os.environ["QUALIFICATION_OUT"])
    require(out.is_absolute() and out.name == "we029-evidence", "fixed evidence directory required")
    {"prepare": prepare, "hosted": hosted, "seal": seal}[sys.argv[1]](out)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"REFUSAL: {error}", file=sys.stderr)
        if len(sys.argv) == 2 and sys.argv[1] in ("prepare", "hosted"):
            directory = Path(os.environ.get("QUALIFICATION_OUT", "/nonexistent"))
            if directory.is_dir() and directory.name == "we029-evidence":
                report_path = directory / "results.json"
                report = json.loads(report_path.read_text()) if report_path.is_file() else {"cases": []}
                report["status"] = "REFUSAL: UNKNOWN CLEANUP" if report.get("cleanup_known") is False else "REFUSAL"
                report["failure"] = str(error)
                dump(report_path, report)
        sys.exit(1)
