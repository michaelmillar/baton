<p align="center">
  <img src="assets/logo.svg" width="200" alt="Baton">
</p>

<h3 align="center">Deploy apps, not infrastructure</h3>

<p align="center">
  Baton is a deployment tool for teams who need to ship services without the overhead of Kubernetes.
  <br>One config file. One binary. Zero YAML.
</p>

---

<p align="center">
  <img src="assets/screenshot.svg" width="680" alt="Baton in action">
</p>

## What it does

Baton reads a single `baton.toml` file and runs your entire stack: processes, containers, databases, workers, cron jobs. It handles dependency ordering, health checks, restarts, service discovery, and graceful shutdown.

```toml
[app]
name = "myapp"
domain = "myapp.com"

[[service]]
name = "db"
image = "postgres:16"
volume = "pg_data"

[[service]]
name = "api"
run = "./api serve"
port = 4000
health = "/health"
after = ["db"]

[[service]]
name = "worker"
run = "./api process-jobs"
after = ["db"]
```

```
$ baton up
starting myapp...

  [ok] db     postgres:16 on :5432
  [ok] api    ./api serve on :4000
  [ok] worker ./api process-jobs running

all services running. ctrl+c to stop.
```

## Install

```
cargo install baton
```

Or build from source:

```
git clone https://github.com/michaelmillar/baton.git
cd baton
cargo build --release
```

## Quick start

```
cd your-project
baton init        # detects your stack, generates baton.toml
baton up          # starts everything
```

`baton init` detects Rust, Go, Node.js, Elixir, and Dockerfile projects automatically.

### Adding services

```
baton add postgres              # adds postgres:16 with volume
baton add redis                 # adds redis:7
baton add worker --run "./app process-jobs"
baton add cron --name reports --run "./app report" --schedule "0 2 * * *"
baton add static                # adds static file serving from ./dist
baton add spa                   # same, with SPA routing
baton add process --name api --run "./api serve" --port 4000
```

Known service types: `postgres`, `redis`, `mysql`, `mariadb`, `mongo`, `rabbitmq`, `nats`, `worker`, `cron`, `static`, `spa`, `process`.

## Config reference

### App

```toml
[app]
name = "myapp"
domain = "myapp.com"
```

### Services

Each `[[service]]` block defines one thing to run. A service must have one of `run`, `build`, `image`, or `static`.

| Field | Type | Purpose |
|-------|------|---------|
| `name` | string | Unique service name |
| `run` | string | Shell command to execute |
| `image` | string | Container image to pull and run |
| `build` | string | Path to build context (Dockerfile) |
| `static` | string | Path to static files to serve |
| `port` | int | Port the service listens on |
| `health` | string | HTTP health check path |
| `after` | list | Services that must start first |
| `volume` | string | Named volume for persistent data |
| `schedule` | string | Cron expression for scheduled tasks |
| `replicas` | int or map | Number of instances |
| `runtime` | string | Runtime hint (e.g. "beam" for Elixir clustering) |
| `cluster` | bool | Enable runtime-specific clustering |
| `team` | string | Team ownership label |
| `spa` | bool | Enable SPA routing for static sites |

### Environments

```toml
[environments.staging]
domain = "staging.myapp.com"
nodes = ["s1", "s2"]

[environments.prod]
domain = "myapp.com"
nodes = ["p1", "p2", "p3", "p4"]
```

### Per-environment replicas

```toml
[[service]]
name = "api"
run = "./api serve"
replicas = { staging = 1, prod = 3 }
```

## Service discovery

Baton injects environment variables so services can find each other:

| Service type | Variables injected |
|---|---|
| Any service with a port | `{NAME}_HOST`, `{NAME}_PORT` |
| Postgres | `DATABASE_URL` |
| Redis | `REDIS_URL` |
| MySQL/MariaDB | `DATABASE_URL` |
| MongoDB | `MONGO_URL` |

## Architecture

Baton is a single binary with three modes:

```
baton up        # local dev: runs everything on this machine
baton server    # control plane: accepts configs, schedules services
baton agent     # node agent: runs on each server, executes services
```

For local development, `baton up` is all you need. For production across multiple servers, run `baton server` somewhere and `baton agent` on each node.

## Examples

See the [examples](examples/) directory:

- [simple-api](examples/simple-api/) - API with Postgres, Redis, worker, and scheduled reports
- [static-site](examples/static-site/) - SPA with static file serving
- [multi-service](examples/multi-service/) - Multiple services across teams with environments

## Status

Baton is in early development. Working today:

- [x] TOML config parsing and validation
- [x] Project auto-detection (`baton init`)
- [x] Process management with restart and backoff
- [x] Container management (Docker/Podman)
- [x] Dependency ordering (topological sort)
- [x] Service discovery via env vars
- [x] Graceful shutdown
- [x] Static file serving with SPA support
- [x] Cron scheduling
- [x] `baton add` scaffolding (12 service types)
- [ ] TLS via Let's Encrypt
- [ ] Remote node management
- [ ] Server and agent modes
- [ ] Rolling deployments

## Licence

MIT
