//! Small, validated desktop preferences. Recovery never owns terminal processes.
use crate::profiles::{DARK, LIGHT, Launch, Profile, SHELL, Theme};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

const MAX_BYTES: u64 = 65_536;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chrome {
    #[default]
    System,
    Dark,
    Light,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cursor {
    #[default]
    Outline,
    Bar,
    Underline,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub chrome: Chrome,
    pub font_family: String,
    pub font_size: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub line_height: f32,
    pub cursor: Cursor,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            chrome: Chrome::System,
            font_family: "System monospace".into(),
            font_size: 14.0,
            padding_x: 6.0,
            padding_y: 6.0,
            line_height: 1.2,
            cursor: Cursor::Outline,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    pub default_profile: String,
    pub workspace_directory: String,
    pub profiles: Vec<Profile>,
    pub themes: Vec<Theme>,
    pub restore_layout: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 2,
            appearance: Appearance::default(),
            default_profile: SHELL.into(),
            workspace_directory: String::new(),
            profiles: Vec::new(),
            themes: Vec::new(),
            restore_layout: true,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 2,
            "Unsupported settings version {} (expected 2)",
            self.version
        );
        let a = &self.appearance;
        ensure!(
            !a.font_family.trim().is_empty()
                && a.font_family.len() <= 200
                && !a.font_family.chars().any(char::is_control),
            "Font family must be a nonempty name of at most 200 bytes"
        );
        for (name, value, min, max) in [
            ("Font size", a.font_size, 8.0, 40.0),
            ("Horizontal padding", a.padding_x, 0.0, 32.0),
            ("Vertical padding", a.padding_y, 0.0, 32.0),
            ("Line height", a.line_height, 1.0, 2.0),
        ] {
            ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "{name} must be between {min} and {max}"
            );
        }
        crate::profiles::label(&self.default_profile, "Default profile ID")?;
        crate::profiles::directory_syntax(&self.workspace_directory)?;
        ensure!(
            self.profiles.len() <= 64 && self.themes.len() <= 64,
            "At most 64 custom profiles and themes"
        );
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::from(["shell".to_owned()]);
        for profile in &self.profiles {
            profile.validate()?;
            ensure!(
                profile.id != SHELL && ids.insert(&profile.id),
                "Duplicate or reserved profile ID"
            );
            ensure!(
                names.insert(profile.name.to_lowercase()),
                "Profile names must be unique"
            );
        }
        ids.clear();
        names.clear();
        for theme in &self.themes {
            theme.validate()?;
            ensure!(
                theme.id != DARK && theme.id != LIGHT && ids.insert(&theme.id),
                "Duplicate or reserved theme ID"
            );
            ensure!(
                !["dark", "light"].contains(&theme.name.to_lowercase().as_str())
                    && names.insert(theme.name.to_lowercase()),
                "Theme names must be unique (Dark and Light are reserved)"
            );
        }
        Ok(())
    }
    pub fn profile(&self, id: &str) -> Option<Profile> {
        if id == SHELL {
            Some(Profile::default())
        } else {
            self.profiles.iter().find(|p| p.id == id).cloned()
        }
    }
    pub fn default_profile(&self) -> Profile {
        self.profile(&self.default_profile).unwrap_or_default()
    }
    pub fn all_profiles(&self) -> Vec<Profile> {
        std::iter::once(Profile::default())
            .chain(self.profiles.clone())
            .collect()
    }
    pub fn theme(&self, id: &str) -> Theme {
        match id {
            DARK => Theme::dark(),
            LIGHT => Theme::light(),
            _ => self
                .themes
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .unwrap_or_else(Theme::dark),
        }
    }
    pub fn all_themes(&self) -> Vec<Theme> {
        [Theme::dark(), Theme::light()]
            .into_iter()
            .chain(self.themes.clone())
            .collect()
    }
    pub fn launch(&self, id: &str) -> Result<Launch> {
        let profile = self
            .profile(id)
            .context("Profile was removed; select another profile")?;
        Ok(Launch {
            theme: self.theme(&profile.theme),
            profile,
            workspace_directory: self.workspace_directory.clone(),
        })
    }
    pub fn reference_notice(&self) -> Option<String> {
        let mut notices = Vec::new();
        if self.profile(&self.default_profile).is_none() {
            notices.push(
                "The default profile was removed; new sessions use the built-in Shell.".to_owned(),
            );
        }
        for profile in &self.profiles {
            if profile.theme != DARK
                && profile.theme != LIGHT
                && !self.themes.iter().any(|t| t.id == profile.theme)
            {
                notices.push(format!(
                    "Theme for {} was removed; using Dark.",
                    profile.name
                ));
            }
        }
        (!notices.is_empty()).then(|| notices.join(" "))
    }
    pub fn recovery_encode(&self) -> Result<Vec<u8>> {
        let mut copy = self.clone();
        for profile in &mut copy.profiles {
            for env in &mut profile.environment {
                if env.secret {
                    env.value.clear();
                }
            }
        }
        copy.encode()
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut settings: Self = toml::from_str(std::str::from_utf8(bytes)?)?;
        if settings.version == 1 {
            settings.version = 2;
        }
        settings.validate()?;
        Ok(settings)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = format!(
            "# Opterm desktop settings. Appearance changes do not restart sessions.\n{}",
            toml::to_string_pretty(self)?
        )
        .into_bytes();
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Settings exceed 64 KiB; reduce profiles, arguments, or environment values"
        );
        Ok(bytes)
    }
}
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub backup: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "opterm")
            .context("Cannot locate Opterm preferences")?;
        let config = std::env::var_os("OPTERM_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.config_dir().join("settings"));
        let recovery = std::env::var_os("OPTERM_RECOVERY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.data_local_dir().join("recovery"));
        let paths = Self {
            config: config.join("settings.toml"),
            backup: recovery.join("last-valid-settings.toml"),
        };
        ensure!(
            paths.config != paths.backup,
            "Settings and backup must be separate files"
        );
        Ok(paths)
    }
}
/// Exact bounded file contents, not timestamps: quick edits are detected reliably.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revision {
    Missing,
    Bytes(Vec<u8>),
    Unreadable(String),
}
fn revision(path: &Path) -> Revision {
    match read_file(path) {
        Ok(bytes) => Revision::Bytes(bytes),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Revision::Missing
        }
        Err(error) => Revision::Unreadable(format!("{error:#}")),
    }
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(meta.is_file(), "Not a regular file: {}", path.display());
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Settings exceed 64 KiB: {}",
        path.display()
    );
    Ok(bytes)
}
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        let meta = fs::symlink_metadata(path)?;
        // SAFETY: geteuid takes no pointers.
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "Use a private user-owned directory (mode 0700): {}",
            path.display()
        );
    }
    #[cfg(windows)]
    {
        use interprocess::os::windows::security_descriptor::{
            AsSecurityDescriptor, SecurityDescriptor,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        };
        fs::create_dir_all(path)?;
        ensure!(
            fs::symlink_metadata(path)?.is_dir(),
            "Not a directory: {}",
            path.display()
        );
        let name = widestring::U16CString::from_os_str(path.as_os_str())?;
        let sddl = widestring::U16CString::from_str("D:P(A;OICI;FA;;;OW)")?;
        let sd = SecurityDescriptor::deserialize(&sddl)?;
        // SAFETY: the path and security descriptor remain valid for the call.
        let ok = unsafe {
            SetFileSecurityW(
                name.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd.as_sd().cast_mut(),
            )
        };
        ensure!(
            ok != 0,
            "Restrict preferences directory: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}
pub(crate) fn prepare(path: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let parent = path.parent().context("Settings path has no parent")?;
    private_dir(parent)?;
    if path.exists() {
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "Refusing to replace a non-file: {}",
            path.display()
        );
    }
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(temp)
}
pub(crate) fn commit(temp: NamedTempFile, path: &Path) -> Result<()> {
    temp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("Replace {}", path.display()))?;
    #[cfg(unix)]
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
fn save_lock(paths: &Paths) -> Result<File> {
    private_dir(paths.backup.parent().unwrap())?;
    let lock_path = paths.backup.parent().unwrap().join("settings.lock");
    if lock_path.exists() {
        ensure!(
            fs::symlink_metadata(&lock_path)?.is_file(),
            "Invalid settings lock file"
        );
    }
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        lock.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    lock.try_lock()
        .context("Another Opterm window is saving settings; retry")?;
    Ok(lock)
}
fn read_recovery(paths: &Paths) -> Result<Settings> {
    private_dir(paths.backup.parent().unwrap())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(&paths.backup)?;
        // SAFETY: geteuid takes no pointers.
        ensure!(
            meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "Recovery file must be private and user-owned: {}",
            paths.backup.display()
        );
    }
    Settings::parse(&read_file(&paths.backup)?)
}
fn backup(paths: &Paths, bytes: &[u8]) -> Result<()> {
    let recovery = Settings::parse(bytes)?.recovery_encode()?;
    let _lock = save_lock(paths)?;
    ensure!(
        revision(&paths.config) == Revision::Bytes(bytes.to_vec()),
        "Settings changed while updating recovery; retry"
    );
    // Avoid rewriting an unchanged recovery copy on each desktop launch.
    if matches!(revision(&paths.backup), Revision::Bytes(ref existing) if existing == &recovery && read_recovery(paths).is_ok())
    {
        return Ok(());
    }
    commit(prepare(&paths.backup, &recovery)?, &paths.backup)
}

pub struct Store {
    pub paths: Paths,
    pub current: Settings,
    pub notice: Option<String>,
    pub observed: Revision,
}
impl Store {
    pub fn load(paths: Paths) -> Self {
        let observed = revision(&paths.config);
        let mut store = Self {
            paths,
            current: Settings::default(),
            notice: None,
            observed,
        };
        if let Revision::Bytes(bytes) = &store.observed {
            match Settings::parse(bytes) {
                Ok(settings) => {
                    store.current = settings;
                    store.notice = store.current.reference_notice();
                    if let Err(error) = backup(&store.paths, bytes) {
                        store.notice = Some(format!(
                            "Settings loaded, but recovery backup could not be updated at {}: {error:#}",
                            store.paths.backup.display()
                        ));
                    }
                    return store;
                }
                Err(error) => {
                    store.notice = Some(format!(
                        "Invalid settings at {}: {error:#}.",
                        store.paths.config.display()
                    ))
                }
            }
        } else {
            store.notice = Some(format!(
                "Settings missing or unreadable at {}.",
                store.paths.config.display()
            ));
        }
        let recovery = read_recovery(&store.paths);
        let message = match recovery {
            Ok(settings) => {
                store.current = settings;
                "Loaded the last-valid recovery copy. The settings file was left untouched; Save to restore it.".to_owned()
            }
            Err(error) => format!(
                "Using built-in defaults. No valid recovery copy is available at {}: {error:#}. Save to create settings and a backup.",
                store.paths.backup.display()
            ),
        };
        store.notice = Some(format!(
            "{} {message}",
            store.notice.take().unwrap_or_default()
        ));
        store
    }
    pub fn disk_revision(&self) -> Revision {
        revision(&self.paths.config)
    }
    pub fn recovery(&self) -> Result<Settings> {
        read_recovery(&self.paths).with_context(|| {
            format!(
                "Read last-valid settings at {}",
                self.paths.backup.display()
            )
        })
    }
    /// Apply valid external edits; keep current preferences on every failure.
    pub fn poll(&mut self) -> bool {
        let next = self.disk_revision();
        if next == self.observed {
            return false;
        }
        self.observed = next;
        let result = match &self.observed {
            Revision::Bytes(bytes) => Settings::parse(bytes),
            _ => Err(anyhow::anyhow!("File is missing or unreadable")),
        };
        match result {
            Ok(settings) => {
                self.current = settings;
                self.notice = if let Revision::Bytes(bytes) = &self.observed {
                    backup(&self.paths, bytes).err().map(|e| {
                        format!(
                            "Settings applied; recovery backup failed at {}: {e:#}",
                            self.paths.backup.display()
                        )
                    })
                } else {
                    None
                };
                if self.notice.is_none() {
                    self.notice = self.current.reference_notice();
                }
                true
            }
            Err(error) => {
                self.notice = Some(format!(
                    "Kept current settings. Invalid file at {}: {error:#}",
                    self.paths.config.display()
                ));
                false
            }
        }
    }
    /// Each replacement is atomic. A crash between files leaves two valid generations.
    /// Prepare both before replacing either; a failed config write cannot advance backup.
    pub fn save(&mut self, draft: &Settings, baseline: &Revision, replace: bool) -> Result<()> {
        let bytes = draft.encode()?;
        let _lock = save_lock(&self.paths)?;
        ensure!(
            replace || &self.disk_revision() == baseline,
            "Settings changed on disk. Reload the file or choose Replace file with my draft."
        );
        let config = prepare(&self.paths.config, &bytes).context("Prepare settings file")?;
        let recovery = prepare(&self.paths.backup, &draft.recovery_encode()?)
            .context("Prepare recovery backup; settings were not saved")?;
        ensure!(
            replace || &self.disk_revision() == baseline,
            "Settings changed on disk. Reload or explicitly replace."
        );
        commit(config, &self.paths.config).context("Save settings")?;
        self.current = draft.clone();
        self.observed = Revision::Bytes(bytes);
        if let Err(error) = commit(recovery, &self.paths.backup) {
            let message = format!(
                "Settings saved, but recovery backup failed at {}: {error:#}. The previous recovery copy was retained if replacement failed; retry Save.",
                self.paths.backup.display()
            );
            self.notice = Some(message.clone());
            bail!(message);
        }
        self.notice = self.current.reference_notice();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths {
            config: temp.path().join("settings/settings.toml"),
            backup: temp.path().join("recovery/last-valid-settings.toml"),
        };
        (temp, Store::load(paths))
    }
    fn save(s: &mut Store, size: f32) {
        let mut draft = s.current.clone();
        draft.appearance.font_size = size;
        s.save(&draft, &s.observed.clone(), false).unwrap();
    }
    #[test]
    fn roundtrip_and_corrupt_startup_recovery_leave_file_untouched() {
        let (_temp, mut s) = store();
        save(&mut s, 17.0);
        assert_eq!(s.recovery().unwrap(), s.current);
        fs::write(&s.paths.config, "invalid = [").unwrap();
        let recovered = Store::load(s.paths.clone());
        assert_eq!(recovered.current.appearance.font_size, 17.0);
        assert!(recovered.notice.unwrap().contains("left untouched"));
        assert_eq!(fs::read_to_string(&s.paths.config).unwrap(), "invalid = [");
        fs::remove_file(&s.paths.config).unwrap();
        assert_eq!(Store::load(s.paths).current.appearance.font_size, 17.0);
    }
    #[test]
    fn invalid_edits_and_drafts_never_overwrite_recovery() {
        let (_temp, mut s) = store();
        save(&mut s, 16.0);
        let good = fs::read(&s.paths.backup).unwrap();
        for invalid in [
            "version = 999",
            "[appearance]\nfont_size = nan",
            "[appearance]\nfont_size = 0",
            "unexpected = true",
        ] {
            fs::write(&s.paths.config, invalid).unwrap();
            assert!(!s.poll());
            assert_eq!(s.current.appearance.font_size, 16.0);
            assert_eq!(fs::read(&s.paths.backup).unwrap(), good);
        }
        let mut draft = s.current.clone();
        draft.appearance.line_height = f32::INFINITY;
        assert!(s.save(&draft, &s.observed.clone(), false).is_err());
        assert_eq!(fs::read(&s.paths.backup).unwrap(), good);
    }
    #[test]
    fn conflict_requires_explicit_replace_and_external_edits_apply() {
        let (_temp, mut s) = store();
        save(&mut s, 14.0);
        let baseline = s.observed.clone();
        let draft = s.current.clone();
        let mut external = draft.clone();
        external.appearance.font_size = 20.0;
        fs::write(&s.paths.config, external.encode().unwrap()).unwrap();
        assert!(s.poll());
        assert_eq!(s.current, external);
        assert!(s.save(&draft, &baseline, false).is_err());
        assert_eq!(s.recovery().unwrap(), external);
        s.save(&draft, &baseline, true).unwrap();
        assert_eq!(s.current, draft);
    }
    #[test]
    fn failed_save_preserves_previous_backup_and_draft() {
        let (_temp, mut s) = store();
        save(&mut s, 14.0);
        let good = fs::read(&s.paths.backup).unwrap();
        fs::remove_file(&s.paths.config).unwrap();
        fs::create_dir(&s.paths.config).unwrap();
        let baseline = s.disk_revision();
        let mut draft = s.current.clone();
        draft.appearance.font_size = 18.0;
        assert!(s.save(&draft, &baseline, false).is_err());
        assert_eq!(fs::read(&s.paths.backup).unwrap(), good);
        assert_eq!(s.current.appearance.font_size, 14.0);
        assert_eq!(draft.appearance.font_size, 18.0);
    }
    #[test]
    fn backup_prepare_failure_does_not_change_active_file() {
        let (_temp, mut s) = store();
        save(&mut s, 14.0);
        let active = fs::read(&s.paths.config).unwrap();
        // A broken destination must fail before the active file is replaced.
        s.paths.backup = s.paths.backup.parent().unwrap().join("blocked");
        fs::create_dir(&s.paths.backup).unwrap();
        let mut draft = s.current.clone();
        draft.appearance.font_size = 18.0;
        assert!(s.save(&draft, &s.observed.clone(), false).is_err());
        assert_eq!(fs::read(&s.paths.config).unwrap(), active);
    }
    #[test]
    fn corrupt_backup_falls_back_and_atomic_replacement_failure_keeps_good_copy() {
        let (_temp, mut s) = store();
        save(&mut s, 17.0);
        let good = fs::read(&s.paths.backup).unwrap();
        let prepared = prepare(&s.paths.backup, &Settings::default().encode().unwrap()).unwrap();
        // Interrupt before replacement: dropping the staged file preserves the old snapshot.
        drop(prepared);
        assert_eq!(fs::read(&s.paths.backup).unwrap(), good);
        fs::write(&s.paths.backup, "broken = [").unwrap();
        fs::remove_file(&s.paths.config).unwrap();
        let loaded = Store::load(s.paths);
        assert_eq!(loaded.current, Settings::default());
        assert!(loaded.notice.unwrap().contains("No valid recovery copy"));
    }
    #[test]
    fn failed_atomic_replacement_retains_previous_valid_copy() {
        let (_temp, mut s) = store();
        save(&mut s, 17.0);
        let good = fs::read(&s.paths.backup).unwrap();
        let temp = prepare(&s.paths.backup, &Settings::default().encode().unwrap()).unwrap();
        fs::remove_file(temp.path()).unwrap(); // Simulate losing the staged file before rename.
        assert!(commit(temp, &s.paths.backup).is_err());
        assert_eq!(fs::read(&s.paths.backup).unwrap(), good);
        assert_eq!(s.recovery().unwrap().appearance.font_size, 17.0);
    }
    #[cfg(unix)]
    #[test]
    fn private_recovery_permissions() {
        use std::os::unix::fs::MetadataExt;
        let (_temp, mut s) = store();
        save(&mut s, 14.0);
        assert_eq!(fs::metadata(&s.paths.backup).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(s.paths.backup.parent().unwrap())
                .unwrap()
                .mode()
                & 0o777,
            0o700
        );
    }
}
