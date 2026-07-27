use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;
use ddd_lint::cli::Cli;
use tracing::Level;

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let stdout = io::stdout();
    let mut output = stdout.lock();
    match ddd_lint::execute(cli, &mut output) {
        Ok(status) => ExitCode::from(status.code()),
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "error: {error}");
            ExitCode::from(2)
        }
    }
}

fn init_logging(verbosity: u8) {
    let level = match verbosity {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let _ = tracing_subscriber::fmt()
        .without_time()
        .with_target(false)
        .with_max_level(level)
        .try_init();
}
