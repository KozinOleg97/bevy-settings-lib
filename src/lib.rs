//! A flexible settings management library for Bevy with async saving, multiple formats, and built‑in validation.
//!
//! This library provides a convenient way to save, load, and reload settings in Bevy applications.
//! It supports text formats (TOML, JSON) and binary (postcard) with atomic write‑then‑rename
//! to prevent file corruption.
//!
//! # Features
//!
//! - **Any number of configurations** – each configuration has its own data type and file name.
//! - **File names can be explicit or derived from the struct name** (automatically converted to snake_case).
//! - **Asynchronous saving** with atomic write‑then‑rename – files are never left in a corrupted state.
//! - **Persistent worker thread** – each settings type has a dedicated background thread that processes save requests sequentially, eliminating file race conditions.
//! - **Format support**: TOML (default), JSON, binary (postcard).
//! - **Load from OS‑standard directories** (via `directories` crate) **or from the game's local folder**.
//! - **Events**: `PersistSetting<S>`, `PersistAllSettings`, `ReloadSetting<S>`, `SettingsSaveError<S>`.
//! - **Partial loading** – if a file does not exist, `S::default()` is used.
//! - **Validation** – every settings type must implement `ValidatedSetting` to normalize values after loading and before saving.
//!
//! # Quick Example
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_settings_lib::{SettingsPlugin, PersistSetting, SettingsPluginConfig, FormatKind, ValidatedSetting, SettingsStorage};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
//! struct MySettings {
//!     volume: f32,
//!     fullscreen: bool,
//! }
//!
//! // Mandatory validation implementation (can be empty if no validation needed)
//! impl ValidatedSetting for MySettings {
//!     fn validate(&mut self) {
//!         self.volume = self.volume.clamp(0.0, 1.0);
//!     }
//! }
//!
//! fn main() {
//!     // Use the system configuration directory (AppData, ~/.config, etc.)
//!     let config = SettingsPluginConfig {
//!         format: FormatKind::Toml,
//!         company: "MyCompany".into(),
//!         project: "MyGame".into(),
//!         file_name: None, // auto‑name → "my_settings"
//!         storage: SettingsStorage::SystemConfigDir,
//!         ..Default::default()
//!     };
//!     App::new()
//!         .add_plugins(SettingsPlugin::<MySettings>::from_config(config))
//!         .add_systems(Update, save_after_delay)
//!         .run();
//! }
//!
//! fn save_after_delay(
//!     mut commands: Commands,
//!     time: Res<Time>,
//!     mut timer: Local<Option<Timer>>,
//! ) {
//!     // Create a one‑shot timer on first run
//!     if timer.is_none() {
//!         *timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
//!     }
//!     let timer = timer.as_mut().unwrap();
//!     timer.tick(time.delta());
//!     if timer.just_finished() {
//!         commands.trigger(PersistSetting::<MySettings> { value: None });
//!     }
//! }
//! ```
//!
//! # Important Notes
//!
//! - **Asynchronous saving**: When the application exits, the last changes may be lost if the save thread hasn't finished.
//!   For guaranteed persistence, implement synchronous saving (e.g., in an `OnExit` system).
//! - **Company and project names** must not contain invalid characters for `ProjectDirs` and **cannot be empty when using `SystemConfigDir`** – the library will panic.
//!   With `GameLocalDir` these fields are optional (may be empty).
//! - **No auto‑save and no auto‑create** – the developer decides when to trigger saving. The settings file is only created on the first explicit save.
//! - **First launch defaults**: The library does not create a file automatically. Use a system to adjust `S::default()` to runtime conditions (screen resolution, language, etc.) by modifying the resource directly. Save explicitly only when needed.
//!
//! # Plugin Configuration
//!
//! Use `SettingsPluginConfig` to choose the format, domain, company, project (for directory),
//! file name, and storage type. Defaults are TOML format, domain `"com"`, and `SystemConfigDir` storage.
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use bevy_settings_lib::{SettingsPlugin, SettingsPluginConfig, FormatKind, SettingsStorage, ValidatedSetting};
//! # use serde::{Serialize, Deserialize};
//! # #[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
//! # struct MySettings;
//! # impl ValidatedSetting for MySettings { fn validate(&mut self) {} }
//! # let mut app = App::new();
//! let config = SettingsPluginConfig {
//!     format: FormatKind::Json,
//!     company: "MyCompany".into(),
//!     project: "MyGame".into(),
//!     file_name: Some("custom_name".into()),
//!     storage: SettingsStorage::GameLocalDir, // save next to the .exe
//!     ..Default::default()
//! };
//! app.add_plugins(SettingsPlugin::<MySettings>::from_config(config));
//! ```

use std::{
    fmt::Debug,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use bevy::prelude::*;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ================================
// 1. Error Handling
// ================================

#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialize(String),
    #[error("Deserialization error: {0}")]
    Deserialize(String),
}

pub type SettingsResult<T> = Result<T, SettingsError>;

// ================================
// 2. Text Formats (TOML, JSON)
// ================================

pub trait SettingsFormat: Send + Sync + 'static {
    fn file_extension() -> &'static str;
    fn serialize<T: Serialize>(value: &T) -> SettingsResult<String>;
    fn deserialize<T: for<'de> Deserialize<'de>>(data: &str) -> SettingsResult<T>;
}

pub struct TomlFormat;
impl SettingsFormat for TomlFormat {
    fn file_extension() -> &'static str {
        "toml"
    }
    fn serialize<T: Serialize>(value: &T) -> SettingsResult<String> {
        toml::to_string(value).map_err(|e| SettingsError::Serialize(e.to_string()))
    }
    fn deserialize<T: for<'de> Deserialize<'de>>(data: &str) -> SettingsResult<T> {
        toml::from_str(data).map_err(|e| SettingsError::Deserialize(e.to_string()))
    }
}

pub struct JsonFormat;
impl SettingsFormat for JsonFormat {
    fn file_extension() -> &'static str {
        "json"
    }
    fn serialize<T: Serialize>(value: &T) -> SettingsResult<String> {
        serde_json::to_string_pretty(value).map_err(|e| SettingsError::Serialize(e.to_string()))
    }
    fn deserialize<T: for<'de> Deserialize<'de>>(data: &str) -> SettingsResult<T> {
        serde_json::from_str(data).map_err(|e| SettingsError::Deserialize(e.to_string()))
    }
}

// ================================
// 3. Binary Format (postcard)
// ================================

fn write_binary<T: Serialize>(path: &Path, value: &T) -> SettingsResult<()> {
    let bytes =
        postcard::to_allocvec(value).map_err(|e| SettingsError::Serialize(e.to_string()))?;
    std::fs::write(path, bytes).map_err(SettingsError::Io)
}

fn read_binary<T: for<'de> Deserialize<'de>>(path: &Path) -> SettingsResult<T> {
    let bytes = std::fs::read(path).map_err(SettingsError::Io)?;
    postcard::from_bytes(&bytes).map_err(|e| SettingsError::Deserialize(e.to_string()))
}

// ================================
// 4. Setting Types and Validation
// ================================

pub trait Setting:
    Resource + Clone + Serialize + Default + for<'de> Deserialize<'de> + Debug + Send + Sync
{
}
impl<T> Setting for T where
    T: Resource + Clone + Serialize + Default + for<'de> Deserialize<'de> + Debug + Send + Sync
{
}

/// Trait for validating and normalizing setting values.
/// **Mandatory to implement** for all types used with `SettingsPlugin`.
/// Called automatically:
/// - after loading from a file (or `S::default()` if the file does not exist),
/// - after `ReloadSetting`,
/// - **before saving** (`PersistSetting` and `PersistAllSettings`),
/// - **when a new value is provided** in `PersistSetting { value: Some(...) }`.
///
/// If validation is not needed, implement the method as empty.
pub trait ValidatedSetting {
    fn validate(&mut self);
}

// ================================
// 5. Plugin Configuration
// ================================

/// Storage type for settings files.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsStorage {
    /// System configuration directory:
    /// - Windows: `%APPDATA%\Company\Project\config`
    /// - macOS:   `~/Library/Application Support/company/project/config`
    /// - Linux:   `~/.config/company/project/config`
    SystemConfigDir,
    /// Local directory where the game executable resides (next to the .exe).
    GameLocalDir,
}

#[derive(Clone)]
pub struct SettingsPluginConfig {
    pub domain: String,
    pub company: String,
    pub project: String,
    pub format: FormatKind,
    pub file_name: Option<String>,
    pub storage: SettingsStorage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Toml,
    Json,
    Binary,
}

impl SettingsPluginConfig {
    /// Validates the configuration. Panics if:
    /// - for `SystemConfigDir` `company` or `project` are empty, or `ProjectDirs` cannot be created
    /// - for `GameLocalDir` no validation is performed (company/project may be empty)
    pub fn validate(&self) {
        match self.storage {
            SettingsStorage::SystemConfigDir => {
                if self.company.is_empty() {
                    panic!(
                        "SettingsPluginConfig: 'company' cannot be empty when using SystemConfigDir. \
                         Please set a valid company name (e.g., 'MyCompany')."
                    );
                }
                if self.project.is_empty() {
                    panic!(
                        "SettingsPluginConfig: 'project' cannot be empty when using SystemConfigDir. \
                         Please set a valid project name (e.g., 'MyGame')."
                    );
                }
                if ProjectDirs::from(&self.domain, &self.company, &self.project).is_none() {
                    panic!(
                        "SettingsPluginConfig: unable to determine standard config directory for domain='{}', company='{}', project='{}'. \
                         Check that the strings do not contain invalid characters (e.g., '/', '\\', ':' on Windows).",
                        self.domain, self.company, self.project
                    );
                }
            }
            SettingsStorage::GameLocalDir => {
                // For local mode, company and project are not used; no validation needed.
                // However, we can warn if they are empty (optional).
                if self.company.is_empty() || self.project.is_empty() {
                    bevy::log::warn!(
                        "SettingsPluginConfig: 'company' or 'project' is empty while using GameLocalDir. \
                         These fields are not required for local storage but may affect compatibility."
                    );
                }
            }
        }
    }
}

impl Default for SettingsPluginConfig {
    fn default() -> Self {
        Self {
            domain: "com".into(),
            company: "".into(),
            project: "".into(),
            format: FormatKind::Toml,
            file_name: None,
            storage: SettingsStorage::SystemConfigDir,
        }
    }
}

// ================================
// 6. Events (Bevy 0.18 Observers)
// ================================

#[derive(Event)]
pub struct PersistAllSettings;

#[derive(Event)]
pub struct PersistSetting<S: Setting> {
    pub value: Option<S>,
}

/// Event that triggers a reload of the settings from disk.
///
/// If the settings file does not exist or cannot be read, the current in‑memory resource is **not** changed.
/// To reset to default values, use `PersistSetting` with `Some(S::default())` or implement your own logic.
#[derive(Event)]
pub struct ReloadSetting<S: Setting> {
    pub _phantom: PhantomData<S>,
}

/// Event emitted when a settings save operation fails.
#[derive(Event)]
pub struct SettingsSaveError<S: Setting> {
    pub error: SettingsError,
    pub _phantom: PhantomData<S>,
}

// ================================
// 7. Internal Resources
// ================================

#[derive(Resource)]
struct SettingsInternal<S: Setting> {
    config: SettingsPluginConfig,
    path: PathBuf,
    temp_path: PathBuf,
    directory: PathBuf,
    error_sender: Sender<SettingsError>,
    _marker: PhantomData<S>,
}

/// Resource for receiving errors from background threads.
#[derive(Resource)]
struct SettingsErrorReceiver<S: Setting> {
    receiver: mpsc::Receiver<SettingsError>,
    _marker: PhantomData<S>,
}

/// Resource that owns the background save worker thread and its channel.
#[derive(Resource)]
struct SaveWorker<S: Setting> {
    /// Sender to queue settings to be saved. `None` is sent to signal shutdown.
    sender: Sender<Option<S>>,
    /// Handle to the background thread; joined on drop.
    handle: Option<JoinHandle<()>>,
    _marker: PhantomData<S>,
}

impl<S: Setting> Drop for SaveWorker<S> {
    fn drop(&mut self) {
        // Send termination signal if channel is still open.
        let _ = self.sender.send(None);
        if let Some(handle) = self.handle.take() {
            // Wait for the thread to finish pending saves.
            let _ = handle.join();
        }
    }
}

impl<S: Setting> SettingsInternal<S> {
    fn new(
        config: SettingsPluginConfig,
        dir: PathBuf,
        path: PathBuf,
        error_sender: Sender<SettingsError>,
    ) -> Self {
        let extension = match config.format {
            FormatKind::Toml => TomlFormat::file_extension(),
            FormatKind::Json => JsonFormat::file_extension(),
            FormatKind::Binary => "bin",
        };
        Self {
            temp_path: path.with_extension(format!("tmp.{}", extension)),
            directory: dir,
            path,
            config,
            error_sender,
            _marker: PhantomData,
        }
    }
}

// ================================
// 8. Main Plugin
// ================================

pub struct SettingsPlugin<S> {
    config: SettingsPluginConfig,
    _marker: PhantomData<S>,
}

impl<S: Setting + ValidatedSetting> SettingsPlugin<S> {
    /// Creates a plugin from a ready‑made configuration.
    /// Performs configuration validation (panics if `company` or `project` are invalid
    /// when using `SystemConfigDir`).
    pub fn from_config(config: SettingsPluginConfig) -> Self {
        config.validate();
        Self {
            config,
            _marker: PhantomData,
        }
    }

    /// Returns the file name (without extension) based on the configuration.
    fn file_stem(&self) -> String {
        if let Some(ref name) = self.config.file_name {
            name.clone()
        } else {
            // Convert the type name to "snake_case" and strip modules.
            let type_name = std::any::type_name::<S>();
            let short_name = type_name.split("::").last().unwrap_or(type_name);
            let mut snake = String::new();
            for (i, ch) in short_name.chars().enumerate() {
                if ch.is_uppercase() && i > 0 {
                    snake.push('_');
                    snake.push(ch.to_ascii_lowercase());
                } else {
                    snake.push(ch.to_ascii_lowercase());
                }
            }
            snake
        }
    }

    /// Returns the base directory depending on the chosen storage.
    fn base_dir(&self) -> PathBuf {
        match self.config.storage {
            SettingsStorage::SystemConfigDir => {
                let proj_dirs = ProjectDirs::from(
                    &self.config.domain,
                    &self.config.company,
                    &self.config.project,
                )
                .expect("Already validated in config");
                proj_dirs.config_dir().to_path_buf()
            }
            SettingsStorage::GameLocalDir => {
                let exe_path =
                    std::env::current_exe().expect("Failed to get current executable path");
                exe_path
                    .parent()
                    .expect("Executable has no parent directory")
                    .to_path_buf()
            }
        }
    }

    fn load(&self) -> SettingsResult<S> {
        let path = self.path();
        let mut settings = if !path.exists() {
            S::default()
        } else {
            match self.config.format {
                FormatKind::Binary => read_binary(&path)?,
                _ => {
                    let data = std::fs::read_to_string(&path).map_err(SettingsError::Io)?;
                    self.deserialize_text(&data)?
                }
            }
        };
        // Apply validation (if the type implements a custom method, it will be called).
        settings.validate();
        Ok(settings)
    }

    fn deserialize_text(&self, data: &str) -> SettingsResult<S> {
        match self.config.format {
            FormatKind::Toml => TomlFormat::deserialize(data),
            FormatKind::Json => JsonFormat::deserialize(data),
            FormatKind::Binary => unreachable!(),
        }
    }

    fn serialize_text(&self, value: &S) -> SettingsResult<String> {
        match self.config.format {
            FormatKind::Toml => TomlFormat::serialize(value),
            FormatKind::Json => JsonFormat::serialize(value),
            FormatKind::Binary => unreachable!(),
        }
    }

    fn path(&self) -> PathBuf {
        let extension = match self.config.format {
            FormatKind::Toml => TomlFormat::file_extension(),
            FormatKind::Json => JsonFormat::file_extension(),
            FormatKind::Binary => "bin",
        };
        let file_stem = self.file_stem();
        self.base_dir().join(format!("{}.{}", file_stem, extension))
    }

    fn directory(&self) -> PathBuf {
        self.base_dir()
    }

    // --- Helper function for saving ---
    fn save_to_file(
        temp_path: &Path,
        path: &Path,
        settings: &S,
        format_kind: FormatKind,
        error_sender: &Sender<SettingsError>,
    ) {
        let write_result = match format_kind {
            FormatKind::Binary => write_binary(temp_path, settings),
            _ => {
                let content = match format_kind {
                    FormatKind::Toml => TomlFormat::serialize(settings),
                    FormatKind::Json => JsonFormat::serialize(settings),
                    _ => unreachable!(),
                };
                match content {
                    Ok(c) => std::fs::write(temp_path, c).map_err(SettingsError::Io),
                    Err(e) => Err(e),
                }
            }
        };

        match write_result {
            Ok(_) => {
                if let Err(e) = std::fs::rename(temp_path, path) {
                    bevy::log::error!("Failed to rename settings file: {}", e);
                    let _ = error_sender.send(SettingsError::Io(e));
                    // Remove the temporary file if rename failed.
                    let _ = std::fs::remove_file(temp_path);
                } else {
                    bevy::log::debug!("Settings saved to {:?}", path);
                }
            }
            Err(e) => {
                bevy::log::error!("Failed to write temp settings file: {}", e);
                let _ = error_sender.send(e);
                // Try to delete the corrupted temporary file.
                let _ = std::fs::remove_file(temp_path);
            }
        }
    }

    // --- Observers ---

    fn persist_setting_observer(
        event: On<PersistSetting<S>>,
        mut settings: ResMut<S>,
        internal: Res<SettingsInternal<S>>,
        worker: Res<SaveWorker<S>>,
    ) {
        let ev = event.event();
        if let Some(new_value) = &ev.value {
            *settings = new_value.clone();
            settings.validate();
        }

        settings.validate();

        // Clone settings and send to worker channel.
        let settings_clone = settings.clone();
        if let Err(e) = worker.sender.send(Some(settings_clone)) {
            bevy::log::error!("Failed to queue settings for saving: {}", e);
            let _ = internal
                .error_sender
                .send(SettingsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Save worker channel closed",
                )));
        }
    }

    fn persist_all_observer(
        _event: On<PersistAllSettings>,
        mut settings: ResMut<S>,
        internal: Res<SettingsInternal<S>>,
        worker: Res<SaveWorker<S>>,
    ) {
        settings.validate();

        let settings_clone = settings.clone();
        if let Err(e) = worker.sender.send(Some(settings_clone)) {
            bevy::log::error!("Failed to queue settings for saving: {}", e);
            let _ = internal
                .error_sender
                .send(SettingsError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Save worker channel closed",
                )));
        }
    }

    fn reload_observer(
        _event: On<ReloadSetting<S>>,
        mut settings: ResMut<S>,
        internal: Res<SettingsInternal<S>>,
    ) {
        let load_result = match internal.config.format {
            FormatKind::Binary => read_binary::<S>(&internal.path),
            _ => {
                let content = match std::fs::read_to_string(&internal.path) {
                    Ok(c) => c,
                    Err(e) => {
                        bevy::log::error!("Failed to read settings file: {}", e);
                        return;
                    }
                };
                match internal.config.format {
                    FormatKind::Toml => TomlFormat::deserialize(&content),
                    FormatKind::Json => JsonFormat::deserialize(&content),
                    _ => unreachable!(),
                }
            }
        };
        match load_result {
            Ok(mut new_settings) => {
                new_settings.validate(); // validation after loading.
                *settings = new_settings;
                bevy::log::info!("Settings reloaded from {:?}", internal.path);
            }
            Err(e) => {
                bevy::log::error!("Failed to reload settings: {}", e);
            }
        }
    }

    /// System that processes errors from background threads and sends them as events.
    fn process_error_messages(
        error_receiver: NonSend<Receiver<SettingsError>>,
        mut commands: Commands,
    ) {
        while let Ok(error) = error_receiver.try_recv() {
            commands.trigger(SettingsSaveError::<S> {
                error,
                _phantom: PhantomData::<S>,
            });
        }
    }
}

impl<S: Setting + ValidatedSetting> Plugin for SettingsPlugin<S> {
    fn build(&self, app: &mut App) {
        let load_result = self.load();
        let mut initial_value = match load_result {
            Ok(v) => v,
            Err(e) => {
                bevy::log::error!(
                    "Failed to load settings for {}: {}, using default",
                    std::any::type_name::<S>(),
                    e
                );
                S::default()
            }
        };
        // Validation (in case default already needs correction).
        initial_value.validate();

        let dir = self.directory();
        let path = self.path();

        // Create channels: one for errors, one for save queue.
        let (error_sender, error_receiver) = mpsc::channel();
        let (save_sender, save_receiver): (Sender<Option<S>>, Receiver<Option<S>>) =
            mpsc::channel();

        // Attempt to create directory, send error through channel if fails
        if let Err(e) = std::fs::create_dir_all(&dir) {
            bevy::log::error!("Failed to create settings directory: {}", e);
            let _ = error_sender.send(SettingsError::Io(e));
        }

        let internal =
            SettingsInternal::<S>::new(self.config.clone(), dir, path, error_sender.clone());

        // Clone the data needed for the worker thread BEFORE moving it into the closure
        let temp_path = internal.temp_path.clone();
        let path_clone = internal.path.clone();
        let config = self.config.clone();

        let handle = thread::Builder::new()
            .name(format!("bevy-settings-save-{}", std::any::type_name::<S>()))
            .spawn(move || {
                // Worker loop: receive Option<S> from channel.
                for maybe_settings in save_receiver {
                    match maybe_settings {
                        Some(settings) => {
                            let error_sender = error_sender.clone();
                            SettingsPlugin::<S>::save_to_file(
                                &temp_path,
                                &path_clone,
                                &settings,
                                config.format,
                                &error_sender,
                            );
                        }
                        None => {
                            // Termination signal received.
                            bevy::log::debug!(
                                "Save worker for {} shutting down.",
                                std::any::type_name::<S>()
                            );
                            break;
                        }
                    }
                }
            })
            .expect("Failed to spawn save worker thread");

        let worker = SaveWorker {
            sender: save_sender,
            handle: Some(handle),
            _marker: PhantomData,
        };

        app.insert_resource(initial_value)
            .insert_resource(internal)
            .insert_non_send_resource(error_receiver)
            .insert_resource(worker)
            .add_observer(Self::persist_setting_observer)
            .add_observer(Self::persist_all_observer)
            .add_observer(Self::reload_observer)
            .add_systems(Update, Self::process_error_messages);
    }
}

// ================================
// 9. Test Utilities (tests only)
// ================================

#[cfg(test)]
mod test_utils {
    use super::*;
    use std::path::PathBuf;

    /// Returns `true` if test files should be cleaned up after tests (default).
    /// Set the environment variable `KEEP_TEST_FILES=1` to disable cleanup.
    pub fn should_cleanup() -> bool {
        std::env::var("KEEP_TEST_FILES").is_err()
    }

    /// Deletes the given files or directories if `should_cleanup()` is true.
    pub fn cleanup_paths(paths: &[PathBuf]) {
        if !should_cleanup() {
            println!("Skipping cleanup due to KEEP_TEST_FILES");
            return;
        }
        for path in paths {
            if path.exists() {
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                } else if path.is_dir() {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        }
    }

    /// Returns the path to the configuration directory for the given `SettingsPluginConfig`.
    /// For `GameLocalDir` in tests we use a temporary directory, because the real executable
    /// path is unstable in the test environment.
    pub fn config_dir(config: &SettingsPluginConfig) -> PathBuf {
        match config.storage {
            SettingsStorage::SystemConfigDir => {
                ProjectDirs::from(&config.domain, &config.company, &config.project)
                    .expect("Failed to determine config directory for test - check domain, company, and project values")
                    .config_dir()
                    .to_path_buf()
            }
            SettingsStorage::GameLocalDir => {
                // For tests we use a temporary directory to avoid dependence on .exe location.
                std::env::temp_dir()
            }
        }
    }

    pub fn settings_path<S: Setting + ValidatedSetting>(config: &SettingsPluginConfig) -> PathBuf {
        let plugin = SettingsPlugin::<S>::from_config(config.clone());
        plugin.path()
    }
}

// ================================
// 10. Tests
// ================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{cleanup_paths, config_dir, settings_path};
    use bevy::app::App;
    use serial_test::serial;
    use std::collections::HashMap;
    use std::time::Duration;

    const TEST_DOMAIN: &str = "com";
    const TEST_COMPANY: &str = "MyCompany";
    const TEST_PROJECT: &str = "mygame";

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
    pub enum GraphicsQuality {
        #[default]
        Low,
        Medium,
        High,
        Ultra,
    }

    #[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct GameConfig {
        pub render_scale: f32,
        pub max_fps: u16,
        pub shadow_map_size: u32,
        pub anisotropic_filtering: i32,
        pub vsync_enabled: bool,
        pub quality: GraphicsQuality,
        pub default_language: String,
        pub enabled_post_effects: Vec<String>,
        pub custom_resolution: Option<(u32, u32)>,
        pub texture_quality_overrides: HashMap<String, u8>,
    }

    impl Default for GameConfig {
        fn default() -> Self {
            Self {
                render_scale: 1.0,
                max_fps: 144,
                shadow_map_size: 2048,
                anisotropic_filtering: 16,
                vsync_enabled: true,
                quality: GraphicsQuality::High,
                default_language: "en-US".to_string(),
                enabled_post_effects: vec!["bloom".to_string(), "ssao".to_string()],
                custom_resolution: None,
                texture_quality_overrides: HashMap::new(),
            }
        }
    }

    impl ValidatedSetting for GameConfig {
        fn validate(&mut self) {
            self.render_scale = self.render_scale.clamp(0.5, 2.0);
            self.max_fps = self.max_fps.clamp(30, 360);
            self.shadow_map_size = self.shadow_map_size.clamp(512, 4096);
            self.anisotropic_filtering = self.anisotropic_filtering.clamp(1, 16);
            if self.default_language.is_empty() {
                self.default_language = "en-US".to_string();
            }
            self.enabled_post_effects
                .retain(|effect| matches!(effect.as_str(), "bloom" | "ssao" | "motion_blur"));
            if let Some((w, h)) = self.custom_resolution {
                if w == 0 || h == 0 {
                    self.custom_resolution = None;
                }
            }
            for (_, quality) in self.texture_quality_overrides.iter_mut() {
                *quality = (*quality).clamp(0, 100);
            }
        }
    }

    #[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct TestPreferences {
        pub volume: f32,
        pub size: f32,
    }

    // Mandatory implementation (empty if not needed).
    impl ValidatedSetting for TestPreferences {
        fn validate(&mut self) {}
    }

    #[test]
    #[serial]
    fn test_automatic_file_name() {
        let mut app = App::new();

        let base_config = SettingsPluginConfig {
            domain: TEST_DOMAIN.into(),
            company: TEST_COMPANY.into(),
            project: TEST_PROJECT.into(),
            format: FormatKind::Toml,
            file_name: None,
            storage: SettingsStorage::SystemConfigDir,
        };

        app.add_plugins(SettingsPlugin::<GameConfig>::from_config(
            base_config.clone(),
        ));
        app.add_plugins(SettingsPlugin::<TestPreferences>::from_config(
            base_config.clone(),
        ));
        app.update();

        // Check name generation.
        let plugin_game = SettingsPlugin::<GameConfig>::from_config(base_config.clone());
        let plugin_prefs = SettingsPlugin::<TestPreferences>::from_config(base_config.clone());
        assert_eq!(plugin_game.file_stem(), "game_config");
        assert_eq!(plugin_prefs.file_stem(), "test_preferences");

        // Modify values.
        {
            let mut game = app.world_mut().resource_mut::<GameConfig>();
            game.render_scale = 1.2;
            game.vsync_enabled = true;
        }
        {
            let mut prefs = app.world_mut().resource_mut::<TestPreferences>();
            prefs.volume = 0.75;
            prefs.size = 1.5;
        }

        // Save.
        app.world_mut()
            .commands()
            .trigger(PersistSetting::<GameConfig> { value: None });
        app.world_mut()
            .commands()
            .trigger(PersistSetting::<TestPreferences> { value: None });
        app.update();
        std::thread::sleep(Duration::from_millis(100));

        let dir = config_dir(&base_config);
        let game_path = dir.join("game_config.toml");
        let prefs_path = dir.join("test_preferences.toml");

        assert!(game_path.exists(), "GameConfig file not found");
        assert!(prefs_path.exists(), "TestPreferences file not found");

        // Content verification.
        let game_content = std::fs::read_to_string(&game_path).unwrap();
        let loaded_game: GameConfig = toml::from_str(&game_content).unwrap();
        assert_eq!(loaded_game.render_scale, 1.2);
        assert_eq!(loaded_game.vsync_enabled, true);

        let prefs_content = std::fs::read_to_string(&prefs_path).unwrap();
        let loaded_prefs: TestPreferences = toml::from_str(&prefs_content).unwrap();
        assert_eq!(loaded_prefs.volume, 0.75);
        assert_eq!(loaded_prefs.size, 1.5);

        // Cleanup (delete only files, leave directory untouched).
        cleanup_paths(&[game_path, prefs_path]);
    }

    #[test]
    #[serial]
    fn test_explicit_file_name() {
        let mut app = App::new();
        let explicit_name = "explicit_name";
        let config = SettingsPluginConfig {
            domain: TEST_DOMAIN.into(),
            company: TEST_COMPANY.into(),
            project: TEST_PROJECT.into(),
            format: FormatKind::Toml,
            file_name: Some(explicit_name.into()),
            storage: SettingsStorage::SystemConfigDir,
        };
        app.add_plugins(SettingsPlugin::<GameConfig>::from_config(config.clone()));
        app.update();

        let plugin = SettingsPlugin::<GameConfig>::from_config(config.clone());
        assert_eq!(plugin.file_stem(), explicit_name);

        {
            let mut game = app.world_mut().resource_mut::<GameConfig>();
            game.render_scale = 2.0;
        }
        app.world_mut()
            .commands()
            .trigger(PersistSetting::<GameConfig> { value: None });
        app.update();
        std::thread::sleep(Duration::from_millis(100));

        let path = settings_path::<GameConfig>(&config);
        assert!(path.exists(), "File does not exist at {:?}", path);

        cleanup_paths(&[path]);
    }
}

#[cfg(test)]
mod comprehensive_tests {
    use super::*;
    use crate::test_utils::{cleanup_paths, settings_path};
    use bevy::app::App;
    use serial_test::serial;
    use std::fs;
    use std::time::Duration;

    const TEST_DOMAIN: &str = "com";
    const TEST_COMPANY: &str = "MyCompany";
    const TEST_PROJECT: &str = "mygame";

    #[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct GameConfig2 {
        pub render_scale: f32,
        pub vsync: bool,
    }

    impl ValidatedSetting for GameConfig2 {
        fn validate(&mut self) {
            self.render_scale = self.render_scale.clamp(0.5, 2.0);
        }
    }

    #[derive(Resource, Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct UserPrefs2 {
        pub music_volume: f32,
        pub sfx_volume: f32,
        pub controls_inverted: bool,
    }

    impl ValidatedSetting for UserPrefs2 {
        fn validate(&mut self) {
            self.music_volume = self.music_volume.clamp(0.0, 1.0);
            self.sfx_volume = self.sfx_volume.clamp(0.0, 1.0);
        }
    }

    #[test]
    #[serial]
    fn test_config_and_prefs_together() {
        let mut app = App::new();

        let config_game = SettingsPluginConfig {
            domain: TEST_DOMAIN.into(),
            company: TEST_COMPANY.into(),
            project: TEST_PROJECT.into(),
            format: FormatKind::Toml,
            file_name: None,
            storage: SettingsStorage::SystemConfigDir,
        };

        let user_prefs = SettingsPluginConfig {
            domain: TEST_DOMAIN.into(),
            company: TEST_COMPANY.into(),
            project: TEST_PROJECT.into(),
            format: FormatKind::Json,
            file_name: None,
            storage: SettingsStorage::GameLocalDir,
        };

        app.add_plugins(SettingsPlugin::<GameConfig2>::from_config(
            config_game.clone(),
        ));
        app.add_plugins(SettingsPlugin::<UserPrefs2>::from_config(
            user_prefs.clone(),
        ));

        // Modify values.
        {
            let mut config = app.world_mut().resource_mut::<GameConfig2>();
            config.render_scale = 1.5;
            config.vsync = true;
        }
        {
            let mut prefs = app.world_mut().resource_mut::<UserPrefs2>();
            prefs.music_volume = 0.8;
            prefs.sfx_volume = 0.9;
            prefs.controls_inverted = true;
        }

        // Save.
        app.world_mut()
            .commands()
            .trigger(PersistSetting::<GameConfig2> { value: None });
        app.world_mut()
            .commands()
            .trigger(PersistSetting::<UserPrefs2> { value: None });
        for _ in 0..10 {
            app.update();
            std::thread::sleep(Duration::from_millis(50));
        }

        let game_path = settings_path::<GameConfig2>(&config_game);
        let prefs_path = settings_path::<UserPrefs2>(&user_prefs);

        assert!(game_path.exists(), "Config file not found");
        assert!(prefs_path.exists(), "Prefs file not found");

        // Content verification.
        let game_content = fs::read_to_string(&game_path).unwrap();
        let loaded_config: GameConfig2 = toml::from_str(&game_content).unwrap();
        assert_eq!(loaded_config.render_scale, 1.5);
        assert_eq!(loaded_config.vsync, true);

        let prefs_content = fs::read_to_string(&prefs_path).unwrap();
        let loaded_prefs: UserPrefs2 = serde_json::from_str(&prefs_content).unwrap();
        assert_eq!(loaded_prefs.music_volume, 0.8);
        assert_eq!(loaded_prefs.sfx_volume, 0.9);
        assert_eq!(loaded_prefs.controls_inverted, true);

        // Simulate external change with invalid values.
        let new_config_content = r#"
            render_scale = 10.0
            vsync = false
        "#;
        fs::write(&game_path, new_config_content).unwrap();

        let new_prefs_content =
            r#"{ "music_volume": 2.0, "sfx_volume": 1.5, "controls_inverted": false }"#;
        fs::write(&prefs_path, new_prefs_content).unwrap();

        // Reload.
        app.world_mut()
            .commands()
            .trigger(ReloadSetting::<GameConfig2> {
                _phantom: PhantomData,
            });
        app.world_mut()
            .commands()
            .trigger(ReloadSetting::<UserPrefs2> {
                _phantom: PhantomData,
            });
        for _ in 0..5 {
            app.update();
            std::thread::sleep(Duration::from_millis(50));
        }

        let config = app.world().resource::<GameConfig2>();
        assert_eq!(config.render_scale, 2.0); // clamped
        assert_eq!(config.vsync, false);

        let prefs = app.world().resource::<UserPrefs2>();
        assert_eq!(prefs.music_volume, 1.0); // clamped
        assert_eq!(prefs.sfx_volume, 1.0); // clamped
        assert_eq!(prefs.controls_inverted, false);

        // Cleanup.
        cleanup_paths(&[game_path, prefs_path]);
    }
}
