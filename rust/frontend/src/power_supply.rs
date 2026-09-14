// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Battery capacity from the kernel's power-supply class, for the header
// HUD on a Steam Deck or a laptop. The `MiSTer` side of the same HUD
// reading lives in `mister_battery.rs` and talks to an optional `SMBus`
// fuel gauge; only the probe differs, `SystemStatus` and the header
// consume both the same way.
//
// The one trap here is that a battery in `/sys/class/power_supply` is not
// necessarily *this machine's* battery. A Steam Deck with a DualSense
// paired reports the pad alongside its own cell:
//
//   BAT1                              type=Battery              capacity=96
//   ps-controller-battery-00:01:...   type=Battery scope=Device capacity=100
//
// so a naive "first Battery wins" reads whichever the kernel happened to
// enumerate first and shows the controller's charge in the status bar.
// `scope` is what separates them: `Device` means the supply belongs to a
// peripheral, and an absent `scope` means the system, per the kernel's
// own power_supply sysfs documentation.

use std::path::Path;

const CLASS_DIR: &str = "/sys/class/power_supply";

/// One entry under `/sys/class/power_supply`, as the sysfs files spell
/// it. Attributes are optional because the set a driver exposes varies:
/// the Deck's `BAT1` has no `scope` at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Supply<'a> {
    name: &'a str,
    kind: &'a str,
    scope: Option<&'a str>,
    present: Option<&'a str>,
    capacity: Option<&'a str>,
}

impl Supply<'_> {
    /// A battery wired into this machine, as opposed to a mains adapter
    /// or a peripheral's cell. An absent `scope` means system scope.
    fn is_system_battery(&self) -> bool {
        self.kind == "Battery"
            && self.scope.is_none_or(|scope| scope == "System")
            && self.present.is_none_or(|present| present == "1")
    }
}

/// Capacity is documented as a percentage, so anything outside 0-100 is a
/// driver bug or a placeholder rather than a reading worth showing.
fn parse_capacity(raw: &str) -> Option<u8> {
    let value = raw.trim().parse::<i32>().ok()?;
    u8::try_from(value).ok().filter(|percent| *percent <= 100)
}

/// The reading for the HUD. With more than one system battery (a laptop
/// with a second cell in the bay) this takes the first by name rather
/// than averaging: the two are rarely the same size, so an unweighted
/// mean would be a made-up number, and every device this actually runs on
/// has one.
fn system_capacity(supplies: &[Supply<'_>]) -> Option<u8> {
    let mut batteries: Vec<&Supply<'_>> =
        supplies.iter().filter(|s| s.is_system_battery()).collect();
    batteries.sort_by_key(|s| s.name);
    batteries
        .into_iter()
        .find_map(|s| s.capacity.and_then(parse_capacity))
}

fn read_attribute(dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(file))
        .ok()
        .map(|value| value.trim().to_string())
}

/// Battery capacity percentage (0-100), or `None` on a machine with no
/// battery, which is the common case on a desktop.
pub fn read_capacity_percent() -> Option<u8> {
    let entries = std::fs::read_dir(CLASS_DIR).ok()?;
    let mut owned = Vec::new();
    for entry in entries.flatten() {
        // One unreadable name must not abandon the scan: the entry we
        // want may still be further down the directory.
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let dir = entry.path();
        let Some(kind) = read_attribute(&dir, "type") else {
            continue;
        };
        owned.push((
            name,
            kind,
            read_attribute(&dir, "scope"),
            read_attribute(&dir, "present"),
            read_attribute(&dir, "capacity"),
        ));
    }
    let supplies: Vec<Supply<'_>> = owned
        .iter()
        .map(|(name, kind, scope, present, capacity)| Supply {
            name,
            kind,
            scope: scope.as_deref(),
            present: present.as_deref(),
            capacity: capacity.as_deref(),
        })
        .collect();
    system_capacity(&supplies)
}

#[cfg(test)]
mod tests {
    use super::{parse_capacity, system_capacity, Supply};

    fn supply<'a>(
        name: &'a str,
        kind: &'a str,
        scope: Option<&'a str>,
        capacity: Option<&'a str>,
    ) -> Supply<'a> {
        Supply {
            name,
            kind,
            scope,
            present: Some("1"),
            capacity,
        }
    }

    #[test]
    fn a_paired_controller_does_not_become_the_system_battery() {
        // Exactly what a Steam Deck with a DualSense paired reports, in
        // the order the kernel enumerated it.
        let supplies = [
            supply("ACAD", "Mains", None, None),
            supply("BAT1", "Battery", None, Some("96")),
            supply(
                "ps-controller-battery-00:01:6c:ee:95:de",
                "Battery",
                Some("Device"),
                Some("100"),
            ),
        ];
        assert_eq!(system_capacity(&supplies), Some(96));
    }

    #[test]
    fn a_controller_alone_reads_as_no_battery() {
        let supplies = [supply(
            "ps-controller-battery-00:01:6c:ee:95:de",
            "Battery",
            Some("Device"),
            Some("100"),
        )];
        assert_eq!(system_capacity(&supplies), None);
    }

    #[test]
    fn an_explicit_system_scope_counts_the_same_as_an_absent_one() {
        let supplies = [supply("BAT0", "Battery", Some("System"), Some("42"))];
        assert_eq!(system_capacity(&supplies), Some(42));
    }

    #[test]
    fn an_absent_battery_bay_is_skipped() {
        let empty = Supply {
            name: "BAT1",
            kind: "Battery",
            scope: None,
            present: Some("0"),
            capacity: Some("0"),
        };
        let full = supply("BAT2", "Battery", None, Some("55"));
        assert_eq!(system_capacity(&[empty, full]), Some(55));
    }

    #[test]
    fn two_system_batteries_take_the_first_by_name() {
        let supplies = [
            supply("BAT1", "Battery", None, Some("30")),
            supply("BAT0", "Battery", None, Some("70")),
        ];
        assert_eq!(system_capacity(&supplies), Some(70));
    }

    #[test]
    fn a_desktop_with_only_mains_has_no_reading() {
        let supplies = [supply("ACAD", "Mains", None, None)];
        assert_eq!(system_capacity(&supplies), None);
    }

    #[test]
    fn a_battery_that_reports_no_capacity_is_not_a_reading() {
        let supplies = [supply("BAT1", "Battery", None, None)];
        assert_eq!(system_capacity(&supplies), None);
    }

    #[test]
    fn capacity_outside_the_documented_range_is_rejected() {
        assert_eq!(parse_capacity("0"), Some(0));
        assert_eq!(parse_capacity(" 100 \n"), Some(100));
        assert_eq!(parse_capacity("101"), None);
        assert_eq!(parse_capacity("-1"), None);
        assert_eq!(parse_capacity(""), None);
        assert_eq!(parse_capacity("unknown"), None);
    }
}
