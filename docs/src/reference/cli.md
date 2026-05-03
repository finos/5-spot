# CLI Reference

Command-line options for the 5-Spot controller.

## Synopsis

```bash
5spot [OPTIONS]
```

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--enable-leader-election` | | `false` | Enable leader election for HA |
| `--lease-name` | | `5spot-leader` | Lease resource name |
| `--lease-duration` | | `15` | Lease validity in seconds |
| `--metrics-port` | | `8080` | Port for Prometheus metrics endpoint |
| `--health-port` | | `8081` | Port for health check endpoints |
| `--verbose` | `-v` | | Enable verbose (debug) logging |
| `--help` | `-h` | | Print help information |
| `--version` | `-V` | | Print version information |

## Environment Variables

All options can be set via environment variables:

| Variable | CLI Equivalent |
|----------|----------------|
| `ENABLE_LEADER_ELECTION` | `--enable-leader-election` |
| `LEASE_NAME` | `--lease-name` |
| `LEASE_DURATION_SECONDS` | `--lease-duration` |
| `LEASE_RENEW_DEADLINE_SECONDS` | Renew deadline (default: 10) |
| `LEASE_RETRY_PERIOD_SECONDS` | Retry period (default: 2) |
| `METRICS_PORT` | `--metrics-port` |
| `HEALTH_PORT` | `--health-port` |
| `RUST_LOG` | `--verbose` (sets to `debug`) |

Environment variables take precedence over CLI arguments.

## Examples

### Basic Usage

```bash
5spot
```

### High Availability with Leader Election

```bash
5spot --enable-leader-election --lease-name 5spot-leader
```

### Custom Ports

```bash
5spot --metrics-port 9090 --health-port 9091
```

### Debug Logging

```bash
5spot --verbose
# Or
RUST_LOG=debug 5spot
```

### Fine-Grained Logging

```bash
RUST_LOG=five_spot=debug,kube=info 5spot
```

## `5spot-reclaim-agent`

Node-side DaemonSet binary that watches `/proc` (or the kernel's
proc connector via netlink) for processes matching the per-node
`killIfCommands` list and PATCHes the local Node with reclaim
annotations to trigger the controller's emergency-reclaim flow. See
the [emergency-reclaim concept doc](../concepts/emergency-reclaim.md)
for the full design and the
[configuration reference](../operations/configuration.md#reclaim-agent-daemonset)
for env-var equivalents.

### Synopsis

```bash
5spot-reclaim-agent [OPTIONS]
```

### Options

| Option | Default | Description |
|---|---|---|
| `--node-name` | _(required)_ | Name of the Node to annotate. Inject via downward API: `valueFrom.fieldRef.fieldPath: spec.nodeName` |
| `--detector <auto\|netlink\|poll>` | `auto` | Process-event source. `auto` picks `netlink` on Linux, `poll` elsewhere |
| `--proc-root <PATH>` | `/proc` | Filesystem root mapped to `/proc`. Override only for sandboxed/test runs |
| `--machine-id-path <PATH>` | `/etc/machine-id` | Host machine-id file (host-identity verification) |
| `--skip-host-id-check` | `false` | Skip the `Node.status.nodeInfo.machineID` cross-check before PATCH. Defence-in-depth — leave off in production |
| `--oneshot` | | Run the detector once and exit (smoke tests / one-off invocations) |
| `--help`, `-h` | | Print help |
| `--version`, `-V` | | Print version |

### Environment variables

| Variable | CLI Equivalent |
|---|---|
| `NODE_NAME` | `--node-name` |
| `RECLAIM_DETECTOR` | `--detector` |
| `RECLAIM_PROC_ROOT` | `--proc-root` |
| `MACHINE_ID_PATH` | `--machine-id-path` |
| `SKIP_HOST_ID_CHECK` | `--skip-host-id-check` |

### Examples

```bash
# Production default (auto-detect → netlink on Linux)
5spot-reclaim-agent --node-name "$(hostname)"

# Force /proc poll (no CAP_NET_ADMIN required, ≤250 ms detection)
5spot-reclaim-agent --node-name node-01 --detector poll

# Local smoke test against a fake /proc tree
5spot-reclaim-agent --node-name dev-01 \
  --proc-root /tmp/fake-proc \
  --skip-host-id-check \
  --oneshot
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Reclaim annotation written successfully (or already present — idempotent) |
| 1 | Unrecoverable error (kube client init, host-identity check failed, netlink subscriber failed, etc.) |

### Capability requirements (rung 2 / netlink only)

Rung 2 (`--detector=netlink`) requires `CAP_NET_ADMIN` on the agent
container. Granted at the pod level in
`deploy/node-agent/daemonset.yaml`. Operators who refuse the cap
can pin `--detector=poll` (rung 1) which needs no extra capability.
See the
[detector tradeoff table](../operations/configuration.md#detector)
for guidance.

## Utility Binaries

### crdgen

Generate CRD YAML from Rust types:

```bash
crdgen > deploy/crds/scheduledmachine.yaml
```

### crddoc

Generate API documentation:

```bash
crddoc > docs/reference/api.md
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |

## Related

- [Configuration](../operations/configuration.md) - Detailed configuration
- [Multi-Instance](../operations/multi-instance.md) - Multi-instance setup
- [API Reference](./api.md) - API documentation
