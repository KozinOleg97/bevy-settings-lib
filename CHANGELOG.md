# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-04-05

### Added
- Initial release of `bevy-settings-lib`
- Support for TOML, JSON, and binary (postcard) formats
- Asynchronous saving with atomic write‑then‑rename
- OS‑standard configuration directories via `directories` crate
- Game‑local directory storage option
- Built‑in validation via `ValidatedSetting` trait
- Event‑driven API: `PersistSetting`, `PersistAllSettings`, `ReloadSetting`, `SettingsSaveError`
- Automatic file naming based on struct name (snake_case conversion)
- Comprehensive test suite and documentation

### Changed
- (No changes – initial release)

### Fixed
- (No fixes – initial release)

### Removed
- (No removals – initial release)

[0.1.0]: https://github.com/yourname/bevy-settings-lib/releases/tag/v0.1.0