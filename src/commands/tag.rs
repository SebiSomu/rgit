use crate::helpers::resolve_commit_from_source;
use crate::refs;

/// `rgit tag` / `rgit tag <name> [<commit>]` / `rgit tag -d <name>` / `rgit tag -l [<pattern>]`
// ///
// /// Lightweight tags only
pub fn tag(name: Option<String>, commit: Option<String>, delete: bool, list: bool) -> anyhow::Result<()> {
    if delete {
        let tag_name = name.ok_or_else(|| {
            anyhow::anyhow!("fatal: tag name required for delete")
        })?;
        refs::delete_tag(&tag_name)?;
        println!("Deleted tag '{}'", tag_name);
        return Ok(());
    }
    
    if list || name.is_none() {
        let tags = refs::list_tags()?;
        let pattern = if list { name.as_deref() } else { None };

        for t in &tags {
            if let Some(p) = pattern {
                if !t.contains(p) {
                    continue;
                }
            }
            println!("{}", t);
        }
        return Ok(());
    }

    let tag_name = name.unwrap();

    let target_commit = match commit {
        Some(c) => resolve_commit_from_source(&c)?,
        None => refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: cannot create tag — no commits yet")
        })?,
    };

    refs::create_tag(&tag_name, &target_commit)?;
    println!("Tag '{}' created at {}", tag_name, &target_commit[..7]);

    Ok(())
}