// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::client::{Client, ClientError};
use crate::media_types::{SystemDefault, UpdateSettingsParams};
use crate::store::{Mutation, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct SetSystemLauncherDefaultArgs {
    pub system_id: String,
    pub launcher: String,
}

#[derive(Debug)]
pub struct SetSystemLauncherDefaultMutation;

impl Mutation for SetSystemLauncherDefaultMutation {
    type Args = SetSystemLauncherDefaultArgs;
    type Output = ();

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            let settings = client.settings().await?;
            let merged = merge_system_launcher_default(
                settings.system_defaults,
                &args.system_id,
                &args.launcher,
            );
            client
                .settings_update(UpdateSettingsParams {
                    system_defaults: Some(merged),
                })
                .await
        })
    }

    fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        vec![Tag::any("Settings")]
    }
}

pub fn merge_system_launcher_default(
    mut defaults: Vec<SystemDefault>,
    system_id: &str,
    launcher: &str,
) -> Vec<SystemDefault> {
    let launcher = launcher.trim();
    if let Some(existing) = defaults.iter_mut().find(|d| d.system == system_id) {
        existing.launcher = launcher.to_string();
    } else if !launcher.is_empty() {
        defaults.push(SystemDefault {
            system: system_id.to_string(),
            launcher: launcher.to_string(),
            before_exit: String::new(),
        });
    }

    defaults
        .retain(|d| !(d.system == system_id && d.launcher.is_empty() && d.before_exit.is_empty()));
    defaults
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default(system: &str, launcher: &str, before_exit: &str) -> SystemDefault {
        SystemDefault {
            system: system.into(),
            launcher: launcher.into(),
            before_exit: before_exit.into(),
        }
    }

    #[test]
    fn merge_replaces_launcher_and_preserves_before_exit() {
        let merged = merge_system_launcher_default(
            vec![default("SNES", "snes9x", "echo bye")],
            "SNES",
            "retroarch",
        );
        assert_eq!(merged, vec![default("SNES", "retroarch", "echo bye")]);
    }

    #[test]
    fn merge_appends_non_empty_launcher() {
        let merged = merge_system_launcher_default(vec![], "SNES", "snes9x");
        assert_eq!(merged, vec![default("SNES", "snes9x", "")]);
    }

    #[test]
    fn merge_default_removes_row_without_before_exit() {
        let merged = merge_system_launcher_default(vec![default("SNES", "snes9x", "")], "SNES", "");
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_default_keeps_row_with_before_exit() {
        let merged =
            merge_system_launcher_default(vec![default("SNES", "snes9x", "echo bye")], "SNES", "");
        assert_eq!(merged, vec![default("SNES", "", "echo bye")]);
    }
}
