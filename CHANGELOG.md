# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-04-08

### Changed

- **Architecture**: Replaced ad‑hoc background threads with a persistent worker thread per settings type. This eliminates file race conditions and ensures save requests are processed sequentially, improving reliability.

### Updated

- **Dependencies**: Updated to latest compatible versions of all dependencies (bevy 0.18, directories 6.0.0, serde 1.0.228, thiserror 2.0.18, toml 1.1, serde_json 1.0, postcard 1.1, serial_test 3.4.0).

[0.1.2]: https://github.com/KozinOleg97/bevy-settings-lib/releases/tag/v0.1.2

## [0.1.1] - 2026-04-07

### Fixed

- **Validation**: `validate()` is now called before saving (`PersistSetting` and `PersistAllSettings`) and when a new
  value is provided via `PersistSetting { value: Some(...) }`, matching the documentation.
- **Documentation**: Clarified that `ReloadSetting` does **not** reset settings to default when the file is missing.
- **Documentation**: Added notes about first launch, dynamic defaults, and lazy file creation.

[0.1.1]: https://github.com/KozinOleg97/bevy-settings-lib/releases/tag/v0.1.1


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