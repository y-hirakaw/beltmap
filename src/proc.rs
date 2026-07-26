//! 外部コマンドの実行。
//!
//! GitHubの認証は `gh` に委譲する(8章)。自前でトークンを持たないため、
//! ここは薄いラッパで足りる。

use std::process::Command;

#[derive(Debug)]
pub enum RunError {
    /// コマンド自体が無い。インストール手順を出して該当機能を無効化する
    NotFound(String),
    /// 実行はできたが失敗した
    Failed { code: Option<i32>, stderr: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NotFound(cmd) => write!(f, "`{cmd}` が見つからない"),
            RunError::Failed { code, stderr } => {
                let head = stderr.lines().next().unwrap_or("(出力なし)");
                match code {
                    Some(c) => write!(f, "終了コード {c}: {head}"),
                    None => write!(f, "異常終了: {head}"),
                }
            }
        }
    }
}

pub fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, RunError> {
    let out = Command::new(program).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RunError::NotFound(program.to_string())
        } else {
            RunError::Failed {
                code: None,
                stderr: e.to_string(),
            }
        }
    })?;

    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(RunError::Failed {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}

pub fn exists(program: &str) -> bool {
    matches!(run(program, &["--version"]), Ok(_))
}
