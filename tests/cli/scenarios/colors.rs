use super::diverged;
use insta::assert_snapshot;

/// Shows each color escape as `<name>` (so the hashes next to them are still relabelled).
fn mask_colors(r: &mut crate::harness::Repo) {
    let names = [
        ("[m", "reset"), ("[1m", "bold"), ("[31m", "red"), ("[32m", "green"), ("[33m", "yellow"), ("[35m", "magenta"),
        ("[36m", "cyan"), ("[41m", "redbg"), ("[1;31m", "bold-red"), ("[1;32m", "bold-green"), ("[1;33m", "bold-yellow"),
        ("[1;36m", "bold-cyan"),
    ];
    for (code, name) in names {
        r.mask(&format!("\u{1b}{}", code), &format!("<{}>", name));
    }
}

/// git's colors with `color.ui=always`: status, branch, log with decorations and the graph,
/// diff, grep and show-branch; then `--no-color` and `never`.
#[test]
fn colored_output() {
    let mut r = diverged();
    mask_colors(&mut r);
    r.run("merge feature -m merged").run("tag v1").run("config color.ui always");
    r.write("main.txt", "on main\nchanged  \n").write("new.txt", "new\n").run("add new.txt");
    r.run("status").run("status -s -b").run("branch -v");
    r.run("log --graph --oneline --decorate --all").run("log --format='%C(auto)%h%d %Cgreen%s%Creset' -2");
    r.run("diff").run("diff --cached --stat").run("grep -n on").run("show-branch main feature");
    r.run("log --oneline --no-color -1").run("diff --color=never");
    r.run("config color.ui never").run("status -s").run("log --oneline --color=always -1");
    assert_snapshot!(r.finish());
}
