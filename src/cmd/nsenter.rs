// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! The hidden `5spot nsenter` subcommand: run a command inside another
//! process's namespaces (ADR 0020).
//!
//! The Kata-config agent restarts the host's k0s service by entering host
//! PID 1's namespaces and running `systemctl` there (ADR 0003). It used to exec
//! an external `nsenter` binary, which the shared image does not contain. It now
//! re-executes its own binary as this subcommand, which implements the subset of
//! `nsenter` it uses, with the same flags:
//!
//! ```text
//! 5spot nsenter -t <pid> [-m] [-u] [-i] [-n] [-p] -- <command> [args...]
//! ```
//!
//! ## Behavioural contract (pinned in `nsenter_tests.rs`)
//!
//! - Every requested `/proc/<pid>/ns/<type>` is opened **before** any is
//!   entered, and the mount namespace is entered **last**, so every path is
//!   resolved in the caller's own mount namespace (`nsenter`'s order).
//! - If any namespace cannot be opened or entered, nothing runs: running the
//!   command in the agent's own namespaces would restart nothing on the host
//!   while reporting success.
//! - The command is spawned only after every namespace is entered, so it starts
//!   in all of them. The PID namespace in particular only applies to children.
//! - With the mount namespace entered, the command runs from `/`, since the
//!   caller's working directory belongs to the old mount namespace.
//! - The exit status passes through; a signal becomes `128 + signal`.
//!
//! ## Why a re-executed subcommand rather than in-process
//!
//! `setns(2)` into a mount namespace fails for a multi-threaded process, and the
//! agent runs a multi-threaded async runtime. Entering after `fork` in a
//! `pre_exec` hook would work but needs `unsafe`. A fresh process of this
//! binary is single-threaded when it reaches here (`main` dispatches this
//! subcommand before building any runtime), so the safe `nix::sched::setns`
//! works and the codebase keeps its no-`unsafe` property (threat model T9).
//!
//! Entering another process's namespaces needs `CAP_SYS_ADMIN`; the Kata agent
//! pod runs `privileged: true` with `hostPID: true` for exactly this (ADR 0004).

use std::os::unix::process::ExitStatusExt as _;
use std::path::PathBuf;
use std::process::{Command as Process, ExitStatus};

use anyhow::{Context as _, Result};

/// Exit status for a command killed by a signal: `128 + signal`, the shell
/// convention, so a signalled `systemctl` is never reported as success.
pub(crate) const SIGNAL_EXIT_BASE: i32 = 128;

/// Exit status when the platform reports neither a code nor a signal.
const UNKNOWN_EXIT_STATUS: i32 = 1;

/// procfs mount point.
const PROC_ROOT: &str = "/proc";

/// Per-process namespace directory under `/proc/<pid>/`.
const NS_DIR: &str = "ns";

/// Working directory for the command once the mount namespace is entered.
const NEW_ROOT_DIR: &str = "/";

/// Arguments of `5spot nsenter`, a subset of util-linux `nsenter`'s.
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// PID whose namespaces to enter
    #[arg(short = 't', long = "target")]
    pub(crate) target: u32,

    /// Enter the mount namespace
    #[arg(short = 'm', long = "mount")]
    pub(crate) mount: bool,

    /// Enter the UTS (hostname) namespace
    #[arg(short = 'u', long = "uts")]
    pub(crate) uts: bool,

    /// Enter the IPC namespace
    #[arg(short = 'i', long = "ipc")]
    pub(crate) ipc: bool,

    /// Enter the network namespace
    #[arg(short = 'n', long = "net")]
    pub(crate) net: bool,

    /// Enter the PID namespace (applies to the command, a child)
    #[arg(short = 'p', long = "pid")]
    pub(crate) pid: bool,

    /// The command to run, after `--`. Later `--` are passed through to it.
    #[arg(last = true, required = true, num_args = 1..)]
    pub(crate) command: Vec<String>,
}

/// A namespace type this subcommand can enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Namespace {
    Ipc,
    Uts,
    Net,
    Pid,
    Mnt,
}

impl Namespace {
    /// The entry name under `/proc/<pid>/ns/`.
    fn proc_name(self) -> &'static str {
        match self {
            Self::Ipc => "ipc",
            Self::Uts => "uts",
            Self::Net => "net",
            Self::Pid => "pid",
            Self::Mnt => "mnt",
        }
    }
}

/// The namespaces `args` asks for, in the order they are entered: the mount
/// namespace last, as `nsenter` does.
pub(crate) fn namespaces(args: &Args) -> Vec<Namespace> {
    [
        (args.ipc, Namespace::Ipc),
        (args.uts, Namespace::Uts),
        (args.net, Namespace::Net),
        (args.pid, Namespace::Pid),
        (args.mount, Namespace::Mnt),
    ]
    .into_iter()
    .filter_map(|(requested, ns)| requested.then_some(ns))
    .collect()
}

/// `/proc/<pid>/ns/<type>` for `ns` of process `pid`.
pub(crate) fn ns_path(pid: u32, ns: Namespace) -> PathBuf {
    PathBuf::from(PROC_ROOT)
        .join(pid.to_string())
        .join(NS_DIR)
        .join(ns.proc_name())
}

/// The process exit status for `status`: its code, or `128 + signal`.
pub(crate) fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    status
        .signal()
        .map_or(UNKNOWN_EXIT_STATUS, |signal| SIGNAL_EXIT_BASE + signal)
}

/// Enter the requested namespaces of `args.target`, then run the command and
/// return its exit status.
///
/// # Errors
/// Returns an error, without running the command, if a namespace cannot be
/// opened or entered; or if the command cannot be spawned.
pub(crate) fn run(args: &Args) -> Result<i32> {
    let namespaces = namespaces(args);
    enter(args.target, &namespaces)?;

    let (program, rest) = args
        .command
        .split_first()
        .context("nsenter needs a command to run after `--`")?;
    let mut process = Process::new(program);
    process.args(rest);
    if namespaces.contains(&Namespace::Mnt) {
        process.current_dir(NEW_ROOT_DIR);
    }
    let status = process
        .status()
        .with_context(|| format!("running {program}"))?;
    Ok(exit_code(status))
}

/// Open every namespace first, then enter each, mount last.
#[cfg(target_os = "linux")]
fn enter(pid: u32, namespaces: &[Namespace]) -> Result<()> {
    use nix::sched::{setns, CloneFlags};

    let flag = |ns: Namespace| match ns {
        Namespace::Ipc => CloneFlags::CLONE_NEWIPC,
        Namespace::Uts => CloneFlags::CLONE_NEWUTS,
        Namespace::Net => CloneFlags::CLONE_NEWNET,
        Namespace::Pid => CloneFlags::CLONE_NEWPID,
        Namespace::Mnt => CloneFlags::CLONE_NEWNS,
    };

    let opened = namespaces
        .iter()
        .map(|&ns| {
            let path = ns_path(pid, ns);
            std::fs::File::open(&path)
                .with_context(|| format!("opening {}", path.display()))
                .map(|file| (ns, file))
        })
        .collect::<Result<Vec<_>>>()?;

    for (ns, file) in &opened {
        setns(file, flag(*ns)).with_context(|| {
            format!(
                "entering the {} namespace of PID {pid} (needs CAP_SYS_ADMIN)",
                ns.proc_name()
            )
        })?;
    }
    Ok(())
}

/// Namespaces are a Linux concept; elsewhere only the no-op case is allowed.
#[cfg(not(target_os = "linux"))]
fn enter(pid: u32, namespaces: &[Namespace]) -> Result<()> {
    let Some(&first) = namespaces.first() else {
        return Ok(());
    };
    anyhow::bail!(
        "entering {} is not supported on this platform (Linux only)",
        ns_path(pid, first).display()
    )
}

#[cfg(test)]
#[path = "nsenter_tests.rs"]
mod tests;
