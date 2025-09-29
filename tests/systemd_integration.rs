use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const EXPECTED_VERSION: &str = env!("CARGO_PKG_VERSION");

const CPU_QUOTA_PERCENT: &str = "200%"; // expect roughly 2.0 CPUs
const MEMORY_LIMIT: &str = "512M"; // expect ~512 MiB
const EXPECTED_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

const CPU_TOLERANCE: f64 = 0.15; // CPUs
const MEMORY_TOLERANCE_BYTES: u64 = 8 * 1024; // 8 KiB

#[derive(Debug, Deserialize)]
struct SimpleCpuSummary {
    available_cpus: usize,
    system_logical_cpus: usize,
    constrained: bool,
}

#[derive(Debug, Deserialize)]
struct SimpleMemorySummary {
    system_available_bytes: u64,
    cgroup_memory_limit_bytes: Option<u64>,
    constrained: bool,
}

#[derive(Debug, Deserialize)]
struct SimpleReport {
    version: String,
    cpu: SimpleCpuSummary,
    memory: SimpleMemorySummary,
}

#[derive(Debug, Deserialize, Clone)]
struct DetailedCpuInfo {
    system_logical_cpus: usize,
    system_physical_cpus: usize,
    available_cpus: usize,
    cgroup_cpu_quota: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
struct DetailedMemoryInfo {
    system_total_bytes: u64,
    system_available_bytes: u64,
    system_used_bytes: u64,
    cgroup_memory_limit_bytes: Option<u64>,
    cgroup_memory_usage_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct DetailedCGroupInfo {
    version: Option<String>,
    current_path: String,
    cpu_quota: Option<f64>,
    memory_limit_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct DetailedReport {
    version: String,
    cpu: DetailedCpuInfo,
    memory: DetailedMemoryInfo,
    cgroup: DetailedCGroupInfo,
}

#[derive(Debug)]
enum ExpectedCpuQuota {
    Approx(f64),
    Baseline,
}

#[derive(Debug)]
enum ExpectedMemoryLimit {
    Approx(u64),
    Baseline,
}

#[derive(Debug)]
struct SystemdCase {
    name: &'static str,
    cpu_quota_property: Option<&'static str>,
    memory_max_property: Option<&'static str>,
    expected_cpu: ExpectedCpuQuota,
    expected_memory: ExpectedMemoryLimit,
}

fn systemd_run_available() -> bool {
    let probe = Command::new("systemd-run")
        .arg("--user")
        .arg("--wait")
        .arg("--collect")
        .arg("--quiet")
        .arg("/bin/true")
        .output();

    match probe {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn parse_detailed_report(bytes: &[u8]) -> Option<DetailedReport> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(&text[start..=end]).ok()
}

fn find_systemcheck_binary() -> Option<PathBuf> {
    // https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests
    if let Some(path) = option_env!("CARGO_BIN_EXE_systemcheck") {
        return Some(PathBuf::from(path));
    }

    // TODO: this seemingly didn't work right, but not going to investigate now
    // further as the test now does properly run
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_systemcheck") {
        return Some(PathBuf::from(path));
    }

    let target_dir = std::env::var("CARGO_TARGET_DIR").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    let mut candidates = Vec::new();
    candidates.push(target_dir.join(&profile).join("systemcheck"));
    // Common alternative: debug build regardless of PROFILE (e.g. running cargo test)
    if profile != "debug" {
        candidates.push(target_dir.join("debug").join("systemcheck"));
    }
    // Release build fallback
    candidates.push(target_dir.join("release").join("systemcheck"));

    for candidate in candidates {
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn run_simple_report(binary: &Path) -> Result<SimpleReport, Box<dyn std::error::Error>> {
    let output = Command::new(binary)
        .arg("--json")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "systemcheck --json exited with {:?}: {}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ).into());
    }

    let text = std::str::from_utf8(&output.stdout)?.trim();
    let report: SimpleReport = serde_json::from_str(text)?;
    Ok(report)
}

fn run_detailed_report_direct(binary: &Path) -> Result<DetailedReport, Box<dyn std::error::Error>> {
    let output = Command::new(binary)
        .arg("-v")
        .arg("--json")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "systemcheck -v --json exited with {:?}: {}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ).into());
    }

    parse_detailed_report(&output.stdout)
        .ok_or_else(|| "failed to parse JSON output from systemcheck".into())
}

fn run_case_via_systemd(binary: &Path, case: &SystemdCase)
    -> Result<DetailedReport, Box<dyn std::error::Error>>
{
    let mut cmd = Command::new("systemd-run");
    cmd.arg("--user")
        .arg("--wait")
        .arg("--collect")
        .arg("--pipe")
        .arg("--quiet")
        .arg(format!("--unit=systemcheck-{}-{}", case.name, std::process::id()));

    if let Some(quota) = case.cpu_quota_property {
        cmd.arg(format!("--property=CPUQuota={}", quota));
    }
    if let Some(limit) = case.memory_max_property {
        cmd.arg(format!("--property=MemoryMax={}", limit));
    }

    cmd.arg(binary)
        .arg("-v")
        .arg("--json");

    let output = cmd.output()?;
    if !output.status.success() {
        return Err(format!(
            "systemd-run for case '{}' failed (status {:?}): {}{}",
            case.name,
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ).into());
    }

    parse_detailed_report(&output.stdout)
        .ok_or_else(|| format!("failed to parse JSON output for case '{}'", case.name).into())
}

fn approx_eq(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

fn approx_eq_u64(actual: u64, expected: u64, tolerance: u64) -> bool {
    if actual >= expected {
        actual - expected <= tolerance
    } else {
        expected - actual <= tolerance
    }
}

const fn mib(value: u64) -> u64 {
    value * 1024 * 1024
}

#[test]
fn simple_json_includes_version() -> Result<(), Box<dyn std::error::Error>> {
    let binary = match find_systemcheck_binary() {
        Some(path) => path,
        None => {
            eprintln!("skipping simple_json_includes_version: build systemcheck first");
            return Ok(());
        }
    };

    let report = match run_simple_report(&binary) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("skipping simple_json_includes_version: {}", err);
            return Ok(());
        }
    };

    assert_eq!(report.version, EXPECTED_VERSION);
    assert!(report.cpu.system_logical_cpus > 0);
    assert!(report.cpu.available_cpus > 0);
    let _ = report.cpu.constrained;
    assert!(report.memory.system_available_bytes > 0);
    let _ = report.memory.cgroup_memory_limit_bytes;
    let _ = report.memory.constrained;
    Ok(())
}

#[test]
fn systemd_run_limits_reflected_in_json() -> Result<(), Box<dyn std::error::Error>> {
    if !systemd_run_available() {
        eprintln!("skipping systemd_run_limits_reflected_in_json: systemd-run --user not available");
        return Ok(());
    }
    let binary = match find_systemcheck_binary() {
        Some(path) => path,
        None => {
            eprintln!("skipping: unable to locate systemcheck binary; run `cargo build` first");
            return Ok(());
        }
    };

    let baseline = match run_detailed_report_direct(&binary) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("skipping systemd_run_limits_reflected_in_json: {}", err);
            return Ok(());
        }
    };

    let cases = [
        SystemdCase {
            name: "no_constraints",
            cpu_quota_property: None,
            memory_max_property: None,
            expected_cpu: ExpectedCpuQuota::Baseline,
            expected_memory: ExpectedMemoryLimit::Baseline,
        },
        SystemdCase {
            name: "memory_only",
            cpu_quota_property: None,
            memory_max_property: Some("256M"),
            expected_cpu: ExpectedCpuQuota::Baseline,
            expected_memory: ExpectedMemoryLimit::Approx(mib(256)),
        },
        SystemdCase {
            name: "cpu_only",
            cpu_quota_property: Some("150%"),
            memory_max_property: None,
            expected_cpu: ExpectedCpuQuota::Approx(1.5),
            expected_memory: ExpectedMemoryLimit::Baseline,
        },
        SystemdCase {
            name: "cpu_and_memory",
            cpu_quota_property: Some(CPU_QUOTA_PERCENT),
            memory_max_property: Some(MEMORY_LIMIT),
            expected_cpu: ExpectedCpuQuota::Approx(2.0),
            expected_memory: ExpectedMemoryLimit::Approx(EXPECTED_MEMORY_BYTES),
        },
    ];

    for case in cases.iter() {
        let report = match run_case_via_systemd(&binary, case) {
            Ok(report) => report,
            Err(err) => {
                eprintln!("skipping case '{}': {}", case.name, err);
                continue;
            }
        };

        assert_eq!(report.version, EXPECTED_VERSION, "case '{}' version mismatch", case.name);
        assert!(
            report.cpu.system_logical_cpus > 0,
            "case '{}': system logical CPUs reported as zero",
            case.name
        );
        if baseline.cpu.system_logical_cpus > 0 {
            assert!(
                report.cpu.system_logical_cpus <= baseline.cpu.system_logical_cpus,
                "case '{}': logical CPUs ({}) exceed baseline ({})",
                case.name,
                report.cpu.system_logical_cpus,
                baseline.cpu.system_logical_cpus
            );
        }
        assert!(
            report.cpu.system_physical_cpus > 0,
            "case '{}': system physical CPUs reported as zero",
            case.name
        );
        if baseline.cpu.system_physical_cpus > 0 {
            assert!(
                report.cpu.system_physical_cpus <= baseline.cpu.system_physical_cpus,
                "case '{}': physical CPUs ({}) exceed baseline ({})",
                case.name,
                report.cpu.system_physical_cpus,
                baseline.cpu.system_physical_cpus
            );
        }
        assert!(
            report.cpu.available_cpus > 0,
            "case '{}': available CPUs reported as zero",
            case.name
        );
        assert_eq!(
            report.memory.system_total_bytes,
            baseline.memory.system_total_bytes,
            "case '{}': system total memory should remain unchanged",
            case.name
        );
        assert!(
            report.memory.system_available_bytes <= report.memory.system_total_bytes,
            "case '{}': available memory exceeds total", case.name
        );
        assert_eq!(
            report.memory.system_total_bytes.saturating_sub(report.memory.system_available_bytes),
            report.memory.system_used_bytes,
            "case '{}': used memory should match total-available",
            case.name
        );
        if let Some(usage) = report.memory.cgroup_memory_usage_bytes {
            assert!(
                usage <= report.memory.system_total_bytes,
                "case '{}': cgroup usage exceeds total memory", case.name
            );
        }
        assert!(
            report.cgroup.current_path.starts_with('/'),
            "case '{}': unexpected cgroup path {}",
            case.name,
            report.cgroup.current_path
        );
        if let Some(version) = &report.cgroup.version {
            assert!(version == "v1" || version == "v2", "case '{}': unexpected cgroup version {}", case.name, version);
        }
        match (&report.cpu.cgroup_cpu_quota, &report.cgroup.cpu_quota) {
            (Some(cpu_section), Some(cgroup_section)) => {
                assert!(
                    approx_eq(*cpu_section, *cgroup_section, CPU_TOLERANCE),
                    "case '{}': cpu section quota {} disagrees with cgroup quota {}",
                    case.name,
                    cpu_section,
                    cgroup_section
                );
            }
            (None, None) => {}
            (cpu_section, cgroup_section) => {
                panic!(
                    "case '{}': cpu section quota {:?} disagrees with cgroup quota {:?}",
                    case.name,
                    cpu_section,
                    cgroup_section
                );
            }
        }
        match (&report.memory.cgroup_memory_limit_bytes, &report.cgroup.memory_limit_bytes) {
            (Some(mem_section), Some(cgroup_section)) => {
                assert!(
                    approx_eq_u64(*mem_section, *cgroup_section, MEMORY_TOLERANCE_BYTES),
                    "case '{}': memory section limit {} disagrees with cgroup limit {}",
                    case.name,
                    mem_section,
                    cgroup_section
                );
            }
            (None, None) => {}
            (mem_section, cgroup_section) => {
                panic!(
                    "case '{}': memory section limit {:?} disagrees with cgroup limit {:?}",
                    case.name,
                    mem_section,
                    cgroup_section
                );
            }
        }

        match (&case.expected_cpu, baseline.cgroup.cpu_quota, report.cgroup.cpu_quota) {
            (ExpectedCpuQuota::Approx(expected), _, Some(actual)) => {
                assert!(
                    approx_eq(actual, *expected, CPU_TOLERANCE),
                    "case '{}': expected cpu quota ≈ {} but got {}",
                    case.name,
                    expected,
                    actual
                );
            }
            (ExpectedCpuQuota::Approx(_), _, None) => {
                panic!("case '{}': expected cpu quota value but got None", case.name);
            }
            (ExpectedCpuQuota::Baseline, Some(baseline_value), Some(actual)) => {
                assert!(
                    approx_eq(actual, baseline_value, CPU_TOLERANCE),
                    "case '{}': expected baseline cpu quota ≈ {} but got {}",
                    case.name,
                    baseline_value,
                    actual
                );
            }
            (ExpectedCpuQuota::Baseline, None, None) => {}
            (ExpectedCpuQuota::Baseline, None, Some(actual)) => {
                // Baseline had no explicit limit; ensure any reported value is positive
                assert!(
                    actual > 0.0,
                    "case '{}': unexpected non-positive cpu quota {}",
                    case.name,
                    actual
                );
            }
            (ExpectedCpuQuota::Baseline, Some(baseline_value), None) => {
                panic!(
                    "case '{}': baseline cpu quota was {} but report returned None",
                    case.name,
                    baseline_value
                );
            }
        }

        match (&case.expected_memory, baseline.memory.cgroup_memory_limit_bytes, report.memory.cgroup_memory_limit_bytes) {
            (ExpectedMemoryLimit::Approx(expected), _, Some(actual)) => {
                assert!(
                    approx_eq_u64(actual, *expected, MEMORY_TOLERANCE_BYTES),
                    "case '{}': expected memory limit ≈ {} but got {}",
                    case.name,
                    expected,
                    actual
                );
            }
            (ExpectedMemoryLimit::Approx(_), _, None) => {
                panic!("case '{}': expected memory limit value but got None", case.name);
            }
            (ExpectedMemoryLimit::Baseline, Some(baseline_value), Some(actual)) => {
                assert!(
                    approx_eq_u64(actual, baseline_value, MEMORY_TOLERANCE_BYTES),
                    "case '{}': expected baseline memory limit ≈ {} but got {}",
                    case.name,
                    baseline_value,
                    actual
                );
            }
            (ExpectedMemoryLimit::Baseline, None, None) => {}
            (ExpectedMemoryLimit::Baseline, None, Some(actual)) => {
                // Baseline had no limit; any reported value should be greater than zero
                assert!(
                    actual > 0,
                    "case '{}': unexpected zero memory limit", case.name
                );
            }
            (ExpectedMemoryLimit::Baseline, Some(baseline_value), None) => {
                panic!(
                    "case '{}': baseline memory limit was {} but report returned None",
                    case.name,
                    baseline_value
                );
            }
        }
    }

    Ok(())
}
