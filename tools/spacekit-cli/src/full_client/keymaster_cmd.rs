//! `spacekit keymaster` — SKKM custody ceremonies via the keymaster-ui TypeScript CLI.

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum KeymasterCommands {
    /// Run SSS + enrollment + recovery + break-glass round-trip tests.
    RoundtripTest {
        /// Network: `mock` (in-memory) or `prod` (coordinator + guardians).
        #[arg(long, default_value = "mock")]
        network: String,
    },

    /// Export manifest + envelopes for offline break-glass recovery.
    ExportBackup {
        /// Output directory (writes `backup-bundle.json`)
        #[arg(long, default_value = "./keymaster-backup")]
        out: String,

        /// Enroll a demo keystore first.
        #[arg(long)]
        demo: bool,

        #[arg(long, default_value = "mock")]
        network: String,
    },

    /// Recover keystore to a local file (never prints secret to stdout).
    Recover {
        /// Path to `backup-bundle.json` or directory containing it (break-glass).
        #[arg(long)]
        backup: Option<String>,

        /// Break-glass mode: skip coordinator, use local backup bundle.
        #[arg(long)]
        break_glass: bool,

        /// Enroll a demo keystore before coordinated recover.
        #[arg(long)]
        demo: bool,

        /// Output file for recovered keystore JSON.
        #[arg(long, default_value = "recovered-keystore.json")]
        output: String,

        #[arg(long, default_value = "mock")]
        network: String,
    },
}

fn resolve_keymaster_ui_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(p) = std::env::var("SPACEKIT_KEYMASTER_UI") {
        let path = PathBuf::from(p);
        if path.join("cli/main.ts").is_file() {
            return Ok(path);
        }
        return Err(format!(
            "SPACEKIT_KEYMASTER_UI set but cli/main.ts not found: {}",
            path.display()
        )
        .into());
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest.join("../spacekit-projects/apps/keymaster/keymaster-ui"),
        manifest.join("../../spacekit-projects/apps/keymaster/keymaster-ui"),
    ];
    for candidate in candidates {
        if candidate.join("cli/main.ts").is_file() {
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    Err(
        "keymaster-ui not found. Set SPACEKIT_KEYMASTER_UI or run from the SpaceKit monorepo."
            .into(),
    )
}

fn run_keymaster_ts(ui_dir: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    if !ui_dir.join("node_modules").is_dir() {
        return Err(format!(
            "keymaster-ui dependencies missing. Run:\n  cd {}\n  npm install",
            ui_dir.display()
        )
        .into());
    }

    // Prefer npm script — resolves tsx reliably across platforms.
    let status = Command::new("npm")
        .arg("run")
        .arg("cli")
        .arg("--")
        .args(args)
        .current_dir(ui_dir)
        .status();

    match status {
        Ok(s) if s.success() => return Ok(()),
        Ok(s) => {
            return Err(format!("keymaster ceremony failed (exit {s})").into());
        }
        Err(e) => {
            eprintln!("npm run cli failed ({e}); trying local tsx…");
        }
    }

    let local_tsx = ui_dir.join("node_modules/.bin/tsx");
    if !local_tsx.is_file() {
        return Err("tsx not found — run `npm install` in keymaster-ui".into());
    }

    let status = Command::new(&local_tsx)
        .arg("cli/main.ts")
        .args(args)
        .current_dir(ui_dir)
        .status()?;

    if !status.success() {
        return Err(format!("keymaster ceremony failed (exit {status})").into());
    }
    Ok(())
}

pub async fn handle_keymaster_command(
    cmd: &KeymasterCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    let ui_dir = resolve_keymaster_ui_dir()?;
    println!("{} keymaster-ui at {}", "Using".cyan(), ui_dir.display());

    match cmd {
        KeymasterCommands::RoundtripTest { network } => {
            run_keymaster_ts(&ui_dir, &["roundtrip-test", "--network", network.as_str()])?;
        }
        KeymasterCommands::ExportBackup { out, demo, network } => {
            let mut args = vec![
                "export-backup",
                "--out",
                out.as_str(),
                "--network",
                network.as_str(),
            ];
            if *demo {
                args.push("--demo");
            }
            run_keymaster_ts(&ui_dir, &args)?;
        }
        KeymasterCommands::Recover {
            backup,
            break_glass,
            demo,
            output,
            network,
        } => {
            let mut args = vec![
                "recover",
                "--output",
                output.as_str(),
                "--network",
                network.as_str(),
            ];
            if let Some(b) = backup {
                args.push("--backup");
                args.push(b.as_str());
            }
            if *break_glass {
                args.push("--break-glass");
            }
            if *demo {
                args.push("--demo");
            }
            run_keymaster_ts(&ui_dir, &args)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Requires the external `spacekit-projects` checkout (keymaster-ui), which is
    // not part of the spacekit-core monorepo. Ignored by default so the suite is
    // green in a bare checkout / CI; run with `cargo test -- --ignored` where the
    // sibling repo is present. The resolver logic itself is covered
    // deterministically by `keymaster_ui_path_resolves_via_env` below.
    #[test]
    #[ignore = "requires external spacekit-projects checkout; run with --ignored"]
    fn keymaster_ui_path_resolves_in_monorepo() {
        let dir =
            resolve_keymaster_ui_dir().expect("keymaster-ui should resolve from spacekit-cli");
        assert!(dir.join("cli/main.ts").is_file());
    }

    // Deterministic: exercises the resolver's `SPACEKIT_KEYMASTER_UI` override
    // branch against a temp fixture, so the resolution logic is tested without
    // depending on any external checkout.
    #[test]
    fn keymaster_ui_path_resolves_via_env() {
        let tmp = std::env::temp_dir().join(format!("sk_km_ui_{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("cli")).unwrap();
        std::fs::write(tmp.join("cli/main.ts"), b"// fixture").unwrap();

        std::env::set_var("SPACEKIT_KEYMASTER_UI", &tmp);
        let resolved = resolve_keymaster_ui_dir();
        std::env::remove_var("SPACEKIT_KEYMASTER_UI");

        let dir = resolved.expect("resolves from SPACEKIT_KEYMASTER_UI");
        assert!(dir.join("cli/main.ts").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
