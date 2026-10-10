//! Portable launch presets and data-only terminal themes.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SHELL: &str = "shell";
pub const DARK: &str = "dark";
pub const LIGHT: &str = "light";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub [u8; 3]);
impl TryFrom<String> for Rgb {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit),
            "Color must be #RRGGBB"
        );
        Ok(Self([
            u8::from_str_radix(&value[1..3], 16)?,
            u8::from_str_radix(&value[3..5], 16)?,
            u8::from_str_radix(&value[5..7], 16)?,
        ]))
    }
}
impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        let [r, g, b] = value.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub ansi: [Rgb; 16],
}
impl Theme {
    pub fn dark() -> Self {
        Self {
            id: DARK.into(),
            name: "Dark".into(),
            foreground: Rgb(crate::terminal::FOREGROUND),
            background: Rgb(crate::terminal::BACKGROUND),
            cursor: Rgb([235, 235, 235]),
            selection: Rgb([65, 83, 112]),
            ansi: crate::terminal::ANSI.map(Rgb),
        }
    }
    pub fn light() -> Self {
        Self {
            id: LIGHT.into(),
            name: "Light".into(),
            foreground: Rgb([35, 35, 38]),
            background: Rgb([248, 248, 249]),
            cursor: Rgb([40, 40, 43]),
            selection: Rgb([190, 211, 242]),
            ansi: [
                [30, 30, 32],
                [167, 45, 45],
                [42, 115, 55],
                [135, 99, 18],
                [47, 91, 165],
                [123, 65, 154],
                [26, 109, 125],
                [160, 160, 164],
                [91, 91, 98],
                [193, 53, 53],
                [52, 137, 65],
                [156, 117, 25],
                [60, 113, 192],
                [144, 81, 177],
                [33, 130, 147],
                [224, 224, 228],
            ]
            .map(Rgb),
        }
    }
    pub fn validate(&self) -> Result<()> {
        label(&self.id, "Theme ID")?;
        label(&self.name, "Theme name")
    }
    pub fn palette(&self, index: usize) -> [u8; 3] {
        match index {
            0..16 => self.ansi[index].0,
            16..256 => crate::terminal::palette(index),
            256 => self.foreground.0,
            257 => self.background.0,
            258 => self.cursor.0,
            259..267 => self.ansi[index - 259]
                .0
                .map(|v| (f32::from(v) * 0.66) as u8),
            267 => self.ansi[15].0,
            268 => self.foreground.0.map(|v| (f32::from(v) * 0.66) as u8),
            _ => self.foreground.0,
        }
    }
    pub fn import(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65_536, "Theme file exceeds 64 KiB");
        let theme: Self = toml::from_str(std::str::from_utf8(&bytes)?)?;
        theme.validate()?;
        Ok(theme)
    }
    pub fn export(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let mut temp = tempfile::NamedTempFile::new_in(
            path.parent().context("Choose a full theme file path")?,
        )?;
        temp.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path)
            .map_err(|e| e.error)
            .context("Export theme (choose a new filename; existing files are preserved)")?;
        Ok(())
    }
}
pub fn label(value: &str, field: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control),
        "{field} must be a nonempty name of at most 128 bytes"
    );
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Environment {
    pub name: String,
    pub value: String,
    pub secret: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub directory: String,
    pub environment: Vec<Environment>,
    pub theme: String,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: SHELL.into(),
            name: "Shell".into(),
            executable: String::new(),
            arguments: Vec::new(),
            directory: String::new(),
            environment: Vec::new(),
            theme: DARK.into(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        label(&self.id, "Profile ID")?;
        label(&self.name, "Profile name")?;
        label(&self.theme, "Theme ID")?;
        ensure!(
            !self.executable.contains('\0') && self.executable.len() <= 4096,
            "Invalid executable in profile {}",
            self.name
        );
        ensure!(
            !self.executable.is_empty() || self.arguments.is_empty(),
            "Specify an executable before adding arguments in {}",
            self.name
        );
        ensure!(
            self.arguments.len() <= 64
                && self
                    .arguments
                    .iter()
                    .all(|s| s.len() <= 4096 && !s.contains('\0')),
            "Invalid arguments in profile {}",
            self.name
        );
        directory_syntax(&self.directory)?;
        ensure!(
            self.environment.len() <= 64,
            "At most 64 environment overrides per profile"
        );
        let mut names = std::collections::BTreeSet::new();
        for env in &self.environment {
            let mut chars = env.name.chars();
            ensure!(
                env.name.len() <= 128
                    && chars
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "Invalid environment variable name in {}",
                self.name
            );
            ensure!(
                names.insert(env.name.to_ascii_lowercase()),
                "Duplicate environment variable {} in {}",
                env.name,
                self.name
            );
            ensure!(
                env.value.len() <= 8192 && !env.value.contains('\0'),
                "Invalid value for {} in {}",
                env.name,
                self.name
            );
        }
        Ok(())
    }
    pub fn tag(&self) -> ProfileTag {
        ProfileTag {
            id: self.id.clone(),
            name: self.name.clone(),
            theme: self.theme.clone(),
        }
    }
}
pub fn directory_syntax(value: &str) -> Result<()> {
    ensure!(
        value.len() <= 4096 && !value.contains('\0'),
        "Invalid starting directory"
    );
    ensure!(
        value.is_empty()
            || value == "~"
            || value.starts_with("~/")
            || value.starts_with("~\\")
            || Path::new(value).is_absolute(),
        "Starting directory must be absolute or start with ~"
    );
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileTag {
    pub id: String,
    pub name: String,
    pub theme: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Launch {
    pub profile: Profile,
    pub workspace_directory: String,
    pub theme: Theme,
}
impl Launch {
    pub fn legacy(program: &[String]) -> Self {
        let mut profile = Profile::default();
        if let Some(executable) = program.first() {
            profile.executable = executable.clone();
            profile.arguments = program[1..].to_vec();
        }
        Self {
            profile,
            workspace_directory: String::new(),
            theme: Theme::dark(),
        }
    }
    pub fn program(&self) -> Vec<String> {
        if self.profile.executable.is_empty() {
            Vec::new()
        } else {
            std::iter::once(self.profile.executable.clone())
                .chain(self.profile.arguments.clone())
                .collect()
        }
    }
    pub fn legacy_compatible(&self) -> bool {
        self.profile.id == SHELL
            && self.profile.directory.is_empty()
            && self.workspace_directory.is_empty()
            && self.profile.environment.is_empty()
            && self.profile.theme == DARK
    }
    pub fn directory(&self) -> Result<PathBuf> {
        self.profile.validate()?;
        self.theme.validate()?;
        directory_syntax(&self.workspace_directory)?;
        let home = directories::BaseDirs::new()
            .context("Cannot find the home directory")?
            .home_dir()
            .to_owned();
        let selected = if !self.profile.directory.is_empty() {
            &self.profile.directory
        } else {
            &self.workspace_directory
        };
        let path = if selected.is_empty() || selected == "~" {
            home
        } else if selected.starts_with("~/") || selected.starts_with("~\\") {
            home.join(&selected[2..])
        } else {
            PathBuf::from(selected)
        };
        ensure!(
            path.is_dir(),
            "Starting directory for {} is unavailable: {}. Edit the profile or workspace folder.",
            self.profile.name,
            path.display()
        );
        for env in &self.profile.environment {
            ensure!(
                !env.secret || !env.value.is_empty(),
                "Reenter the secret value for {} in profile {} before launching",
                env.name,
                self.profile.name
            );
        }
        Ok(path)
    }
}

pub fn fresh_id(prefix: &str) -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("Generate preset ID: {e}"))?;
    Ok(format!(
        "{prefix}-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
