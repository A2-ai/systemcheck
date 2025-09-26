# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust application called `systemcheck` that uses the `num_cpus` crate for system information gathering.

## Development Commands

### Build
```bash
cargo build           # Debug build
cargo build --release # Release build
```

### Run
```bash
cargo run             # Run debug version
cargo run --release   # Run release version
```

### Test
```bash
cargo test            # Run all tests
cargo test [test_name] # Run specific test
cargo test -- --nocapture # Show stdout during tests
```

### Lint & Format
```bash
cargo fmt             # Format code
cargo fmt --check     # Check formatting without changes
cargo clippy          # Run linter
cargo clippy -- -W clippy::all # Run with all warnings
```

### Check
```bash
cargo check           # Quick compile check without producing binary
```

## Architecture

The project is a simple Rust binary application with:
- Entry point: `src/main.rs`
- Single dependency: `num_cpus` for CPU information retrieval
- Uses Rust 2024 edition

The codebase is minimal and focused on system information gathering functionality.