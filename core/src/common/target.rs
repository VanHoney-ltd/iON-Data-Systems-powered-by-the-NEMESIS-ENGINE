use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePath {
    pub domain: String,
    pub relative_path: String,
    pub kind: CandidateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    Primary,
    Alternate,
}

#[derive(Debug, Clone)]
pub struct KnownTarget {
    pub artifact_key: &'static str,
    pub clean_file_name: &'static str,
    pub candidates: Vec<CandidatePath>,
    pub sqlite_like: bool,
}

impl KnownTarget {
    pub fn clean_path(&self) -> PathBuf {
        PathBuf::from(self.clean_file_name)
    }
}

fn candidate(domain: &str, relative_path: &str, kind: CandidateKind) -> CandidatePath {
    CandidatePath {
        domain: domain.to_string(),
        relative_path: relative_path.to_string(),
        kind,
    }
}

pub fn sms_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "sms",
        clean_file_name: "Library/SMS/sms.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/SMS/sms.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn modern_notes_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "modern_notes",
        clean_file_name: "NoteStore.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "AppDomainGroup-group.com.apple.notes",
                "NoteStore.sqlite",
                CandidateKind::Primary,
            ),
            candidate(
                "HomeDomain",
                "Library/Notes/NoteStore.sqlite",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn legacy_notes_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "legacy_notes",
        clean_file_name: "Library/Notes/notes.sqlite",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Notes/notes.sqlite",
            CandidateKind::Primary,
        )],
    }
}

pub fn call_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "call_history",
        clean_file_name: "Library/CallHistoryDB/CallHistory.storedata",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/CallHistoryDB/CallHistory.storedata",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_history",
        clean_file_name: "Library/Safari/History.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Safari/History.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_bookmarks_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_bookmarks",
        clean_file_name: "Library/Safari/Bookmarks.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Safari/Bookmarks.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn safari_tabs_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "safari_tabs",
        clean_file_name: "Library/Safari/SafariTabs.db",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "HomeDomain",
                "Library/Safari/SafariTabs.db",
                CandidateKind::Primary,
            ),
            candidate(
                "HomeDomain",
                "Library/Safari/iCloudTabs.db",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn voicemail_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "voicemail",
        clean_file_name: "Library/Voicemail/voicemail.db",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/Voicemail/voicemail.db",
            CandidateKind::Primary,
        )],
    }
}

pub fn photos_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "photos",
        clean_file_name: "Media/PhotoData/Photos.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate(
                "CameraRollDomain",
                "Media/PhotoData/Photos.sqlite",
                CandidateKind::Primary,
            ),
            candidate(
                "MediaDomain",
                "PhotoData/Photos.sqlite",
                CandidateKind::Alternate,
            ),
        ],
    }
}

pub fn addressbook_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "addressbook",
        clean_file_name: "Library/AddressBook/AddressBook.sqlitedb",
        sqlite_like: true,
        candidates: vec![candidate(
            "HomeDomain",
            "Library/AddressBook/AddressBook.sqlitedb",
            CandidateKind::Primary,
        )],
    }
}

pub fn apple_maps_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_history",
        clean_file_name: "History.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "History.sqlite", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "History.sqlite", CandidateKind::Alternate),
        ],
    }
}

pub fn apple_maps_cloud_history_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_cloud_history",
        clean_file_name: "CloudHistory.syncedDB",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "CloudHistory.syncedDB", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "CloudHistory.syncedDB", CandidateKind::Alternate),
        ],
    }
}

pub fn apple_maps_geo_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "apple_maps_geo",
        clean_file_name: "geo.db",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.apple.Maps", "geo.db", CandidateKind::Primary),
            candidate("AppDomainGroup-group.com.apple.Maps", "geo.db", CandidateKind::Alternate),
        ],
    }
}

pub fn google_maps_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "google_maps",
        clean_file_name: "Session.sqlite",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.google.Maps", "Library/Application Support/GoogleMaps/Session.sqlite", CandidateKind::Primary),
            candidate("AppDomain-com.google.Maps", "Library/Application Support/DataStore/Session.sqlite", CandidateKind::Alternate),
            candidate("AppDomain-com.google.Maps", "Documents/OSCacheData", CandidateKind::Alternate),
        ],
    }
}

pub fn snapchat_maps_target() -> KnownTarget {
    KnownTarget {
        artifact_key: "snapchat_maps",
        clean_file_name: "scmap.db",
        sqlite_like: true,
        candidates: vec![
            candidate("AppDomain-com.toyopagroup.picaboo", "Documents/scmap.db", CandidateKind::Primary),
            candidate("AppDomain-com.toyopagroup.picaboo", "scmap.db", CandidateKind::Alternate),
            candidate("AppDomain-com.toyopagroup.picaboo", "Library/Caches/scmap.db", CandidateKind::Alternate),
        ],
    }
}

pub fn chronos_targets() -> Vec<KnownTarget> {
    vec![
        sms_target(),
        call_history_target(),
        modern_notes_target(),
        legacy_notes_target(),
        safari_history_target(),
        safari_bookmarks_target(),
        safari_tabs_target(),
        voicemail_target(),
        photos_target(),
        addressbook_target(),
    ]
}
