# Changelog

All notable changes to 5-Spot will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial MkDocs documentation setup
- Comprehensive API reference documentation
- Machine lifecycle documentation with Mermaid diagrams

### Changed
- Migrated documentation to MkDocs Material theme

## [0.1.0-alpha] - 2025-XX-XX

### Added
- Core `ScheduledMachine` CRD with full specification
- Time-based scheduling with timezone support
- Day ranges (`mon-fri`) and hour ranges (`9-17`)
- CAPI integration for machine lifecycle
- Priority-based resource distribution
- Multi-instance support with consistent hashing
- Graceful shutdown with configurable timeout
- Kill switch for emergency removal
- CRD code generation (`crdgen` binary)
- API documentation generation (`crddoc` binary)
- Health and readiness endpoints
- Prometheus metrics endpoint

### Technical Details
- Built with kube-rs framework
- Async/await reconciliation loop
- Event-driven architecture
- Kubernetes-native status conditions

## Future Releases

See [Project Roadmap](./roadmaps/project-roadmap-2026.md) for planned features.
