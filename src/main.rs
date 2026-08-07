//! Binary entry point. Everything lives in `aqs::cli`.

use std::process::ExitCode;

fn main() -> ExitCode {
    match aqs::cli::run() {
        Ok(code) => code,
        Err(code) => code,
    }
}
