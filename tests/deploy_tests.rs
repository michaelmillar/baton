use std::io::Write;
use tempfile::NamedTempFile;

fn load_config(toml_content: &str) -> Result<baton::config::Config, anyhow::Error> {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml_content.as_bytes()).unwrap();
    baton::config::Config::load(f.path())
}

#[test]
fn config_with_backup_field() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"
        volume = "pg_data"
        backup = "pg_dump"
    "#,
    )
    .unwrap();
    assert_eq!(cfg.services[0].backup.as_deref(), Some("pg_dump"));
}

#[test]
fn config_with_migrate_field() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "api"
        run = "./api serve"
        migrate = "./api migrate"
    "#,
    )
    .unwrap();
    assert_eq!(cfg.services[0].migrate.as_deref(), Some("./api migrate"));
}

#[test]
fn config_with_both_backup_and_migrate() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"
        backup = "pg_dump"

        [[service]]
        name = "api"
        run = "./api serve"
        port = 4000
        health = "/health"
        after = ["db"]
        migrate = "./api migrate"
    "#,
    )
    .unwrap();
    assert_eq!(cfg.services[0].backup.as_deref(), Some("pg_dump"));
    assert_eq!(cfg.services[1].migrate.as_deref(), Some("./api migrate"));
}

#[test]
fn config_backup_and_migrate_are_optional() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "web"
        run = "./app"
    "#,
    )
    .unwrap();
    assert!(cfg.services[0].backup.is_none());
    assert!(cfg.services[0].migrate.is_none());
}

#[test]
fn postgres_has_implicit_backup() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"
    "#,
    )
    .unwrap();
    assert!(baton::snapshot::resolve_has_backup(&cfg.services[0]));
}

#[test]
fn redis_has_implicit_backup() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "cache"
        image = "redis:7"
    "#,
    )
    .unwrap();
    assert!(baton::snapshot::resolve_has_backup(&cfg.services[0]));
}

#[test]
fn plain_process_has_no_implicit_backup() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "api"
        run = "./api serve"
    "#,
    )
    .unwrap();
    assert!(!baton::snapshot::resolve_has_backup(&cfg.services[0]));
}

#[test]
fn custom_backup_overrides_implicit() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"
        backup = "custom_backup.sh"
    "#,
    )
    .unwrap();
    assert!(baton::snapshot::resolve_has_backup(&cfg.services[0]));
    assert_eq!(cfg.services[0].backup.as_deref(), Some("custom_backup.sh"));
}

#[test]
fn snapshot_list_empty_when_no_dir() {
    let snapshots = baton::snapshot::list_snapshots().unwrap();
    assert!(snapshots.is_empty());
}

#[test]
fn history_empty_when_no_file() {
    let records = baton::history::load_history().unwrap();
    assert!(records.is_empty());
}

#[test]
fn deploy_recorder_tracks_events() {
    let mut recorder = baton::history::DeployRecorder::start();
    recorder.set_snapshot("20260329-143000");
    recorder.migrate_ok("api");
    recorder.restart("api");
    recorder.health_pass("api");
    recorder.finish(baton::history::DeployOutcome::Success);

    assert_eq!(recorder.snapshot_id(), Some("20260329-143000"));
}

#[test]
fn deploy_recorder_tracks_failure() {
    let mut recorder = baton::history::DeployRecorder::start();
    recorder.set_snapshot("20260329-143000");
    recorder.migrate_fail("api", "migration error");
    recorder.rollback("restored snapshot 20260329-143000");
    recorder.finish(baton::history::DeployOutcome::RolledBack);

    assert_eq!(recorder.snapshot_id(), Some("20260329-143000"));
}

#[test]
fn migrations_run_in_topo_order() {
    let cfg = load_config(
        r#"
        [app]
        name = "test"

        [[service]]
        name = "db"
        image = "postgres:16"

        [[service]]
        name = "api"
        run = "./api serve"
        after = ["db"]
        migrate = "./api migrate"

        [[service]]
        name = "worker"
        run = "./worker"
        after = ["api"]
        migrate = "./worker migrate"
    "#,
    )
    .unwrap();

    let order = baton::runner::toposort(&cfg.services).unwrap();

    let api_pos = order.iter().position(|n| n == "api").unwrap();
    let worker_pos = order.iter().position(|n| n == "worker").unwrap();
    assert!(api_pos < worker_pos);
}

#[test]
fn full_deploy_config_parses() {
    let cfg = load_config(
        r#"
        [app]
        name = "myapp"
        domain = "myapp.com"

        [[service]]
        name = "db"
        image = "postgres:16"
        volume = "pg_data"
        backup = "pg_dump"

        [[service]]
        name = "redis"
        image = "redis:7"

        [[service]]
        name = "api"
        run = "./api serve"
        port = 4000
        health = "/health"
        after = ["db", "redis"]
        migrate = "./api migrate"

        [[service]]
        name = "worker"
        run = "./api process-jobs"
        after = ["db", "redis"]

        [[service]]
        name = "reports"
        run = "./api generate-reports"
        schedule = "0 2 * * *"
        after = ["db"]
    "#,
    )
    .unwrap();

    assert_eq!(cfg.services.len(), 5);
    assert!(baton::snapshot::resolve_has_backup(&cfg.services[0]));
    assert!(baton::snapshot::resolve_has_backup(&cfg.services[1]));
    assert!(!baton::snapshot::resolve_has_backup(&cfg.services[2]));
    assert_eq!(cfg.services[2].migrate.as_deref(), Some("./api migrate"));
}
