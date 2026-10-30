//! `.fcproj` save/open through the command interface: schema envelope, legacy upgrade, Save a Copy,
//! Revert.

use super::*;
use serde_json::json;

fn demo() -> Session {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn saved_file_is_versioned_and_save_copy_keeps_state() {
    let d = temp_dir("schema");
    let path = d.join("p.fcproj").to_string_lossy().to_string();
    let mut s = demo();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(v["schema_version"], filmcraft_format::SCHEMA_VERSION);
    assert_eq!(v["format"], "filmcraft.project");
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    assert!(s.is_dirty());
    let copy = d.join("copy.fcproj").to_string_lossy().to_string();
    s.execute("file.saveCopy", json!({"path": copy})).unwrap();
    assert!(s.is_dirty(), "Save a Copy leaves the project unsaved");
    assert_eq!(s.path.as_deref(), Some(path.as_str()));
    let mut t = Session::default();
    t.execute("file.open", json!({"path": copy})).unwrap();
    assert_eq!(*t.project, *s.project);
    // Revert discards the edit.
    s.execute("file.revert", json!({})).unwrap();
    assert!(!s.is_dirty());
    assert_ne!(*t.project, *s.project);
    assert!(!Session::default().is_enabled("file.revert"), "nothing to revert to before a save");
    // No temp files left next to the project.
    let stray: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
    assert!(stray.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn legacy_project_opens_and_first_save_keeps_a_backup() {
    let d = temp_dir("legacy");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../format/tests/fixtures/v1-edit.fcproj");
    let path = d.join("old.fcproj");
    std::fs::copy(&fixture, &path).unwrap();
    let p = path.to_string_lossy().to_string();
    let mut s = Session::default();
    let r = s.execute("file.open", json!({"path": p})).unwrap();
    assert_eq!(r["schemaVersion"], 1);
    assert_eq!(r["migrated"], true);
    assert_eq!(s.project.name, "Legacy Edit");
    assert!(s.drain_events().iter().any(|e| matches!(e, Event::Toast { message, .. } if message.contains("Upgraded"))));
    let r = s.execute("file.save", json!({})).unwrap();
    let backup = r["backup"].as_str().unwrap().to_string();
    assert!(backup.ends_with("old (schema v1 backup).fcproj"), "{backup}");
    assert_eq!(std::fs::read(&backup).unwrap(), std::fs::read(&fixture).unwrap());
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(v["schema_version"], filmcraft_format::SCHEMA_VERSION);
    // Second save: no new backup.
    assert!(s.execute("file.save", json!({})).unwrap()["backup"].is_null());
    // A file from the future is refused clearly.
    let mut v = v;
    v["schema_version"] = json!(filmcraft_format::SCHEMA_VERSION + 1);
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let e = Session::default().execute("file.open", json!({"path": p})).unwrap_err().to_string();
    assert!(e.contains("newer version of FilmCraft"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn project_is_named_after_its_file() {
    let d = temp_dir("name");
    let mut s = demo();
    let a = d.join("Apollo 11 - Tranquility.fcproj").to_string_lossy().to_string();
    s.execute("file.saveAs", json!({"path": a})).unwrap();
    assert_eq!(s.project.name, "Apollo 11 - Tranquility");
    assert_eq!(s.project.root.name, "Apollo 11 - Tranquility");
    assert!(!s.is_dirty(), "renaming on Save As is not an edit");
    // Save a Copy keeps the current name.
    let c = d.join("Backup copy.fcproj").to_string_lossy().to_string();
    s.execute("file.saveCopy", json!({"path": c})).unwrap();
    assert_eq!(s.project.name, "Apollo 11 - Tranquility");
    s.execute("file.open", json!({"path": c})).unwrap();
    assert_eq!(s.project.name, "Apollo 11 - Tranquility");
    let _ = std::fs::remove_dir_all(&d);
}

/// #1545: `file.newProject` answers with the project it made (name and revision, as
/// `project_inspect` reports them) instead of `null`.
#[test]
fn new_project_answers_with_its_name_and_revision() {
    let mut s = Session::default();
    let r = s.execute("file.newProject", json!({"name": "Trailer"})).unwrap();
    assert_eq!(r["name"], "Trailer");
    assert_eq!(r["revision"].as_u64(), Some(s.revision));
    assert_eq!(s.project.name, "Trailer");
    let r = s.execute("file.newProject", json!({})).unwrap();
    assert_eq!(r["name"], "Untitled", "the default name");
}

/// Opening the demo after a saved project must not keep that path: Save would overwrite it.
#[test]
fn open_demo_project_clears_saved_path_so_save_does_not_overwrite() {
    let d = temp_dir("demo-path");
    let original = d.join("original.fcproj").to_string_lossy().to_string();
    let mut s = Session::default();
    s.execute("file.newProject", json!({"name": "Original"})).unwrap();
    s.execute("file.newBin", json!({"name": "ORIGINAL"})).unwrap();
    s.execute("file.saveAs", json!({"path": original})).unwrap();
    s.execute("file.openDemoProject", json!({})).unwrap();
    assert!(s.path.is_none(), "demo is unsaved, like New Project");
    let err = s.execute("file.save", json!({})).unwrap_err().to_string();
    assert!(err.contains("no path") || err.contains("Save As"), "{err}");
    let mut reopen = Session::default();
    reopen.execute("file.open", json!({"path": original})).unwrap();
    assert!(
        reopen.project.root.children.iter().any(|e| matches!(e, filmcraft_project::BinEntry::Bin(b) if b.name == "ORIGINAL")),
        "original.fcproj still has the ORIGINAL bin"
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn snapshot_session(tag: &str) -> (Session, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("fc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut s = Session::default();
    s.start_autosave(crate::autosave::AutosaveConfig::new(&d)).unwrap();
    (s, d)
}

fn listed_auto_saves(s: &mut Session, name: &str) -> Vec<String> {
    let r = s.execute("file.listAutoSaves", json!({"name": name})).unwrap();
    r["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect()
}

#[test]
fn new_project_snapshots_a_dirty_never_saved_project_first() {
    let (mut s, _d) = snapshot_session("snap-new");
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    assert!(s.is_dirty());
    let before = (*s.project).clone();
    let r = s.execute("file.newProject", json!({"name": "Next"})).unwrap();
    let snap = r["snapshot"].as_str().expect("returns the snapshot path").to_string();
    assert_eq!(listed_auto_saves(&mut s, "FilmCraft Demo"), vec![snap.clone()]);
    let restored = filmcraft_format::decode(&std::fs::read(&snap).unwrap()).unwrap();
    assert_eq!(restored.project, before, "the snapshot holds the replaced project");
    assert_eq!(s.project.name, "Next");
}

#[test]
fn new_project_and_open_write_nothing_for_a_clean_project() {
    let (mut s, d) = snapshot_session("snap-clean");
    let r = s.execute("file.newProject", json!({"name": "A"})).unwrap();
    assert!(r["snapshot"].is_null());
    let path = d.join("p.fcproj").to_string_lossy().to_string();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    assert!(!s.is_dirty());
    let r = s.execute("file.open", json!({"path": path})).unwrap();
    assert!(r["snapshot"].is_null());
    assert!(listed_auto_saves(&mut s, "A").is_empty());
}

#[test]
fn open_snapshots_a_dirty_project_first() {
    let (mut s, d) = snapshot_session("snap-open");
    let other = d.join("other.fcproj").to_string_lossy().to_string();
    s.execute("file.saveAs", json!({"path": other})).unwrap();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    assert!(s.is_dirty());
    let before = (*s.project).clone();
    let r = s.execute("file.open", json!({"path": other})).unwrap();
    let snap = r["snapshot"].as_str().expect("snapshot path").to_string();
    assert_eq!(listed_auto_saves(&mut s, "FilmCraft Demo").len(), 1);
    let restored = filmcraft_format::decode(&std::fs::read(&snap).unwrap()).unwrap();
    assert_eq!(restored.project, before);
}

#[test]
fn a_replaced_saved_project_is_listed_from_its_own_directory() {
    let (mut s, d) = snapshot_session("snap-dir");
    let elsewhere = d.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = elsewhere.join("film.fcproj").to_string_lossy().to_string();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    let r = s.execute("file.newProject", json!({"name": "Next"})).unwrap();
    let snap = std::path::PathBuf::from(r["snapshot"].as_str().unwrap());
    assert!(snap.starts_with(&elsewhere), "beside the replaced project: {snap:?}");
    let listed = s.execute("file.listAutoSaves", json!({"name": "film", "dir": snap.parent().unwrap()})).unwrap();
    assert_eq!(listed["files"].as_array().unwrap().len(), 1, "{listed}");
}

#[test]
fn snapshots_in_the_same_second_keep_each_other() {
    let (mut s, _d) = snapshot_session("snap-twice");
    let mut taken = std::collections::BTreeSet::new();
    for n in 0..3 {
        s.execute("file.openDemoProject", json!({})).unwrap();
        s.execute("sequence.addEdit", json!({"seconds": 1.0 + f64::from(n)})).unwrap();
        let r = s.execute("file.newProject", json!({"name": "Same"})).unwrap();
        taken.insert(r["snapshot"].as_str().unwrap().to_string());
    }
    assert_eq!(taken.len(), 3, "each replacement keeps its own version: {taken:?}");
    assert_eq!(listed_auto_saves(&mut s, "FilmCraft Demo").len(), 3);
}

#[test]
fn a_one_version_limit_still_keeps_the_latest_snapshot() {
    let (mut s, _d) = snapshot_session("snap-one");
    s.prefs.auto_save.max_versions = 1;
    for n in 0..3 {
        s.execute("file.openDemoProject", json!({})).unwrap();
        s.execute("sequence.addEdit", json!({"seconds": 1.0 + f64::from(n)})).unwrap();
        let r = s.execute("file.newProject", json!({"name": "Same"})).unwrap();
        let snap = r["snapshot"].as_str().unwrap();
        assert!(std::path::Path::new(snap).exists(), "snapshot {n} survives its own rotation: {snap}");
    }
}

#[test]
fn opening_the_only_retained_version_is_not_rotated_away() {
    let (mut s, _d) = snapshot_session("snap-open-self");
    s.prefs.auto_save.max_versions = 1;
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    let first = s.execute("file.newProject", json!({"name": "A"})).unwrap()["snapshot"].as_str().unwrap().to_string();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 2.0})).unwrap();
    // the dirty project is replaced by the one retained version, which the new snapshot would rotate away
    let r = s.execute("file.open", json!({"path": first}));
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(s.project.name, "FilmCraft Demo");
    assert!(s.is_dirty(), "the opened version was rotated away: nothing backs it, so it stays unsaved");
    assert!(s.execute("file.open", json!({"path": "/nonexistent/x.fcproj"})).is_err());
}

#[test]
fn a_snapshot_that_cannot_be_placed_refuses_the_replacement() {
    let (mut s, d) = snapshot_session("snap-overflow");
    s.prefs.auto_save.max_versions = 1;
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    let dir = d.join(filmcraft_format::autosave::AUTO_SAVE_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("FilmCraft Demo-2026-01-01_00-00-00-9223372036854775807.fcproj"), b"x").unwrap();
    assert!(s.execute("file.newProject", json!({"name": "Next"})).is_err());
    assert_eq!(s.project.name, "FilmCraft Demo", "the dirty project is kept");
    assert!(s.is_dirty());
    // one second short of the limit: the next time cannot be named so that the list finds it
    std::fs::remove_file(dir.join("FilmCraft Demo-2026-01-01_00-00-00-9223372036854775807.fcproj")).unwrap();
    std::fs::write(dir.join("FilmCraft Demo-2026-01-01_00-00-00-9223372036854775806.fcproj"), b"x").unwrap();
    assert!(s.execute("file.newProject", json!({"name": "Next"})).is_err());
    assert_eq!(s.project.name, "FilmCraft Demo");
}

#[test]
fn a_dirty_project_with_nowhere_to_keep_a_copy_is_not_discarded_silently() {
    let mut s = Session::default();
    assert!(s.persistence.is_none() && s.path.is_none());
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    let e = s.execute("file.newProject", json!({"name": "Next"})).unwrap_err().to_string();
    assert!(e.contains("discard"), "{e}");
    assert_eq!(s.project.name, "FilmCraft Demo", "refused: the project is kept");
    assert!(s.is_dirty());
    let d = std::env::temp_dir().join(format!("fc-nowhere-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let other = d.join("o.fcproj").to_string_lossy().to_string();
    let mut t = Session::default();
    t.execute("file.saveAs", json!({"path": other})).unwrap();
    assert!(s.execute("file.open", json!({"path": other})).unwrap_err().to_string().contains("discard"));
    // dropping the changes on purpose
    let r = s.execute("file.newProject", json!({"name": "Next", "discard": true})).unwrap();
    assert!(r["snapshot"].is_null());
    assert_eq!(s.project.name, "Next");
    // a clean session is replaced without ceremony
    s.execute("file.newProject", json!({"name": "Again"})).unwrap();
}

#[test]
fn import_replace_does_not_drop_unsaved_changes_that_cannot_be_kept() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("sequence.addEdit", json!({"seconds": 1.0})).unwrap();
    let e = s.execute("file.import", json!({"paths": ["/nonexistent/clip.mp4"], "replace": true})).unwrap_err().to_string();
    assert!(e.contains("discard"), "{e}");
    assert_eq!(s.project.name, "FilmCraft Demo");
    assert!(s.is_dirty());
}
