# Changelog

All notable changes to this project are documented here. Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-19

First public release.

### Added
- System information: CPU, memory, GPU, disks, network, board and kernel.
- Live sensors (temperatures, fans, voltages, power) with history charts.
- CPU, RAM and GPU stress tests and benchmarks.
- Disk (SMART) and network diagnostics.
- Fan control with curves and profiles; RGB/LED control.
- Game library detection and tray icon.
- Privileged `coreforge-helper` run through `pkexec` with a polkit policy; accepts only a closed set of validated actions.
- Packages: `.deb`, `.rpm` and AppImage, with desktop entry and AppStream metadata. Portuguese and English interface.

[Unreleased]: https://github.com/portugueseboyz/CoreForge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/portugueseboyz/CoreForge/releases/tag/v0.1.0
