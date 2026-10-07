//! Layering rules (docs/PLAN.md §3, ADR 0001):
//! - only the app and the UI kit may depend on GPUI;
//! - `sc-api` and `sc-audio` never depend on each other;
//! - library crates never depend on the app;
//! - `sc-platform` stands alone and only the app uses it.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// (crate name, dependency names) for every workspace member.
fn members() -> Vec<(String, Vec<String>)> {
    let workspace: toml::Table = std::fs::read_to_string(root().join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    workspace["workspace"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            let manifest: toml::Table =
                std::fs::read_to_string(root().join(member.as_str().unwrap()).join("Cargo.toml"))
                    .unwrap()
                    .parse()
                    .unwrap();
            let name = manifest["package"]["name"].as_str().unwrap().to_owned();
            let deps = ["dependencies", "dev-dependencies", "build-dependencies"]
                .iter()
                .filter_map(|section| manifest.get(*section).and_then(|s| s.as_table()))
                .flat_map(|table| table.keys().cloned())
                .collect();
            (name, deps)
        })
        .collect()
}

fn depends_on(deps: &[String], prefix: &str) -> bool {
    deps.iter()
        .any(|dep| dep == prefix || dep.starts_with(&format!("{prefix}_")))
}

#[test]
fn only_the_app_and_the_ui_kit_use_gpui() {
    for (name, deps) in members() {
        if depends_on(&deps, "gpui") {
            assert!(
                matches!(name.as_str(), "cloudrs" | "cloudrs-ui"),
                "{name} must not depend on GPUI"
            );
        }
    }
}

#[test]
fn api_and_audio_stay_independent() {
    for (name, deps) in members() {
        match name.as_str() {
            "sc-api" => assert!(!deps.contains(&"sc-audio".to_owned())),
            "sc-audio" => assert!(!deps.contains(&"sc-api".to_owned())),
            _ => {}
        }
    }
}

#[test]
fn libraries_never_depend_on_the_app() {
    for (name, deps) in members() {
        if name != "cloudrs" {
            assert!(
                !deps.contains(&"cloudrs".to_owned()),
                "{name} depends on the app"
            );
        }
    }
}

#[test]
fn the_app_reaches_soundcloud_and_audio_only_through_the_core() {
    for (name, deps) in members() {
        if name == "cloudrs" {
            for lower in ["sc-api", "sc-audio"] {
                assert!(
                    !deps.contains(&lower.to_owned()),
                    "the app depends on {lower}"
                );
            }
        }
    }
}

#[test]
fn the_core_knows_nothing_about_the_ui() {
    for (name, deps) in members() {
        if name == "sc-core" {
            assert!(!depends_on(&deps, "gpui"));
            assert!(!deps.contains(&"cloudrs-ui".to_owned()));
        }
    }
}

#[test]
fn the_platform_layer_stands_alone() {
    for (name, deps) in members() {
        if name == "sc-platform" {
            for other in ["sc-api", "sc-audio", "sc-core", "cloudrs-ui", "cloudrs"] {
                assert!(
                    !deps.contains(&other.to_owned()),
                    "sc-platform depends on {other}"
                );
            }
            assert!(!depends_on(&deps, "gpui"));
        }
        if name != "cloudrs" {
            assert!(
                !deps.contains(&"sc-platform".to_owned()),
                "only the app may use sc-platform, not {name}"
            );
        }
    }
}
