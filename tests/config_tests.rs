use std::io::Write;
use tempfile::NamedTempFile;

fn load_config(toml_content: &str) -> Result<baton::config::Config, anyhow::Error> {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml_content.as_bytes()).unwrap();
    baton::config::Config::load(f.path())
}

#[test]
fn minimal_valid_config() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        run = "./app"
    "#)
    .unwrap();

    assert_eq!(cfg.app.name, "test");
    assert_eq!(cfg.services.len(), 1);
    assert_eq!(cfg.services[0].name, "web");
}

#[test]
fn missing_app_name_fails() {
    let result = load_config(r#"
        [app]

        [[service]]
        name = "web"
        run = "./app"
    "#);
    assert!(result.is_err());
}

#[test]
fn service_without_source_fails() {
    let result = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "broken"
    "#);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("must have one of"));
}

#[test]
fn service_with_run_passes() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        run = "./app serve"
        port = 3000
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].port, Some(3000));
}

#[test]
fn service_with_image_passes() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"
        volume = "pg_data"
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].image.as_deref(), Some("postgres:16"));
}

#[test]
fn service_with_static_passes() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "site"
        static = "./dist"
        spa = true
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].static_dir.as_deref(), Some("./dist"));
    assert_eq!(cfg.services[0].spa, Some(true));
}

#[test]
fn service_with_build_passes() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        build = "."
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].build.as_deref(), Some("."));
}

#[test]
fn dependency_on_missing_service_fails() {
    let result = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        run = "./app"
        after = ["db"]
    "#);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("does not exist"));
}

#[test]
fn valid_dependency_chain() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"

        [[service]]
        name = "redis"
        image = "redis:7"

        [[service]]
        name = "api"
        run = "./api"
        after = ["db", "redis"]
    "#)
    .unwrap();
    assert_eq!(cfg.services.len(), 3);
    assert_eq!(cfg.services[2].after, vec!["db", "redis"]);
}

#[test]
fn invalid_cron_schedule_fails() {
    let result = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "job"
        run = "./task"
        schedule = "not a cron"
    "#);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("invalid schedule"));
}

#[test]
fn valid_5_field_cron() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "job"
        run = "./task"
        schedule = "0 2 * * *"
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].schedule.as_deref(), Some("0 2 * * *"));
}

#[test]
fn environments_parsed() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [environments.staging]
        domain = "staging.test.com"

        [environments.prod]
        domain = "test.com"

        [[service]]
        name = "web"
        run = "./app"
    "#)
    .unwrap();
    assert_eq!(cfg.environments.len(), 2);
    assert_eq!(cfg.environments["prod"].domain.as_deref(), Some("test.com"));
}

#[test]
fn empty_services_is_valid() {
    let cfg = load_config(r#"
        [app]
        name = "test"
    "#)
    .unwrap();
    assert!(cfg.services.is_empty());
}

#[test]
fn all_optional_fields() {
    let cfg = load_config(r#"
        [app]
        name = "full"
        domain = "full.example.com"

        [[service]]
        name = "web"
        run = "./app serve"
        port = 4000
        health = "/healthz"
        after = []
    "#)
    .unwrap();
    let svc = &cfg.services[0];
    assert_eq!(svc.health.as_deref(), Some("/healthz"));
}

#[test]
fn duplicate_service_names_rejected() {
    let result = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        run = "./a"

        [[service]]
        name = "web"
        run = "./b"
    "#);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate"));
}

#[test]
fn runtime_field() {
    let cfg = load_config(r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        build = "."
        runtime = "beam"
    "#)
    .unwrap();
    assert_eq!(cfg.services[0].runtime.as_deref(), Some("beam"));
}

#[test]
fn many_services_stress() {
    let mut toml = String::from("[app]\nname = \"stress\"\n\n");
    for i in 0..100 {
        toml.push_str(&format!(
            "[[service]]\nname = \"svc-{i}\"\nrun = \"./app-{i}\"\nport = {}\n\n",
            3000 + i
        ));
    }
    let cfg = load_config(&toml).unwrap();
    assert_eq!(cfg.services.len(), 100);
}

#[test]
fn deep_dependency_chain() {
    let mut toml = String::from("[app]\nname = \"chain\"\n\n");
    toml.push_str("[[service]]\nname = \"svc-0\"\nrun = \"./app\"\n\n");
    for i in 1..50 {
        toml.push_str(&format!(
            "[[service]]\nname = \"svc-{i}\"\nrun = \"./app\"\nafter = [\"svc-{}\"]\n\n",
            i - 1
        ));
    }
    let cfg = load_config(&toml).unwrap();
    assert_eq!(cfg.services.len(), 50);
}
