//! Resolve source-build identities without changing release package versions.

use std::io;
use std::path::Path;
use std::process::Command;

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=AIT_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=RELEASE_TAG");
    for name in ["HEAD", "refs", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name])
            && Path::new(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let version = std::env::var("AIT_BUILD_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let release = format!("v{}", env!("CARGO_PKG_VERSION"));
            if std::env::var("RELEASE_TAG").as_deref() == Ok(&release) {
                return Some(env!("CARGO_PKG_VERSION").to_owned());
            }
            let hash = git(&["rev-parse", "--short=8", "HEAD"])?;
            let date = git(&["show", "-s", "--format=%cs", "HEAD"])?;
            Some(format!("git-{date}-{hash}"))
        })
        .ok_or_else(|| io::Error::other(
            "Cannot identify source build: provide Git metadata, AIT_BUILD_VERSION or RELEASE_TAG.",
        ))?;
    println!("cargo:rustc-env=AIT_BUILD_VERSION={version}");
    Ok(())
}
