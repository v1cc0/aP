use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let frontend_dir = manifest_dir.join("frontend");
    let dist_index = frontend_dir.join("dist").join("index.html");

    println!("cargo:rerun-if-env-changed=AP_SKIP_FRONTEND_BUILD");
    println!("cargo:rerun-if-env-changed=VITE_APP_VERSION");
    println!("cargo:rerun-if-env-changed=VITE_API_PROXY_TARGET");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/package-lock.json");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/vite.config.js");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/public");

    if std::env::var_os("AP_SKIP_FRONTEND_BUILD").is_some() {
        if dist_index.exists() {
            println!("cargo:warning=AP_SKIP_FRONTEND_BUILD is set; using existing frontend/dist");
            return;
        }
        panic!("AP_SKIP_FRONTEND_BUILD is set, but frontend/dist/index.html does not exist");
    }

    ensure_node_deps(&frontend_dir);
    run(
        &frontend_dir,
        "node",
        &["node_modules/vite/bin/vite.js", "build"],
    );

    if !dist_index.exists() {
        panic!(
            "frontend build completed without producing {}",
            dist_index.display()
        );
    }
}

fn ensure_node_deps(frontend_dir: &Path) {
    let package_lock = frontend_dir.join("package-lock.json");
    let node_lock = frontend_dir.join("node_modules").join(".package-lock.json");
    let vite_cli = frontend_dir
        .join("node_modules")
        .join("vite")
        .join("dist")
        .join("node")
        .join("cli.js");

    if !vite_cli.exists() || is_newer(&package_lock, &node_lock) {
        run(frontend_dir, "npm", &["ci"]);
    }
}

fn is_newer(source: &Path, target: &Path) -> bool {
    let Ok(source_meta) = fs::metadata(source) else {
        return false;
    };
    let Ok(target_meta) = fs::metadata(target) else {
        return true;
    };

    let source_modified = source_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let target_modified = target_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    source_modified > target_modified
}

fn run(cwd: &Path, program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "failed to run `{}` in {}: {}",
                command_line(program, args),
                cwd.display(),
                err
            )
        });

    if !status.success() {
        panic!(
            "`{}` failed in {} with status {}",
            command_line(program, args),
            cwd.display(),
            status
        );
    }
}

fn command_line(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}
