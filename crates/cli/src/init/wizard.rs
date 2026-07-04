//! Interactive onboarding flow (dialoguer). Bare `appctl init` (no headless
//! flags) runs this to build a [`Config`] by prompting for branding, the
//! project profile, and the optional extras. Requires a TTY; headless callers
//! pass `--profile`/`--config` instead and never reach here.

use super::config::{Config, Profile, Surface};
use anyhow::Result;
use dialoguer::{Confirm, Input, Select};

/// Run the wizard, returning an un-expanded [`Config`] (the caller expands and
/// plans). `defaults` seeds the prompts (e.g. org auto-detected from git).
pub fn run(defaults: &Config) -> Result<Config> {
    println!("appctl init — interactive setup\n");

    let project_name: String = Input::new()
        .with_prompt("Project name")
        .default(defaults.project_name.clone())
        .interact_text()?;

    let cli_name: String = Input::new()
        .with_prompt("CLI binary name")
        .default(defaults.cli_name.clone())
        .interact_text()?;

    let org: String = Input::new()
        .with_prompt("GitHub owner / org")
        .default(defaults.org.clone())
        .interact_text()?;

    let description: String = Input::new()
        .with_prompt("One-line description")
        .default(defaults.description.clone())
        .interact_text()?;

    let profile_idx = Select::new()
        .with_prompt("Project shape")
        .items(
            Profile::ALL
                .iter()
                .map(|p| profile_label(*p))
                .collect::<Vec<_>>(),
        )
        .default(profile_default_idx(defaults.profile))
        .interact()?;
    let profile = Profile::ALL[profile_idx];

    let mut config = Config::from_profile(profile);
    config.project_name = project_name;
    config.cli_name = cli_name;
    config.org = org;
    config.description = description;

    // Optional extras, only meaningful when the HTTP API is present.
    if config.has_surface(Surface::HttpApi) {
        config.frontend = Confirm::new()
            .with_prompt("Include the optional React/Vite frontend?")
            .default(config.frontend)
            .interact()?;
        config.docker = Confirm::new()
            .with_prompt("Include a Dockerfile for the server?")
            .default(config.docker)
            .interact()?;
    }
    config.docs = Confirm::new()
        .with_prompt("Keep the docs site (docs/)?")
        .default(config.docs)
        .interact()?;

    Ok(config)
}

/// Confirm a mutating apply after the plan is shown. Returns the user's choice.
pub fn confirm_apply() -> Result<bool> {
    Ok(Confirm::new()
        .with_prompt("Apply this plan? This mutates files in place")
        .default(false)
        .interact()?)
}

fn profile_label(p: Profile) -> String {
    match p {
        Profile::CliOnly => "cli-only     — CLI diagnostics, no server".to_string(),
        Profile::ServerOnly => "server-only  — HTTP API, no CLI diagnostics".to_string(),
        Profile::CliServer => "cli+server   — both (template default)".to_string(),
    }
}

fn profile_default_idx(p: Profile) -> usize {
    Profile::ALL.iter().position(|x| *x == p).unwrap_or(2)
}
