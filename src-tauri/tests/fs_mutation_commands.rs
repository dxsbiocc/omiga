use omiga_lib::commands::fs::{
    create_directory, create_file, delete_fs_entry, list_directory, rename_fs_entry,
};
use tempfile::tempdir;

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn filetree_mutation_commands_create_rename_delete_entries() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();

    let folder = root.join("created");
    let created_folder = create_directory(path_string(&folder))
        .await
        .expect("create directory");
    assert!(std::path::Path::new(&created_folder).is_dir());

    let file = folder.join("note.md");
    let created_file = create_file(path_string(&file)).await.expect("create file");
    assert!(std::path::Path::new(&created_file).is_file());

    let listing = list_directory(path_string(&folder), None, None)
        .await
        .expect("list directory");
    assert_eq!(listing.total, 1);
    assert_eq!(listing.entries[0].name, "note.md");
    assert!(!listing.entries[0].is_directory);

    let renamed = folder.join("renamed.md");
    let renamed_file = rename_fs_entry(path_string(&file), path_string(&renamed))
        .await
        .expect("rename file");
    assert!(std::path::Path::new(&renamed_file).is_file());
    assert!(!file.exists());
    assert!(renamed.exists());

    delete_fs_entry(path_string(&renamed))
        .await
        .expect("delete file");
    assert!(!renamed.exists());

    delete_fs_entry(path_string(&folder))
        .await
        .expect("delete folder");
    assert!(!folder.exists());
}

#[tokio::test]
async fn filetree_mutation_commands_reject_conflicts_and_missing_paths() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let a = root.join("a.txt");
    let b = root.join("b.txt");

    create_file(path_string(&a)).await.expect("create a");
    create_file(path_string(&b)).await.expect("create b");

    assert!(create_file(path_string(&a)).await.is_err());
    assert!(rename_fs_entry(path_string(&a), path_string(&b))
        .await
        .is_err());
    assert!(delete_fs_entry(path_string(&root.join("missing.txt")))
        .await
        .is_err());
}
