use std::{env, fs};

const BASE_CONFIG_PATH: &str = "tauri.conf.json";
const APP_IDENTIFIER_ENV: &str = "DARA_APP_IDENTIFIER";
const PRODUCTION_APP_IDENTIFIER: &str = "com.rohan.dara";

fn main() {
    let base_config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(BASE_CONFIG_PATH).expect("Tauri base config should be readable"),
    )
    .expect("Tauri base config should be valid JSON");
    let config_override = env::var("TAURI_CONFIG")
        .ok()
        .map(|value| {
            serde_json::from_str::<serde_json::Value>(&value)
                .expect("TAURI_CONFIG should be valid JSON")
        })
        .unwrap_or_default();
    let identifier = config_override
        .get("identifier")
        .or_else(|| base_config.get("identifier"))
        .and_then(serde_json::Value::as_str)
        .expect("Tauri config should define an application identifier");

    println!("cargo:rerun-if-changed={BASE_CONFIG_PATH}");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo:rustc-env={APP_IDENTIFIER_ENV}={identifier}");
    println!(
        "cargo:rustc-env=DARA_PRODUCTION_BUILD={}",
        identifier == PRODUCTION_APP_IDENTIFIER
    );
    tauri_build::build()
}
