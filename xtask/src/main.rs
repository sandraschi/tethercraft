//! Workspace tooling: `cargo xtask <command>`.
//! VENDORED from mcp-central-docs/templates/craft-app/xtask/src/main.rs
//!
//! Pure Rust (std + serde_json). Implements the storytold craft-contrib & craft-common-actions
//! quality gate contract: `ci`, `assets`, `layers`, `wasm`, `deny`, `version`, `ico`.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo
)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci              fmt --check, clippy -D warnings, test, assets, layers, wasm, deny
  assets          check that every icon/image/asset is attributed in ATTRIBUTION.md
  layers          enforce downward crate dependency layering (no egui/eframe in core/store/mcp)
  wasm            cargo check --target wasm32-unknown-unknown for wasm-safe crates
  deny            cargo deny check against deny.toml (22-SPDX allowlist)
  version [...]   show or set [workspace.package] version (`cargo xtask version [set X.Y.Z]`)
  ico <out> <png> pack square PNGs (<= 256 px) into a Windows .ico
";

const ASSET_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "ico", "icns", "svg", "pdf", "ttf",
    "otf", "woff", "woff2", "mp4", "wav", "mp3",
];
const ASSET_DIRS: &[&str] = &["assets/", "docs/images/"];
const UI_CRATES: &[&str] = &["egui", "eframe", "winit", "egui_kittest", "rfd"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let res = match args.first().map(String::as_str) {
        Some("ci") | None => cmd_ci(),
        Some("assets") => cmd_assets(&root()),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("deny") => cmd_deny(),
        Some("version") => cmd_version(&root(), &rest),
        Some("ico") => cmd_ico(&rest),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn cargo() -> Command {
    let bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut c = Command::new(bin);
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd
        .status()
        .map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: exited with {status}"))
    }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo()
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn is_asset(path: &str) -> bool {
    if path.ends_with("MANIFEST.txt") || path.ends_with(".md") || path.ends_with(".txt") {
        return false;
    }
    let ext = Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    ASSET_DIRS.iter().any(|d| path.starts_with(d)) || ASSET_EXT.contains(&ext.as_str())
}

pub fn cmd_assets(repo_root: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-files failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let files: Vec<String> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|l| repo_root.join(l).exists())
        .collect();
    let attr_path = repo_root.join("ATTRIBUTION.md");
    let md = std::fs::read_to_string(&attr_path).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let missing: Vec<String> = files
        .iter()
        .filter(|f| is_asset(f) && !md.contains(&format!("`{f}`")))
        .cloned()
        .collect();
    if missing.is_empty() {
        let count = files.iter().filter(|f| is_asset(f)).count();
        println!("assets: all {count} asset files attributed in ATTRIBUTION.md");
        Ok(())
    } else {
        Err(format!(
            "{} asset file(s) lack an ATTRIBUTION.md entry (author, source, licence):\n  {}",
            missing.len(),
            missing.join("\n  ")
        ))
    }
}

fn classify_layer(name: &str) -> u8 {
    if name == "xtask" {
        99
    } else if name.ends_with("-app")
        || name.ends_with("-desktop")
        || name.ends_with("-web")
        || name.ends_with("-ui")
        || name.ends_with("-ui-egui")
    {
        6
    } else if name.ends_with("-mcp") || name.ends_with("-cli") {
        5
    } else if name.ends_with("-core") {
        1
    } else if name.ends_with("-store") || name.ends_with("-geom") || name.ends_with("-format") {
        0
    } else {
        // Default: standalone or app binary crate at root
        6
    }
}

pub fn cmd_layers() -> Result<(), String> {
    let meta = metadata()?;
    let pkgs = meta["packages"]
        .as_array()
        .ok_or_else(|| "metadata: no packages array".to_string())?;
    let mut violations = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().unwrap_or_default();
        let layer = classify_layer(name);
        if layer >= 6 {
            continue;
        }
        if let Some(deps) = p["dependencies"].as_array() {
            for d in deps {
                let dname = d["name"].as_str().unwrap_or_default();
                if UI_CRATES.contains(&dname) {
                    violations.push(format!(
                        "{name} (L{layer}) depends on UI crate `{dname}`; UI toolkits are only allowed in L6+"
                    ));
                }
            }
        }
    }
    if violations.is_empty() {
        println!(
            "layers: ok ({} workspace crates checked, zero UI leaks in L0-L5)",
            pkgs.len()
        );
        Ok(())
    } else {
        Err(format!(
            "{} layering violation(s):\n  {}",
            violations.len(),
            violations.join("\n  ")
        ))
    }
}

pub fn cmd_wasm() -> Result<(), String> {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
        .unwrap_or(false);
    if !installed {
        let hint = "wasm32-unknown-unknown target not installed (`rustup target add wasm32-unknown-unknown`)";
        if std::env::var_os("CI").is_some() {
            return Err(hint.into());
        }
        eprintln!("wasm: skipped ({hint})");
        return Ok(());
    }
    let meta = metadata()?;
    let pkgs = meta["packages"]
        .as_array()
        .ok_or_else(|| "metadata: no packages array".to_string())?;
    let mut wasm_pkgs = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().unwrap_or_default();
        if name.ends_with("-web") {
            wasm_pkgs.push(name.to_string());
        } else if name.ends_with("-core") {
            // Check if core pulls native-only tokio/rusqlite/reqwest before enforcing wasm
            let native_only = p["dependencies"]
                .as_array()
                .map(|deps| {
                    deps.iter().any(|d| {
                        let dn = d["name"].as_str().unwrap_or_default();
                        matches!(dn, "rusqlite" | "tokio" | "reqwest")
                    })
                })
                .unwrap_or(false);
            if !native_only {
                wasm_pkgs.push(name.to_string());
            }
        }
    }
    if wasm_pkgs.is_empty() {
        println!("wasm: no pure wasm32 crates without native I/O deps in workspace (ok)");
        return Ok(());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown"]);
    for pkg in &wasm_pkgs {
        c.args(["-p", pkg]);
    }
    run(
        c,
        &format!(
            "cargo check --target wasm32-unknown-unknown -p {}",
            wasm_pkgs.join(" -p ")
        ),
    )
}

pub fn cmd_deny() -> Result<(), String> {
    if !root().join("deny.toml").exists() {
        return Err("missing deny.toml in workspace root".into());
    }
    let installed = cargo()
        .args(["deny", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !installed {
        let hint = "cargo-deny is not installed (`cargo install cargo-deny --locked`)";
        if std::env::var_os("CI").is_some() {
            return Err(hint.into());
        }
        eprintln!("deny: skipped ({hint})");
        return Ok(());
    }
    let mut c = cargo();
    c.args(["deny", "check"]);
    run(c, "cargo deny check")
}

fn cmd_test() -> Result<(), String> {
    let nextest = cargo()
        .args(["nextest", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if nextest {
        let mut c = cargo();
        c.args(["nextest", "run", "--workspace"]);
        run(c, "cargo nextest run --workspace")?;
        let mut doc = cargo();
        doc.args(["test", "--workspace", "--doc"]);
        run(doc, "cargo test --workspace --doc")
    } else {
        let mut c = cargo();
        c.args(["test", "--workspace"]);
        run(c, "cargo test --workspace")
    }
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args([
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("assets", Box::new(|| cmd_assets(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
        ("deny", Box::new(cmd_deny)),
    ];
    let skip = std::env::var("XTASK_CI_SKIP").unwrap_or_default();
    let skipped: Vec<&str> = skip
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let mut done = Vec::new();
    for (name, f) in &steps {
        if skipped.contains(name) {
            continue;
        }
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!(
        "\nCI summary: all {} steps passed ({})",
        done.len(),
        done.join(", ")
    );
    Ok(())
}

fn cmd_version(repo_root: &Path, args: &[&str]) -> Result<(), String> {
    let path = repo_root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read Cargo.toml: {e}"))?;
    match args {
        [] => {
            for line in text.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("version") {
                    let rest = rest.trim_start();
                    if let Some(val) = rest.strip_prefix('=') {
                        let v = val.trim().trim_matches('"');
                        if !v.is_empty() {
                            println!("{v}");
                            return Ok(());
                        }
                    }
                }
            }
            Err("no version found in Cargo.toml".into())
        }
        ["set", new_ver] => {
            let mut out = String::with_capacity(text.len());
            let mut replaced = false;
            for line in text.split_inclusive('\n') {
                let t = line.trim();
                if !replaced && t.starts_with("version") && t.contains('=') {
                    let eol = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                    out.push_str(&format!("version = \"{new_ver}\"{eol}"));
                    replaced = true;
                } else {
                    out.push_str(line);
                }
            }
            if !replaced {
                return Err("could not locate version line in Cargo.toml".into());
            }
            std::fs::write(&path, out).map_err(|e| format!("write Cargo.toml: {e}"))?;
            println!("version set to {new_ver}");
            Ok(())
        }
        _ => Err("usage: cargo xtask version [set X.Y.Z]".into()),
    }
}

fn cmd_ico(args: &[&str]) -> Result<(), String> {
    let [out_path, inputs @ ..] = args else {
        return Err("usage: cargo xtask ico <out.ico> <in.png>...".into());
    };
    if inputs.is_empty() {
        return Err("need at least one input PNG".into());
    }
    let mut pngs = Vec::new();
    for p in inputs {
        let bytes = std::fs::read(p).map_err(|e| format!("read {p}: {e}"))?;
        if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
            return Err(format!("{p}: not a valid PNG file"));
        }
        pngs.push(bytes);
    }
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    let count = u16::try_from(pngs.len()).map_err(|_| "too many PNGs")?;
    ico.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * pngs.len();
    for png in &pngs {
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        if w != h || w == 0 || w > 256 {
            return Err(format!(
                "icon PNG must be square and <= 256 px, got {w}x{h}"
            ));
        }
        let dim = if w == 256 {
            0
        } else {
            u8::try_from(w).unwrap_or(0)
        };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        let len32 = u32::try_from(png.len()).map_err(|_| "PNG too large")?;
        let off32 = u32::try_from(offset).map_err(|_| "ICO too large")?;
        ico.extend_from_slice(&len32.to_le_bytes());
        ico.extend_from_slice(&off32.to_le_bytes());
        offset += png.len();
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::write(out_path, &ico).map_err(|e| format!("write {out_path}: {e}"))?;
    println!("{out_path}: {} images, {} bytes", pngs.len(), ico.len());
    Ok(())
}
