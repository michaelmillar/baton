use std::path::Path;

use anyhow::{bail, Context, Result};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

pub struct AddOptions {
    pub service_type: String,
    pub name: Option<String>,
    pub port: Option<u16>,
    pub run: Option<String>,
    pub schedule: Option<String>,
}

struct ServiceTemplate {
    name: String,
    fields: Vec<(&'static str, Value)>,
}

pub fn run(opts: AddOptions) -> Result<()> {
    let config_path = Path::new("baton.toml");

    if !config_path.exists() {
        bail!("baton.toml not found. run 'baton init' first.");
    }

    let content = std::fs::read_to_string(config_path)
        .context("failed to read baton.toml")?;
    let mut doc: DocumentMut = content.parse()
        .context("failed to parse baton.toml")?;

    let template = build_template(&opts)?;

    if !doc.contains_key("service") {
        doc["service"] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let services = doc["service"]
        .as_array_of_tables_mut()
        .context("'service' must be an array of tables")?;

    let exists = services.iter().any(|t| {
        t.get("name")
            .and_then(|v| v.as_str())
            .map(|n| n == template.name)
            .unwrap_or(false)
    });

    if exists {
        bail!("service '{}' already exists in baton.toml", template.name);
    }

    let mut table = Table::new();
    table.insert("name", toml_edit::value(&template.name));
    for (key, val) in template.fields {
        table.insert(key, Item::Value(val));
    }

    services.push(table);

    std::fs::write(config_path, doc.to_string())
        .context("failed to write baton.toml")?;

    println!("added '{}' to baton.toml", template.name);
    Ok(())
}

fn build_template(opts: &AddOptions) -> Result<ServiceTemplate> {
    let stype = opts.service_type.to_lowercase();
    let name = opts.name.clone().unwrap_or_else(|| stype.clone());

    let fields = match stype.as_str() {
        "postgres" | "postgresql" | "pg" => vec![
            ("image", Value::from("postgres:16")),
            ("volume", Value::from("pg_data")),
        ],
        "redis" => vec![
            ("image", Value::from("redis:7")),
        ],
        "mysql" | "mariadb" => {
            let img = if stype == "mariadb" { "mariadb:11" } else { "mysql:8" };
            vec![
                ("image", Value::from(img)),
                ("volume", Value::from("mysql_data")),
            ]
        }
        "mongo" | "mongodb" => vec![
            ("image", Value::from("mongo:7")),
            ("volume", Value::from("mongo_data")),
        ],
        "rabbitmq" | "rabbit" => vec![
            ("image", Value::from("rabbitmq:3-management")),
        ],
        "nats" => vec![
            ("image", Value::from("nats:latest")),
        ],
        "worker" => {
            let cmd = opts.run.clone()
                .unwrap_or_else(|| "./app worker".to_string());
            let mut f = vec![("run", Value::from(&*cmd))];
            if let Some(after) = build_after_array(opts) {
                f.push(("after", Value::Array(after)));
            }
            f
        }
        "cron" | "scheduled" => {
            let cmd = opts.run.clone()
                .unwrap_or_else(|| "./app task".to_string());
            let sched = opts.schedule.clone()
                .unwrap_or_else(|| "0 * * * *".to_string());
            let mut f = vec![
                ("run", Value::from(&*cmd)),
                ("schedule", Value::from(&*sched)),
            ];
            if let Some(after) = build_after_array(opts) {
                f.push(("after", Value::Array(after)));
            }
            f
        }
        "static" | "spa" => {
            let mut f = vec![
                ("static", Value::from("./dist")),
                ("port", Value::from(3000_i64)),
            ];
            if stype == "spa" {
                f.push(("spa", Value::from(true)));
            }
            f
        }
        "process" | "service" => {
            let cmd = opts.run.clone()
                .unwrap_or_else(|| "./app serve".to_string());
            let mut f = vec![("run", Value::from(&*cmd))];
            if let Some(p) = opts.port {
                f.push(("port", Value::from(p as i64)));
                f.push(("health", Value::from("/health")));
            }
            if let Some(after) = build_after_array(opts) {
                f.push(("after", Value::Array(after)));
            }
            f
        }
        _ => {
            if let Some(cmd) = &opts.run {
                let mut f = vec![("run", Value::from(cmd.as_str()))];
                if let Some(p) = opts.port {
                    f.push(("port", Value::from(p as i64)));
                }
                if let Some(s) = &opts.schedule {
                    f.push(("schedule", Value::from(s.as_str())));
                }
                f
            } else {
                bail!(
                    "unknown service type '{}'. known types: postgres, redis, mysql, mongo, \
                     rabbitmq, nats, worker, cron, static, spa, process.\n\
                     or use --run to specify a custom command.",
                    stype
                );
            }
        }
    };

    let mut fields = fields;
    if let Some(p) = opts.port
        && !fields.iter().any(|(k, _)| *k == "port")
    {
        fields.push(("port", Value::from(p as i64)));
    }

    Ok(ServiceTemplate { name, fields })
}

fn build_after_array(_opts: &AddOptions) -> Option<Array> {
    None
}
