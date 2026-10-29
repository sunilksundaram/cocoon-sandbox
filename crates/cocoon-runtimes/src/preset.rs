use cocoon_core::CocoonError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Python,
    Node,
    Bun,
    Deno,
    PowerShell,
    Bash,
    Rust,
    Go,
    Ruby,
    Php,
    Lua,
    Java,
    Custom,
}

impl std::str::FromStr for RuntimeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python" | "py" | "python3" => Ok(RuntimeKind::Python),
            "node" | "js" | "javascript" => Ok(RuntimeKind::Node),
            "bun" => Ok(RuntimeKind::Bun),
            "deno" | "ts" | "typescript" => Ok(RuntimeKind::Deno),
            "pwsh" | "powershell" | "ps1" => Ok(RuntimeKind::PowerShell),
            "bash" | "sh" | "shell" => Ok(RuntimeKind::Bash),
            "rust" | "cargo" | "rs" => Ok(RuntimeKind::Rust),
            "go" | "golang" => Ok(RuntimeKind::Go),
            "ruby" | "rb" => Ok(RuntimeKind::Ruby),
            "php" => Ok(RuntimeKind::Php),
            "lua" => Ok(RuntimeKind::Lua),
            "java" => Ok(RuntimeKind::Java),
            "custom" => Ok(RuntimeKind::Custom),
            other => Err(format!("Unknown runtime preset: '{}'", other)),
        }
    }
}

pub struct RuntimePreset;

impl RuntimePreset {
    /// Resolves executable path and arguments for a given runtime and inline script or file
    pub fn configure(
        kind: RuntimeKind,
        script_code: Option<&str>,
        script_file: Option<&Path>,
    ) -> Result<(PathBuf, Vec<String>), CocoonError> {
        match kind {
            RuntimeKind::Python => {
                let exe = Self::find_executable(&["python3", "python"])?;
                let mut args = vec!["-B".into()]; // No bytecode cache on host, allow loading local workspace modules
                if let Some(code) = script_code {
                    args.push("-c".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Python runtime requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Node => {
                let exe = Self::find_executable(&["node", "nodejs"])?;
                let mut args = vec!["--no-addons".into()];
                if let Some(code) = script_code {
                    args.push("-e".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Node runtime requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Bun => {
                let exe = Self::find_executable(&["bun"])?;
                let mut args = Vec::new();
                if let Some(code) = script_code {
                    args.push("-e".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Bun runtime requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Deno => {
                let exe = Self::find_executable(&["deno"])?;
                let mut args = Vec::new();
                if let Some(code) = script_code {
                    args.push("eval".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push("run".into());
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Deno runtime requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::PowerShell => {
                let exe = Self::find_executable(&["pwsh", "powershell.exe", "powershell"])?;
                let mut args = vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                ];
                if let Some(code) = script_code {
                    args.push("-Command".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push("-File".into());
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "PowerShell requires either inline command (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Bash => {
                let exe = Self::find_executable(&["bash", "sh"])?;
                let mut args = vec!["--noprofile".into(), "--norc".into()];
                if let Some(code) = script_code {
                    args.push("-c".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Bash requires either inline command (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Rust => {
                let exe = Self::find_executable(&["cargo"])?;
                let mut args = vec!["script".into()];
                if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else if let Some(code) = script_code {
                    args.push("-e".into());
                    args.push(code.into());
                }
                Ok((exe, args))
            }
            RuntimeKind::Go => {
                let exe = Self::find_executable(&["go"])?;
                let mut args = vec!["run".into()];
                if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Go runtime requires a script or main file to run (e.g. -f main.go)".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Ruby => {
                let exe = Self::find_executable(&["ruby"])?;
                let mut args = Vec::new();
                if let Some(code) = script_code {
                    args.push("-e".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Ruby requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Php => {
                let exe = Self::find_executable(&["php"])?;
                let mut args = Vec::new();
                if let Some(code) = script_code {
                    args.push("-r".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "PHP requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Lua => {
                let exe = Self::find_executable(&["lua", "luajit"])?;
                let mut args = Vec::new();
                if let Some(code) = script_code {
                    args.push("-e".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Lua requires either inline code (-c) or a script file".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Java => {
                let exe = Self::find_executable(&["java"])?;
                let mut args = Vec::new();
                if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "Java runtime requires a source or jar file (e.g. -f App.java)".into(),
                    ));
                }
                Ok((exe, args))
            }
            RuntimeKind::Custom => Err(CocoonError::Config(
                "Custom runtime must specify an explicit executable command".into(),
            )),
        }
    }

    /// Finds the first matching executable name available in the system PATH
    pub fn find_executable(candidates: &[&str]) -> Result<PathBuf, CocoonError> {
        for candidate in candidates {
            if let Ok(path) = which(candidate) {
                return Ok(path);
            }
        }
        // Fallback to first candidate name
        Ok(PathBuf::from(candidates[0]))
    }
}

/// Simple PATH resolver
pub fn which(name: &str) -> Result<PathBuf, ()> {
    if let Ok(paths) = std::env::var("PATH") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        for dir in paths.split(separator) {
            let p = Path::new(dir).join(name);
            if p.is_file() {
                return Ok(p);
            }
            if cfg!(windows) {
                let p_exe = Path::new(dir).join(format!("{}.exe", name));
                if p_exe.is_file() {
                    return Ok(p_exe);
                }
            }
        }
    }
    Err(())
}
