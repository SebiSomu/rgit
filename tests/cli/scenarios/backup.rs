//! rgit-only `backup`: dated bundles of every ref, with retention, restorable with clone.
use super::three_commits;
use insta::assert_snapshot;

/// Backup file names carry the time they were made: `repo-<STAMP>.bundle` in the snapshot.
fn stable(text: String) -> String {
    regex::Regex::new(r"-(?:<NOW-DATE>|\d{4}-\d{2}-\d{2})-\d{6}(?:-\d+)?\.bundle").unwrap().replace_all(&text, "-<STAMP>.bundle").to_string()
}

/// The newest backup in `dir` (relative to the repository).
fn newest(r: &crate::harness::Repo, dir: &str) -> String {
    let mut names: Vec<String> = std::fs::read_dir(r.path().join(dir)).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort_by_key(|n| (n.len(), n.clone()));
    format!("{}/{}", dir, names.last().unwrap())
}

#[test]
fn backup_retention_skip_and_restore() {
    let mut r = three_commits();
    r.run("backup --list").run("backup --dir ../backups").run("backup --dir ../backups");
    r.write("a.txt", "one\ntwo\nthree\n").run("commit -a -m 'fourth commit'").run("backup --dir ../backups --keep 2");
    r.run("tag v1").run("backup --dir ../backups --keep 2").run("backup --dir ../backups --list");
    // Uncommitted changes are not in a backup, and it says so.
    r.write("b.txt", "dirty\n").run("backup --dir ../backups -f --keep 0");
    let file = newest(&r, "../backups");
    r.run_in("..", &format!("clone {} restored", file.trim_start_matches("../"))).run_in("../restored", "log --oneline --all").run_in("../restored", "tag");
    assert_snapshot!(stable(r.finish()));
}

#[test]
fn backup_default_directory() {
    let mut r = three_commits();
    r.run("config rgit.backupKeep 1").run("backup").run("backup -f").run("backup --list");
    assert_snapshot!(stable(r.finish()));
}
