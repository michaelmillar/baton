<p align="center">
  <img src="assets/logo.svg" width="200" alt="Baton">
</p>

<h3 align="center">A single-binary orchestrator for teams that left Kubernetes on purpose</h3>

<p align="center">
  One TOML file. Processes, containers, databases, workers, cron jobs.<br>
  Dependency-ordered startup, health checks, graceful shutdown, and a live dashboard.
</p>

---

https://github.com/user-attachments/assets/be3fb534-0b78-46f9-a5b1-d88b0e167189

---

## What it does

Baton reads a single `baton.toml` and runs your entire stack on one machine. It handles dependency ordering, health checks, crash recovery with backoff, service discovery via environment variables, and graceful shutdown (SIGTERM, wait, SIGKILL).

```toml
[app]
name = "myapp"
domain = "myapp.com"

[[service]]
name = "db"
image = "postgres:16"
volume = "pg_data"

[[service]]
name = "redis"
image = "redis:7"

[[service]]
name = "api"
run = "./api serve"
port = 4000
health = "/health"
after = ["db", "redis"]

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
baton up --ui     # starts everything + web dashboard on :9500
```

`baton init` detects Rust, Go, Node.js, Elixir, and Dockerfile projects automatically.

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

## Why baton

There are many tools in this space. Here is an honest look at where baton sits.

**The gap.** Every existing tool either requires Docker and cannot manage native processes, or manages native processes and cannot manage containers. There is nothing that does both from a single config, as a single binary, with no daemon dependency.

| Tool | Native processes | Containers | Cron | Service discovery | Single binary | Dashboard |
|------|-----------------|------------|------|-------------------|---------------|-----------|
| Docker Compose | No | Yes | No | Docker DNS | No (needs Docker) | No |
| Dokku | No | Yes | Plugin | Docker DNS | No (shell scripts) | No |
| Coolify | No | Yes | Partial | Traefik | No (Laravel in Docker) | Yes |
| CapRover | No | Yes (Swarm) | No | Swarm DNS | No (Node.js in Docker) | Yes |
| Kamal | No | Yes | No | No | No (Ruby gem) | No |
| Foreman/Overmind | Yes | No | No | No | Overmind only | No |
| systemd | Yes | Yes (Podman) | Yes (timers) | No | Built-in | No |
| PM2 | Yes | No | Partial | No | No (needs Node.js) | Paid cloud |
| **Baton** | **Yes** | **Yes** | **Yes** | **Yes** | **Yes** | **Yes** |

**What baton is not.** It is not a PaaS (no git-push deploys, no buildpacks). It is not a cluster scheduler. It does not manage TLS certificates. It is not for teams deploying for the first time. It is for people who know what good deployment looks like and decided one machine is enough.

**Where baton is weaker.** Compose has a much larger ecosystem and community. Dokku and Coolify handle TLS and git-push workflows. Kamal has zero-downtime rolling deploys. systemd has decades of battle-testing and cgroup resource limits. Baton has none of these yet.

**Where baton is stronger.** Mixed runtime (a Go binary, a Postgres container, and a Python cron job in the same TOML). Single binary with zero runtime dependencies. Dependency-ordered startup with health-check gates. Automatic service discovery via env vars. Live dashboard that reflects real process state. Graceful SIGTERM/SIGKILL shutdown. All of this without Docker for process-only stacks.

**The closest alternative is systemd + Podman/Quadlet.** This combination can manage native processes, containers, and cron (via timers). But it has no deployment UX, no config-driven service discovery, no dashboard, and no unified config format. Each service is a separate unit file. Baton is what you'd build if you wanted systemd's capability model with a deployment tool's UX.

## Status

68 tests. Single binary. Zero external dependencies at runtime.

Not yet implemented:

- TLS via Let's Encrypt
- Rolling deploys / zero-downtime updates
- Database backup and restore
- Log aggregation
- Resource limits (CPU/memory)

## Licence

MIT
