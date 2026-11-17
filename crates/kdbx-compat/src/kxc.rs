//! Thin wrapper around keepassxc-cli, used as the reference implementation.
//!
//! keepassxc-cli hangs if stdin is not redirected, so stdin is always piped.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
pub struct Key {
    pub password: Option<String>,
    pub keyfile: Option<PathBuf>,
}

impl Key {
    pub fn to_keepass(&self) -> std::io::Result<keepass::DatabaseKey> {
        let mut key = keepass::DatabaseKey::new();
        if let Some(p) = &self.password {
            key = key.with_password(p);
        }
        if let Some(k) = &self.keyfile {
            key = key.with_keyfile(&mut std::fs::File::open(k)?)?;
        }
        Ok(key)
    }
}

pub struct Kxc {
    exe: PathBuf,
}

impl Kxc {
    pub fn find() -> Option<Kxc> {
        if let Some(p) = std::env::var_os("KEEPASSXC_CLI") {
            return Some(Kxc { exe: p.into() });
        }
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            for name in ["keepassxc-cli.exe", "keepassxc-cli"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(Kxc { exe: candidate });
                }
            }
        }
        None
    }

    fn run(&self, args: &[&str], key: &Key, db: &Path) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.exe);
        cmd.args(args);
        if let Some(k) = &key.keyfile {
            cmd.arg("-k").arg(k);
        }
        if key.password.is_none() {
            cmd.arg("--no-password");
        }
        cmd.arg(db);
        self.finish(cmd, key, &[])
    }

    fn run_with(
        &self,
        args: &[&str],
        key: &Key,
        db: &Path,
        tail: &[&str],
    ) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.exe);
        cmd.args(args);
        if let Some(k) = &key.keyfile {
            cmd.arg("-k").arg(k);
        }
        if key.password.is_none() {
            cmd.arg("--no-password");
        }
        cmd.arg(db);
        self.finish(cmd, key, tail)
    }

    fn finish(&self, mut cmd: Command, key: &Key, tail: &[&str]) -> Result<Vec<u8>, String> {
        cmd.args(tail);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot start keepassxc-cli: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let input = key
                .password
                .as_deref()
                .map(|p| format!("{p}\n"))
                .unwrap_or_default();
            let _ = stdin.write_all(input.as_bytes());
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            // never echo stdout here: it may contain decrypted data
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    pub fn version(&self) -> String {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--version");
        self.finish(cmd, &Key::default(), &[])
            .map(|o| String::from_utf8_lossy(&o).trim().to_string())
            .unwrap_or_else(|e| format!("unknown ({e})"))
    }

    pub fn export_xml(&self, db: &Path, key: &Key) -> Result<String, String> {
        let out = self.run(&["export", "-q", "-f", "xml"], key, db)?;
        String::from_utf8(out).map_err(|e| e.to_string())
    }

    /// Opens the database and prints its summary; used for timing.
    pub fn db_info(&self, db: &Path, key: &Key) -> Result<(), String> {
        self.run(&["db-info", "-q"], key, db).map(|_| ())
    }

    /// Exports to a temporary file: `--stdout` goes through a text stream on
    /// Windows and mangles binary data.
    pub fn attachment(
        &self,
        db: &Path,
        key: &Key,
        entry: &str,
        name: &str,
        scratch: &Path,
    ) -> Result<Vec<u8>, String> {
        let _ = std::fs::remove_file(scratch);
        let target = scratch.to_string_lossy().to_string();
        self.run_with(
            &["attachment-export", "-q"],
            key,
            db,
            &[entry, name, &target],
        )?;
        let data = std::fs::read(scratch).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(scratch);
        data
    }

    pub fn totp(&self, db: &Path, key: &Key, entry: &str) -> Result<String, String> {
        let out = self.run_with(&["show", "-q", "-t"], key, db, &[entry])?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }
}
