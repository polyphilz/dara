// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match dara_lib::run_recovery_from_args(std::env::args_os()) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => dara_lib::run(),
        Err(error) => {
            eprintln!("dara recovery failed: {error}");
            std::process::exit(1);
        }
    }
}
