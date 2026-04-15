# Contributing

Guidelines for contributing to 5-Spot.

## Getting Started

1. Fork the repository
2. Clone your fork
3. Create a feature branch
4. Make your changes
5. Submit a pull request

## Development Process

### Branch Naming

- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation updates
- `refactor/` - Code refactoring

Example: `feature/add-cron-schedule-support`

### Commit Messages

Follow conventional commits:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `refactor`: Code refactoring
- `test`: Test additions/changes
- `chore`: Maintenance tasks

Example:
```
feat(schedule): add support for cron expressions

Implements cron-style schedule expressions as an alternative
to the current day/hour range syntax.

Closes #123
```

### Code Style

```bash
# Format code
cargo fmt

# Lint code
cargo clippy -- -D warnings
```

All code must pass both checks before merging.

## Pull Request Process

### Before Submitting

1. **Tests pass**: `cargo test`
2. **Code formatted**: `cargo fmt`
3. **No clippy warnings**: `cargo clippy -- -D warnings`
4. **Documentation updated**: If applicable
5. **Changelog updated**: If user-facing change

### PR Template

```markdown
## Description
Brief description of changes.

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing
How was this tested?

## Checklist
- [ ] Tests pass
- [ ] Code is formatted
- [ ] No clippy warnings
- [ ] Documentation updated
```

### Review Process

1. Automated checks run
2. Maintainer reviews code
3. Address feedback
4. Maintainer approves
5. PR is merged

## Code Guidelines

### Rust Best Practices

```rust
// Use descriptive names
fn evaluate_schedule(schedule: &Schedule, current_time: DateTime<Tz>) -> bool

// Document public APIs
/// Evaluates whether the given schedule matches the current time.
///
/// # Arguments
/// * `schedule` - The schedule to evaluate
/// * `current_time` - The time to check against
///
/// # Returns
/// `true` if the current time falls within the schedule window
pub fn evaluate_schedule(schedule: &Schedule, current_time: DateTime<Tz>) -> bool {
    // ...
}

// Handle errors explicitly
fn parse_schedule(input: &str) -> Result<Schedule, ParseError> {
    // ...
}
```

### Error Handling

```rust
// Use custom error types
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("Invalid day format: {0}")]
    InvalidDay(String),
    
    #[error("Invalid hour range: {start}-{end}")]
    InvalidHourRange { start: u8, end: u8 },
}
```

### Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_schedule() {
        let schedule = Schedule::new(/* ... */);
        assert!(schedule.is_valid());
    }

    #[test]
    fn test_invalid_schedule_returns_error() {
        let result = Schedule::parse("invalid");
        assert!(result.is_err());
    }
}
```

## Documentation

### Code Documentation

- All public items must have doc comments
- Include examples where helpful
- Document error conditions

### User Documentation

- Update relevant pages in `docs/src/`
- Include practical examples
- Test documentation builds: `cd docs && mkdocs build`

## Reporting Issues

### Bug Reports

Include:
- 5-Spot version
- Kubernetes version
- Steps to reproduce
- Expected behavior
- Actual behavior
- Relevant logs

### Feature Requests

Include:
- Use case description
- Proposed solution
- Alternatives considered

## Community

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Questions and ideas
- **Pull Requests**: Code contributions

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 license.

## Related

- [Development Setup](./setup.md) - Environment setup
- [Building](./building.md) - Build instructions
- [Testing](./testing.md) - Test execution
