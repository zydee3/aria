//! External symbol detection and categorization.
//!
//! Categorizes unresolved calls into: syscalls, libc, macros, or unknown external.

use std::collections::{HashMap, HashSet};

/// Categories for external (unresolved) symbols
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalKind {
    Syscall,
    Libc,
    Macro,
    External,
}

impl ExternalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExternalKind::Syscall => "syscall",
            ExternalKind::Libc => "libc",
            ExternalKind::Macro => "macro",
            ExternalKind::External => "external",
        }
    }
}

/// Database of known external symbols
pub struct ExternalDb {
    syscalls: HashMap<&'static str, &'static str>,
    libc: HashMap<&'static str, &'static str>,
    macros: HashSet<String>,
}

impl ExternalDb {
    pub fn new() -> Self {
        Self {
            syscalls: build_syscall_db(),
            libc: build_libc_db(),
            macros: HashSet::new(),
        }
    }

    /// Add known macros from header scanning
    #[allow(dead_code)]
    pub fn add_macro(&mut self, name: String) {
        self.macros.insert(name);
    }

    /// Categorize an unresolved symbol
    pub fn categorize(&self, name: &str) -> (ExternalKind, Option<&'static str>) {
        // Check sys_* prefix for syscalls
        let syscall_name = if name.starts_with("sys_") {
            &name[4..]
        } else {
            name
        };

        if let Some(summary) = self.syscalls.get(syscall_name) {
            return (ExternalKind::Syscall, Some(summary));
        }
        if let Some(summary) = self.syscalls.get(name) {
            return (ExternalKind::Syscall, Some(summary));
        }

        if let Some(summary) = self.libc.get(name) {
            return (ExternalKind::Libc, Some(summary));
        }

        if self.macros.contains(name) || is_likely_macro(name) {
            return (ExternalKind::Macro, None);
        }

        (ExternalKind::External, None)
    }

    /// Format target string for index
    #[allow(dead_code)]
    pub fn format_target(&self, name: &str) -> String {
        let (kind, _) = self.categorize(name);
        format!("[{}:{}]", kind.as_str(), name)
    }
}

impl Default for ExternalDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Heuristic detection of likely macros
fn is_likely_macro(name: &str) -> bool {
    // ALL_CAPS (with underscores) is usually a macro
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()) {
        return true;
    }

    // Known macro prefixes from common C projects
    const MACRO_PREFIXES: &[&str] = &[
        "pr_",           // Linux kernel / CRIU logging
        "list_",         // Linux list macros
        "list_for_",     // list iteration
        "hlist_",        // hash list macros
        "atomic_",       // atomic operations (often macros)
        "READ_ONCE",
        "WRITE_ONCE",
        "likely",
        "unlikely",
        "container_of",
        "__",            // compiler/internal macros
    ];

    for prefix in MACRO_PREFIXES {
        if name.starts_with(prefix) {
            return true;
        }
    }

    // Known specific macros
    const KNOWN_MACROS: &[&str] = &[
        "offsetof",
        "sizeof",
        "typeof",
        "alignof",
        "NULL",
        "true",
        "false",
        "errno",
    ];

    KNOWN_MACROS.contains(&name)
}

/// Build syscall database with summaries
fn build_syscall_db() -> HashMap<&'static str, &'static str> {
    let mut db = HashMap::new();

    // File operations
    db.insert("open", "Opens a file and returns a file descriptor");
    db.insert("openat", "Opens a file relative to a directory fd");
    db.insert("openat2", "Opens a file with extended flags");
    db.insert("close", "Closes a file descriptor");
    db.insert("read", "Reads bytes from a file descriptor");
    db.insert("write", "Writes bytes to a file descriptor");
    db.insert("pread64", "Reads from fd at offset without changing position");
    db.insert("pwrite64", "Writes to fd at offset without changing position");
    db.insert("preadv", "Reads into multiple buffers from fd at offset");
    db.insert("pwritev", "Writes multiple buffers to fd at offset");
    db.insert("lseek", "Repositions file offset");
    db.insert("fstat", "Gets file status by fd");
    db.insert("stat", "Gets file status by path");
    db.insert("lstat", "Gets symlink status");
    db.insert("fstatat", "Gets file status relative to directory fd");
    db.insert("access", "Checks file accessibility");
    db.insert("faccessat", "Checks file accessibility relative to directory fd");
    db.insert("dup", "Duplicates a file descriptor");
    db.insert("dup2", "Duplicates fd to specific number");
    db.insert("dup3", "Duplicates fd with flags");
    db.insert("fcntl", "Manipulates file descriptor");
    db.insert("ioctl", "Device-specific control operations");
    db.insert("flock", "Apply advisory lock on file");
    db.insert("fsync", "Synchronizes file to disk");
    db.insert("fdatasync", "Synchronizes file data to disk");
    db.insert("truncate", "Truncates file to specified length");
    db.insert("ftruncate", "Truncates file by fd");
    db.insert("fallocate", "Manipulates file space");
    db.insert("readlink", "Reads symbolic link target");
    db.insert("readlinkat", "Reads symlink target relative to directory fd");
    db.insert("symlink", "Creates a symbolic link");
    db.insert("symlinkat", "Creates symlink relative to directory fd");
    db.insert("link", "Creates a hard link");
    db.insert("linkat", "Creates hard link relative to directory fd");
    db.insert("unlink", "Removes a file");
    db.insert("unlinkat", "Removes file relative to directory fd");
    db.insert("rename", "Renames a file");
    db.insert("renameat", "Renames file relative to directory fd");
    db.insert("renameat2", "Renames file with flags");
    db.insert("mkdir", "Creates a directory");
    db.insert("mkdirat", "Creates directory relative to directory fd");
    db.insert("rmdir", "Removes an empty directory");
    db.insert("getcwd", "Gets current working directory");
    db.insert("chdir", "Changes current directory");
    db.insert("fchdir", "Changes directory by fd");
    db.insert("chroot", "Changes root directory");
    db.insert("chmod", "Changes file permissions");
    db.insert("fchmod", "Changes file permissions by fd");
    db.insert("fchmodat", "Changes permissions relative to directory fd");
    db.insert("chown", "Changes file ownership");
    db.insert("fchown", "Changes ownership by fd");
    db.insert("fchownat", "Changes ownership relative to directory fd");
    db.insert("lchown", "Changes symlink ownership");
    db.insert("umask", "Sets file creation mask");
    db.insert("getdents", "Reads directory entries");
    db.insert("getdents64", "Reads directory entries (64-bit)");

    // Memory management
    db.insert("mmap", "Maps memory or file into address space");
    db.insert("munmap", "Unmaps memory region");
    db.insert("mremap", "Remaps memory region");
    db.insert("mprotect", "Sets memory protection");
    db.insert("madvise", "Gives advice about memory usage");
    db.insert("brk", "Changes data segment size");
    db.insert("sbrk", "Increments data segment size");
    db.insert("mlock", "Locks memory pages");
    db.insert("munlock", "Unlocks memory pages");
    db.insert("mlockall", "Locks all memory pages");
    db.insert("munlockall", "Unlocks all memory pages");
    db.insert("mincore", "Checks if pages are resident");
    db.insert("membarrier", "Memory barrier across threads");

    // Process management
    db.insert("fork", "Creates a new process");
    db.insert("vfork", "Creates process sharing memory");
    db.insert("clone", "Creates process/thread with flags");
    db.insert("clone3", "Creates process/thread (extended)");
    db.insert("execve", "Executes a program");
    db.insert("execveat", "Executes program relative to directory fd");
    db.insert("exit", "Terminates the process");
    db.insert("exit_group", "Terminates all threads in process");
    db.insert("wait4", "Waits for process with rusage");
    db.insert("waitpid", "Waits for specific process");
    db.insert("waitid", "Waits for process state change");
    db.insert("getpid", "Gets process ID");
    db.insert("gettid", "Gets thread ID");
    db.insert("getppid", "Gets parent process ID");
    db.insert("getpgid", "Gets process group ID");
    db.insert("setpgid", "Sets process group ID");
    db.insert("getpgrp", "Gets process group");
    db.insert("setsid", "Creates new session");
    db.insert("getsid", "Gets session ID");
    db.insert("getuid", "Gets user ID");
    db.insert("geteuid", "Gets effective user ID");
    db.insert("setuid", "Sets user ID");
    db.insert("seteuid", "Sets effective user ID");
    db.insert("setreuid", "Sets real and effective UID");
    db.insert("setresuid", "Sets real, effective, and saved UID");
    db.insert("getresuid", "Gets real, effective, and saved UID");
    db.insert("getgid", "Gets group ID");
    db.insert("getegid", "Gets effective group ID");
    db.insert("setgid", "Sets group ID");
    db.insert("setegid", "Sets effective group ID");
    db.insert("setregid", "Sets real and effective GID");
    db.insert("setresgid", "Sets real, effective, and saved GID");
    db.insert("getresgid", "Gets real, effective, and saved GID");
    db.insert("setfsuid", "Sets filesystem UID");
    db.insert("setfsgid", "Sets filesystem GID");
    db.insert("getgroups", "Gets supplementary group IDs");
    db.insert("setgroups", "Sets supplementary group IDs");
    db.insert("prctl", "Process control operations");
    db.insert("arch_prctl", "Architecture-specific process control");
    db.insert("ptrace", "Process tracing and debugging");
    db.insert("seccomp", "Secure computing mode");
    db.insert("capget", "Gets process capabilities");
    db.insert("capset", "Sets process capabilities");

    // Signals
    db.insert("kill", "Sends signal to process");
    db.insert("tkill", "Sends signal to thread");
    db.insert("tgkill", "Sends signal to thread in group");
    db.insert("sigaction", "Sets signal handler");
    db.insert("rt_sigaction", "Sets signal handler (realtime)");
    db.insert("sigprocmask", "Sets blocked signals");
    db.insert("rt_sigprocmask", "Sets blocked signals (realtime)");
    db.insert("sigpending", "Gets pending signals");
    db.insert("sigsuspend", "Waits for signal");
    db.insert("sigaltstack", "Sets alternate signal stack");
    db.insert("sigreturn", "Returns from signal handler");
    db.insert("rt_sigreturn", "Returns from signal handler (realtime)");
    db.insert("rt_sigqueueinfo", "Queues signal with info");
    db.insert("rt_tgsigqueueinfo", "Queues signal to thread");
    db.insert("signalfd", "Creates signal file descriptor");
    db.insert("signalfd4", "Creates signal fd with flags");
    db.insert("pause", "Waits for signal");
    db.insert("setitimer", "Sets interval timer");
    db.insert("getitimer", "Gets interval timer");
    db.insert("alarm", "Sets alarm clock");

    // Time
    db.insert("time", "Gets time in seconds");
    db.insert("gettimeofday", "Gets time with microseconds");
    db.insert("settimeofday", "Sets time of day");
    db.insert("clock_gettime", "Gets clock time");
    db.insert("clock_settime", "Sets clock time");
    db.insert("clock_getres", "Gets clock resolution");
    db.insert("clock_nanosleep", "High-resolution sleep");
    db.insert("nanosleep", "Sleeps for nanoseconds");
    db.insert("timer_create", "Creates POSIX timer");
    db.insert("timer_delete", "Deletes POSIX timer");
    db.insert("timer_settime", "Sets timer expiration");
    db.insert("timer_gettime", "Gets timer expiration");
    db.insert("timer_getoverrun", "Gets timer overrun count");
    db.insert("timerfd_create", "Creates timer file descriptor");
    db.insert("timerfd_settime", "Sets timer fd expiration");
    db.insert("timerfd_gettime", "Gets timer fd expiration");

    // Sockets
    db.insert("socket", "Creates a socket");
    db.insert("socketpair", "Creates connected socket pair");
    db.insert("bind", "Binds socket to address");
    db.insert("listen", "Marks socket as passive");
    db.insert("accept", "Accepts connection on socket");
    db.insert("accept4", "Accepts with flags");
    db.insert("connect", "Initiates connection on socket");
    db.insert("send", "Sends data on socket");
    db.insert("sendto", "Sends data to address");
    db.insert("sendmsg", "Sends message on socket");
    db.insert("sendmmsg", "Sends multiple messages");
    db.insert("recv", "Receives data from socket");
    db.insert("recvfrom", "Receives data with source address");
    db.insert("recvmsg", "Receives message from socket");
    db.insert("recvmmsg", "Receives multiple messages");
    db.insert("shutdown", "Shuts down socket");
    db.insert("getsockname", "Gets socket name");
    db.insert("getpeername", "Gets peer name");
    db.insert("setsockopt", "Sets socket option");
    db.insert("getsockopt", "Gets socket option");
    db.insert("socketcall", "Socket system call multiplexer");

    // IPC
    db.insert("pipe", "Creates pipe");
    db.insert("pipe2", "Creates pipe with flags");
    db.insert("shmget", "Gets shared memory segment");
    db.insert("shmat", "Attaches shared memory");
    db.insert("shmdt", "Detaches shared memory");
    db.insert("shmctl", "Shared memory control");
    db.insert("semget", "Gets semaphore set");
    db.insert("semop", "Semaphore operations");
    db.insert("semctl", "Semaphore control");
    db.insert("msgget", "Gets message queue");
    db.insert("msgsnd", "Sends message to queue");
    db.insert("msgrcv", "Receives message from queue");
    db.insert("msgctl", "Message queue control");
    db.insert("ipc", "IPC system call multiplexer");
    db.insert("mq_open", "Opens message queue");
    db.insert("mq_close", "Closes message queue");
    db.insert("mq_unlink", "Removes message queue");
    db.insert("mq_send", "Sends to message queue");
    db.insert("mq_receive", "Receives from message queue");
    db.insert("futex", "Fast userspace locking");

    // Filesystem
    db.insert("mount", "Mounts filesystem");
    db.insert("umount", "Unmounts filesystem");
    db.insert("umount2", "Unmounts with flags");
    db.insert("pivot_root", "Changes root filesystem");
    db.insert("statfs", "Gets filesystem statistics");
    db.insert("fstatfs", "Gets filesystem stats by fd");
    db.insert("sync", "Synchronizes all filesystems");
    db.insert("syncfs", "Synchronizes one filesystem");
    db.insert("quotactl", "Disk quota control");
    db.insert("move_mount", "Moves mount point");
    db.insert("open_tree", "Opens mount tree");
    db.insert("fsopen", "Opens filesystem context");
    db.insert("fsmount", "Mounts filesystem context");

    // Event/polling
    db.insert("select", "Synchronous I/O multiplexing");
    db.insert("pselect6", "Select with sigmask");
    db.insert("poll", "Polls file descriptors");
    db.insert("ppoll", "Poll with sigmask");
    db.insert("epoll_create", "Creates epoll instance");
    db.insert("epoll_create1", "Creates epoll with flags");
    db.insert("epoll_ctl", "Controls epoll instance");
    db.insert("epoll_wait", "Waits for epoll events");
    db.insert("epoll_pwait", "Epoll wait with sigmask");
    db.insert("eventfd", "Creates event file descriptor");
    db.insert("eventfd2", "Creates event fd with flags");
    db.insert("inotify_init", "Creates inotify instance");
    db.insert("inotify_init1", "Creates inotify with flags");
    db.insert("inotify_add_watch", "Adds inotify watch");
    db.insert("inotify_rm_watch", "Removes inotify watch");

    // I/O
    db.insert("io_setup", "Creates async I/O context");
    db.insert("io_destroy", "Destroys async I/O context");
    db.insert("io_submit", "Submits async I/O operations");
    db.insert("io_cancel", "Cancels async I/O operation");
    db.insert("io_getevents", "Gets async I/O events");
    db.insert("io_uring_setup", "Sets up io_uring");
    db.insert("io_uring_enter", "Enters io_uring");
    db.insert("io_uring_register", "Registers io_uring buffers");
    db.insert("splice", "Moves data between fds");
    db.insert("tee", "Duplicates pipe content");
    db.insert("vmsplice", "Splice user pages to pipe");
    db.insert("sendfile", "Transfers data between fds");
    db.insert("copy_file_range", "Copies file range");

    // Misc
    db.insert("uname", "Gets system information");
    db.insert("sysinfo", "Gets system statistics");
    db.insert("syslog", "Reads/controls kernel log");
    db.insert("getrlimit", "Gets resource limits");
    db.insert("setrlimit", "Sets resource limits");
    db.insert("prlimit64", "Gets/sets resource limits");
    db.insert("getrusage", "Gets resource usage");
    db.insert("times", "Gets process times");
    db.insert("sched_yield", "Yields processor");
    db.insert("sched_setscheduler", "Sets scheduling policy");
    db.insert("sched_getscheduler", "Gets scheduling policy");
    db.insert("sched_setparam", "Sets scheduling parameters");
    db.insert("sched_getparam", "Gets scheduling parameters");
    db.insert("sched_setaffinity", "Sets CPU affinity");
    db.insert("sched_getaffinity", "Gets CPU affinity");
    db.insert("setpriority", "Sets process priority");
    db.insert("getpriority", "Gets process priority");
    db.insert("personality", "Sets process execution domain");
    db.insert("getrandom", "Gets random bytes");
    db.insert("memfd_create", "Creates anonymous memory fd");
    db.insert("set_tid_address", "Sets clear_child_tid address");
    db.insert("set_robust_list", "Sets robust futex list");
    db.insert("get_robust_list", "Gets robust futex list");
    db.insert("rseq", "Restartable sequences");
    db.insert("cacheflush", "Flushes CPU cache");

    db
}

/// Build libc database with summaries
fn build_libc_db() -> HashMap<&'static str, &'static str> {
    let mut db = HashMap::new();

    // stdio
    db.insert("printf", "Prints formatted output to stdout");
    db.insert("fprintf", "Prints formatted output to stream");
    db.insert("sprintf", "Prints formatted output to string");
    db.insert("snprintf", "Prints formatted output with size limit");
    db.insert("vprintf", "Prints with va_list to stdout");
    db.insert("vfprintf", "Prints with va_list to stream");
    db.insert("vsprintf", "Prints with va_list to string");
    db.insert("vsnprintf", "Prints with va_list and size limit");
    db.insert("scanf", "Reads formatted input from stdin");
    db.insert("fscanf", "Reads formatted input from stream");
    db.insert("sscanf", "Reads formatted input from string");
    db.insert("fopen", "Opens a file stream");
    db.insert("fclose", "Closes a file stream");
    db.insert("fread", "Reads binary data from stream");
    db.insert("fwrite", "Writes binary data to stream");
    db.insert("fgets", "Reads line from stream");
    db.insert("fputs", "Writes string to stream");
    db.insert("fgetc", "Reads character from stream");
    db.insert("fputc", "Writes character to stream");
    db.insert("getc", "Reads character (macro)");
    db.insert("putc", "Writes character (macro)");
    db.insert("getchar", "Reads character from stdin");
    db.insert("putchar", "Writes character to stdout");
    db.insert("puts", "Writes string to stdout");
    db.insert("gets", "Reads line from stdin (unsafe)");
    db.insert("fseek", "Sets stream position");
    db.insert("ftell", "Gets stream position");
    db.insert("rewind", "Resets stream position");
    db.insert("fflush", "Flushes stream buffer");
    db.insert("feof", "Tests end-of-file indicator");
    db.insert("ferror", "Tests error indicator");
    db.insert("clearerr", "Clears error indicators");
    db.insert("perror", "Prints error message");
    db.insert("fileno", "Gets fd from stream");
    db.insert("fdopen", "Opens stream from fd");
    db.insert("freopen", "Reopens stream");
    db.insert("setbuf", "Sets stream buffer");
    db.insert("setvbuf", "Sets stream buffering mode");

    // stdlib
    db.insert("malloc", "Allocates memory");
    db.insert("calloc", "Allocates zeroed memory");
    db.insert("realloc", "Resizes allocated memory");
    db.insert("free", "Frees allocated memory");
    db.insert("atoi", "Converts string to int");
    db.insert("atol", "Converts string to long");
    db.insert("atof", "Converts string to double");
    db.insert("strtol", "Converts string to long with validation");
    db.insert("strtoul", "Converts string to unsigned long");
    db.insert("strtoll", "Converts string to long long");
    db.insert("strtoull", "Converts string to unsigned long long");
    db.insert("strtod", "Converts string to double");
    db.insert("strtof", "Converts string to float");
    db.insert("rand", "Generates random number");
    db.insert("srand", "Seeds random number generator");
    db.insert("abort", "Aborts program");
    db.insert("atexit", "Registers exit handler");
    db.insert("getenv", "Gets environment variable");
    db.insert("setenv", "Sets environment variable");
    db.insert("unsetenv", "Removes environment variable");
    db.insert("putenv", "Changes environment variable");
    db.insert("qsort", "Sorts array");
    db.insert("bsearch", "Binary search in sorted array");
    db.insert("abs", "Integer absolute value");
    db.insert("labs", "Long absolute value");
    db.insert("llabs", "Long long absolute value");
    db.insert("div", "Integer division with remainder");
    db.insert("ldiv", "Long division with remainder");
    db.insert("lldiv", "Long long division with remainder");

    // string
    db.insert("strlen", "Gets string length");
    db.insert("strcpy", "Copies string");
    db.insert("strncpy", "Copies string with limit");
    db.insert("strcat", "Concatenates strings");
    db.insert("strncat", "Concatenates with limit");
    db.insert("strcmp", "Compares strings");
    db.insert("strncmp", "Compares strings with limit");
    db.insert("strchr", "Finds character in string");
    db.insert("strrchr", "Finds last character in string");
    db.insert("strstr", "Finds substring");
    db.insert("strtok", "Tokenizes string");
    db.insert("strtok_r", "Tokenizes string (reentrant)");
    db.insert("strdup", "Duplicates string");
    db.insert("strndup", "Duplicates string with limit");
    db.insert("strerror", "Gets error message string");
    db.insert("strerror_r", "Gets error message (reentrant)");
    db.insert("strcasecmp", "Compares strings ignoring case");
    db.insert("strncasecmp", "Compares with limit ignoring case");
    db.insert("strspn", "Gets span of characters in set");
    db.insert("strcspn", "Gets span of characters not in set");
    db.insert("strpbrk", "Finds any character from set");
    db.insert("strsep", "Extracts token from string");

    // memory
    db.insert("memcpy", "Copies memory");
    db.insert("memmove", "Copies memory (overlapping safe)");
    db.insert("memset", "Fills memory with value");
    db.insert("memcmp", "Compares memory");
    db.insert("memchr", "Finds byte in memory");
    db.insert("memmem", "Finds memory in memory");
    db.insert("bzero", "Zeros memory (deprecated)");
    db.insert("bcopy", "Copies memory (deprecated)");
    db.insert("bcmp", "Compares memory (deprecated)");

    // ctype
    db.insert("isalnum", "Tests alphanumeric");
    db.insert("isalpha", "Tests alphabetic");
    db.insert("isdigit", "Tests decimal digit");
    db.insert("isxdigit", "Tests hexadecimal digit");
    db.insert("islower", "Tests lowercase");
    db.insert("isupper", "Tests uppercase");
    db.insert("isspace", "Tests whitespace");
    db.insert("isprint", "Tests printable");
    db.insert("iscntrl", "Tests control character");
    db.insert("ispunct", "Tests punctuation");
    db.insert("isgraph", "Tests graphical");
    db.insert("isblank", "Tests blank");
    db.insert("tolower", "Converts to lowercase");
    db.insert("toupper", "Converts to uppercase");

    // assert
    db.insert("assert", "Assertion check (macro)");
    db.insert("__assert_fail", "Assertion failure handler");

    // errno
    db.insert("__errno_location", "Gets errno address");

    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_syscall() {
        let db = ExternalDb::new();

        let (kind, summary) = db.categorize("sys_openat");
        assert_eq!(kind, ExternalKind::Syscall);
        assert!(summary.is_some());

        let (kind, _) = db.categorize("sys_close");
        assert_eq!(kind, ExternalKind::Syscall);
    }

    #[test]
    fn test_categorize_libc() {
        let db = ExternalDb::new();

        let (kind, summary) = db.categorize("printf");
        assert_eq!(kind, ExternalKind::Libc);
        assert!(summary.is_some());

        let (kind, _) = db.categorize("malloc");
        assert_eq!(kind, ExternalKind::Libc);
    }

    #[test]
    fn test_categorize_macro() {
        let db = ExternalDb::new();

        let (kind, _) = db.categorize("BUG_ON");
        assert_eq!(kind, ExternalKind::Macro);

        let (kind, _) = db.categorize("pr_err");
        assert_eq!(kind, ExternalKind::Macro);

        let (kind, _) = db.categorize("ARRAY_SIZE");
        assert_eq!(kind, ExternalKind::Macro);

        let (kind, _) = db.categorize("list_for_each_entry");
        assert_eq!(kind, ExternalKind::Macro);
    }

    #[test]
    fn test_categorize_external() {
        let db = ExternalDb::new();

        let (kind, _) = db.categorize("some_unknown_function");
        assert_eq!(kind, ExternalKind::External);
    }

    #[test]
    fn test_format_target() {
        let db = ExternalDb::new();

        assert_eq!(db.format_target("sys_openat"), "[syscall:sys_openat]");
        assert_eq!(db.format_target("printf"), "[libc:printf]");
        assert_eq!(db.format_target("BUG_ON"), "[macro:BUG_ON]");
        assert_eq!(db.format_target("unknown"), "[external:unknown]");
    }
}
