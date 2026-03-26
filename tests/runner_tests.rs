use std::collections::HashMap;

use baton::config::Service;
use baton::runner::{default_port_for_image, register_env_vars, toposort};

#[test]
fn toposort_no_deps() {
    let services = vec![
        svc("c"),
        svc("a"),
        svc("b"),
    ];
    let order = toposort(&services).unwrap();
    assert_eq!(order.len(), 3);
}

#[test]
fn toposort_linear_chain() {
    let services = vec![
        svc("db"),
        svc_after("cache", &["db"]),
        svc_after("api", &["cache"]),
        svc_after("frontend", &["api"]),
    ];
    let order = toposort(&services).unwrap();
    let pos = |name: &str| order.iter().position(|s| s == name).unwrap();
    assert!(pos("db") < pos("cache"));
    assert!(pos("cache") < pos("api"));
    assert!(pos("api") < pos("frontend"));
}

#[test]
fn toposort_diamond() {
    let services = vec![
        svc("db"),
        svc_after("auth", &["db"]),
        svc_after("users", &["db"]),
        svc_after("api", &["auth", "users"]),
    ];
    let order = toposort(&services).unwrap();
    let pos = |name: &str| order.iter().position(|s| s == name).unwrap();
    assert!(pos("db") < pos("auth"));
    assert!(pos("db") < pos("users"));
    assert!(pos("auth") < pos("api"));
    assert!(pos("users") < pos("api"));
}

#[test]
fn toposort_circular_detected() {
    let services = vec![
        svc_after("a", &["b"]),
        svc_after("b", &["a"]),
    ];
    let result = toposort(&services);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("circular"));
}

#[test]
fn toposort_self_referencing() {
    let services = vec![
        svc_after("a", &["a"]),
    ];
    let result = toposort(&services);
    assert!(result.is_err());
}

#[test]
fn toposort_complex_graph() {
    let services = vec![
        svc("db"),
        svc("redis"),
        svc_after("auth", &["db"]),
        svc_after("cache", &["redis"]),
        svc_after("api", &["auth", "cache"]),
        svc_after("worker", &["db", "redis"]),
        svc_after("gateway", &["api"]),
    ];
    let order = toposort(&services).unwrap();
    let pos = |name: &str| order.iter().position(|s| s == name).unwrap();
    assert!(pos("db") < pos("auth"));
    assert!(pos("redis") < pos("cache"));
    assert!(pos("auth") < pos("api"));
    assert!(pos("cache") < pos("api"));
    assert!(pos("api") < pos("gateway"));
    assert!(pos("db") < pos("worker"));
    assert!(pos("redis") < pos("worker"));
}

#[test]
fn toposort_many_independent() {
    let services: Vec<Service> = (0..100).map(|i| svc(&format!("svc-{i}"))).collect();
    let order = toposort(&services).unwrap();
    assert_eq!(order.len(), 100);
}

#[test]
fn toposort_long_chain() {
    let mut services = vec![svc("svc-0")];
    for i in 1..200 {
        services.push(svc_after(&format!("svc-{i}"), &[&format!("svc-{}", i - 1)]));
    }
    let order = toposort(&services).unwrap();
    assert_eq!(order.len(), 200);
    for i in 1..200 {
        let pos_prev = order.iter().position(|s| *s == format!("svc-{}", i - 1)).unwrap();
        let pos_curr = order.iter().position(|s| *s == format!("svc-{i}")).unwrap();
        assert!(pos_prev < pos_curr);
    }
}

#[test]
fn toposort_three_node_cycle() {
    let services = vec![
        svc_after("a", &["c"]),
        svc_after("b", &["a"]),
        svc_after("c", &["b"]),
    ];
    assert!(toposort(&services).is_err());
}

#[test]
fn default_ports_known_images() {
    assert_eq!(default_port_for_image("postgres:16"), Some(5432));
    assert_eq!(default_port_for_image("redis:7"), Some(6379));
    assert_eq!(default_port_for_image("mysql:8"), Some(3306));
    assert_eq!(default_port_for_image("mariadb:11"), Some(3306));
    assert_eq!(default_port_for_image("mongo:7"), Some(27017));
    assert_eq!(default_port_for_image("rabbitmq:3"), Some(5672));
    assert_eq!(default_port_for_image("nats:latest"), Some(4222));
    assert_eq!(default_port_for_image("nginx:latest"), None);
    assert_eq!(default_port_for_image("custom-app:v1"), None);
}

#[test]
fn env_var_injection_postgres() {
    let mut svc = svc("db");
    svc.image = Some("postgres:16".to_string());
    let mut env = HashMap::new();
    register_env_vars(&svc, "myapp", 5432, &mut env);
    assert_eq!(env["DB_HOST"], "localhost");
    assert_eq!(env["DB_PORT"], "5432");
    assert_eq!(env["DATABASE_URL"], "postgres://postgres:baton@localhost:5432/myapp");
}

#[test]
fn env_var_injection_redis() {
    let mut svc = svc("redis");
    svc.image = Some("redis:7".to_string());
    let mut env = HashMap::new();
    register_env_vars(&svc, "myapp", 6379, &mut env);
    assert_eq!(env["REDIS_HOST"], "localhost");
    assert_eq!(env["REDIS_PORT"], "6379");
    assert_eq!(env["REDIS_URL"], "redis://localhost:6379");
}

#[test]
fn env_var_injection_mysql() {
    let mut svc = svc("db");
    svc.image = Some("mysql:8".to_string());
    let mut env = HashMap::new();
    register_env_vars(&svc, "myapp", 3306, &mut env);
    assert_eq!(env["DATABASE_URL"], "mysql://root:baton@localhost:3306/myapp");
}

#[test]
fn env_var_injection_mongo() {
    let mut svc = svc("db");
    svc.image = Some("mongo:7".to_string());
    let mut env = HashMap::new();
    register_env_vars(&svc, "myapp", 27017, &mut env);
    assert_eq!(env["MONGO_URL"], "mongodb://localhost:27017/myapp");
}

#[test]
fn env_var_injection_plain_service() {
    let svc = svc("api");
    let mut env = HashMap::new();
    register_env_vars(&svc, "myapp", 4000, &mut env);
    assert_eq!(env["API_HOST"], "localhost");
    assert_eq!(env["API_PORT"], "4000");
    assert!(!env.contains_key("DATABASE_URL"));
}

#[test]
fn env_vars_accumulate() {
    let mut db = svc("db");
    db.image = Some("postgres:16".to_string());
    let mut redis = svc("cache");
    redis.image = Some("redis:7".to_string());

    let mut env = HashMap::new();
    register_env_vars(&db, "myapp", 5432, &mut env);
    register_env_vars(&redis, "myapp", 6379, &mut env);

    assert_eq!(env.len(), 6);
    assert!(env.contains_key("DATABASE_URL"));
    assert!(env.contains_key("REDIS_URL"));
    assert!(env.contains_key("DB_HOST"));
    assert!(env.contains_key("CACHE_HOST"));
}

fn svc(name: &str) -> Service {
    let mut s = Service::new(name);
    s.run = Some("./app".to_string());
    s
}

fn svc_after(name: &str, deps: &[&str]) -> Service {
    let mut s = svc(name);
    s.after = deps.iter().map(|d| d.to_string()).collect();
    s
}
