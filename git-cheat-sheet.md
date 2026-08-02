# Git Cheat Sheet — Complete Study Guide

## 1. Core Concepts (the mental model)

Git tracks content across **three (or four) areas**:

| Area | What it is | Command that moves content forward |
|---|---|---|
| Working Directory | Your actual files on disk | (you editing) |
| Staging Area (Index) | A draft of the next commit | `git add` |
| Local Repository (.git) | Committed history | `git commit` |
| Remote Repository | Shared history (e.g. GitHub) | `git push` |

Key ideas to internalize:

- **A commit is a snapshot**, not a diff. Git stores the full tree state at each commit (compressed/deduped internally), unlike older systems (SVN) that store diffs.
- **HEAD** is a pointer to the current commit (usually via a branch pointer). "Detached HEAD" means HEAD points directly to a commit, not a branch.
- **A branch is just a movable pointer** (a 41-byte file with a SHA) to a commit. This is why branching in Git is instant and cheap — it's not copying files.
- **Every commit has a SHA-1/SHA-256 hash** derived from its content + parent(s) + metadata. Change anything upstream → every descendant hash changes (this is why rebase "rewrites history").
- **The staging area lets you craft commits** — you don't have to commit everything you changed at once.

---

## 2. First-Time Setup

```bash
git config --global user.name "Your Name"
git config --global user.email "you@example.com"
git config --global init.defaultBranch main
git config --global core.editor "code --wait"   # or vim, nano, etc.
git config --list                                # see all settings
```

`--global` = applies to all repos for your user. Omit it to set per-repo (run inside the repo folder).

---

## 3. Starting a Repository

```bash
git init                          # new repo in current folder
git clone <url>                   # copy an existing remote repo
git clone <url> <folder-name>     # clone into a custom folder name
git clone --depth 1 <url>         # shallow clone (only latest snapshot, faster)
```

---

## 4. The Basic Daily Loop

```bash
git status                # what's changed / staged / untracked
git diff                  # unstaged changes vs last commit
git diff --staged         # staged changes vs last commit (a.k.a. --cached)
git add <file>            # stage a specific file
git add .                 # stage everything in current dir (careful!)
git add -p                # stage interactively, hunk by hunk (great habit)
git commit -m "message"   # commit staged changes
git commit -am "message"  # stage tracked-file changes AND commit (skips new/untracked files)
```

**Good commit message rule of thumb:** short imperative summary line (≤50 chars), blank line, then details in the body if needed. "Fix null check in parser" not "Fixed a bug" or "stuff".

---

## 5. Branching

```bash
git branch                     # list local branches
git branch -a                  # list local + remote branches
git branch <name>              # create branch (doesn't switch to it)
git switch <name>               # switch to branch (modern, safer)
git switch -c <name>            # create + switch in one step
git checkout <name>             # old way to switch (still common in the wild)
git checkout -b <name>          # old way to create + switch
git branch -d <name>            # delete branch (safe: refuses if unmerged)
git branch -D <name>            # force delete (even if unmerged — dangerous)
git branch -m <old> <new>       # rename a branch
```

> `switch`/`restore` were introduced to split `checkout`'s overloaded behavior (it did both branch-switching AND file-restoring, which confused everyone). Prefer `switch`/`restore` when possible.

---

## 6. Merging vs Rebasing (the concept people mix up most)

Both answer: "bring changes from branch A into branch B." They do it very differently.

### Merge
```bash
git switch main
git merge feature
```
- Creates a **new merge commit** with two parents.
- History shows exactly what happened, including when branches diverged and rejoined.
- **Non-destructive** — never rewrites existing commits.
- Good for: shared/public branches, preserving true history.

### Rebase
```bash
git switch feature
git rebase main
```
- **Replays** feature's commits one-by-one on top of main's latest commit.
- Produces a **linear history** — looks like feature was written after main's latest work, even if it wasn't.
- **Rewrites commit hashes** for every replayed commit.
- Good for: cleaning up your own local/private branch before sharing.

### The golden rule of rebasing
> **Never rebase commits that have been pushed and that others might have based work on.**
If you rebase shared history, everyone else's local copy diverges from the rewritten one, causing painful duplicate-commit messes on their next pull.

### Interactive rebase (rewriting your own history before sharing)
```bash
git rebase -i HEAD~5     # edit the last 5 commits
```
In the editor you can: `pick`, `reword`, `edit`, `squash` (meld into previous, keep both messages), `fixup` (meld, discard message), `drop`.

---

## 7. Remotes

```bash
git remote -v                       # list remotes and URLs
git remote add origin <url>         # add a remote named "origin"
git remote remove <name>            # remove a remote
git fetch origin                    # download new data, don't merge/change working dir
git pull                            # fetch + merge (equivalent to fetch + merge)
git pull --rebase                   # fetch + rebase instead of merge (cleaner history)
git push origin <branch>            # push a branch
git push -u origin <branch>         # push AND set upstream tracking (do this once per branch)
git push --force-with-lease         # force-push safely (checks no one else pushed first)
git push --force                    # force-push (dangerous — can overwrite others' work)
```

**`fetch` vs `pull`:** `fetch` is always safe — it just updates your knowledge of the remote (`origin/main`, etc.) without touching your working files. `pull` = `fetch` + immediately integrate. When unsure, `fetch` then inspect with `git log origin/main` before merging.

---

## 8. Undoing Things (the part everyone Googles under pressure)

This is the area with the most ways to shoot yourself in the foot — know the differences cold.

| Situation | Command | Effect |
|---|---|---|
| Discard unstaged changes in a file | `git restore <file>` | File reverts to last commit. **Uncommitted work is gone.** |
| Unstage a file (keep changes) | `git restore --staged <file>` | Moves file from staged → unstaged, content untouched |
| Amend the last commit | `git commit --amend` | Replaces last commit (new hash) with currently staged changes + optionally new message |
| Undo last commit, keep changes staged | `git reset --soft HEAD~1` | Commit undone, changes stay staged |
| Undo last commit, keep changes unstaged | `git reset --mixed HEAD~1` (default mode) | Commit undone, changes go back to working dir |
| Undo last commit, **discard changes entirely** | `git reset --hard HEAD~1` | ⚠️ Destructive — changes are gone (unless recoverable via reflog) |
| Undo a commit that's already pushed/shared | `git revert <commit>` | Creates a **new commit** that undoes the old one — safe for shared history |
| Recover "lost" commits/resets | `git reflog` | Shows a log of where HEAD has pointed — your safety net for almost everything |

### `reset` cheat: soft / mixed / hard
```
--soft   : moves HEAD only               (commit undone, index + working dir untouched)
--mixed  : moves HEAD + resets index     (commit undone, unstaged, working dir untouched) [default]
--hard   : moves HEAD + index + files    (commit undone, ALL changes gone)
```

### Reflog — your undo button for almost anything
```bash
git reflog
git reset --hard HEAD@{2}     # jump back to a state you see in reflog
```
Even after a "destructive" `reset --hard`, the old commit usually still exists in Git's object database for a while (default ~30–90 days) and reflog can find it. This is the #1 thing to remember when panicking.

---

## 9. Stashing (shelving work-in-progress)

```bash
git stash                       # shelve tracked changes
git stash -u                     # also include untracked files
git stash list                   # see all stashes
git stash pop                    # reapply latest stash AND remove it from stash list
git stash apply                  # reapply latest stash but KEEP it in the list
git stash apply stash@{2}        # apply a specific one
git stash drop stash@{2}         # delete a specific stash without applying
git stash show -p stash@{0}      # view the diff inside a stash
```

---

## 10. Inspecting History

```bash
git log                          # full history
git log --oneline                # compact, one line per commit
git log --oneline --graph --all  # visual branch graph (very useful)
git log -p <file>                # show diffs for a file's history
git log --author="name"          # filter by author
git log -- <path>                # history touching a path
git show <commit>                # show one commit's diff and metadata
git blame <file>                 # who last changed each line, and when
git bisect start                 # binary search commits to find what introduced a bug
```

---

## 11. Tags (marking releases)

```bash
git tag v1.0.0                          # lightweight tag on current commit
git tag -a v1.0.0 -m "Release 1.0.0"    # annotated tag (stores author, date, message — preferred)
git tag                                 # list tags
git push origin v1.0.0                  # push one tag
git push origin --tags                  # push all tags
```

---

## 12. Cherry-picking & Bisecting

```bash
git cherry-pick <commit>          # apply one specific commit from another branch onto current
git bisect start
git bisect bad                    # mark current commit as broken
git bisect good <commit>          # mark a known-good commit
# Git checks out commits in between; you test and mark good/bad each time
git bisect reset                  # finish and return to original HEAD
```

---

## 13. .gitignore Essentials

```
node_modules/
*.log
.env
/dist
!important-file.log     # exception: force-track this one even though *.log is ignored
```
Rules:
- Only affects **untracked** files. If a file is already tracked, `.gitignore` won't hide it — you must `git rm --cached <file>` first, then commit.
- Patterns are gitignore-glob syntax, not regex.

---

## 14. Common Mistakes & Gotchas

1. **Committing then realizing you're on the wrong branch.**
   Fix: `git reset --soft HEAD~1` (undo commit, keep changes staged) → `git switch correct-branch` → `git commit`.

2. **Force-pushing over a shared branch and clobbering teammates' commits.**
   Prefer `--force-with-lease` over plain `--force` — it refuses if the remote has commits you haven't seen.

3. **Merge conflicts panic.**
   Conflict markers look like:
   ```
   <<<<<<< HEAD
   your version
   =======
   their version
   >>>>>>> branch-name
   ```
   Edit the file to the correct final content, remove the markers, then `git add <file>` and continue (`git merge --continue` or `git rebase --continue`, depending which you were doing). You can always bail out with `git merge --abort` / `git rebase --abort`.

4. **Rebasing a branch other people already pulled.** Causes duplicate commits and confusion for them. Rebase only unpublished/private work.

5. **`git add .` staging things you didn't mean to** (build artifacts, `.env`, secrets). Use `.gitignore` proactively, and `git status`/`git diff --staged` before committing.

6. **Detached HEAD confusion** — happens after `git checkout <commit-hash>` or `<tag>` directly. You're not on any branch; commits made here can be "lost" (until reflog garbage collects them) unless you create a branch: `git switch -c new-branch-name` while still there.

7. **Thinking `git pull` is always safe.** It merges automatically, which can create surprise merge commits or conflicts mid-flow. Prefer `git fetch` + review, or `git pull --rebase` for a cleaner local history on solo/feature branches.

8. **Losing work after `reset --hard`, then panicking.** Remember `git reflog` almost always still has it, for a while.

9. **Committing large binaries / generated files** and bloating repo size forever (history keeps them even if later deleted). Use `.gitignore` from day one; for existing bloat, tools like `git filter-repo` exist (advanced, rewrites history — coordinate with the team first).

10. **Confusing `git checkout -- file` (old) with `git restore file` (new) with `git reset file`.** Restore = working-dir/staging content. Reset = moves branch pointer + optionally staging/working dir. Checkout historically did a bit of everything, which is exactly why it's confusing.

11. **Not setting upstream, then `git push` fails or pushes to the wrong place.** First push of a new branch: `git push -u origin <branch>`. After that, plain `git push` works.

12. **Squashing/rewriting commits that are referenced elsewhere** (e.g. in an open PR others are commenting on) — causes GitHub/GitLab PR history to look broken. Coordinate before rewriting pushed history.

---

## 15. Quick Command Reference Table

| Goal | Command |
|---|---|
| See current state | `git status` |
| Stage changes | `git add <file>` / `git add .` |
| Commit | `git commit -m "msg"` |
| See history | `git log --oneline --graph --all` |
| New branch | `git switch -c <name>` |
| Switch branch | `git switch <name>` |
| Merge branch into current | `git merge <branch>` |
| Rebase current onto another | `git rebase <branch>` |
| Push new branch first time | `git push -u origin <branch>` |
| Pull | `git pull` (or `git pull --rebase`) |
| Undo last commit, keep changes | `git reset --soft HEAD~1` |
| Discard local file changes | `git restore <file>` |
| Undo a pushed commit safely | `git revert <commit>` |
| Shelve work temporarily | `git stash` / `git stash pop` |
| Recover "lost" work | `git reflog` |
| Apply one commit from elsewhere | `git cherry-pick <commit>` |

---

## 16. Mental Model Summary (the one-paragraph version)

Git = a graph of immutable, content-addressed snapshots (commits), with branches as lightweight movable labels pointing into that graph, and a staging area letting you curate exactly what goes into the next snapshot. Almost every "scary" Git operation (`reset`, `rebase`, force-push) is scary only because it moves labels or rewrites which snapshots are reachable — the underlying data usually isn't gone immediately, which is why `reflog` is such a powerful safety net. The core discipline that avoids 90% of Git pain: **commit often in small units, don't rewrite history that's already shared, and always know whether a command touches your working directory, the staging area, or just history/pointers.**
