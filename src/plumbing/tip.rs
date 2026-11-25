pub mod cli;
pub mod engine;
pub mod types;

use crate::plumbing::tip::engine::{TIPS, tip_of_the_day, wrap};

/// `rgit tip [<n> | -r | --all]` (also `rgit bingo`): a short tip about git or rgit, with a
/// command to try. By default the tip of the day (the same all day); `<n>` a given one, `-r` a
/// random one, `--all` the list of titles. Works outside a repository.
pub fn tip(number: Option<usize>, random: bool, all: bool) -> anyhow::Result<()> {
    if all {
        for (i, t) in TIPS.iter().enumerate() {
            println!("{:>3}  {}", i + 1, t.title);
        }
        println!("`rgit tip <n>` shows one.");
        return Ok(());
    }
    let index = match number {
        Some(n) if (1..=TIPS.len()).contains(&n) => n - 1,
        Some(n) => crate::ui::die_fatal(format!("there is no tip {}: they go from 1 to {}", n, TIPS.len())),
        None if random => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize ^ d.as_secs() as usize).unwrap_or(0);
            nanos % TIPS.len()
        }
        None => tip_of_the_day(),
    };
    let t = &TIPS[index];
    println!("Tip {} of {}: {}", index + 1, TIPS.len(), t.title);
    println!();
    for line in wrap(t.text, 76) {
        println!("  {}", line);
    }
    println!();
    println!("  Try: {}", t.example);
    Ok(())
}
