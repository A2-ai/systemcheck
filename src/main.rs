use std::collections::HashSet;
use std::fs;
use std::path::Path;

use clap::Parser;
use humanize_bytes::humanize_bytes_binary;
use serde::Serialize;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = "systemcheck", version)]
struct Cli {
    /// Verbose output (detailed sections)
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Emit JSON to stdout
    #[arg(long = "json")]
    json: bool,
}

#[derive(Serialize)]
struct SimpleCpuSummary {
    available_cpus: usize,
    system_logical_cpus: usize,
    constrained: bool,
}

#[derive(Serialize)]
struct SimpleMemorySummary {
    system_available_bytes: u64,
    cgroup_memory_limit_bytes: Option<u64>,
    constrained: bool,
}

#[derive(Serialize)]
struct SimpleReport {
    version: String,
    cpu: SimpleCpuSummary,
    memory: SimpleMemorySummary,
}

#[derive(Serialize)]
struct DetailedCpuInfo {
    system_logical_cpus: usize,
    system_physical_cpus: usize,
    available_cpus: usize,
    cgroup_cpu_quota: Option<f64>,
}

#[derive(Serialize)]
struct DetailedMemoryInfo {
    system_total_bytes: u64,
    system_available_bytes: u64,
    system_used_bytes: u64,
    cgroup_memory_limit_bytes: Option<u64>,
    cgroup_memory_usage_bytes: Option<u64>,
}

#[derive(Serialize)]
struct DetailedCGroupInfo {
    version: Option<String>,
    current_path: String,
    cpu_quota: Option<f64>,
    memory_limit_bytes: Option<u64>,
}

#[derive(Serialize)]
struct DetailedReport {
    version: String,
    cpu: DetailedCpuInfo,
    memory: DetailedMemoryInfo,
    cgroup: DetailedCGroupInfo,
}

fn main() {
    let cli = Cli::parse();

    // Gather data once
    let system_logical_cpus = get_system_cpu_count();
    let system_physical_cpus = get_system_physical_cpu_count();
    let available_cpus = num_cpus::get();
    let cgroup_path = get_current_cgroup_path();
    let cgroup_cpu_quota = get_cgroup_cpu_quota_for_path(&cgroup_path);
    let (system_total, system_available) = get_system_memory_from_proc();
    let system_used = system_total.saturating_sub(system_available);
    let cgroup_memory_limit = get_cgroup_memory_limit_for_path(&cgroup_path);
    let cgroup_memory_usage = get_cgroup_memory_usage_for_path(&cgroup_path);

    let cgroup_v2 = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    let cgroup_v1 = Path::new("/sys/fs/cgroup/cpu").exists()
        || Path::new("/sys/fs/cgroup/memory").exists();
    let cgroup_version = if cgroup_v2 {
        Some("v2".to_string())
    } else if cgroup_v1 {
        Some("v1".to_string())
    } else {
        None
    };

    if cli.json {
        if cli.verbose {
            let report = DetailedReport {
                version: VERSION.to_string(),
                cpu: DetailedCpuInfo {
                    system_logical_cpus,
                    system_physical_cpus,
                    available_cpus,
                    cgroup_cpu_quota,
                },
                memory: DetailedMemoryInfo {
                    system_total_bytes: system_total,
                    system_available_bytes: system_available,
                    system_used_bytes: system_used,
                    cgroup_memory_limit_bytes: cgroup_memory_limit,
                    cgroup_memory_usage_bytes: cgroup_memory_usage,
                },
                cgroup: DetailedCGroupInfo {
                    version: cgroup_version,
                    current_path: cgroup_path.clone(),
                    cpu_quota: cgroup_cpu_quota,
                    memory_limit_bytes: cgroup_memory_limit,
                },
            };
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            let constrained_cpu = available_cpus < system_logical_cpus;
            let constrained_mem = cgroup_memory_limit
                .map(|lim| lim < system_total)
                .unwrap_or(false);
            let report = SimpleReport {
                version: VERSION.to_string(),
                cpu: SimpleCpuSummary {
                    available_cpus,
                    system_logical_cpus,
                    constrained: constrained_cpu,
                },
                memory: SimpleMemorySummary {
                    system_available_bytes: system_available,
                    cgroup_memory_limit_bytes: cgroup_memory_limit,
                    constrained: constrained_mem,
                },
            };
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        return;
    }

    if cli.verbose {
        // Verbose, current-style sections
        println!("systemcheck v{}\n", VERSION);
        println!("=== System Check - Resource Diagnostics ===\n");
        print_cpu_info();
        println!();
        print_memory_info();
        println!();
        print_cgroup_info();
        return;
    }

    // Simple summary output
    println!("systemcheck: {}\n", VERSION);
    println!("CPU Usage:");
    if available_cpus < system_logical_cpus {
        println!("Constrained to {} of {} CPUs", available_cpus, system_logical_cpus);
    } else {
        println!("Not constrained: {} CPUs available", available_cpus);
    }
    println!();

    // Memory summary line
    if let Some(limit) = cgroup_memory_limit {
        println!(
            "Memory: Limited to {} of {} available",
            humanize_bytes_binary!(limit),
            humanize_bytes_binary!(system_available)
        );
    } else {
        println!(
            "Memory: Unconstrained, {} available",
            humanize_bytes_binary!(system_available)
        );
    }

    // CGroup summary note
    let looks_default_user = is_default_user_slice_path(&cgroup_path);
    let explicit_limits = has_explicit_limits_at_path(&cgroup_path);
    if looks_default_user && !explicit_limits {
        println!("CGroup: default user slice (no explicit limits)");
    } else if !cgroup_path.is_empty() && cgroup_path != "/" {
        if explicit_limits {
            println!("CGroup: limits present at {}", cgroup_path);
        } else {
            println!("CGroup: {} (no explicit limits)", cgroup_path);
        }
    }
    println!("\nsee more details with systemcheck -v");
}

fn print_cpu_info() {
    println!("CPU Information:");
    println!("----------------");

    // Get actual system CPUs (not limited by cgroups)
    let system_logical_cpus = get_system_cpu_count();
    let system_physical_cpus = get_system_physical_cpu_count();

    // Get cgroup-limited CPUs
    let available_cpus = num_cpus::get();

    println!("  System Logical CPUs:     {} threads", system_logical_cpus);
    println!("  System Physical CPUs:    {} cores", system_physical_cpus);
    println!("  Available CPUs (cgroup): {}", available_cpus);

    if available_cpus < system_logical_cpus {
        println!("  ⚠️  CPU is constrained by cgroups to {} of {} system CPUs",
                 available_cpus, system_logical_cpus);
    }

    if let Some(cpu_quota) = get_cgroup_cpu_quota() {
        println!("  CGroup CPU Quota:        {:.2} CPUs", cpu_quota);
    }
}

fn print_memory_info() {
    println!("Memory Information:");
    println!("-------------------");

    // Get real system memory from /proc/meminfo
    let (system_total, system_available) = get_system_memory_from_proc();

    println!("  System Total Memory:     {}", humanize_bytes_binary!(system_total));
    println!("  System Available Memory: {}", humanize_bytes_binary!(system_available));

    let system_used = system_total.saturating_sub(system_available);
    println!("  System Used Memory:      {}", humanize_bytes_binary!(system_used));

    // Get the current cgroup path and check its memory limit
    let cgroup_path = get_current_cgroup_path();

    if let Some(cgroup_limit) = get_cgroup_memory_limit_for_path(&cgroup_path) {
        println!("  CGroup Memory Limit:     {}", humanize_bytes_binary!(cgroup_limit));

        if cgroup_limit < system_total {
            println!("  ⚠️  Memory is constrained by cgroups!");

            if let Some(current_usage) = get_cgroup_memory_usage_for_path(&cgroup_path) {
                let usage_percent = (current_usage as f64 / cgroup_limit as f64) * 100.0;
                println!("  CGroup Memory Usage:     {} ({:.1}% of limit)",
                    humanize_bytes_binary!(current_usage), usage_percent);
            }
        }
    }
}

fn print_cgroup_info() {
    println!("CGroup Information:");
    println!("-------------------");

    let cgroup_v2 = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    let cgroup_v1 = Path::new("/sys/fs/cgroup/cpu").exists() ||
                    Path::new("/sys/fs/cgroup/memory").exists();

    if cgroup_v2 {
        println!("  CGroup Version: v2 (unified hierarchy)");
    } else if cgroup_v1 {
        println!("  CGroup Version: v1");
    } else {
        println!("  CGroup Version: Not detected or not in container");
    }

    if let Ok(contents) = fs::read_to_string("/proc/self/cgroup") {
        println!("  Current Process CGroups:");
        for line in contents.lines() {
            if !line.is_empty() {
                println!("    {}", line);
            }
        }
    }

    // Show resource constraints for the current cgroup
    let cgroup_path = get_current_cgroup_path();
    if !cgroup_path.is_empty() && cgroup_path != "/" {
        println!("\n  Resource Constraints for Current CGroup:");

        // CPU constraints
        if let Some(cpu_quota) = get_cgroup_cpu_quota_for_path(&cgroup_path) {
            println!("    CPU Quota: {:.2} CPUs", cpu_quota);
        }

        // Memory constraints
        if let Some(mem_limit) = get_cgroup_memory_limit_for_path(&cgroup_path) {
            println!("    Memory Limit: {}", humanize_bytes_binary!(mem_limit));
        }

        // Extra hint: detect if this looks like a default user.slice with no explicit limits
        let looks_default_user = is_default_user_slice_path(&cgroup_path);
        let explicit_limits = has_explicit_limits_at_path(&cgroup_path);
        if looks_default_user && !explicit_limits {
            println!("\n  Note: no explicit cpu/memory/cpuset limits detected at this cgroup; this looks like a default systemd user slice.");
        }
    }
}

fn is_default_user_slice_path(cgroup_path: &str) -> bool {
    // Heuristic for systemd user sessions, e.g.: /user.slice/user-1000.slice/session-4.scope
    cgroup_path.starts_with("/user.slice/user-") && cgroup_path.contains("/session-")
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn has_explicit_limits_at_path(cgroup_path: &str) -> bool {
    // Check cgroup v2 first
    let v2 = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    if v2 {
        // cpu.max at path set?
        let cpu_max_path = format!("/sys/fs/cgroup{}/cpu.max", cgroup_path);
        if let Some(line) = read_trimmed(&cpu_max_path) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[0] != "max" {
                return true;
            }
        }
        // memory.max at path set?
        let mem_max_path = format!("/sys/fs/cgroup{}/memory.max", cgroup_path);
        if let Some(val) = read_trimmed(&mem_max_path) {
            if val != "max" {
                return true;
            }
        }
        // cpuset constrained?
        let path_cpuset = read_trimmed(&format!("/sys/fs/cgroup{}/cpuset.cpus.effective", cgroup_path));
        let root_cpuset = read_trimmed("/sys/fs/cgroup/cpuset.cpus.effective");
        if let (Some(p), Some(r)) = (path_cpuset, root_cpuset) {
            if !p.is_empty() && !r.is_empty() && p != r {
                return true;
            }
        }
        return false;
    }
    // cgroup v1: check cpu quota and memory limit at path, cpuset
    let quota_path = format!("/sys/fs/cgroup/cpu{}/cpu.cfs_quota_us", cgroup_path);
    let period_path = format!("/sys/fs/cgroup/cpu{}/cpu.cfs_period_us", cgroup_path);
    if let (Some(qs), Some(ps)) = (read_trimmed(&quota_path), read_trimmed(&period_path)) {
        if let (Ok(q), Ok(p)) = (qs.parse::<i64>(), ps.parse::<i64>()) {
            if q > 0 && p > 0 { return true; }
        }
    }
    let mem_limit_path = format!("/sys/fs/cgroup/memory{}/memory.limit_in_bytes", cgroup_path);
    if let Some(ls) = read_trimmed(&mem_limit_path) {
        if let Ok(limit) = ls.parse::<u64>() {
            if limit < 9223372036854771712 { return true; }
        }
    }
    let path_cpuset = read_trimmed(&format!("/sys/fs/cgroup/cpuset{}/cpuset.cpus", cgroup_path));
    let root_cpuset = read_trimmed("/sys/fs/cgroup/cpuset/cpuset.cpus");
    if let (Some(p), Some(r)) = (path_cpuset, root_cpuset) {
        if !p.is_empty() && !r.is_empty() && p != r { return true; }
    }
    false
}

fn get_system_memory_from_proc() -> (u64, u64) {
    let mut total_kb = 0u64;
    let mut available_kb = 0u64;

    if let Ok(contents) = fs::read_to_string("/proc/meminfo") {
        for line in contents.lines() {
            if line.starts_with("MemTotal:") {
                if let Some(value) = parse_meminfo_line(line) {
                    total_kb = value;
                }
            } else if line.starts_with("MemAvailable:") {
                if let Some(value) = parse_meminfo_line(line) {
                    available_kb = value;
                }
            }
        }
    }

    // Convert from KB to bytes
    (total_kb * 1024, available_kb * 1024)
}

fn parse_meminfo_line(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse::<u64>().ok()
    } else {
        None
    }
}

fn get_system_cpu_count() -> usize {
    // Try to get the actual system CPU count by reading /proc/cpuinfo
    if let Ok(contents) = fs::read_to_string("/proc/cpuinfo") {
        let count = contents
            .lines()
            .filter(|line| line.starts_with("processor"))
            .count();
        if count > 0 {
            return count;
        }
    }

    // Fallback to sysconf if available
    unsafe {
        let count = libc::sysconf(libc::_SC_NPROCESSORS_ONLN);
        if count > 0 {
            return count as usize;
        }
    }

    // Last resort: use num_cpus (which may be cgroup limited)
    num_cpus::get()
}

fn get_system_physical_cpu_count() -> usize {
    // Try to get physical cores by parsing /proc/cpuinfo
    if let Ok(contents) = fs::read_to_string("/proc/cpuinfo") {
        let mut core_ids = HashSet::new();
        let mut current_physical_id = None;

        for line in contents.lines() {
            if line.starts_with("physical id") {
                current_physical_id = line.split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<usize>().ok());
            } else if line.starts_with("core id") {
                if let Some(phys_id) = current_physical_id {
                    if let Some(core_id) = line.split(':')
                        .nth(1)
                        .and_then(|s| s.trim().parse::<usize>().ok()) {
                        core_ids.insert((phys_id, core_id));
                    }
                }
            }
        }

        if !core_ids.is_empty() {
            return core_ids.len();
        }
    }

    // Fallback: use num_cpus for physical cores
    num_cpus::get_physical()
}

fn get_current_cgroup_path() -> String {
    if let Ok(contents) = fs::read_to_string("/proc/self/cgroup") {
        // For cgroup v2, the format is: 0::/path
        for line in contents.lines() {
            if line.starts_with("0::") {
                return line[3..].to_string();
            }
        }

        // For cgroup v1, get the memory controller path
        for line in contents.lines() {
            if line.contains(":memory:") {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 {
                    return parts[2].to_string();
                }
            }
        }
    }
    String::new()
}

fn get_cgroup_cpu_quota() -> Option<f64> {
    let cgroup_path = get_current_cgroup_path();
    get_cgroup_cpu_quota_for_path(&cgroup_path)
}

fn get_cgroup_cpu_quota_for_path(cgroup_path: &str) -> Option<f64> {
    // Try cgroup v2 first
    if let Ok(quota) = read_cgroup_v2_cpu_quota_for_path(cgroup_path) {
        return Some(quota);
    }

    // Fall back to cgroup v1
    read_cgroup_v1_cpu_quota_for_path(cgroup_path)
}

fn read_cgroup_v2_cpu_quota_for_path(cgroup_path: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let cpu_max_path = format!("/sys/fs/cgroup{}/cpu.max", cgroup_path);

    // Try the specific cgroup path first
    if let Ok(cpu_max) = fs::read_to_string(&cpu_max_path) {
        let parts: Vec<&str> = cpu_max.trim().split_whitespace().collect();
        if parts.len() == 2 && parts[0] != "max" {
            let quota: i64 = parts[0].parse()?;
            let period: i64 = parts[1].parse()?;
            return Ok(quota as f64 / period as f64);
        }
    }

    // Fall back to root cgroup
    let cpu_max = fs::read_to_string("/sys/fs/cgroup/cpu.max")?;
    let parts: Vec<&str> = cpu_max.trim().split_whitespace().collect();

    if parts.len() == 2 && parts[0] != "max" {
        let quota: i64 = parts[0].parse()?;
        let period: i64 = parts[1].parse()?;
        return Ok(quota as f64 / period as f64);
    }

    Err("No CPU quota set in cgroup v2".into())
}

fn read_cgroup_v1_cpu_quota() -> Option<f64> {
    let quota_path = "/sys/fs/cgroup/cpu/cpu.cfs_quota_us";
    let period_path = "/sys/fs/cgroup/cpu/cpu.cfs_period_us";

    if let (Ok(quota_str), Ok(period_str)) = (
        fs::read_to_string(quota_path),
        fs::read_to_string(period_path)
    ) {
        if let (Ok(quota), Ok(period)) = (
            quota_str.trim().parse::<i64>(),
            period_str.trim().parse::<i64>()
        ) {
            if quota > 0 && period > 0 {
                return Some(quota as f64 / period as f64);
            }
        }
    }

    None
}

fn read_cgroup_v1_cpu_quota_for_path(cgroup_path: &str) -> Option<f64> {
    let quota_path = format!("/sys/fs/cgroup/cpu{}/cpu.cfs_quota_us", cgroup_path);
    let period_path = format!("/sys/fs/cgroup/cpu{}/cpu.cfs_period_us", cgroup_path);

    if let (Ok(quota_str), Ok(period_str)) = (
        fs::read_to_string(&quota_path),
        fs::read_to_string(&period_path)
    ) {
        if let (Ok(quota), Ok(period)) = (
            quota_str.trim().parse::<i64>(),
            period_str.trim().parse::<i64>()
        ) {
            if quota > 0 && period > 0 {
                return Some(quota as f64 / period as f64);
            }
        }
    }

    // Fall back to root cgroup
    read_cgroup_v1_cpu_quota()
}

fn get_cgroup_memory_limit_for_path(cgroup_path: &str) -> Option<u64> {
    // Try cgroup v2
    let mem_max_path = format!("/sys/fs/cgroup{}/memory.max", cgroup_path);
    if let Ok(limit_str) = fs::read_to_string(&mem_max_path) {
        if let Ok(limit) = limit_str.trim().parse::<u64>() {
            if limit < u64::MAX {
                return Some(limit);
            }
        }
    }

    // Try cgroup v2 root
    if let Ok(limit_str) = fs::read_to_string("/sys/fs/cgroup/memory.max") {
        if let Ok(limit) = limit_str.trim().parse::<u64>() {
            if limit < u64::MAX {
                return Some(limit);
            }
        }
    }

    // Try cgroup v1 with path
    let mem_limit_path = format!("/sys/fs/cgroup/memory{}/memory.limit_in_bytes", cgroup_path);
    if let Ok(limit_str) = fs::read_to_string(&mem_limit_path) {
        if let Ok(limit) = limit_str.trim().parse::<u64>() {
            // Check if it's not the default unlimited value
            if limit < 9223372036854771712 {
                return Some(limit);
            }
        }
    }

    // Try cgroup v1 root
    if let Ok(limit_str) = fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes") {
        if let Ok(limit) = limit_str.trim().parse::<u64>() {
            // Check if it's not the default unlimited value
            if limit < 9223372036854771712 {
                return Some(limit);
            }
        }
    }

    None
}

fn get_cgroup_memory_usage_for_path(cgroup_path: &str) -> Option<u64> {
    // Try cgroup v2 with path
    let mem_current_path = format!("/sys/fs/cgroup{}/memory.current", cgroup_path);
    if let Ok(usage_str) = fs::read_to_string(&mem_current_path) {
        if let Ok(usage) = usage_str.trim().parse::<u64>() {
            return Some(usage);
        }
    }

    // Try cgroup v2 root
    if let Ok(usage_str) = fs::read_to_string("/sys/fs/cgroup/memory.current") {
        if let Ok(usage) = usage_str.trim().parse::<u64>() {
            return Some(usage);
        }
    }

    // Try cgroup v1 with path
    let mem_usage_path = format!("/sys/fs/cgroup/memory{}/memory.usage_in_bytes", cgroup_path);
    if let Ok(usage_str) = fs::read_to_string(&mem_usage_path) {
        if let Ok(usage) = usage_str.trim().parse::<u64>() {
            return Some(usage);
        }
    }

    // Try cgroup v1 root
    if let Ok(usage_str) = fs::read_to_string("/sys/fs/cgroup/memory/memory.usage_in_bytes") {
        if let Ok(usage) = usage_str.trim().parse::<u64>() {
            return Some(usage);
        }
    }

    None
}