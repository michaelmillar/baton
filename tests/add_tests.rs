use std::fs;
use tempfile::TempDir;

fn run_add_in(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    let baton = env!("CARGO_BIN_EXE_baton");
    std::process::Command::new(baton)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run baton")
}

fn setup_dir(initial_toml: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("baton.toml"), initial_toml).unwrap();
    dir
}

fn read_toml(dir: &std::path::Path) -> String {
    fs::read_to_string(dir.join("baton.toml")).unwrap()
}

#[test]
fn add_postgres() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "postgres"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("postgres:16"));
    assert!(toml.contains("pg_data"));
}

#[test]
fn add_redis() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "redis"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("redis:7"));
}

#[test]
fn add_mysql() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "mysql"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("mysql:8"));
}

#[test]
fn add_mongo() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "mongo"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("mongo:7"));
}

#[test]
fn add_worker_with_custom_command() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "worker", "--run", "./my-worker"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("./my-worker"));
}

#[test]
fn add_cron_with_schedule() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(
        dir.path(),
        &["add", "cron", "--name", "nightly", "--run", "./cleanup", "--schedule", "0 2 * * *"],
    );
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("./cleanup"));
    assert!(toml.contains("0 2 * * *"));
    assert!(toml.contains("nightly"));
}

#[test]
fn add_static() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "static"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("./dist"));
}

#[test]
fn add_spa() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "spa"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("spa = true"));
}

#[test]
fn add_duplicate_fails() {
    let dir = setup_dir("[app]\nname = \"test\"\n\n[[service]]\nname = \"db\"\nimage = \"postgres:16\"\n");
    let out = run_add_in(dir.path(), &["add", "postgres", "--name", "db"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already exists"));
}

#[test]
fn add_preserves_existing_content() {
    let initial = "[app]\nname = \"myapp\"\ndomain = \"myapp.com\"\n\n[[service]]\nname = \"web\"\nrun = \"./app serve\"\nport = 4000\n";
    let dir = setup_dir(initial);
    run_add_in(dir.path(), &["add", "redis"]);
    let toml = read_toml(dir.path());
    assert!(toml.contains("myapp.com"));
    assert!(toml.contains("./app serve"));
    assert!(toml.contains("redis:7"));
}

#[test]
fn add_without_baton_toml_fails() {
    let dir = TempDir::new().unwrap();
    let out = run_add_in(dir.path(), &["add", "postgres"]);
    assert!(!out.status.success());
}

#[test]
fn add_unknown_type_without_run_fails() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "foobar"]);
    assert!(!out.status.success());
}

#[test]
fn add_unknown_type_with_run_succeeds() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "custom", "--run", "./custom-thing"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("./custom-thing"));
}

#[test]
fn add_with_custom_port() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "postgres", "--port", "5433"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("5433"));
}

#[test]
fn add_multiple_services_sequentially() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    run_add_in(dir.path(), &["add", "postgres"]);
    run_add_in(dir.path(), &["add", "redis"]);
    run_add_in(dir.path(), &["add", "worker", "--run", "./w"]);
    let toml = read_toml(dir.path());
    assert!(toml.contains("postgres:16"));
    assert!(toml.contains("redis:7"));
    assert!(toml.contains("./w"));
    let count = toml.matches("[[service]]").count();
    assert_eq!(count, 3);
}

#[test]
fn add_rabbitmq() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "rabbitmq"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("rabbitmq:3-management"));
}

#[test]
fn add_nats() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "nats"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("nats:latest"));
}

#[test]
fn add_process_with_port() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "process", "--name", "api", "--run", "./api serve", "--port", "4000"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("./api serve"));
    assert!(toml.contains("4000"));
    assert!(toml.contains("/health"));
}

#[test]
fn add_pg_alias() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "pg"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("postgres:16"));
}

#[test]
fn add_mariadb() {
    let dir = setup_dir("[app]\nname = \"test\"\n");
    let out = run_add_in(dir.path(), &["add", "mariadb"]);
    assert!(out.status.success());
    let toml = read_toml(dir.path());
    assert!(toml.contains("mariadb:11"));
}
