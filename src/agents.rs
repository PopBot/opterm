//! CLI identity, launch presets, and local process recognition. No provider APIs.
use crate::profiles::{Profile, Rgb, Theme};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
    Copilot,
    Antigravity,
}
impl Provider {
    pub const ALL: [Self; 4] = [Self::Antigravity, Self::Codex, Self::Claude, Self::Copilot];
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Antigravity => "antigravity",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
            Self::Antigravity => "Antigravity",
        }
    }
    pub fn executable(self) -> &'static str {
        match self {
            Self::Antigravity => "agy",
            _ => self.id(),
        }
    }
    pub fn accent(self) -> [u8; 3] {
        match self {
            Self::Claude => [217, 119, 87],
            Self::Codex => [111, 210, 178],
            Self::Copilot => [174, 145, 245],
            Self::Antigravity => [115, 165, 255],
        }
    }
    pub fn profile(self) -> Profile {
        Profile {
            id: format!("agent-{}", self.id()),
            name: self.name().into(),
            executable: self.executable().into(),
            theme: format!("agent-{}", self.id()),
            provider: Some(self),
            ..Profile::default()
        }
    }
    pub fn theme(self) -> Theme {
        let accent = self.accent();
        let mut theme = Theme {
            id: format!("agent-{}", self.id()),
            name: format!("{} Dark", self.name()),
            cursor: Rgb(accent),
            selection: Rgb(accent.map(|v| (u16::from(v) / 3 + 15) as u8)),
            ..Theme::dark()
        };
        theme.ansi[6] = Rgb(accent);
        theme.ansi[14] = Rgb(accent.map(|v| v.saturating_add(20)));
        theme
    }
    pub fn from_executable(value: &str) -> Option<Self> {
        let name = value.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        let name = name.strip_suffix(".cmd").unwrap_or(name);
        match name {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            "agy" | "antigravity" => Some(Self::Antigravity),
            _ => None,
        }
    }
    fn from_command(executable: &str, command: &[String]) -> Option<Self> {
        Self::from_executable(executable)
            .or_else(|| command.first().and_then(|s| Self::from_executable(s)))
            .or_else(|| {
                // Node/npm and script launchers: inspect the script operand only,
                // never arbitrary prompts, arguments, or shell command strings.
                let runner = Path::new(executable).file_stem()?.to_str()?;
                if !matches!(
                    runner,
                    "node"
                        | "nodejs"
                        | "bun"
                        | "deno"
                        | "bash"
                        | "sh"
                        | "dash"
                        | "ash"
                        | "zsh"
                        | "ksh"
                        | "python"
                        | "python3"
                ) {
                    return None;
                }
                let script = command.get(1)?;
                if script.starts_with('-') {
                    return None;
                }
                Self::from_executable(script).or_else(|| {
                    let normalized = script.replace('\\', "/");
                    for (package, provider) in [
                        ("/@anthropic-ai/claude-code/", Self::Claude),
                        ("/@openai/codex/", Self::Codex),
                        ("/@github/copilot/", Self::Copilot),
                        ("/antigravity-cli/", Self::Antigravity),
                    ] {
                        if normalized.contains(package) && normalized.ends_with(".js") {
                            return Some(provider);
                        }
                    }
                    None
                })
            })
    }
}

#[derive(Clone, Copy)]
pub struct ProcessTarget {
    pub root: u32,
    pub foreground: Option<u32>,
}
struct Process {
    parent: Option<u32>,
    provider: Option<Provider>,
}
fn ancestry(processes: &HashMap<u32, Process>, pid: u32, root: u32) -> Option<Vec<u32>> {
    let mut chain = Vec::new();
    let mut current = pid;
    for _ in 0..128 {
        chain.push(current);
        if current == root {
            return Some(chain);
        }
        current = processes.get(&current)?.parent?;
    }
    None
}
fn recognize(
    processes: &HashMap<u32, Process>,
    target: ProcessTarget,
    in_foreground: impl Fn(u32, u32) -> bool,
) -> Option<Provider> {
    if let Some(foreground) = target.foreground {
        // Retain the agent identity when it starts a foreground tool subprocess.
        let chain = ancestry(processes, foreground, target.root)?;
        if let Some(provider) = chain.iter().find_map(|pid| processes.get(pid)?.provider) {
            return Some(provider);
        }
    }
    let mut matches = processes.iter().filter_map(|(&pid, process)| {
        let provider = process.provider?;
        ancestry(processes, pid, target.root)?;
        if target
            .foreground
            .is_some_and(|group| !in_foreground(pid, group))
        {
            return None;
        }
        Some(provider)
    });
    let provider = matches.next()?;
    // Without foreground information, don't guess between different agents.
    matches.all(|other| other == provider).then_some(provider)
}

#[derive(Default)]
pub struct Detector {
    system: System,
}
impl Detector {
    pub fn scan(&mut self, targets: &[ProcessTarget]) -> Vec<Option<Provider>> {
        if targets.is_empty() {
            return Vec::new();
        }
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let mut processes: HashMap<_, _> = self
            .system
            .processes()
            .iter()
            .map(|(pid, p)| {
                (
                    pid.as_u32(),
                    Process {
                        parent: p.parent().map(Pid::as_u32),
                        provider: None,
                    },
                )
            })
            .collect();
        let related: Vec<_> = processes
            .keys()
            .copied()
            .filter(|pid| {
                targets
                    .iter()
                    .any(|target| ancestry(&processes, *pid, target.root).is_some())
            })
            .map(Pid::from_u32)
            .collect();
        // Only inspect executable/command data for these terminal sessions.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&related),
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cmd(UpdateKind::Always)
                .with_exe(UpdateKind::Always),
        );
        for pid in related {
            if let (Some(process), Some(info)) =
                (processes.get_mut(&pid.as_u32()), self.system.process(pid))
            {
                let executable = info
                    .exe()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| info.name().to_string_lossy().into_owned());
                let command: Vec<_> = info
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                process.provider = Provider::from_command(&executable, &command);
            }
        }
        targets
            .iter()
            .map(|target| {
                recognize(&processes, *target, |pid, group| {
                    #[cfg(unix)]
                    // SAFETY: getpgid only queries a process ID; it dereferences no pointers.
                    {
                        unsafe { libc::getpgid(pid as libc::pid_t) == group as libc::pid_t }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (pid, group);
                        false
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_and_script_matching_does_not_read_prompts() {
        for provider in Provider::ALL {
            assert_eq!(
                Provider::from_executable(provider.executable()),
                Some(provider)
            );
            assert_eq!(
                Provider::from_executable(&format!("C:\\bin\\{}.exe", provider.executable())),
                Some(provider)
            );
        }
        let cmd = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            Provider::from_command(
                "/usr/bin/node",
                &cmd(&["node", "/opt/node_modules/@openai/codex/bin/codex.js"])
            ),
            Some(Provider::Codex)
        );
        assert_eq!(
            Provider::from_command("/bin/bash", &cmd(&["bash", "-c", "codex"])),
            None
        );
        assert_eq!(
            Provider::from_command("/bin/echo", &cmd(&["echo", "claude"])),
            None
        );
        assert_eq!(
            Provider::from_executable("antigravity"),
            Some(Provider::Antigravity)
        );
    }
    #[test]
    fn foreground_ancestry_excludes_background_agents_and_other_sessions() {
        let processes = HashMap::from([
            (
                1,
                Process {
                    parent: None,
                    provider: None,
                },
            ),
            (
                2,
                Process {
                    parent: Some(1),
                    provider: Some(Provider::Claude),
                },
            ),
            (
                3,
                Process {
                    parent: Some(1),
                    provider: Some(Provider::Codex),
                },
            ),
            (
                4,
                Process {
                    parent: Some(3),
                    provider: None,
                },
            ),
            (
                5,
                Process {
                    parent: None,
                    provider: Some(Provider::Copilot),
                },
            ),
        ]);
        assert_eq!(
            recognize(
                &processes,
                ProcessTarget {
                    root: 1,
                    foreground: Some(4)
                },
                |_, _| false
            ),
            Some(Provider::Codex)
        );
        assert_eq!(
            recognize(
                &processes,
                ProcessTarget {
                    root: 1,
                    foreground: Some(1)
                },
                |_, _| false
            ),
            None
        );
        assert_eq!(
            recognize(
                &processes,
                ProcessTarget {
                    root: 1,
                    foreground: None
                },
                |_, _| true
            ),
            None
        );
    }
}
