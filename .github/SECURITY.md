# Security Policy

## Supported Versions

We actively support the following versions with security updates:

| Version | Supported          |
| ------- | ------------------ |
| 1.x.x   | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security vulnerability, please follow these steps:

### 1. Do NOT create a public issue

Please do not report security vulnerabilities through public GitHub issues, discussions, or pull requests.

### 2. Report privately

Instead, please report security vulnerabilities by emailing us at:
**afzalimdad9@gmail.com**

Include the following information in your report:
- Type of issue (e.g. buffer overflow, SQL injection, cross-site scripting, etc.)
- Full paths of source file(s) related to the manifestation of the issue
- The location of the affected source code (tag/branch/commit or direct URL)
- Any special configuration required to reproduce the issue
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact of the issue, including how an attacker might exploit the issue

### 3. Response timeline

- We will acknowledge receipt of your vulnerability report within 48 hours
- We will provide a detailed response within 7 days indicating next steps
- We will keep you informed of our progress towards a fix and announcement
- We may ask for additional information or guidance

### 4. Disclosure policy

- We ask that you give us a reasonable amount of time to fix the issue before any disclosure to the public or a third party
- We will credit you in our security advisory (unless you prefer to remain anonymous)
- We will coordinate the timing of the disclosure with you

## Security best practices

When contributing to this project, please follow these security guidelines:

- Never commit secrets, API keys, passwords, or other sensitive information
- Use environment variables for configuration
- Validate all inputs and sanitize outputs
- Follow the principle of least privilege
- Keep dependencies up to date
- Use secure coding practices

## Security features

This project implements the following security measures:

- Input validation and sanitization
- Authentication and authorization
- Secure database connections
- Rate limiting
- CORS protection
- Security headers

## Dependencies

We regularly monitor our dependencies for known vulnerabilities using:
- Dependabot (automated dependency updates)
- `cargo audit` for Rust dependencies
- Regular security reviews

## Questions?

If you have questions about this security policy, please contact us at afzalimdad9@gmail.com.