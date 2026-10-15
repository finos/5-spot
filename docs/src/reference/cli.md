# CLI Reference

Every 5-Spot runtime component is a subcommand of one binary, `5spot`, shipped
in one image, `ghcr.io/finos/5-spot` (ADR 0020). Each manifest in `deploy/`
selects its component with `args`.

## Synopsis

```bash
5spot [OPTIONS]                        # the controller (no subcommand)
5spot controller [OPTIONS]             # the controller, explicitly
5spot capacity-controller [OPTIONS]
5spot provider time-based [OPTIONS]
5spot provider capital-markets [OPTIONS]
5spot agent reclaim --node-name <NODE> [OPTIONS]
5spot agent kata-config --node-name <NODE> [OPTIONS]
```

| Subcommand | Component | Manifest |
|---|---|---|
| `controller` | `ScheduledMachine` controller | `deploy/deployment/` |
| `capacity-controller` | `ScheduledCapacity` controller (ADR 0011) | `deploy/capacity-controller/` |
| `provider time-based` | `TimeBasedSpotSchedule` provider | `deploy/spot-schedule-providers/time-based/` |
| `provider capital-markets` | `CapitalMarketsSchedule` provider | `deploy/spot-schedule-providers/capital-markets/` |
| `agent reclaim` | node-side emergency reclaim trigger | `deploy/node-agent/` |
| `agent kata-config` | node-side Kata drop-in delivery (privileged) | `deploy/kata-config-agent/` |

**Bare `5spot` runs the controller**, so controller Deployments that predate
the subcommands keep working. Controller flags given without a subcommand cannot
be combined with another subcommand.

Every option can also be set through the environment variable shown. An
explicit command-line flag takes precedence over its environment variable.
Boolean options are flags (`--enable-leader-election`); set them to `false`
through the environment variable (`FORCE_FINALIZER_ON_TIMEOUT=false`).

Each component runs under its own ServiceAccount and RBAC. Sharing a binary
grants no permission: the capacity controller still cannot delete a `Machine`,
and the controller's `ClusterRole` is not extended (ADR 0011).

## `5spot controller`

The `ScheduledMachine` controller.

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `--instance-id` | `OPERATOR_INSTANCE_ID` | `0` | Instance ID for multi-instance deployments |
| `--instance-count` | `OPERATOR_INSTANCE_COUNT` | `1` | Total number of instances |
| `--metrics-port` | `METRICS_PORT` | `8080` | Prometheus `/metrics` port |
| `--health-port` | `HEALTH_PORT` | `8081` | Health and readiness port |
| `--verbose`, `-v` | | | Debug logging (`RUST_LOG` overrides) |
| `--log-format` | `RUST_LOG_FORMAT` | `json` | `json` for structured logs, `text` for local development |
| `--enable-leader-election` | `ENABLE_LEADER_ELECTION` | off | Only the lease holder reconciles; required for multiple replicas |
| `--lease-name` | `LEASE_NAME` | `5spot-leader` | Leader-election `Lease` name |
| `--lease-namespace` | `POD_NAMESPACE` | `5spot-system` | Namespace of the `Lease` |
| `--lease-duration-secs` | `LEASE_DURATION_SECONDS` | `15` | How long the `Lease` is considered held |
| `--lease-renew-deadline-secs` | `LEASE_RENEW_DEADLINE_SECONDS` | `10` | The leader must renew before this elapses |
| `--lease-retry-period-secs` | `LEASE_RETRY_PERIOD_SECONDS` | `2` | Documented for parity; not a `LeaseManager` parameter |
| `--force-finalizer-on-timeout` | `FORCE_FINALIZER_ON_TIMEOUT` | on | On finalizer-cleanup timeout, remove the finalizer so namespace deletion is not blocked. Set the variable to `false` for strict cleanup |

```bash
5spot                                                    # defaults
5spot --enable-leader-election --lease-name 5spot-leader # HA
RUST_LOG=five_spot=debug,kube=info 5spot --log-format text
```

## `5spot capacity-controller`

The `ScheduledCapacity` controller. Its lease name defaults to its own, so it
never contends with the machine controller.

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `--metrics-port` | `METRICS_PORT` | `8080` | Prometheus `/metrics` port |
| `--health-port` | `HEALTH_PORT` | `8081` | `/healthz` and `/readyz` port |
| `--enable-leader-election` | `ENABLE_LEADER_ELECTION` | off | Only the lease holder reconciles |
| `--lease-name` | `LEASE_NAME` | `5spot-capacity-controller` | Leader-election `Lease` name |
| `--lease-namespace` | `LEASE_NAMESPACE` | `5spot-system` | Namespace of the `Lease` |
| `--lease-duration-secs` | `LEASE_DURATION_SECS` | `15` | `Lease` duration |
| `--lease-renew-deadline-secs` | `LEASE_RENEW_DEADLINE_SECS` | `10` | Renew deadline |

## `5spot provider time-based` and `5spot provider capital-markets`

The first-party spot-schedule providers (ADR 0009). Each computes
`status.active` for its schedule kind; without a provider running,
`ScheduledMachine`s never activate.

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `--metrics-port` | `METRICS_PORT` | `8080` | Prometheus `/metrics` port |

## `5spot agent reclaim`

Node-side DaemonSet that watches `/proc` (or the kernel's proc connector via
netlink) for processes matching the per-node `killIfCommands` list, and PATCHes
the local Node with reclaim annotations to trigger the controller's
emergency-reclaim flow. See the
[emergency-reclaim concept doc](../concepts/emergency-reclaim.md) and the
[configuration reference](../operations/configuration.md#reclaim-agent-daemonset).

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `--node-name` | `NODE_NAME` | _(required)_ | Node to annotate; from the downward API (`spec.nodeName`) |
| `--detector` | `RECLAIM_DETECTOR` | `auto` | `auto`, `netlink` or `poll`. `auto` uses `netlink` if its socket opens, else `poll`. The shipped DaemonSet sets `poll` |
| `--proc-root` | `RECLAIM_PROC_ROOT` | `/proc` | Filesystem root mapped to `/proc`; override for tests |
| `--machine-id-path` | `MACHINE_ID_PATH` | `/etc/machine-id` | Host machine-id file for the host-identity check |
| `--skip-host-id-check` | `SKIP_HOST_ID_CHECK` | off | Skip the `status.nodeInfo.machineID` cross-check. Leave off in production |
| `--oneshot` | | | Wait for the per-node ConfigMap watch to finish its initial list (up to 30 s), scan once, and exit |

```bash
5spot agent reclaim --node-name node-01 --detector poll       # what the DaemonSet runs
5spot agent reclaim --node-name dev-01 --proc-root /tmp/fake-proc \
  --skip-host-id-check --oneshot                              # local smoke test
```

Exit codes: `0` when the reclaim annotation is written (or already present),
`1` on an unrecoverable error (client initialisation, host-identity check,
netlink subscriber).

`--detector=netlink` needs both `CAP_NET_ADMIN` and the host's initial network
namespace, neither of which the shipped DaemonSet has, so it sets `poll` and
adds no capability (ADR 0021). See the
[detector tradeoff table](../operations/configuration.md#detector).

## `5spot agent kata-config`

Node-side DaemonSet that delivers a Kata containerd drop-in to the fixed host
path `/etc/k0s/containerd.d/kata.toml` and restarts the host k0s service so
containerd reloads it (ADR 0002, 0003, 0005).

| Option | Environment variable | Default | Description |
|---|---|---|---|
| `--node-name` | `NODE_NAME` | _(required)_ | This pod's Node; the agent reads its `kata-config-ref` annotation |
| `--host-root` | `HOST_ROOT` | `/host` | Host filesystem mount root inside the pod |
| `--poll-interval-secs` | `POLL_INTERVAL_SECS` | `30` | Seconds between drift-watch sweeps |
| `--metrics-port` | `METRICS_PORT` | `8080` | Prometheus `/metrics` port |
| `--oneshot` | `ONESHOT` | off | Run one reconcile and exit |

The restart enters host PID 1's namespaces and runs the host's `systemctl`. The
agent does this by re-executing its own binary as an internal, hidden
subcommand, `5spot nsenter -t 1 -m -u -i -n -p -- systemctl restart -- <unit>`,
so the image needs no `nsenter`, shell or `systemctl` of its own. It requires
`privileged: true` and `hostPID: true` (ADR 0004), and refuses to run the
command if the namespaces cannot be entered.

## Build-time tools

Separate binaries, never shipped in the image. Prefer the Makefile targets:

```bash
make crds     # crdgen: CRD YAML from the Rust types into deploy/crds/
make crddoc   # crddoc: docs/src/reference/api.md
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Runtime error |
| 2 | Invalid command line (unknown subcommand, bad or missing option) |

## Related

- [Configuration](../operations/configuration.md): detailed configuration
- [Multi-Instance](../operations/multi-instance.md): multi-instance setup
- [API Reference](./api.md): CRD reference
