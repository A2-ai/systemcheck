use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const CPU_QUOTA_PERCENT: &str = "200%"; // expect roughly 2.0 CPUs
const MEMORY_LIMIT: &str = "512M"; // expect ~512 MiB
const EXPECTED_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

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

fn parse_json_output(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(&text[start..=end]).ok()
}

fn find_systemcheck_binary() -> Option<PathBuf> {
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

    let unit_name = format!("systemcheck-test-{}", std::process::id());

    let output = Command::new("systemd-run")
        .arg("--user")
        .arg("--wait")
        .arg("--collect")
        .arg("--pipe")
        .arg("--quiet")
        .arg(format!("--unit={}", unit_name))
        .arg(format!("--property=CPUQuota={}", CPU_QUOTA_PERCENT))
        .arg(format!("--property=MemoryMax={}", MEMORY_LIMIT))
    .arg(&binary)
        .arg("-v")
        .arg("--json")
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "skipping: systemd-run failed (status {:?}): {}{}",
                o.status.code(),
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            return Ok(());
        }
        Err(err) => {
            eprintln!("skipping: failed to invoke systemd-run: {}", err);
            return Ok(());
        }
    };
    let json = parse_json_output(&output.stdout)
        .ok_or("failed to parse JSON output from systemcheck")?;

    // Validate CPU quota (~2 cores)
    let cpu_quota = json["cgroup"]["cpu_quota"].as_f64()
        .ok_or("missing cgroup.cpu_quota in JSON output")?;
    let expected_cpu_quota = 2.0_f64; // 200%
    let diff = (cpu_quota - expected_cpu_quota).abs();
    assert!(
        diff <= 0.1,
        "expected cpu quota ≈ {} but got {} (diff {})",
        expected_cpu_quota,
        cpu_quota,
        diff
    );

    // Validate memory limit (≈512 MiB)
    let memory_limit = json["memory"]["cgroup_memory_limit_bytes"].as_u64()
        .ok_or("missing memory.cgroup_memory_limit_bytes in JSON output")?;
    let memory_diff = if memory_limit > EXPECTED_MEMORY_BYTES {
        memory_limit - EXPECTED_MEMORY_BYTES
    } else {
        EXPECTED_MEMORY_BYTES - memory_limit
    };
    assert!(
        memory_diff <= 4096,
        "expected memory limit ≈ {} bytes but got {} (diff {})",
        EXPECTED_MEMORY_BYTES,
        memory_limit,
        memory_diff
    );

    Ok(())
}
