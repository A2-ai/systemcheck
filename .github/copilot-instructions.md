# Copilot Instructions for `systemcheck`

Purpose: Help AI coding agents work effectively in this Rust repo by capturing the real architecture, workflows, and patterns used here.

## Big picture
- Single Rust binary that prints Linux resource diagnostics to stdout: CPU, Memory, and CGroup details.
- All logic lives in `src/main.rs`. Dependencies in `Cargo.toml`: `num_cpus`, `libc`, `humanize-bytes`. Edition: 2024.
- Releases are automated with `cargo-dist` via GitHub Actions; config in `dist-workspace.toml` and `.github/workflows/release.yml`.

## Key files
- `src/main.rs` – Entry point and all functions (CPU/memory/cgroup readers and printers).
- `Cargo.toml` – Crates, profiles (`[profile.dist]`), repo metadata.
- `dist-workspace.toml` – `cargo-dist` settings (targets: `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`).
- `.github/workflows/release.yml` – Tag-driven release workflow using `cargo-dist`.

## Architecture and patterns
- CPU info
  - System logical cores: parse `/proc/cpuinfo` (`processor` count) → fallback `libc::sysconf(_SC_NPROCESSORS_ONLN)` → fallback `num_cpus::get()`.
  - System physical cores: parse `physical id` + `core id` pairs in `/proc/cpuinfo` → fallback `num_cpus::get_physical()`.
  - "Available CPUs (cgroup)": `num_cpus::get()` (can be cgroup-limited).
  - See: `get_system_cpu_count`, `get_system_physical_cpu_count`, `print_cpu_info`.
- Memory info
  - Read `/proc/meminfo` (`MemTotal`, `MemAvailable`) in KB → convert to bytes. Used memory via `saturating_sub`.
  - Byte formatting uses `humanize_bytes::humanize_bytes_binary!` macro.
  - See: `get_system_memory_from_proc`, `print_memory_info`.
- CGroup detection and limits
  - Version detection: check v2 (`/sys/fs/cgroup/cgroup.controllers`) else v1 (`/sys/fs/cgroup/cpu` or `/sys/fs/cgroup/memory`).
  - Current cgroup path: parse `/proc/self/cgroup` (v2: `0::/path`, v1: `:memory:` line).
  - Quotas/limits pattern (strict order):
    - Try cgroup v2 under `/sys/fs/cgroup{cgroup_path}/...`, then v2 root.
    - Else try cgroup v1 under controller path with `{cgroup_path}`, then v1 root.
    - Treat `max` (v2) and very large sentinel values (v1) as "unlimited" → return `None`.
  - Functions: `get_cgroup_cpu_quota_for_path`, `read_cgroup_v2_cpu_quota_for_path`, `read_cgroup_v1_cpu_quota_for_path`, `get_cgroup_memory_limit_for_path`, `get_cgroup_memory_usage_for_path`, `print_cgroup_info`.
- Output shape
  - Three sections with headings and a warning symbol when constrained: see `print_cpu_info`, `print_memory_info`, `print_cgroup_info`.

## Conventions to follow
- Linux-only paths (`/proc`, `/sys/fs/cgroup`); code should be tolerant when files are missing/permissions restricted.
- Prefer graceful degradation with `Option`/`None` instead of panics; mirror existing fallback ordering.
- Keep output human-readable; use `humanize_bytes_binary!` for sizes and consistent labels/indentation.
- Keep changes small and dependency footprint minimal (current deps cover needs).

## Developer workflows
- Build/Run
  - `cargo build` (or `--release`), `cargo run`.
- Tests
  - `cargo test` (stdout with `-- --nocapture`).
- Lint/Format
  - `cargo fmt` (or `--check`), `cargo clippy` (e.g., `-- -W clippy::all`).
- Quick typecheck: `cargo check`.
- Release
  - Push a semver tag (e.g., `v0.1.0`) → GitHub Actions `Release` workflow runs `cargo-dist` v0.30.0 to build and upload artifacts for Linux (x86_64, aarch64).

## Extending the tool
- Adding a new resource section? Follow the existing print pattern: header + details + warnings when constrained.
- When reading kernel/cgroup files, use the same v2→v1 and specific-path→root fallbacks; return `Option` on "unlimited" or missing values.
- Reference existing helpers for examples: `get_current_cgroup_path`, `get_cgroup_memory_limit_for_path`.
