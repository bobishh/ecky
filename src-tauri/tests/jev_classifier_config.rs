use ecky_cad_lib::contracts::{decode_config, encode_config, Config};
use serde_json::json;

fn config(jev: serde_json::Value) -> Config {
    serde_json::from_value(json!({
        "engines": [], "selectedEngineId": "", "jevClassifier": jev,
    }))
    .unwrap()
}

#[test]
fn jev_secret_and_enabled_round_trip_through_json_and_canonical_edn() {
    let expected = json!({"enabled": true, "apiKey": "fixture-private-token"});
    let original = config(expected.clone());
    let decoded = decode_config(&encode_config(&original).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["jevClassifier"],
        expected
    );
    assert!(!format!("{original:?}").contains("fixture-private-token"));
}

#[test]
fn invalid_jev_save_keeps_previous_durable_config_and_secret_out_of_error() {
    let root = std::env::temp_dir().join(format!("ecky-jev-config-{}", uuid::Uuid::new_v4()));
    let old = config(json!({"enabled": false, "apiKey": ""}));
    ecky_cad_lib::config_store::save_config(&root, old).unwrap();
    let before = std::fs::read(root.join("config.edn")).unwrap();
    let invalid = config(json!({"enabled": true, "apiKey": ""}));
    let error = ecky_cad_lib::config_store::save_config(&root, invalid).unwrap_err();
    assert!(error.message.contains("Jev API token is required"));
    assert_eq!(std::fs::read(root.join("config.edn")).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_jev_settings_decode_to_disabled() {
    let legacy: Config =
        serde_json::from_value(json!({"engines": [], "selectedEngineId": ""})).unwrap();
    let decoded = decode_config(&encode_config(&legacy).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["jevClassifier"],
        json!({"enabled": false, "apiKey": ""})
    );
}
