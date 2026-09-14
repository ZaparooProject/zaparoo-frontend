//! Explicit text boundaries for UI enums. Disk/API vocabulary stays unchanged.

use crate::{ErrorKind, Orientation, Screen, SettingsPage, VideoStandard};

macro_rules! tokens {
    ($ty:ident { $($variant:ident => $token:literal),+ $(,)? }) => {
        impl $ty {
            pub fn token(self) -> &'static str {
                match self { $(Self::$variant => $token),+ }
            }
        }
        impl TryFrom<&str> for $ty {
            type Error = &'static str;
            fn try_from(token: &str) -> Result<Self, Self::Error> {
                match token {
                    $($token => Ok(Self::$variant),)+
                    _ => Err(concat!("unknown ", stringify!($ty), " token")),
                }
            }
        }
    };
}

tokens!(Screen {
    Hub => "hub", Systems => "systems", FavoriteSystems => "favorite-systems",
    Games => "games", Favorites => "favorites", Recents => "recents",
    Settings => "settings", About => "about", None => "",
});
tokens!(SettingsPage {
    Root => "", Appearance => "pageAppearance", Library => "pageLibraryData",
    Display => "pageDisplayInterface", Controls => "pageControlsInput",
    Language => "pageLanguage", About => "pageSupportAbout",
});
tokens!(Orientation { Horizontal => "horizontal", Cw => "cw", Ccw => "ccw" });
tokens!(VideoStandard { Ntsc => "ntsc", Pal => "pal" });
tokens!(ErrorKind {
    Generic => "", Launch => "launch", Favorite => "favorite", AddToHub => "add_to_hub",
    MediaIndex => "media_index", MediaScrape => "media_scrape", MediaScrapers => "media_scrapers",
    MediaCancel => "media_cancel", Launcher => "launcher", LauncherSave => "launcher_save",
    AlternateDiscovery => "alternate_discovery", QrCode => "qr_code", CardWrite => "card_write", Setting => "setting",
});

impl From<zaparoo_app::hub::Reason> for crate::DisabledReason {
    fn from(value: zaparoo_app::hub::Reason) -> Self {
        match value {
            zaparoo_app::hub::Reason::None => Self::None,
            zaparoo_app::hub::Reason::NoRecentGames => Self::NoRecentGames,
            zaparoo_app::hub::Reason::NoInternet => Self::NoInternet,
            zaparoo_app::hub::Reason::NotAvailable => Self::NotAvailable,
        }
    }
}

impl From<zaparoo_app::settings::Control> for crate::ControlKind {
    fn from(value: zaparoo_app::settings::Control) -> Self {
        use zaparoo_app::settings::Control;
        match value {
            Control::Picker => Self::Picker,
            Control::Toggle => Self::Toggle,
            Control::Action => Self::Action,
            Control::Navigate => Self::Navigate,
        }
    }
}

impl From<zaparoo_app::media_setup::ScopeKind> for crate::ScopeKind {
    fn from(value: zaparoo_app::media_setup::ScopeKind) -> Self {
        match value {
            zaparoo_app::media_setup::ScopeKind::All => Self::All,
            zaparoo_app::media_setup::ScopeKind::Category => Self::Category,
            zaparoo_app::media_setup::ScopeKind::System => Self::System,
        }
    }
}

impl From<zaparoo_app::media_setup::Kind> for crate::SetupKind {
    fn from(value: zaparoo_app::media_setup::Kind) -> Self {
        match value {
            zaparoo_app::media_setup::Kind::Index => Self::Index,
            zaparoo_app::media_setup::Kind::Scrape => Self::Scrape,
        }
    }
}

impl From<zaparoo_app::log_upload::Phase> for crate::LogPhase {
    fn from(value: zaparoo_app::log_upload::Phase) -> Self {
        match value {
            zaparoo_app::log_upload::Phase::Uploading => Self::Uploading,
            zaparoo_app::log_upload::Phase::Done => Self::Done,
            zaparoo_app::log_upload::Phase::Failed => Self::Failed,
        }
    }
}

impl From<zaparoo_app::status_line::Kind> for crate::StatusKind {
    fn from(value: zaparoo_app::status_line::Kind) -> Self {
        use zaparoo_app::status_line::Kind;
        match value {
            Kind::None => Self::None,
            Kind::Disconnected => Self::Disconnected,
            Kind::Reconnecting => Self::Reconnecting,
            Kind::Connecting => Self::Connecting,
            Kind::CoreError => Self::CoreError,
            Kind::IndexingPaused => Self::IndexingPaused,
            Kind::Indexing => Self::Indexing,
            Kind::Optimizing => Self::Optimizing,
            Kind::ImportingPaused => Self::ImportingPaused,
            Kind::Importing => Self::Importing,
            Kind::IndexedFiles => Self::IndexedFiles,
            Kind::IndexingComplete => Self::IndexingComplete,
            Kind::ImportFailed => Self::ImportFailed,
            Kind::Imported => Self::Imported,
            Kind::PlaytimeWarning => Self::PlaytimeWarning,
            Kind::Inbox => Self::Inbox,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_vocabulary_round_trips_and_unknowns_are_rejected() {
        for value in [
            Screen::Hub,
            Screen::Systems,
            Screen::FavoriteSystems,
            Screen::Games,
            Screen::Favorites,
            Screen::Recents,
            Screen::Settings,
            Screen::About,
            Screen::None,
        ] {
            assert_eq!(Screen::try_from(value.token()), Ok(value));
        }
        for page in [
            SettingsPage::Root,
            SettingsPage::Appearance,
            SettingsPage::Library,
            SettingsPage::Display,
            SettingsPage::Controls,
            SettingsPage::Language,
            SettingsPage::About,
        ] {
            assert_eq!(SettingsPage::try_from(page.token()), Ok(page));
        }
        for orientation in [Orientation::Horizontal, Orientation::Cw, Orientation::Ccw] {
            assert_eq!(Orientation::try_from(orientation.token()), Ok(orientation));
        }
        for standard in [VideoStandard::Ntsc, VideoStandard::Pal] {
            assert_eq!(VideoStandard::try_from(standard.token()), Ok(standard));
        }
        assert!(Screen::try_from("unknown").is_err());
        assert!(SettingsPage::try_from("unknown").is_err());
        assert!(Orientation::try_from("unknown").is_err());
        assert!(VideoStandard::try_from("unknown").is_err());
        assert_eq!(SettingsPage::Root.token(), "");
    }

    #[test]
    fn registry_and_error_vocabulary_have_typed_projections() {
        for page in zaparoo_app::settings::PAGES {
            assert!(
                SettingsPage::try_from(page.id).is_ok(),
                "unmapped page {}",
                page.id
            );
        }
        for token in zaparoo_app::action_error::KINDS {
            assert_eq!(ErrorKind::try_from(token).map(ErrorKind::token), Ok(token));
        }
        // The shared error queue intentionally accepts future kinds; UI uses generic copy.
        assert_eq!(
            ErrorKind::try_from("future-kind").unwrap_or(ErrorKind::Generic),
            ErrorKind::Generic
        );
    }
}
