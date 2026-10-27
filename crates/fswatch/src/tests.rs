use std::fs;
use std::thread::sleep;
use std::time::Duration;

use super::*;

fn write(root: &Path, relative: &str, contents: &[u8]) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write test file");
}

#[test]
fn detects_add_modify_remove_and_rename() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();
    write(root, "one.jpg", b"one");
    write(root, "nested/two.jpg", b"two");

    let mut watcher = RootWatcher::new(root).expect("watcher");
    write(root, "three.jpg", b"three");
    let added = watcher.poll().expect("poll add");
    assert_eq!(added.state, RootState::Online);
    assert_eq!(
        added.events,
        vec![ChangeEvent {
            relative_path: PathBuf::from("three.jpg"),
            kind: ChangeKind::Added
        }]
    );

    sleep(Duration::from_millis(20));
    write(root, "one.jpg", b"one changed");
    let modified = watcher.poll().expect("poll modify");
    assert_eq!(modified.events[0].kind, ChangeKind::Modified);

    fs::rename(root.join("nested/two.jpg"), root.join("nested/renamed.jpg")).expect("rename file");
    let renamed = watcher.poll().expect("poll rename");
    assert_eq!(renamed.events.len(), 2);
    assert!(renamed.events.iter().any(|event| {
        event.relative_path == Path::new("nested/two.jpg") && event.kind == ChangeKind::Removed
    }));
    assert!(renamed.events.iter().any(|event| {
        event.relative_path == Path::new("nested/renamed.jpg") && event.kind == ChangeKind::Added
    }));

    fs::remove_file(root.join("three.jpg")).expect("remove file");
    let removed = watcher.poll().expect("poll remove");
    assert_eq!(
        removed.events,
        vec![ChangeEvent {
            relative_path: PathBuf::from("three.jpg"),
            kind: ChangeKind::Removed
        }]
    );
}

#[test]
fn ignores_hidden_files_and_symlinks() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path();
    write(root, ".hidden.jpg", b"hidden");
    write(root, ".cache/image.jpg", b"hidden dir");
    write(root, "visible.jpg", b"visible");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("visible.jpg"), root.join("link.jpg"))
        .expect("create symlink");

    let mut watcher = RootWatcher::new(root).expect("watcher");
    let result = watcher.poll().expect("poll");
    assert!(result.events.is_empty());

    write(root, "visible.jpg", b"changed");
    let result = watcher.poll().expect("poll");
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].relative_path, Path::new("visible.jpg"));
}

#[test]
fn offline_root_does_not_emit_removals_and_recovers() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = temporary.path().join("volume");
    fs::create_dir(&root).expect("create root");
    write(&root, "kept.jpg", b"kept");
    let mut watcher = RootWatcher::new(&root).expect("watcher");

    let stash = temporary.path().join("detached");
    fs::rename(&root, &stash).expect("detach volume");
    let offline = watcher.poll().expect("poll offline");
    assert_eq!(offline.state, RootState::Offline);
    assert!(offline.state_changed);
    assert_eq!(offline.events.len(), 1);
    assert_eq!(offline.events[0].kind, ChangeKind::Offline);

    fs::rename(&stash, &root).expect("reattach volume");
    write(&root, "new.jpg", b"new");
    let online = watcher.poll().expect("poll online");
    assert_eq!(online.state, RootState::Online);
    assert!(online
        .events
        .iter()
        .any(|event| event.kind == ChangeKind::Online));
    assert!(online.events.iter().any(|event| {
        event.relative_path == Path::new("new.jpg") && event.kind == ChangeKind::Added
    }));
    assert!(!online
        .events
        .iter()
        .any(|event| event.kind == ChangeKind::Removed));
}

#[test]
fn rejects_a_root_that_is_not_a_directory() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let file = temporary.path().join("file.jpg");
    fs::write(&file, b"x").expect("write file");
    assert!(RootWatcher::new(&file).is_err());
}
