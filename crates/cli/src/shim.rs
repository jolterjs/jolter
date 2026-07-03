use std::process::ExitCode;

fn main() -> ExitCode {
    match jolter_shim::run_invoked_command() {
        Ok(code) => code,
        Err(error) => {
            let command =
                jolter_shim::invoked_command_name().unwrap_or_else(|| "jolter-shim".to_owned());
            eprintln!("{command}: {error}");
            ExitCode::FAILURE
        }
    }
}
