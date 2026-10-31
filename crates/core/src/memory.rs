//! How much of the machine's memory a filegoblin process may hold, and the
//! guard that holds the CLI to it.
//!
//! PDF workers measured themselves and stopped past half the machine's RAM,
//! but the CLI process had no ceiling: whatever it buffered without bound grew
//! until the OS stepped in. One run went to 100 GB on a 24 GB Mac — the
//! machine swapped to a standstill before jetsam killed it, and two more
//! processes had reached 60–70 GB by the time they were force-quit. [`guard`]
//! gives the CLI the same self-measured ceiling, so a runaway ends as one
//! failed run with a message instead.
//!
//! Not `setrlimit`, for the reasons in `docs/ADR.md` ("PDFs convert in worker
//! processes"): macOS refuses any useful `RLIMIT_AS`, and Linux's would count
//! reserved address space rather than memory used.

use anyhow::{Context, Result};
use colored::Colorize;
use std::time::Duration;

/// The CLI's ceiling, in MB.
pub const MEMORY_ENV: &str = "FILEGOBLIN_MEMORY_MB";

/// How the CLI exits once over its ceiling.
pub const OUT_OF_MEMORY_EXIT: i32 = 5;

/// Ends this process, with a message and [`OUT_OF_MEMORY_EXIT`], once it holds
/// more than `FILEGOBLIN_MEMORY_MB` (half the machine's RAM by default). It
/// measures itself every 25 ms on a thread of its own, as a PDF worker does.
/// The CLI calls this at startup; library users are not limited unless they
/// do. Fails on a malformed setting.
pub fn guard() -> Result<()> {
    let limit_mb = env_number(MEMORY_ENV, half_of_ram_mb())?;
    std::thread::Builder::new()
        .name("filegoblin-memory".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(used) = footprint().filter(|used| *used > limit_mb << 20) {
                    eprintln!(
                        "{} filegoblin stopped: it held {} MB, more than its {} MB limit \
                         (half this machine's memory unless {} says otherwise).",
                        "❌".red(),
                        used >> 20,
                        limit_mb,
                        MEMORY_ENV
                    );
                    std::process::exit(OUT_OF_MEMORY_EXIT);
                }
            }
        })
        .context("Failed to start the memory guard")?;
    Ok(())
}

/// Half the machine's memory (8 GiB where that cannot be told): the default
/// for the CLI and for each PDF worker. Above what real PDFs take (with the
/// vision models, the PDFs in `test_assets` peak at 1.7 GB in the median and
/// 6.4 GB at most), while a bomb is stopped with half the memory still free.
pub(crate) fn half_of_ram_mb() -> u64 {
    physical_memory().map_or(8192, |bytes| (bytes >> 20) / 2)
}

/// The memory this process may use, in MB: the lower of the CLI's ceiling
/// and, in a PDF worker, the worker's allowance (both half the RAM unless
/// set). Work that can trade speed for memory sizes itself to it.
#[cfg(feature = "vision")]
pub(crate) fn budget_mb() -> u64 {
    let half = half_of_ram_mb();
    [MEMORY_ENV, "FILEGOBLIN_PDF_MEMORY_MB"]
        .iter()
        .map(|name| env_number(name, half).unwrap_or(half))
        .min()
        .unwrap_or(half)
}

pub(crate) fn env_number(name: &str, default: u64) -> Result<u64> {
    parse_number(name, std::env::var(name).ok().as_deref(), default)
}

/// A positive whole number from the environment; anything else set there is
/// an error rather than a silently ignored setting.
pub(crate) fn parse_number(name: &str, value: Option<&str>, default: u64) -> Result<u64> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .with_context(|| format!("{name} must be a whole number above 0, not {value:?}"))
}

/// The memory this process holds: private pages, whether resident, compressed
/// or swapped where the OS says. `None` where it cannot be told, which leaves
/// the process without a memory limit.
#[cfg(target_os = "macos")]
pub(crate) fn footprint() -> Option<u64> {
    // SAFETY: a zeroed rusage_info_v2 is a valid value, and proc_pid_rusage
    // writes exactly one of them for RUSAGE_INFO_V2.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let buffer = (&raw mut info).cast::<libc::rusage_info_t>();
        (libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, buffer) == 0)
            .then_some(info.ri_phys_footprint)
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn footprint() -> Option<u64> {
    // Resident pages that are not file-backed: statm's resident minus shared.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let mut fields = statm.split_whitespace().skip(1);
    let resident: u64 = fields.next()?.parse().ok()?;
    let shared: u64 = fields.next()?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(resident.saturating_sub(shared) * u64::try_from(page).ok()?)
}

#[cfg(windows)]
pub(crate) fn footprint() -> Option<u64> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing,
    // and `size` is the size of the struct the counters are written into.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) }.ok()?;
    // Private committed memory.
    Some(counters.PagefileUsage as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn footprint() -> Option<u64> {
    None
}

/// The machine's installed memory.
#[cfg(unix)]
pub(crate) fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf has no preconditions.
    let (pages, page) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    Some(u64::try_from(pages).ok()? * u64::try_from(page).ok()?)
}

#[cfg(windows)]
pub(crate) fn physical_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `dwLength` is the size of the struct the status is written into.
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some(status.ullTotalPhys)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn physical_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_can_measure_its_memory_and_the_machines() {
        if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
            let used = footprint().expect("a footprint on this platform");
            let installed = physical_memory().expect("the installed memory");
            assert!(0 < used && used < installed, "{used} of {installed}");
            assert_eq!(half_of_ram_mb(), (installed >> 20) / 2);
        }
    }

    #[test]
    fn settings_must_be_positive_numbers() {
        assert_eq!(parse_number("X", None, 7).unwrap(), 7);
        assert_eq!(parse_number("X", Some(" 30 "), 7).unwrap(), 30);
        for bad in ["0", "-1", "1.5", "lots", ""] {
            let e = parse_number("X", Some(bad), 7).unwrap_err().to_string();
            assert!(e.contains("X must be a whole number"), "{e}");
        }
    }
}
