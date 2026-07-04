//! `.env` bootstrap. Copies `.env.example` → `.env` on first init so the new
//! project has a place to fill in `APP__*` secrets. Existence-guarded: an
//! existing `.env` is never overwritten (idempotent).

use anyhow::Result;
use std::path::Path;

/// Ensure a `.env` exists under `root`, seeded from `.env.example`. Returns
/// whether a `.env` was (or would be, in `dry_run`) created.
pub fn ensure_env(root: &Path, dry_run: bool) -> Result<bool> {
    let example = root.join(".env.example");
    let target = root.join(".env");

    if target.exists() || !example.exists() {
        return Ok(false);
    }
    if !dry_run {
        std::fs::copy(&example, &target)?;
        // `.env` holds `APP__*` secrets — restrict it to the owner so it isn't
        // world-readable (the copy inherits `.env.example`'s broad perms). If the
        // chmod fails, delete the file so secrets aren't left world-readable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) =
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            {
                let _ = std::fs::remove_file(&target);
                return Err(e.into());
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_env_from_example() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env.example"), "APP__DEV_ENV=dev").unwrap();

        assert!(ensure_env(root, false).unwrap());
        assert_eq!(
            std::fs::read_to_string(root.join(".env")).unwrap(),
            "APP__DEV_ENV=dev"
        );
    }

    #[cfg(unix)]
    #[test]
    fn seeded_env_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env.example"), "APP__OPENAI_API_KEY=secret").unwrap();

        assert!(ensure_env(root, false).unwrap());
        let mode = std::fs::metadata(root.join(".env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "seeded .env must be owner-only");
    }

    #[test]
    fn does_not_clobber_existing_env() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env.example"), "APP__DEV_ENV=dev").unwrap();
        std::fs::write(root.join(".env"), "APP__DEV_ENV=prod").unwrap();

        assert!(!ensure_env(root, false).unwrap());
        assert_eq!(
            std::fs::read_to_string(root.join(".env")).unwrap(),
            "APP__DEV_ENV=prod"
        );
    }

    #[test]
    fn dry_run_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env.example"), "X=1").unwrap();
        assert!(ensure_env(root, true).unwrap());
        assert!(!root.join(".env").exists());
    }
}
