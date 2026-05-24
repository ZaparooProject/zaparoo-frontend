// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::warn;
use zaparoo_core::endpoints::launchers::LaunchersEndpoint;
use zaparoo_core::endpoints::settings::SettingsEndpoint;
use zaparoo_core::endpoints::system_launcher_default::{
    SetSystemLauncherDefaultArgs, SetSystemLauncherDefaultMutation,
};
use zaparoo_core::media_types::{LauncherInfo, SettingsResult, SystemDefault};
use zaparoo_core::remote_resource::ResourceStatus;

const DEFAULT_LAUNCHER_ID: &str = "__default__";

#[allow(
    clippy::struct_excessive_bools,
    reason = "QML state bag tracks independent endpoint/update flags"
)]
#[derive(Default)]
pub struct SystemLaunchersRust {
    loading: bool,
    error_message: QString,
    update_pending: bool,
    update_error: QString,
    picker_ids: QStringList,
    picker_labels: QStringList,
    current_launcher: QString,
    launchers: Vec<LauncherInfo>,
    system_defaults: Vec<SystemDefault>,
    launchers_loading: bool,
    settings_loading: bool,
    launchers_error: String,
    settings_error: String,
    update_seq: Arc<AtomicU64>,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, loading)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, update_pending)]
        #[qproperty(QString, update_error)]
        #[qproperty(QStringList, picker_ids)]
        #[qproperty(QStringList, picker_labels)]
        #[qproperty(QString, current_launcher)]
        type SystemLaunchers = super::SystemLaunchersRust;

        #[qinvokable]
        fn prepare_system(self: Pin<&mut SystemLaunchers>, system_id: QString);

        #[qinvokable]
        fn launcher_count_for_system(self: &SystemLaunchers, system_id: &QString) -> i32;

        #[qinvokable]
        fn set_system_launcher(
            self: Pin<&mut SystemLaunchers>,
            system_id: QString,
            launcher_id: QString,
        );
    }

    impl cxx_qt::Initialize for SystemLaunchers {}
    impl cxx_qt::Threading for SystemLaunchers {}
}

impl Initialize for ffi::SystemLaunchers {
    fn initialize(mut self: Pin<&mut Self>) {
        let store = global_store();
        let mut launchers_rx = store.subscribe::<LaunchersEndpoint>(()).subscribe();
        let mut settings_rx = store.subscribe::<SettingsEndpoint>(()).subscribe();

        apply_launchers_state(
            self.as_mut(),
            project_launchers(&launchers_rx.borrow_and_update()),
        );
        apply_settings_state(
            self.as_mut(),
            project_settings(&settings_rx.borrow_and_update()),
        );

        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            while launchers_rx.changed().await.is_ok() {
                let projected = project_launchers(&launchers_rx.borrow_and_update());
                let _ = qt_thread.queue(move |model| apply_launchers_state(model, projected));
            }
        });

        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            while settings_rx.changed().await.is_ok() {
                let projected = project_settings(&settings_rx.borrow_and_update());
                let _ = qt_thread.queue(move |model| apply_settings_state(model, projected));
            }
        });
    }
}

fn project_launchers(
    status: &ResourceStatus<zaparoo_core::media_types::LaunchersResult>,
) -> (Option<Vec<LauncherInfo>>, bool, String) {
    match status {
        ResourceStatus::Ready(data) => (Some(data.launchers.clone()), false, String::new()),
        ResourceStatus::Errored { message, .. } => (None, false, message.clone()),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, true, String::new()),
    }
}

fn project_settings(
    status: &ResourceStatus<SettingsResult>,
) -> (Option<Vec<SystemDefault>>, bool, String) {
    match status {
        ResourceStatus::Ready(data) => (Some(data.system_defaults.clone()), false, String::new()),
        ResourceStatus::Errored { message, .. } => (None, false, message.clone()),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, true, String::new()),
    }
}

fn apply_launchers_state(
    mut model: Pin<&mut ffi::SystemLaunchers>,
    (launchers, loading, error): (Option<Vec<LauncherInfo>>, bool, String),
) {
    if let Some(launchers) = launchers {
        model.as_mut().rust_mut().launchers = launchers;
    }
    model.as_mut().rust_mut().launchers_loading = loading;
    model.as_mut().rust_mut().launchers_error = error;
    refresh_status(model);
}

fn apply_settings_state(
    mut model: Pin<&mut ffi::SystemLaunchers>,
    (defaults, loading, error): (Option<Vec<SystemDefault>>, bool, String),
) {
    if let Some(defaults) = defaults {
        model.as_mut().rust_mut().system_defaults = defaults;
    }
    model.as_mut().rust_mut().settings_loading = loading;
    model.as_mut().rust_mut().settings_error = error;
    refresh_status(model);
}

fn refresh_status(mut model: Pin<&mut ffi::SystemLaunchers>) {
    let loading = model.rust().launchers_loading || model.rust().settings_loading;
    if model.loading != loading {
        model.as_mut().set_loading(loading);
    }
    let error = if model.rust().launchers_error.is_empty() {
        model.rust().settings_error.clone()
    } else {
        model.rust().launchers_error.clone()
    };
    let qerr = QString::from(error.as_str());
    if model.error_message != qerr {
        model.as_mut().set_error_message(qerr);
    }
}

impl ffi::SystemLaunchers {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn prepare_system(mut self: Pin<&mut Self>, system_id: QString) {
        let system_id = system_id.to_string();
        let current = current_launcher_for_system(&self.system_defaults, &system_id)
            .unwrap_or_else(|| DEFAULT_LAUNCHER_ID.to_string());
        let entries = picker_entries_for_system(&self.launchers, &self.system_defaults, &system_id);
        let mut ids = QStringList::default();
        let mut labels = QStringList::default();
        for entry in entries {
            ids.append(QString::from(entry.id.as_str()));
            labels.append(QString::from(entry.label.as_str()));
        }
        self.as_mut().set_picker_ids(ids);
        self.as_mut().set_picker_labels(labels);
        self.as_mut()
            .set_current_launcher(QString::from(current.as_str()));
    }

    fn launcher_count_for_system(&self, system_id: &QString) -> i32 {
        launchers_for_system(&self.launchers, &system_id.to_string()).len() as i32
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_system_launcher(mut self: Pin<&mut Self>, system_id: QString, launcher_id: QString) {
        let launcher = launcher_id.to_string();
        let launcher = if launcher == DEFAULT_LAUNCHER_ID {
            String::new()
        } else {
            launcher
        };
        let system_id = system_id.to_string();
        let store = global_store();
        let seq = self.rust().update_seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.as_mut().set_update_error(QString::default());
        self.as_mut().set_update_pending(true);
        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            let result = store
                .run_mutation::<SetSystemLauncherDefaultMutation>(SetSystemLauncherDefaultArgs {
                    system_id,
                    launcher,
                })
                .await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                let error = match result {
                    Ok(()) => QString::default(),
                    Err(e) => {
                        warn!("system launcher update failed: {}", e.message);
                        QString::from(e.message.as_str())
                    }
                };
                model.as_mut().set_update_error(error);
                model.as_mut().set_update_pending(false);
            });
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry {
    pub id: String,
    pub label: String,
}

pub fn launchers_for_system(launchers: &[LauncherInfo], system_id: &str) -> Vec<LauncherInfo> {
    launchers
        .iter()
        .filter(|launcher| launcher.system_id == system_id)
        .cloned()
        .collect()
}

pub fn current_launcher_for_system(defaults: &[SystemDefault], system_id: &str) -> Option<String> {
    defaults
        .iter()
        .find(|default| default.system == system_id)
        .and_then(|default| {
            if default.launcher.is_empty() {
                None
            } else {
                Some(default.launcher.clone())
            }
        })
}

pub fn picker_entries_for_system(
    launchers: &[LauncherInfo],
    defaults: &[SystemDefault],
    system_id: &str,
) -> Vec<PickerEntry> {
    let mut entries = vec![PickerEntry {
        id: DEFAULT_LAUNCHER_ID.into(),
        label: "Default".into(),
    }];
    let matching = launchers_for_system(launchers, system_id);
    for launcher in &matching {
        entries.push(PickerEntry {
            id: launcher.id.clone(),
            label: launcher.id.clone(),
        });
    }
    if let Some(current) = current_launcher_for_system(defaults, system_id) {
        let known = matching.iter().any(|launcher| launcher.id == current);
        if !known {
            entries.push(PickerEntry {
                id: current.clone(),
                label: format!("Current: {current}"),
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(id: &str, system_id: &str) -> LauncherInfo {
        LauncherInfo {
            id: id.into(),
            system_id: system_id.into(),
            ..LauncherInfo::default()
        }
    }

    fn default(system: &str, launcher: &str) -> SystemDefault {
        SystemDefault {
            system: system.into(),
            launcher: launcher.into(),
            before_exit: String::new(),
        }
    }

    #[test]
    fn filters_launchers_by_exact_system_id() {
        let launchers = vec![launcher("snes9x", "SNES"), launcher("nestopia", "NES")];
        let filtered = launchers_for_system(&launchers, "SNES");
        assert_eq!(filtered, vec![launcher("snes9x", "SNES")]);
    }

    #[test]
    fn picker_entries_put_default_first() {
        let entries = picker_entries_for_system(&[launcher("snes9x", "SNES")], &[], "SNES");
        assert_eq!(entries[0].id, DEFAULT_LAUNCHER_ID);
        assert_eq!(entries[0].label, "Default");
        assert_eq!(entries[1].id, "snes9x");
    }

    #[test]
    fn picker_entries_include_unknown_current_override() {
        let entries = picker_entries_for_system(
            &[launcher("snes9x", "SNES")],
            &[default("SNES", "libretro")],
            "SNES",
        );
        assert_eq!(
            entries,
            vec![
                PickerEntry {
                    id: DEFAULT_LAUNCHER_ID.into(),
                    label: "Default".into()
                },
                PickerEntry {
                    id: "snes9x".into(),
                    label: "snes9x".into()
                },
                PickerEntry {
                    id: "libretro".into(),
                    label: "Current: libretro".into()
                },
            ]
        );
    }
}
