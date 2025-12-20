# Contributing to Realtime Universal API

Thank you for your interest in contributing to the Realtime Universal API! This document provides guidelines and information for contributors.

## 🚀 Getting Started

### Prerequisites

- Rust 1.70 or later
- Docker and Docker Compose
- Git

### Development Setup

1. **Fork and clone the repository**:
   ```bash
   git clone https://github.com/your-username/realtime-universal-api.git
   cd realtime-universal-api
   ```

2. **Set up the development environment**:
   ```bash
   cp .env.example .env
   docker-compose up -d
   ```

3. **Build and test**:
   ```bash
   cargo build
   cargo test
   ```

## 📋 How to Contribute

### Reporting Issues

- Use the GitHub issue tracker to report bugs
- Include detailed reproduction steps
- Provide system information (OS, Rust version, etc.)
- Check existing issues before creating new ones

### Suggesting Features

- Open an issue with the "enhancement" label
- Describe the use case and expected behavior
- Consider if the feature aligns with the project's goals

### Code Contributions

1. **Check the task list**: Review [`.kiro/specs/realtime-saas-platform/tasks.md`](.kiro/specs/realtime-saas-platform/tasks.md) for available tasks
2. **Create a branch**: Use descriptive branch names like `feature/http2-support` or `fix/websocket-reconnection`
3. **Follow the spec**: Implement according to the design document in [`.kiro/specs/realtime-saas-platform/design.md`](.kiro/specs/realtime-saas-platform/design.md)
4. **Write tests**: Include both unit tests and property-based tests where applicable
5. **Update documentation**: Update relevant documentation and comments

## 🧪 Testing Guidelines

### Test Types

- **Unit Tests**: Test individual functions and modules
- **Property-Based Tests**: Use `proptest` for correctness properties
- **Integration Tests**: Test component interactions
- **Load Tests**: Performance and scalability testing

### Running Tests

```bash
# All tests
cargo test

# Property-based tests only
cargo test --test property_tests

# Integration tests
cargo test --test integration_tests

# Load tests (requires additional setup)
cargo test --test load_tests --release
```

## 📝 Code Style

### Rust Guidelines

- Follow standard Rust formatting (`cargo fmt`)
- Use `cargo clippy` for linting
- Write clear, self-documenting code
- Add comprehensive documentation for public APIs

### Commit Messages

Use conventional commit format:
```
type(scope): description

[optional body]

[optional footer]
```

Examples:
- `feat(websocket): add connection pooling`
- `fix(auth): resolve JWT token validation issue`
- `docs(readme): update installation instructions`

## 🏗️ Architecture Guidelines

### Protocol Implementation

When adding new protocol support:

1. **Design First**: Update the design document with protocol specifications
2. **Task Planning**: Add implementation tasks to the task list
3. **Property Tests**: Define correctness properties for the protocol
4. **Integration**: Ensure the protocol integrates with existing authentication and event systems

### Code Organization

- Keep protocol-specific code in separate modules
- Use the existing authentication and authorization patterns
- Follow the established error handling patterns
- Maintain tenant isolation for all new features

## 🔍 Review Process

### Pull Request Guidelines

- **Clear Description**: Explain what the PR does and why
- **Link Issues**: Reference related issues or tasks
- **Small Changes**: Keep PRs focused and reasonably sized
- **Tests Included**: Ensure all new code is tested
- **Documentation**: Update relevant documentation

### Review Criteria

- Code quality and style
- Test coverage and quality
- Performance implications
- Security considerations
- Documentation completeness

## 🌟 Recognition

Contributors will be recognized in:
- The project README
- Release notes for significant contributions
- The project's contributor list

## 📞 Getting Help

- **Discord**: Join our community Discord server (link coming soon)
- **Issues**: Use GitHub issues for technical questions
- **Discussions**: Use GitHub Discussions for general questions

## 📄 License

By contributing to this project, you agree that your contributions will be licensed under the MIT License.

Thank you for contributing to the Realtime Universal API! 🚀