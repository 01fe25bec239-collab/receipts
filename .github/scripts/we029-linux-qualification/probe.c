#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <linux/audit.h>
#include <linux/capability.h>
#include <linux/filter.h>
#include <linux/landlock.h>
#include <linux/sched.h>
#include <linux/seccomp.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <sched.h>

/* Fixed synthetic probes only. No worker-supplied executable, argv or policy. */
#define CONTROLLER 61001
#define WORKER 61002
#define REQUIRE(x) do { if (!(x)) { fprintf(stderr, "REFUSAL line=%d expression=%s errno=%d\n", __LINE__, #x, errno); exit(70); } } while (0)
static const uint64_t fs_all = (1ULL << 15) - 1; /* Landlock ABI 3, through TRUNCATE. */
static char group_path[4096];

static void denied(const char *name, long result) {
    int e = errno;
    printf("DENIAL name=%s result=%ld errno=%d\n", name, result, e);
    REQUIRE(result == -1 && (e == EACCES || e == EPERM || e == EROFS));
}
static void closed_fd(int fd) {
    errno = 0;
    REQUIRE(fcntl(fd, F_GETFD) == -1 && errno == EBADF);
    errno = 0; REQUIRE(write(fd, "bad", 3) == -1 && errno == EBADF);
    errno = 0; REQUIRE(ftruncate(fd, 0) == -1 && errno == EBADF);
    errno = 0; REQUIRE(fchmod(fd, 0777) == -1 && errno == EBADF);
    printf("CLOSED inherited_metadata_fd=%d write/truncate/chmod=EBADF\n", fd);
}
static void read_text(const char *path, char *buf, size_t size) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    REQUIRE(fd >= 0);
    ssize_t n = read(fd, buf, size - 1);
    REQUIRE(n >= 0 && (size_t)n < size - 1);
    buf[n] = 0;
    REQUIRE(close(fd) == 0);
}
static void write_text(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    REQUIRE(fd >= 0);
    size_t n = strlen(text);
    REQUIRE(write(fd, text, n) == (ssize_t)n);
    REQUIRE(close(fd) == 0);
}
static double now(void) {
    struct timespec t;
    REQUIRE(clock_gettime(CLOCK_MONOTONIC, &t) == 0);
    return (double)t.tv_sec + (double)t.tv_nsec / 1e9;
}
static void tick(void) {
    struct timespec t = {.tv_nsec = 10000000};
    REQUIRE(nanosleep(&t, NULL) == 0);
}
static int wait_bounded(pid_t pid) {
    double end = now() + 15;
    int status;
    while (now() < end) {
        pid_t r = waitpid(pid, &status, WNOHANG);
        REQUIRE(r >= 0);
        if (r == pid) {
            printf("REAP pid=%d status=%d\n", pid, status);
            return status;
        }
        tick();
    }
    REQUIRE(0 && "leader wait deadline");
    return -1;
}
static int populated(void) {
    char p[4200], b[512];
    REQUIRE(snprintf(p, sizeof(p), "%s/cgroup.events", group_path) > 0);
    read_text(p, b, sizeof(b));
    REQUIRE(strstr(b, "populated 0\n") || strstr(b, "populated 1\n"));
    return strstr(b, "populated 1\n") != NULL;
}
static void kill_group(void) {
    char p[4200];
    REQUIRE(snprintf(p, sizeof(p), "%s/cgroup.kill", group_path) > 0);
    write_text(p, "1");
    double end = now() + 5;
    while (populated() && now() < end) tick();
    REQUIRE(!populated());
    printf("CLEANUP cgroup.kill=written populated=0\n");
}
static void identity(int controller) {
    uid_t u = controller ? CONTROLLER : WORKER;
    uint32_t cap = controller ? (1U << CAP_DAC_READ_SEARCH) : 0;
    REQUIRE(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) == 1);
    REQUIRE(prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) == 0);
    for (int c = 0; c <= CAP_LAST_CAP; ++c) {
        if (!(controller && c == CAP_DAC_READ_SEARCH))
            REQUIRE(prctl(PR_CAPBSET_DROP, c, 0, 0, 0) == 0);
    }
    REQUIRE(setgroups(0, NULL) == 0);
    REQUIRE(prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0) == 0);
    REQUIRE(setresgid(u, u, u) == 0);
    REQUIRE(setresuid(u, u, u) == 0);
    struct __user_cap_header_struct h = {_LINUX_CAPABILITY_VERSION_3, 0};
    struct __user_cap_data_struct d[2] = {{0}, {0}};
    d[0].effective = d[0].permitted = d[0].inheritable = cap;
    REQUIRE(syscall(SYS_capset, &h, d) == 0);
    if (controller)
        REQUIRE(prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, CAP_DAC_READ_SEARCH, 0, 0) == 0);
    REQUIRE(prctl(PR_SET_KEEPCAPS, 0, 0, 0, 0) == 0);
}
static void verify_identity(int controller) {
    uid_t u = controller ? CONTROLLER : WORKER;
    uint32_t cap = controller ? (1U << CAP_DAC_READ_SEARCH) : 0;
    struct __user_cap_header_struct h = {_LINUX_CAPABILITY_VERSION_3, 0};
    struct __user_cap_data_struct d[2];
    REQUIRE(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) == 1);
    uid_t r, e, s;
    gid_t gr, ge, gs;
    REQUIRE(getresuid(&r, &e, &s) == 0 && r == u && e == u && s == u);
    REQUIRE(getresgid(&gr, &ge, &gs) == 0 && gr == u && ge == u && gs == u);
    REQUIRE(getgroups(0, NULL) == 0);
    memset(d, 0, sizeof(d));
    REQUIRE(syscall(SYS_capget, &h, d) == 0);
    REQUIRE(d[0].effective == cap && d[0].permitted == cap && d[0].inheritable == cap);
    REQUIRE(d[1].effective == 0 && d[1].permitted == 0 && d[1].inheritable == 0);
    uint64_t bound = 0, ambient = 0;
    for (int c = 0; c <= CAP_LAST_CAP; ++c) {
        int b = prctl(PR_CAPBSET_READ, c, 0, 0, 0);
        int a = prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, c, 0, 0);
        REQUIRE(b >= 0 && a >= 0);
        bound |= (uint64_t)b << c;
        ambient |= (uint64_t)a << c;
    }
    REQUIRE(bound == cap && ambient == cap);
    printf("IDENTITY uid=%u/%u/%u gid=%u/%u/%u groups=0 eff=%x prm=%x inh=%x bnd=%llx amb=%llx inherited_nnp=1\n",
           r, e, s, gr, ge, gs, cap, cap, cap,
           (unsigned long long)bound, (unsigned long long)ambient);
    errno = 0; denied("regain-root", setresuid(0, 0, 0));
}

/* Small explicit syscall policy supplements Landlock's path mediation.
 * Fail closed on non-x86_64 and x32; install before any untrusted descendants.
 * Handle opens bypass pathname resolution with CAP_DAC_READ_SEARCH.
 * Namespace, mount and FD-stealing APIs are unnecessary for this fixed fixture.
 */
#define BLOCK(n) BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_##n, 0, 1), BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM)
static void syscall_policy(void) {
    struct sock_filter f[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JSET | BPF_K, 0x40000000, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        BLOCK(open_by_handle_at), BLOCK(setns), BLOCK(unshare), BLOCK(mount),
        BLOCK(umount2), BLOCK(pivot_root), BLOCK(chroot), BLOCK(ptrace),
        BLOCK(process_vm_readv), BLOCK(process_vm_writev), BLOCK(pidfd_getfd),
        BLOCK(open_tree), BLOCK(move_mount), BLOCK(fsopen), BLOCK(fsmount),
        BLOCK(mount_setattr),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW)
    };
    struct sock_fprog p = {.len = sizeof(f) / sizeof(f[0]), .filter = f};
    REQUIRE(syscall(SYS_seccomp, SECCOMP_SET_MODE_FILTER, 0, &p) == 0);
}
static void landlock_rule(int rules, const char *path, uint64_t access) {
    int fd = open(path, O_PATH | O_CLOEXEC);
    REQUIRE(fd >= 0);
    struct landlock_path_beneath_attr p = {.allowed_access = access, .parent_fd = fd};
    REQUIRE(syscall(SYS_landlock_add_rule, rules, LANDLOCK_RULE_PATH_BENEATH, &p, 0) == 0);
    REQUIRE(close(fd) == 0);
}
static void confine_controller(void) {
    int abi = (int)syscall(SYS_landlock_create_ruleset, NULL, 0, LANDLOCK_CREATE_RULESET_VERSION);
    printf("LANDLOCK abi=%d required=3\n", abi);
    REQUIRE(abi >= 3);
    struct landlock_ruleset_attr a = {.handled_access_fs = fs_all};
    int rules = (int)syscall(SYS_landlock_create_ruleset, &a, sizeof(a), 0);
    REQUIRE(rules >= 0);
    landlock_rule(rules, "/scope", fs_all);
    uint64_t read_exec = LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;
    landlock_rule(rules, "/tool", read_exec);
    landlock_rule(rules, "/lib", read_exec);
    landlock_rule(rules, "/lib64", read_exec);
    landlock_rule(rules, "/dev/null", LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_WRITE_FILE);
    REQUIRE(syscall(SYS_landlock_restrict_self, rules, 0) == 0);
    REQUIRE(close(rules) == 0);
    syscall_policy();
}
static void mount_view(const char *root) {
    REQUIRE(unshare(CLONE_NEWNS) == 0);
    REQUIRE(mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL) == 0);
    char p[4200];
    REQUIRE(snprintf(p, sizeof(p), "%s/proc", root) > 0);
    REQUIRE(mount("proc", p, "proc", MS_NOSUID | MS_NODEV | MS_NOEXEC, NULL) == 0);
    REQUIRE(snprintf(p, sizeof(p), "%s/cgroups", root) > 0);
    REQUIRE(mount("/sys/fs/cgroup", p, NULL, MS_BIND, NULL) == 0);
    REQUIRE(mount(NULL, p, NULL, MS_BIND | MS_REMOUNT | MS_RDONLY | MS_NOSUID | MS_NODEV | MS_NOEXEC, NULL) == 0);
    REQUIRE(chroot(root) == 0 && chdir("/scope") == 0);
}
static void protection_matrix(void) {
    const char *targets[] = {"/scope/.git/config", "/scope/.git/admitted", "/scope/.git/config.worktree",
        "/scope/.git", "/scope", "/"};
    for (size_t i = 0; i < sizeof(targets) / sizeof(targets[0]); ++i) {
        const char *p = targets[i];
        printf("TARGET %s\n", p);
        int regular = i < 3;
        if (regular) {
            denied("write", open(p, O_WRONLY));
            denied("truncate", truncate(p, 0));
            denied("open-truncate", open(p, O_WRONLY | O_TRUNC));
            denied("hardlink", link(p, "/scope/alias"));
        }
        denied("chmod", chmod(p, 0777));
        if (strcmp(p, "/") && strcmp(p, "/scope")) {
            /* Renaming / and the chroot mount is a VFS EBUSY, not a DAC witness. */
            if (regular) denied("unlink", unlink(p));
            else denied("rmdir", rmdir(p));
            denied("rename", rename(p, "/scope/replaced"));
            denied("exchange", syscall(SYS_renameat2, AT_FDCWD, p, AT_FDCWD, "/scope/bait", RENAME_EXCHANGE));
        }
    }
    denied("ancestor-new-entry", open("/new-entry", O_WRONLY | O_CREAT, 0600));
    denied("ancestor-rename-root", rename("/scope", "/moved"));
    denied("ancestor-exchange", syscall(SYS_renameat2, AT_FDCWD, "/scope", AT_FDCWD, "/tool", RENAME_EXCHANGE));
    denied("ancestor-rmdir", rmdir("/scope"));
    denied("ancestor-redirection", symlink("/scope", "/redirect"));
    REQUIRE(symlink("/scope/.git/config", "/scope/redirect") == 0);
    denied("payload-symlink-to-metadata", open("/scope/redirect", O_WRONLY | O_TRUNC));
    REQUIRE(unlink("/scope/redirect") == 0);
    denied("git-redirection", symlink("/scope/evil", "/scope/.git/config"));
    denied("metadata-new-entry", open("/scope/.git/new-entry", O_WRONLY | O_CREAT, 0600));
}
static void exec_identity(int controller) {
    pid_t p = fork(); REQUIRE(p >= 0);
    if (!p) {
        execl("/tool/probe", "/tool/probe", controller ? "controller-exec" : "worker-exec", NULL);
        _exit(71);
    }
    REQUIRE(wait_bounded(p) == 0);
}
static void migration(const char *cg_name) {
    denied("cgroup-root-migration", open("/cgroups/cgroup.procs", O_WRONLY));
    char p[4200];
    REQUIRE(snprintf(p, sizeof(p), "/cgroups/%s/cgroup.procs", cg_name) > 0);
    denied("cgroup-own-migration", open(p, O_WRONLY));
    REQUIRE(snprintf(p, sizeof(p), "/cgroups/%s/../escape/cgroup.procs", cg_name) > 0);
    denied("cgroup-sibling-migration", open(p, O_WRONLY));
}
static void worker(const char *cg_name, int inherited) {
    identity(0);
    verify_identity(0);
    closed_fd(inherited);
    syscall_policy();
    denied("proc-root-metadata", open("/proc/self/root/scope/.git/config", O_WRONLY));
    exec_identity(0);
    char b[4096];
    read_text("/proc/self/cgroup", b, sizeof(b));
    printf("PLACEMENT %s", b);
    REQUIRE(strstr(b, cg_name));
    protection_matrix();
    pid_t child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        REQUIRE(setsid() >= 0);
        verify_identity(0);
        exit(0);
    }
    REQUIRE(wait_bounded(child) == 0);
    /* An actual descendant repeats the mutation probes after fork/setsid. */
    child = fork(); REQUIRE(child >= 0);
    if (!child) { REQUIRE(setsid() >= 0); protection_matrix(); migration(cg_name); exit(0); }
    REQUIRE(wait_bounded(child) == 0);
    denied("new-mount-namespace", syscall(SYS_unshare, CLONE_NEWNS));
    denied("new-user-namespace", syscall(SYS_unshare, CLONE_NEWUSER));
    denied("namespace-join", syscall(SYS_setns, -1, 0));
    migration(cg_name);
    write_text("/scope/edit", "edited\n");
    REQUIRE(unlink("/scope/delete") == 0);
    int fd = open("/scope/private", O_CREAT | O_EXCL | O_WRONLY, 0600);
    REQUIRE(fd >= 0 && write(fd, "worker0600\n", 11) == 11 && close(fd) == 0);
    printf("WORKER root-edit=ok root-delete=ok new-private0600=ok\n");
}
static void git(const char *a, const char *b, const char *c) {
    printf("COMMAND /tool/git %s %s %s\n", a, b ? b : "", c ? c : "");
    fflush(NULL);
    pid_t p = fork(); REQUIRE(p >= 0);
    if (!p) {
        char *argv[] = {"/tool/git", (char *)a, (char *)b, (char *)c, NULL};
        execv(argv[0], argv);
        _exit(71);
    }
    int status = wait_bounded(p);
    REQUIRE(WIFEXITED(status) && WEXITSTATUS(status) == 0);
}
static void controller(int inherited, int mount_fd, int namespace_fd, struct file_handle *handle, int handle_ok) {
    identity(1);
    verify_identity(1);
    closed_fd(inherited);
    /* Same UID and capability positive controls, before adding confinement. */
    int control = open_by_handle_at(mount_fd, handle, O_RDONLY);
    REQUIRE(control >= 0);
    char outside[32] = {0};
    REQUIRE(read(control, outside, sizeof(outside)) == 18 && !strcmp(outside, "outside-synthetic\n"));
    REQUIRE(close(control) == 0);
    printf("POSITIVE controller-same-cap-handle-read-before-confinement bytes=18\n");
    /* The outside mount FD is deliberately retained solely for adversarial probes. */
    confine_controller();
    exec_identity(1);
    char b[128];
    struct stat st;
    REQUIRE(stat("/scope/private", &st) == 0 && (st.st_mode & 0777) == 0600 && st.st_uid == WORKER);
    read_text("/scope/private", b, sizeof(b));
    REQUIRE(strcmp(b, "worker0600\n") == 0);
    printf("POSITIVE controller-read-worker0600 bytes=11 mode=0600\n");
    denied("outside-visible-sentinel", open("/outside", O_RDONLY));
    REQUIRE(stat("/outside", &st) == 0); /* Landlock doesn't hide existence. */
    denied("proc-root-outside", open("/proc/self/root/outside", O_RDONLY));
    denied("inherited-dirfd-outside", openat(mount_fd, "outside", O_RDONLY));
    char fdpath[128];
    REQUIRE(snprintf(fdpath, sizeof(fdpath), "/proc/self/fd/%d/outside", mount_fd) > 0);
    denied("proc-dirfd-outside", open(fdpath, O_RDONLY));
    errno = 0;
    denied("open-by-handle-seccomp", syscall(SYS_open_by_handle_at, mount_fd, handle, O_RDONLY));
    REQUIRE(errno == EPERM);
    printf("HANDLE positive-host-control=%d explicit-syscall-policy=EPERM\n", handle_ok);
    REQUIRE(handle_ok); /* Unsupported handles are a REFUSAL, never a PASS. */
    REQUIRE(close(mount_fd) == 0);
    struct stat ns;
    REQUIRE(fstat(namespace_fd, &ns) == 0);
    denied("controller-setns-real-host-namespace", syscall(SYS_setns, namespace_fd, CLONE_NEWNS));
    REQUIRE(close(namespace_fd) == 0);
    /* Commit and checkpoint observations stay in the preflight repository. */
    git("diff", "--binary", "--no-ext-diff");
    git("add", "-A", "--");
    git("commit", "-m", "synthetic edit delete private");
    git("rev-parse", "HEAD", NULL);
    git("status", "--porcelain=v1", "-z");
    git("show", "--format=fuller", "--stat");
    git("diff-tree", "--root", "HEAD");
    REQUIRE(stat("/scope/private", &st) == 0 && (st.st_mode & 0777) == 0600);
    printf("CONTROLLER flow-complete worker-mode-preserved=0600\n");
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc == 2 && (!strcmp(argv[1], "worker-exec") || !strcmp(argv[1], "controller-exec"))) {
        verify_identity(!strcmp(argv[1], "controller-exec"));
        printf("EXEC inheritance-verified\n"); return 0;
    }
    REQUIRE(clearenv() == 0);
    REQUIRE(setenv("PATH", "/tool", 1) == 0 && setenv("HOME", "/scope", 1) == 0);
    REQUIRE(setenv("LC_ALL", "C", 1) == 0 && setenv("TZ", "UTC", 1) == 0);
    REQUIRE(setenv("GIT_CONFIG_NOSYSTEM", "1", 1) == 0);
    REQUIRE(setenv("GIT_CONFIG_GLOBAL", "/dev/null", 1) == 0);
    REQUIRE(setenv("GIT_TERMINAL_PROMPT", "0", 1) == 0);
    REQUIRE(argc == 4 && geteuid() == 0);
    REQUIRE(!strcmp(argv[1], "worker") || !strcmp(argv[1], "controller") ||
            !strcmp(argv[1], "lifecycle") || !strcmp(argv[1], "owned-failure") || !strcmp(argv[1], "preflight"));
    REQUIRE(strlen(argv[3]) < sizeof(group_path));
    strcpy(group_path, argv[3]);
    char cap_last[32];
    read_text("/proc/sys/kernel/cap_last_cap", cap_last, sizeof(cap_last));
    REQUIRE(atoi(cap_last) == CAP_LAST_CAP);
    printf("CAPABILITY_RANGE last=%s", cap_last);
    REQUIRE(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0);
    REQUIRE(prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) == 0);
    int cg = open(group_path, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    REQUIRE(cg >= 0);
    char victim[4200];
    REQUIRE(snprintf(victim, sizeof(victim), "%s/outside", argv[2]) > 0);
    char inherited_path[4200];
    REQUIRE(snprintf(inherited_path, sizeof(inherited_path), "%s/scope/.git/config", argv[2]) > 0);
    int inherited = open(inherited_path, O_RDWR | O_CLOEXEC);
    REQUIRE(inherited >= 0);
    int namespace_fd = open("/proc/self/ns/mnt", O_RDONLY | O_CLOEXEC);
    REQUIRE(namespace_fd >= 0);
    int mount_fd = open(argv[2], O_PATH | O_DIRECTORY | O_CLOEXEC);
    REQUIRE(mount_fd >= 0);
    union { uint64_t alignment; unsigned char storage[256]; } h;
    memset(&h, 0, sizeof(h));
    struct file_handle *handle = (struct file_handle *)&h;
    enum { HANDLE_CAPACITY = MAX_HANDLE_SZ };
    _Static_assert(HANDLE_CAPACITY > 0 && HANDLE_CAPACITY <= MAX_HANDLE_SZ, "legal handle capacity");
    _Static_assert(sizeof(h) >= sizeof(*handle) + HANDLE_CAPACITY, "handle header and payload fit storage");
    _Static_assert(_Alignof(__typeof__(h)) >= _Alignof(struct file_handle), "handle storage alignment");
    handle->handle_bytes = HANDLE_CAPACITY;
    int mount_id, handle_ok = 0;
    if (!strcmp(argv[1], "controller")) {
        int r = name_to_handle_at(AT_FDCWD, victim, handle, &mount_id, 0);
        printf("HANDLE name_to_handle_at=%d errno=%d bytes=%u type=%d\n", r, errno, handle->handle_bytes, handle->handle_type);
        if (r == 0) {
            int fd = open_by_handle_at(mount_fd, handle, O_RDONLY);
            printf("HANDLE privileged-positive-open=%d errno=%d\n", fd, errno);
            if (fd >= 0) {
                char content[32] = {0};
                REQUIRE(read(fd, content, sizeof(content)) == 18 && !strcmp(content, "outside-synthetic\n"));
                REQUIRE(close(fd) == 0); handle_ok = 1;
            }
        }
        REQUIRE(handle_ok);
    }
    int pipefd[2]; REQUIRE(pipe2(pipefd, O_CLOEXEC | O_NONBLOCK) == 0);
    struct clone_args args = {.flags = CLONE_INTO_CGROUP, .exit_signal = SIGCHLD, .cgroup = (uint64_t)cg};
    pid_t child = (pid_t)syscall(SYS_clone3, &args, sizeof(args));
    REQUIRE(child >= 0);
    if (!child) {
        REQUIRE(close(cg) == 0 && close(pipefd[0]) == 0 && close(inherited) == 0);
        char placement[4096], expected[4200];
        read_text("/proc/self/cgroup", placement, sizeof(placement));
        REQUIRE(snprintf(expected, sizeof(expected), "0::/%s\n", group_path + strlen("/sys/fs/cgroup/")) > 0);
        REQUIRE(strcmp(placement, expected) == 0);
        printf("AT_CREATION %s", placement);
        mount_view(argv[2]);
        if (!strcmp(argv[1], "worker")) {
            REQUIRE(close(mount_fd) == 0 && close(namespace_fd) == 0 && close(pipefd[1]) == 0);
            worker(group_path + strlen("/sys/fs/cgroup/"), inherited);
        } else if (!strcmp(argv[1], "controller")) {
            REQUIRE(close(pipefd[1]) == 0);
            controller(inherited, mount_fd, namespace_fd, handle, handle_ok);
        } else if (!strcmp(argv[1], "preflight")) {
            REQUIRE(close(mount_fd) == 0 && close(namespace_fd) == 0 && close(pipefd[1]) == 0);
            identity(1); verify_identity(1); confine_controller();
            git("rev-parse", "--show-toplevel", NULL);
            git("symbolic-ref", "--short", "HEAD");
            git("rev-parse", "--verify", "HEAD^{commit}");
            git("status", "--porcelain=v1", "-z");
        } else {
            REQUIRE(close(mount_fd) == 0 && close(namespace_fd) == 0);
            identity(0); syscall_policy();
            pid_t descendant = fork(); REQUIRE(descendant >= 0);
            if (!descendant) {
                REQUIRE(setsid() >= 0);
                pid_t grandchild = fork(); REQUIRE(grandchild >= 0);
                if (grandchild) exit(0);
                REQUIRE(setsid() >= 0);
                verify_identity(0);
                REQUIRE(write(pipefd[1], "R", 1) == 1);
                for (;;) pause(); /* Owned by the cgroup and the parent's fixed deadline. */
            }
            REQUIRE(close(pipefd[1]) == 0);
            if (!strcmp(argv[1], "owned-failure")) {
                fprintf(stderr, "INJECTED_FAILURE leader exits 72 while owned descendants hold pipe\n");
                exit(72);
            }
        }
        exit(0);
    }
    REQUIRE(close(cg) == 0 && close(inherited) == 0 && close(mount_fd) == 0 &&
            close(namespace_fd) == 0 && close(pipefd[1]) == 0);
    int status = wait_bounded(child);
    if (!strcmp(argv[1], "lifecycle") || !strcmp(argv[1], "owned-failure")) {
        char b;
        double end = now() + 5;
        ssize_t n;
        do { n = read(pipefd[0], &b, 1); if (n < 0 && errno == EAGAIN) tick(); else break; } while (now() < end);
        REQUIRE(n == 1 && b == 'R' && populated());
        errno = 0; REQUIRE(read(pipefd[0], &b, 1) == -1 && errno == EAGAIN);
        printf("INDEPENDENT leader-reaped=1 populated=1 pipe-eof=0 injected-failure=%d\n", !strcmp(argv[1], "owned-failure"));
    }
    kill_group();
    char b;
    REQUIRE(read(pipefd[0], &b, 1) == 0);
    REQUIRE(close(pipefd[0]) == 0);
    printf("CLEANUP pipe-eof=1 (separate observation)\n");
    /* Subreaped descendant zombies are not cgroup population. Reap separately. */
    double end = now() + 5;
    for (;;) {
        int s;
        pid_t p = waitpid(-1, &s, WNOHANG);
        if (p > 0) { printf("DESCENDANT_REAP pid=%d status=%d\n", p, s); continue; }
        if (p == -1 && errno == ECHILD) break;
        REQUIRE(p == 0 && now() < end); tick();
    }
    int expected_exit = !strcmp(argv[1], "owned-failure") ? 72 : 0;
    REQUIRE(WIFEXITED(status) && WEXITSTATUS(status) == expected_exit);
    printf("CASE_COMPLETE %s\n", argv[1]);
    return expected_exit;
}
