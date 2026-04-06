# bevy-settings-lib

[![Crates.io](https://img.shields.io/crates/v/bevy-settings-lib.svg)](https://crates.io/crates/bevy-settings-lib)
[![Documentation](https://docs.rs/bevy-settings-lib/badge.svg)](https://docs.rs/bevy-settings-lib)
[![Bevy version](https://img.shields.io/badge/bevy-0.18-blue.svg)](https://bevyengine.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)

A flexible settings management library for Bevy with async saving, multiple formats, and built‑in validation.

This library provides a convenient way to save, load, and reload settings in Bevy applications.
It supports text formats (TOML, JSON) and binary (postcard) with atomic write‑then‑rename
to prevent file corruption, with a clean, event‑driven API.

## Features

- **Any number of configurations** – each configuration has its own data type and file name.
- **File names can be explicit or derived from the struct name** (automatically converted to snake_case).
- **Asynchronous saving** with atomic write‑then‑rename – files are never left in a corrupted state.
- **Format support**: TOML (default), JSON, binary (postcard).
- **Load from OS‑standard directories** (via `directories` crate) **or from the game's local folder**.
- **Events**: `PersistSetting<S>`, `PersistAllSettings`, `ReloadSetting<S>`, `SettingsSaveError<S>`.
- **Partial loading** – if a file does not exist, `S::default()` is used.
- **Validation** – every settings type must implement `ValidatedSetting` to normalize values after loading and before
  saving.
- **Thread‑safe** – background threads handle file I/O without blocking the main thread.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy-settings-lib = "0.1.0"
```

## Quick Start

```rust
use bevy::prelude::*;
use bevy_settings_lib::{SettingsPlugin, PersistSetting, SettingsPluginConfig, FormatKind, ValidatedSetting, SettingsStorage};
use serde::{Serialize, Deserialize};

#[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
struct MySettings {
    volume: f32,
    fullscreen: bool,
}

// Mandatory validation implementation (can be empty if no validation needed)
impl ValidatedSetting for MySettings {
    fn validate(&mut self) {
        self.volume = self.volume.clamp(0.0, 1.0);
    }
}

fn main() {
    // Use the system configuration directory (AppData, ~/.config, etc.)
    let config = SettingsPluginConfig {
        format: FormatKind::Toml,
        company: "MyCompany".into(),
        project: "MyGame".into(),
        file_name: None, // auto‑name → "my_settings"
        storage: SettingsStorage::SystemConfigDir,
        ..Default::default()
    };
    App::new()
        .add_plugins(SettingsPlugin::<MySettings>::from_config(config))
        .add_systems(Update, save_on_keypress)
        .run();
}

fn save_on_keypress(mut commands: Commands, keyboard: Res<ButtonInput<KeyCode>>) {
    if keyboard.just_pressed(KeyCode::KeyS) {
        commands.trigger(PersistSetting::<MySettings> { value: None });
    }
}
```

## Guide

### 1. Defining a Settings Type

Your settings type must be a Bevy `Resource` and implement `Serialize`, `Deserialize`, `Clone`, `Debug`, `Default`, and
`PartialEq`.
It also **must** implement the `ValidatedSetting` trait, which is called after loading and before saving to normalize
values.

```rust
#[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
struct GameConfig {
    render_scale: f32,
    vsync: bool,
    resolution: (u32, u32),
}

impl ValidatedSetting for GameConfig {
    fn validate(&mut self) {
        self.render_scale = self.render_scale.clamp(0.5, 2.0);
        if self.resolution.0 == 0 || self.resolution.1 == 0 {
            self.resolution = (1920, 1080);
        }
    }
}
```

### 2. Plugin Configuration

Use `SettingsPluginConfig` to control where and how settings are stored.

| Field       | Description                                                                                                    | Default           |
|-------------|----------------------------------------------------------------------------------------------------------------|-------------------|
| `domain`    | Top‑level domain for OS‑specific paths (e.g., `"com"`, `"org"`).                                               | `"com"`           |
| `company`   | Company/organization name (required for `SystemConfigDir`).                                                    | `""`              |
| `project`   | Project name (required for `SystemConfigDir`).                                                                 | `""`              |
| `format`    | Serialization format: `FormatKind::Toml`, `Json`, or `Binary`.                                                 | `Toml`            |
| `file_name` | Explicit file name (without extension). If `None`, derived from the struct name.                               | `None`            |
| `storage`   | Where to store files: `SettingsStorage::SystemConfigDir` (OS‑standard) or `GameLocalDir` (next to executable). | `SystemConfigDir` |

Example with custom configuration:

```rust
let config = SettingsPluginConfig {
format: FormatKind::Json,
company: "MyCompany".into(),
project: "MyGame".into(),
file_name: Some("user_prefs".into()),
storage: SettingsStorage::GameLocalDir, // save next to the .exe
..Default::default ()
};
app.add_plugins(SettingsPlugin::<MySettings>::from_config(config));
```

### 3. Storage Locations

- **`SystemConfigDir`** – uses the OS‑standard configuration directory:
    - Windows: `%APPDATA%\Company\Project\config\`
    - macOS: `~/Library/Application Support/company/project/config/`
    - Linux: `~/.config/company/project/config/`

- **`GameLocalDir`** – uses the directory containing the executable (ideal for portable installations).

### 4. Saving Settings

Trigger saving by sending a `PersistSetting<S>` event. The event can optionally carry a new value to replace the current
settings before saving.

```rust
// Save current settings
commands.trigger(PersistSetting::<MySettings> { value: None });

// Save with a new value
commands.trigger(PersistSetting::<MySettings> {
value: Some(MySettings { volume: 0.8, fullscreen: true }),
});
```

To save **all** settings types at once (if you have multiple plugins), use `PersistAllSettings`:

```rust
commands.trigger(PersistAllSettings);
```

### 5. Reloading Settings

Reload settings from disk with a `ReloadSetting<S>` event:

```rust
commands.trigger(ReloadSetting::<MySettings> {
_phantom: std::marker::PhantomData,
});
```

 > **Note**: If the settings file does not exist when reloading, the current in‑memory settings remain unchanged.  
 > The library does **not** automatically reset to `S::default()`. To reset, send `PersistSetting` with a new value or implement your own logic.

### 6. Handling Errors

If a background save fails, a `SettingsSaveError<S>` event is emitted. You can observe it to notify the user or log the
error.

```rust
fn handle_save_errors(
    mut errors: MessageReader<SettingsSaveError<MySettings>>,
) {
    for error in errors.read() {
        error!("Failed to save settings: {}", error.error);
    }
}
```

### 7. Multiple Settings Types

You can have as many independent settings types as you need – just add a separate `SettingsPlugin` for each.

```rust
app.add_plugins(SettingsPlugin::<GameConfig>::from_config(game_config))
.add_plugins(SettingsPlugin::<UserPrefs>::from_config(user_prefs));
```

Each will be stored in its own file and can be saved/reloaded independently.

### 8. First Launch: Dynamic Defaults and File Creation

By default, the library **does not create a settings file** on disk until the first call to `PersistSetting` (or
`PersistAllSettings`).  
This lazy creation is safe and efficient: your game runs with in‑memory `S::default()` values, and the file appears only
when the player actually changes and saves something.

However, you may need to adapt default settings to the runtime environment (screen resolution, system language, hardware
capabilities).  
Here is the recommended pattern:

1. **Generate dynamic defaults** in a system that runs after the window is created (e.g., in `PostStartup` or an
   `OnEnter` state).
2. **Modify the `ResMut<S>` resource** directly – no file is written yet.
3. **Optionally force file creation** only if you really need a file to exist from the start (e.g., for external tools).
   In that case, trigger `PersistSetting` after setting your dynamic defaults, but check `path.exists()` first to avoid
   overwriting an existing configuration.

## Important Notes

- **Company and project names** must not contain invalid characters for `ProjectDirs` and **cannot be empty when
  using `SystemConfigDir`** – the library will panic.
  With `GameLocalDir` these fields are optional (may be empty).
- **No auto‑save** – the developer decides when to trigger saving.
- **Validation is mandatory** – even if you don't need validation, you must provide an empty `validate` implementation.

## Examples

#TODO

Run an example with:

```bash
cargo run --example basic
```

## API Reference

Full API documentation is available on [docs.rs](https://docs.rs/bevy-settings-lib).

## Contributing

Contributions are welcome! Please open an issue or pull request
on [GitHub](https://github.com/yourname/bevy-settings-lib).

See [CHANGELOG.md](CHANGELOG.md) for a history of changes.

## License

Licensed under the MIT license ([LICENSE-MIT](LICENSE.md) or http://opensource.org/licenses/MIT).