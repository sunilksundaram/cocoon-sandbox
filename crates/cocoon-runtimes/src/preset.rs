use cocoon_core::CocoonError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Python,
    #[serde(rename = "python-skill")]
    PythonSkill,
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
            "python-skill" | "py-skill" | "pyskill" => Ok(RuntimeKind::PythonSkill),
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
                // Untrusted Model Mode: -I (isolated from host/user env), -S (no site packages), -B (no bytecode)
                let mut args = vec!["-I".into(), "-S".into(), "-B".into()];
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
            RuntimeKind::PythonSkill => {
                let exe = Self::find_executable(&["python3", "python"])?;
                // Skill Mode: allows loading format/document packages (openpyxl, python-docx, etc.)
                let mut args = vec!["-B".into()];
                if let Some(code) = script_code {
                    args.push("-c".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push(file.to_string_lossy().to_string());
                } else {
                    return Err(CocoonError::Config(
                        "PythonSkill runtime requires either inline code (-c) or a script file"
                            .into(),
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
                let mut args = vec!["--no-install".into()];
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
                    args.push("--no-prompt".into());
                    args.push(code.into());
                } else if let Some(file) = script_file {
                    args.push("run".into());
                    args.push("--no-prompt".into());
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
                let mut args = vec!["script".into(), "--offline".into()];
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
                let mut args = vec!["run".into(), "-mod=readonly".into()];
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
                let mut args = vec!["--disable-gems".into()];
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
                let mut args = vec!["-n".into()];
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
            if let Some(path) = which(candidate) {
                return Ok(path);
            }
        }
        // Fallback to first candidate name
        Ok(PathBuf::from(candidates[0]))
    }
}

/// Simple PATH resolver
pub fn which(name: &str) -> Option<PathBuf> {
    if let Ok(paths) = std::env::var("PATH") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        for dir in paths.split(separator) {
            let p = Path::new(dir).join(name);
            if p.is_file() {
                #[cfg(windows)]
                if p.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
                    continue; // Skip 0-byte Windows Store execution alias stubs
                }
                return Some(p);
            }
            if cfg!(windows) {
                let p_exe = Path::new(dir).join(format!("{}.exe", name));
                if p_exe.is_file() {
                    if p_exe.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
                        continue; // Skip 0-byte Windows Store execution alias stubs
                    }
                    return Some(p_exe);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_kind_from_str() {
        assert_eq!(
            "python".parse::<RuntimeKind>().unwrap(),
            RuntimeKind::Python
        );
        assert_eq!("py".parse::<RuntimeKind>().unwrap(), RuntimeKind::Python);
        assert_eq!(
            "python-skill".parse::<RuntimeKind>().unwrap(),
            RuntimeKind::PythonSkill
        );
        assert_eq!("node".parse::<RuntimeKind>().unwrap(), RuntimeKind::Node);
        assert_eq!("bun".parse::<RuntimeKind>().unwrap(), RuntimeKind::Bun);
        assert_eq!(
            "pwsh".parse::<RuntimeKind>().unwrap(),
            RuntimeKind::PowerShell
        );
        assert_eq!("bash".parse::<RuntimeKind>().unwrap(), RuntimeKind::Bash);
        assert_eq!("rust".parse::<RuntimeKind>().unwrap(), RuntimeKind::Rust);
        assert_eq!("go".parse::<RuntimeKind>().unwrap(), RuntimeKind::Go);
        assert!("invalid_xyz".parse::<RuntimeKind>().is_err());
    }

    #[test]
    fn test_python_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Python, Some("print('unit test')"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"-I".to_string()));
        assert!(args.contains(&"-S".to_string()));
        assert!(args.contains(&"-B".to_string()));
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"print('unit test')".to_string()));
    }

    #[test]
    fn test_python_skill_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::PythonSkill, Some("print('skill test')"), None)
                .expect("Config should succeed");

        assert!(!args.contains(&"-S".to_string()));
        assert!(args.contains(&"-B".to_string()));
        assert!(args.contains(&"-c".to_string()));
    }

    #[test]
    fn test_node_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Node, Some("console.log('test')"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"--no-addons".to_string()));
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"console.log('test')".to_string()));
    }

    #[test]
    fn test_bun_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Bun, Some("console.log('bun test')"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"--no-install".to_string()));
        assert!(args.contains(&"-e".to_string()));
    }

    #[test]
    fn test_deno_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Deno, Some("console.log('deno test')"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"eval".to_string()));
        assert!(args.contains(&"--no-prompt".to_string()));
    }

    #[test]
    fn test_ruby_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Ruby, Some("puts 'ruby test'"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"--disable-gems".to_string()));
        assert!(args.contains(&"-e".to_string()));
    }

    #[test]
    fn test_php_args_configuration() {
        let (_exe, args) =
            RuntimePreset::configure(RuntimeKind::Php, Some("echo 'php test';"), None)
                .expect("Config should succeed");

        assert!(args.contains(&"-n".to_string()));
        assert!(args.contains(&"-r".to_string()));
    }
}
