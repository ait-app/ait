//! The built-in provider adapters: the one place that names every adapter the daemon ships.
//!
//! The runtime's [`Providers`] container stays adapter-agnostic; this crate decides which
//! adapters it holds, their default programs, and the environment variables that override them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use provider::Providers;
use provider_antigravity::AntigravityClient;
use provider_claude::ClaudeClient;
use provider_deepseek_harness::DeepSeekHarnessClient;
use provider_minimax::MiniMaxClient;
use provider_omp::OmpClient;

/// Built-in adapters configured from the process environment and installed native programs.
///
/// `data_dir` is the daemon's persistent storage root, used for native image artifacts.
/// Returns configured adapters without starting provider processes or creating storage.
#[must_use]
pub fn providers(data_dir: &Path) -> Providers {
    configured(data_dir, |name| std::env::var_os(name))
}

/// Like [`providers`], reading launcher overrides such as `AIT_SERVER_CODEX_BIN` from `environment`.
///
/// In-process hosts and tests select offline native programs this way without mutating the
/// process environment. Returns adapters without starting processes or creating storage.
#[doc(hidden)]
#[must_use]
pub fn configured(data_dir: &Path, environment: impl Fn(&str) -> Option<OsString>) -> Providers {
    let omp = provider_omp::installed_program(&environment);
    let antigravity = environment("AIT_SERVER_ANTIGRAVITY_BIN");
    let claude =
        environment("AIT_SERVER_CLAUDE_BIN").map_or_else(|| PathBuf::from("claude"), PathBuf::from);
    let dsh = environment("AIT_SERVER_DEEPSEEK_HARNESS_BIN");
    let dsh_profile = environment("AIT_SERVER_DEEPSEEK_HARNESS_PROFILE");
    let dsh_acp =
        environment("AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT").as_deref() == Some("acp".as_ref());
    let minimax = environment("AIT_SERVER_MINIMAX_CODE_BIN")
        .map_or_else(|| PathBuf::from("mcode"), PathBuf::from);
    Providers::configured(data_dir, &environment)
        .with(|images| {
            antigravity
                .map_or_else(AntigravityClient::installed, |program| {
                    AntigravityClient::new(program.into())
                })
                .with_image_directory(images.to_owned())
        })
        .with(|images| ClaudeClient::new(claude).with_image_directory(images.to_owned()))
        .with(|images| {
            let mut client = dsh
                .map_or_else(DeepSeekHarnessClient::installed, |program| {
                    DeepSeekHarnessClient::new(program.into())
                })
                .with_image_directory(images.to_owned());
            if let Some(profile) = dsh_profile {
                client = client.with_native_profile(profile);
            }
            if dsh_acp {
                client = client.with_acp_profile();
            }
            client
        })
        .with(|images| MiniMaxClient::new(minimax).with_image_directory(images.to_owned()))
        .with(|images| OmpClient::new(omp).with_image_directory(images.to_owned()))
}

#[cfg(test)]
mod tests;
