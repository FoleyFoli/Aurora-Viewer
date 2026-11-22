//! Persist only local organization, in a separate file that stock Aurora never rewrites.
use super::organized_model::Preferences;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

// v6 stores independent group order and per-location collapsed sections.
const CURRENT_VERSION: u32 = 6;

#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    change: Uuid,
    layout: Preferences,
}

#[derive(Default)]
pub(super) struct Store {
    account: Option<PathBuf>,
    root: Uuid,
    disk_change: Option<Uuid>,
    saved_revision: u64,
    pub(super) layout: Preferences,
    pub(super) error: Option<String>,
    recovery: Option<PathBuf>,
    retry: Option<std::time::Instant>,
}

fn read(path: &Path) -> io::Result<Option<Document>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut doc: Document = serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if !(1..=CURRENT_VERSION).contains(&doc.version) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, aurora_i18n::tr!("organized-version")));
    }
    if doc.version < 6 {
        doc.layout.migrate_group_direction();
    }
    Ok(Some(doc))
}

impl Store {
    pub(super) fn set_account(&mut self, path: PathBuf) {
        if self.account.as_ref() != Some(&path) {
            self.retry = None;
            self.save();
            *self = Self {
                account: Some(path),
                ..Default::default()
            };
        }
    }
    fn path(&self) -> Option<PathBuf> {
        self.account
            .as_ref()
            .map(|dir| dir.join(format!("inventory-explorer-{}.json", self.root)))
    }
    pub(super) fn enter(&mut self, root: Uuid) {
        if self.root == root {
            return;
        }
        self.save();
        *self = Self {
            account: self.account.take(),
            root,
            ..Self::default()
        };
        if !root.is_nil()
            && let Some(path) = self.path()
        {
            match read(&path) {
                Ok(Some(doc)) => {
                    self.disk_change = Some(doc.change);
                    self.layout = doc.layout;
                }
                Ok(None) => {}
                Err(e) => {
                    self.use_recovery(&path, Some(e));
                }
            }
        }
        self.saved_revision = self.layout.revision();
    }
    pub(super) fn save(&mut self) {
        if self.root.is_nil() || self.layout.revision() == self.saved_revision {
            return;
        }
        if self.retry.is_some_and(|t| t > std::time::Instant::now()) {
            return;
        }
        let Some(path) = self.path() else { return };
        let result = self.write(&path);
        match result {
            Ok(()) => {
                self.saved_revision = self.layout.revision();
                self.error = self
                    .recovery
                    .as_ref()
                    .map(|path| aurora_i18n::tr!("organized-conflict", path = path.display().to_string()).to_string());
                self.retry = None;
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.retry = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
            }
        }
    }
    fn use_recovery(&mut self, path: &Path, error: Option<io::Error>) {
        let recovery = path.with_extension(format!("{}.conflict.json", Uuid::new_v4()));
        let message = aurora_i18n::tr!("organized-conflict", path = recovery.display().to_string());
        self.error = Some(match error {
            Some(error) => format!("{error}. {message}"),
            None => message.to_string(),
        });
        self.recovery = Some(recovery);
    }
    fn write(&mut self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(path.with_extension("lock"))?;
        lock.try_lock().map_err(io::Error::other)?;
        if self.recovery.is_none() {
            match read(path) {
                Ok(doc) if doc.as_ref().map(|doc| doc.change) != self.disk_change => self.use_recovery(path, None),
                Err(e) if e.kind() == io::ErrorKind::InvalidData => self.use_recovery(path, Some(e)),
                Err(e) => return Err(e),
                Ok(_) => {}
            }
        }
        let change = Uuid::new_v4();
        let doc = Document {
            version: CURRENT_VERSION,
            change,
            layout: self.layout.clone(),
        };
        let bytes = serde_json::to_vec(&doc).map_err(io::Error::other)?;
        if let Some(copy) = &self.recovery {
            return crate::atomic_file::write(copy, &bytes);
        }
        crate::atomic_file::write(path, &bytes)?;
        self.disk_change = Some(change);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::inventory::organized_model::{Group, Location};
    use crate::world::inventory::Inventory;

    #[test]
    fn organized_store_keeps_root_scopes_and_both_concurrent_versions() {
        let dir = std::env::temp_dir().join(format!("aurora-explorer-{}", Uuid::new_v4()));
        let root = Uuid::new_v4();
        let open = || {
            let mut store = Store {
                account: Some(dir.clone()),
                ..Default::default()
            };
            store.enter(root);
            store
        };
        let mut first = open();
        let mut second = open();
        let inv = Inventory::default();
        let one = Location::Group(Group::Appearance);
        let two = Location::Group(Group::System);
        assert!(first.layout.unpin_location(&inv, one));
        first.save();
        assert!(first.error.is_none());
        assert!(!open().layout.pins(&inv).contains(&one));
        assert!(second.layout.unpin_location(&inv, two));
        second.save();
        let backup = second.recovery.clone().unwrap();
        assert!(read(&backup).unwrap().unwrap().layout.pins(&inv).contains(&one));
        assert!(!read(&backup).unwrap().unwrap().layout.pins(&inv).contains(&two));
        assert!(open().layout.pins(&inv).contains(&two));
        assert!(second.layout.unpin_location(&inv, one));
        second.save();
        assert!(!read(&backup).unwrap().unwrap().layout.pins(&inv).contains(&one));
        first.enter(Uuid::new_v4());
        assert!(first.layout.pins(&inv).contains(&one));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn organized_store_upgrades_a_v1_layout_on_edit_and_reopens_each_folder_view() {
        use crate::ui::inventory::organized_model::{DirectoryView, GroupBy, IconSize, SortKey, ViewLayout};
        let dir = std::env::temp_dir().join(format!("aurora-explorer-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let root = Uuid::new_v4();
        let path = dir.join(format!("inventory-explorer-{root}.json"));
        let inv = Inventory::default();
        let pin = Location::Group(Group::Appearance);
        let old_json = serde_json::json!({"shortcuts":[{"Place":{"Group":"System"}}],"folders":{},"placements":{}});
        let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"change":Uuid::new_v4(),"layout":old_json})).unwrap();
        fs::write(&path, &bytes).unwrap();
        let open = || {
            let mut store = Store {
                account: Some(dir.clone()),
                ..Default::default()
            };
            store.enter(root);
            store
        };
        let mut store = open();
        assert!(!store.layout.pins(&inv).contains(&pin));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let photo = Location::Folder(Uuid::new_v4());
        let style = DirectoryView {
            layout: ViewLayout::Icons(IconSize::ExtraLarge),
            sort: SortKey::Date,
            descending: true,
            group: GroupBy::Type,
            ..Default::default()
        };
        assert!(store.layout.set_view(photo, style));
        store.save();
        assert_eq!(read(&path).unwrap().unwrap().version, CURRENT_VERSION);
        let restart = open();
        assert_eq!(restart.layout.view(photo), style);
        assert_eq!(restart.layout.view(Location::MyFolders), DirectoryView::default());
        assert!(!restart.layout.pins(&inv).contains(&pin));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explorer_v5_migrates_type_group_order_and_persists_independent_choices_after_edit() {
        use crate::ui::inventory::organized_model::{GroupKey, NameBucket};
        let dir = std::env::temp_dir().join(format!("aurora-explorer-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let root = Uuid::new_v4();
        let path = dir.join(format!("inventory-explorer-{root}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version":5, "change":Uuid::new_v4(),
            "layout":{"views":[
                ["Overview", {"sort":"Type", "group":"Type", "descending":true}],
                ["Home", {"sort":"Name", "group":"Type", "descending":true}]
            ]}
        })).unwrap();
        fs::write(&path, &bytes).unwrap();
        let open = || {
            let mut store = Store { account: Some(dir.clone()), ..Default::default() };
            store.enter(root);
            store
        };
        let mut store = open();
        let mut view = store.layout.view(Location::Home);
        assert!(view.group_descending);
        assert!(!store.layout.view(Location::MyFolders).group_descending);
        store.save();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        view.group_descending = false;
        store.layout.set_view(Location::Home, view);
        let key = GroupKey::Name(NameBucket::ItoP);
        store.layout.set_group_collapsed(Location::Home, &key, true);
        store.save();
        let restart = open();
        assert_eq!(restart.layout.view(Location::Home), view);
        assert!(restart.layout.group_collapsed(Location::Home, &key));
        assert!(!restart.layout.group_collapsed(Location::MyFolders, &key));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn organized_store_upgrades_v2_flat_pins_only_after_edit_and_reopens_the_group() {
        let dir = std::env::temp_dir().join(format!("aurora-explorer-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let root = Uuid::new_v4();
        let path = dir.join(format!("inventory-explorer-{root}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version":2,"change":Uuid::new_v4(),
            "layout":{"shortcuts":[{"Place":{"Group":"System"}},{"Place":{"Group":"Appearance"}}],"views":[]}
        })).unwrap();
        fs::write(&path, &bytes).unwrap();
        let open = || {
            let mut store = Store { account: Some(dir.clone()), ..Default::default() };
            store.enter(root);
            store
        };
        let inv = Inventory::default();
        let appearance = Location::Group(Group::Appearance);
        let mut store = open();
        assert!(store.layout.pin_groups().is_empty());
        assert_eq!(store.layout.pins(&inv), [Location::Group(Group::System), appearance]);
        store.save();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let id = store.layout.create_pin_group("Build").unwrap();
        assert!(store.layout.move_pin_to_group(&inv, appearance, Some(id)));
        store.layout.toggle_pin_group(Some(id));
        store.save();
        assert_eq!(read(&path).unwrap().unwrap().version, CURRENT_VERSION);
        let restart = open();
        let groups = restart.layout.pin_sections(&inv);
        assert_eq!(groups[0].0.as_ref().unwrap().id, id);
        assert!(groups[0].0.as_ref().unwrap().collapsed);
        assert_eq!(groups[0].1, [appearance]);
        assert_eq!(groups[1].1, [Location::Group(Group::System)]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn organized_store_preserves_an_unreadable_document() {
        let dir = std::env::temp_dir().join(format!("aurora-explorer-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let root = Uuid::new_v4();
        let path = dir.join(format!("inventory-explorer-{root}.json"));
        fs::write(&path, b"old data").unwrap();
        let mut store = Store {
            account: Some(dir.clone()),
            ..Default::default()
        };
        store.enter(root);
        assert!(store.error.is_some());
        assert!(
            store
                .layout
                .unpin_location(&Inventory::default(), Location::Group(Group::Appearance))
        );
        store.save();
        assert_eq!(fs::read(&path).unwrap(), b"old data");
        assert!(read(store.recovery.as_ref().unwrap()).unwrap().is_some());
        fs::remove_dir_all(dir).unwrap();
    }
}
