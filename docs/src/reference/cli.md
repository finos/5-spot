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
