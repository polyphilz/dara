use std::{env, fs};

const APPLICATION_IDENTITIES_PATH: &str = "app-identities.json";
const BASE_CONFIG_PATH: &str = "tauri.conf.json";
const APP_IDENTIFIER_ENV: &str = "DARA_APP_IDENTIFIER";
const PRODUCTION_APP_IDENTIFIER_ENV: &str = "DARA_PRODUCTION_APP_IDENTIFIER";
const PRODUCTION_IDENTITY_KEY: &str = "production";
const IDENTIFIER_FIELD: &str = "identifier";

fn main() {
    let application_identities: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(APPLICATION_IDENTITIES_PATH)
            .expect("application identity mapping should be readable"),
    )
    .expect("application identity mapping should be valid JSON");
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
    let production_identifier = application_identities
        .get(PRODUCTION_IDENTITY_KEY)
        .and_then(|identity| identity.get(IDENTIFIER_FIELD))
        .and_then(serde_json::Value::as_str)
        .expect("application identity mapping should define the production identifier");
    let known_identifier = application_identities
        .as_object()
        .expect("application identity mapping should be an object")
        .values()
        .filter_map(|identity| identity.get(IDENTIFIER_FIELD))
        .filter_map(serde_json::Value::as_str)
        .any(|known| known == identifier);
    assert!(
        known_identifier,
        "Tauri config identifier {identifier:?} is absent from the application identity mapping"
    );

    println!("cargo:rerun-if-changed={APPLICATION_IDENTITIES_PATH}");
    println!("cargo:rerun-if-changed={BASE_CONFIG_PATH}");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo:rustc-env={APP_IDENTIFIER_ENV}={identifier}");
    println!("cargo:rustc-env={PRODUCTION_APP_IDENTIFIER_ENV}={production_identifier}");
    tauri_build::build()
}
