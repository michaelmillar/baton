<p align="center">
  <img src="assets/logo.svg" width="200" alt="Baton">
</p>

<h3 align="center">A single-binary deploy engine for single-node production systems</h3>

<p align="center">
  Snapshot your database before every deploy. Gate on migration success.<br>
  Roll back automatically if health checks fail. One TOML file, one binary, no cluster required.
</p>

---

https://github.com/user-attachments/assets/86e9be51-e6ef-409e-a047-418642a6d98f

---

## What it does

Baton manages your entire stack on one machine: containers, native processes, workers, cron jobs. It handles dependency ordering, health checks, crash recovery with backoff, service discovery via environment variables, and graceful shutdown.

What makes it different: `baton deploy` snapshots your stateful services before every deploy, runs migrations in dependency order, gates on health checks, and rolls back automatically if anything fails. No other single-node tool does this.

```toml
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
```

### Development

```
$ baton up --ui
loaded 3 vars from .env
starting myapp...

  [ok] db      postgres:16 on :5432
  [ok] redis   redis:7 on :6379
  [ok] api     ./api serve on :4000
  [ok] worker  ./api process-jobs running
  [ok] reports ./api generate-reports scheduled (0 2 * * *)
  [ui] dashboard at http://localhost:9500

all services running. ctrl+c to stop.
```

### Production deploy

```
$ baton deploy
loaded 3 vars from .env
deploying myapp...

  snapshotting stateful services...
    [ok] db (pg_dump)
    [ok] redis (redis)

  running migrations...
    api ... ok

  restarting services...
    [ok] api (container)
    [ok] worker (signalled)

  checking health...
    api :4000/health ... ok

deploy complete.
```

If the health check had failed, baton would have restored the database snapshot and reported the rollback. No manual intervention required.

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
baton up          # starts everything (development)
baton up --ui     # starts everything + web dashboard on :9500
baton deploy      # safe production deploy with snapshots + rollback
```

`baton init` detects Rust, Go, Node.js, Elixir, and Dockerfile projects automatically.

## Deploy lifecycle

`baton deploy` runs the following steps in order:

1. **Validate** config and check all services are reachable
2. **Snapshot** stateful services (Postgres via pg_dump, Redis via BGSAVE, or custom command)
3. **Migrate** in dependency order (each service's `migrate` command, if set)
4. **Restart** application services
5. **Health gate** (wait for each service's `/health` endpoint to pass)
6. **Roll back** if health fails (restore snapshot, report failure)
7. **Record** the full event timeline to `.baton/history.json`

If any step fails, everything after it is skipped and the appropriate rollback runs.

## Deploy commands

```
baton deploy                # safe deploy: snapshot, migrate, restart, health gate
baton rollback              # restore the latest snapshot
baton rollback <id>         # restore a specific snapshot
baton history               # show deploy timeline
baton snapshot              # take a manual snapshot without deploying
baton restore <id>          # restore a specific snapshot manually
```

## Backup configuration

For known database images, baton snapshots automatically:

| Image | Method | Automatic |
|-------|--------|-----------|
| `postgres:*` | pg_dump | Yes |
| `redis:*` | BGSAVE + copy | Yes |

For anything else, set the `backup` field to a shell command. Baton sets `BATON_SNAPSHOT_PATH` and `BATON_SERVICE_NAME` in the environment. Your command must write to `$BATON_SNAPSHOT_PATH`.

```toml
[[service]]
name = "search"
image = "elasticsearch:8"
backup = "./scripts/backup-elastic.sh"
```

Snapshots are stored locally in `.baton/snapshots/` with a `meta.json` manifest.

## Adding services

```
baton add postgres
baton add redis
baton add worker --run "./app process-jobs"
baton add cron --name reports --run "./app report" --schedule "0 2 * * *"
baton add static
baton add spa
baton add process --name api --run "./api serve" --port 4000
```

Known types: `postgres`, `redis`, `mysql`, `mariadb`, `mongo`, `rabbitmq`, `nats`, `worker`, `cron`, `static`, `spa`, `process`.

## Config reference

### App

```toml
[app]
name = "myapp"
domain = "myapp.com"      # enables reverse proxy with subdomain routing
proxy_port = 8443         # reverse proxy port (default 8443)
```

### Services

Each `[[service]]` must have one of `run`, `build`, `image`, or `static`.

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
| `backup` | string | Backup method or command (auto-detected for postgres/redis) |
| `migrate` | string | Migration command to run during deploys |
| `runtime` | string | Runtime hint (e.g. "beam" for Elixir) |
| `spa` | bool | Enable SPA routing for static sites |

### Environments

Override the app domain per environment:

```toml
[environments.staging]
domain = "staging.myapp.com"

[environments.prod]
domain = "myapp.com"
```

```
baton up --env staging    # uses staging.myapp.com as domain
```

## Service discovery

Baton injects environment variables so services can find each other:

| Service type | Variables |
|---|---|
| Any service with a port | `{NAME}_HOST`, `{NAME}_PORT` |
| Postgres | `DATABASE_URL` |
| Redis | `REDIS_URL` |
| MySQL/MariaDB | `DATABASE_URL` |
| MongoDB | `MONGO_URL` |

Database passwords are generated per app (stable across restarts) or overridden via `.env`:

```
# .env
POSTGRES_PASSWORD=my-secure-password
```

## Dashboard

```
baton up --ui                     # dashboard on :9500
baton up --ui --ui-port 8080      # custom port
```

The dashboard shows live service state, updating every 2 seconds. It reflects actual process status: running, restarting, crashed, stopped. Restart counts are tracked per service.

## Graceful shutdown

When you press ctrl+c, baton:

1. Sends SIGTERM to all managed processes
2. Waits up to 10 seconds for each to exit
3. Sends SIGKILL to any that did not stop
4. Stops containers in reverse dependency order

## Examples

See the [examples](examples/) directory:

- [simple-api](examples/simple-api/) ... API with Postgres, Redis, worker, and scheduled reports
- [static-site](examples/static-site/) ... SPA with static file serving
- [multi-service](examples/multi-service/) ... Multiple services with environment overrides

## How it compares

Every existing tool either requires Docker and cannot manage native processes, or manages native processes and cannot manage containers. Nothing does both from a single config, as a single binary, with deploy-aware state management.

| Tool | Native processes | Containers | Cron | Deploy snapshots | Migration gates | Auto rollback | Single binary |
|------|-----------------|------------|------|-----------------|----------------|---------------|---------------|
| Docker Compose | No | Yes | No | No | No | No | No (needs Docker) |
| Nomad | Yes | Yes | Yes | No | No | No | Yes |
| Dokku | No | Yes | Plugin | No | No | No | No (shell scripts) |
| Coolify | No | Yes | Partial | No | No | No | No (Laravel) |
| Kamal | No | Yes | No | No | No | Yes (deploys) | No (Ruby gem) |
| systemd | Yes | Yes (Podman) | Yes (timers) | No | No | No | Built-in |
| PM2 | Yes | No | Partial | No | No | No | No (Node.js) |
| **Baton** | **Yes** | **Yes** | **Yes** | **Yes** | **Yes** | **Yes** | **Yes** |

**Where baton is stronger.** Mixed runtime (a Go binary, a Postgres container, and a Python cron job in the same TOML). Pre-deploy database snapshots with automatic rollback on health failure. Migration orchestration in dependency order. Single binary with zero runtime dependencies.

**Where baton is weaker.** Compose has a much larger ecosystem and community. Dokku and Coolify handle TLS and git-push workflows. Kamal has zero-downtime rolling deploys across multiple hosts. Nomad scales to clusters. systemd has decades of battle-testing and cgroup resource limits. Baton has none of these yet.

**The closest alternative is Nomad.** Nomad already covers mixed workloads, periodic jobs, and has a web UI. But Nomad was designed for clusters, carries that weight in configuration complexity, and has no concept of pre-deploy snapshots, migration gates, or automatic data rollback. Baton is what you would build if you wanted Nomad's capability model scoped to a single node with deploy safety built in.

## Status

82 tests. Single binary. Zero external dependencies at runtime.

Not yet implemented:

- TLS via Let's Encrypt
- Rolling deploys / zero-downtime updates
- Log aggregation
- Resource limits (CPU/memory)
- MySQL/MariaDB snapshot support (Postgres and Redis are supported)
- Remote snapshot storage

## Licence

MIT
