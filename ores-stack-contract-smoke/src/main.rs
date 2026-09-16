use std::{env, fs, path::PathBuf, process};

use ores_api_docs::{materialize_finalized_page_build, read_page_build_manifest, write_page_build_outputs};

fn main() {
    let root = unique_temp("valid");
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(root.join("src/pages/users/[id]")).unwrap();
    fs::write(
        root.join("src/pages/page.rs"),
        r#"#[ores_page(renderer = "mash", delivery = "ssr_only", title = "Home")]
pub async fn page() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/pages/users/[id]/page.rs"),
        r#"#[ores_page(
renderer = "mash",
delivery = "ssr_only",
render = "dynamic",
title = "User",
database = "read_only",
data_sources("rpc:GetUser", "orm:user_read"),
tags("fiducia-test", "consumer")
)]
pub async fn page() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/pages/users/[id]/style.css"),
        b"main { display: grid; }\n",
    )
    .unwrap();

    let first_pass = root.join(".ores-stack/first-pass");
    let outputs = write_page_build_outputs(&root, &first_pass).expect("first pass must succeed");
    let manifest = read_page_build_manifest(&outputs.manifest_path).expect("manifest must parse");
    assert_eq!(manifest.schema_version, "1.2.0");
    assert_eq!(manifest.routes.len(), 2);

    let user = manifest
        .routes
        .iter()
        .find(|route| route.canonical_path == "/users/{id}")
        .expect("dynamic user route");
    assert_eq!(user.axum_paths, vec!["/users/{id}"]);
    assert_eq!(user.renderer, "mash");
    assert_eq!(user.delivery, "ssr_only");
    assert_eq!(user.render, "dynamic");
    assert_eq!(user.database, "read_only");
    assert_eq!(user.data_sources, vec!["rpc:GetUser", "orm:user_read"]);
    assert!(user.css.is_some());
    assert!(user.wasm.is_none());

    let finalized = root.join(".ores-stack/materialized");
    let materialized = materialize_finalized_page_build(
        &root,
        &finalized,
        &outputs.manifest_path,
        &outputs.manifest_path.parent().unwrap().join("page-assets"),
    )
    .expect("finalized materialization must succeed");
    assert!(materialized.compile_glue_path.is_file());
    assert!(materialized.manifest_path.is_file());
    assert!(materialized.rerun_if_changed.contains(&outputs.manifest_path.canonicalize().unwrap()));
    assert!(materialized
        .rerun_if_changed
        .iter()
        .any(|path| path.file_name().and_then(|value| value.to_str()) == Some("page-assets")));

    let glue = fs::read_to_string(&materialized.compile_glue_path).unwrap();
    assert!(glue.contains("users"));
    assert!(glue.contains("PageContext") || glue.contains("axum"));

    exercise_conflict_rejection();
    exercise_static_only_generator_requirement();
    fs::remove_dir_all(&root).unwrap();
    println!("fiducia-cloud-test ores-stack contract smoke passed");
}

fn exercise_conflict_rejection() {
    let root = unique_temp("conflict");
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    for segment in ["[id]", "[slug]"] {
        let dir = root.join("src/pages/users").join(segment);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("page.rs"),
            r#"#[ores_page(renderer = "mash", delivery = "ssr_only")]
pub async fn page() {}
"#,
        )
        .unwrap();
    }
    let error = write_page_build_outputs(&root, &root.join(".ores-stack/first-pass"))
        .expect_err("ambiguous dynamic siblings must fail");
    let message = error.to_string().to_lowercase();
    assert!(message.contains("conflict") || message.contains("ambiguous"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

fn exercise_static_only_generator_requirement() {
    let root = unique_temp("missing-generator");
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    let dir = root.join("src/pages/docs/[slug]");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("page.rs"),
        r#"#[ores_page(renderer = "mash", delivery = "ssr_only", render = "static_only")]
pub async fn page() {}
"#,
    )
    .unwrap();
    let error = write_page_build_outputs(&root, &root.join(".ores-stack/first-pass"))
        .expect_err("dynamic static_only route without gen.rs must fail");
    let message = error.to_string();
    assert!(message.contains("requires sibling gen.rs"), "{message}");
    fs::remove_dir_all(&root).unwrap();
}

fn unique_temp(suffix: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "ores-stack-contract-smoke-{}-{}-{suffix}",
        process::id(),
        now_nanos()
    ))
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
