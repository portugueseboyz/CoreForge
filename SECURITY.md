# Security policy

## Reporting a vulnerability

Please report privately through GitHub security advisories:
<https://github.com/portugueseboyz/CoreForge/security/advisories/new>. Do not open a public issue for security problems.
Include the version, distribution, and steps to reproduce. You will get a first answer within a few days.

## Scope

Most relevant: the privileged part of CoreForge.

- `coreforge-helper` (installed in `/usr/libexec/coreforge/`, run as root through `pkexec`): argument validation, path handling,
  anything that lets an unprivileged user do more than the documented actions.
- The polkit policy `org.coreforge.helper.policy`: wrong defaults, actions that skip authentication or are too broad.
- The unprivileged app: command injection, unsafe file writes, or leaking data in exported reports.

Out of scope: issues that need an already-root attacker, and bugs in third-party tools the app calls (smartctl, lm-sensors, ...).

## Supported versions

Only the latest release receives fixes.
