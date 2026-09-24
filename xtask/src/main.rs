// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Red Hat, Inc.

//! Development tasks for praxis-extproc.
//!
//! One task family today, `cargo xtask fips`: the FIPS compliance report and
//! the Red Hat base image checks, wrapped by the Makefile's `fips-*` targets.

#![allow(
    clippy::exit,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::unused_result_ok,
    clippy::unwrap_used,
    reason = "development tooling: it talks to a terminal and exits with a status"
)]
#![allow(let_underscore_drop, reason = "development tooling")]

mod fips;

use clap::{Parser, Subcommand};

// -----------------------------------------------------------------------------
// CLI Definition
// -----------------------------------------------------------------------------

/// Top-level CLI for xtask development commands.
#[derive(Parser)]
#[command(name = "xtask", about = "praxis-extproc development tasks")]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// Available xtask subcommands.
#[derive(Subcommand)]
enum Command {
    /// FIPS build tooling: the compliance report and the Red Hat base image
    /// checks.
    Fips(fips::Args),
}

// -----------------------------------------------------------------------------
// Entry Point
// -----------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Fips(args) => fips::run(args),
    }
}
