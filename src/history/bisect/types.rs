use clap::Subcommand;

/// Represents the step calculation result of a binary search commit inspection in bisect.
// Used by: src/history/bisect.rs, src/history/bisect/engine.rs
// Used by command/helper: `bisect_step` helper and the `bisect` command
pub enum BisectOutcome {
    WaitingForBad,
    WaitingForGood,
    Continue(String, usize),
    Found(String),
}

/// Enum of subcommands supported by the bisect command.
// Used by: src/cli.rs, src/history/bisect.rs
// Used by command/helper: CLI argument parser and the `bisect` command dispatcher
#[derive(Subcommand, Debug)]
pub enum BisectAction {
    Start {
        bad: Option<String>,
        good: Vec<String>,
    },
    Bad {
        rev: Option<String>,
    },
    Good {
        rev: Option<String>,
    },
    Skip {
        revs: Vec<String>,
    },
    Reset {
        commit: Option<String>,
    },
    Log,
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}
