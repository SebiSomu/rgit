use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum ReflogAction {
    Show {
        reference: Option<String>,
        #[arg(short = 'n', long = "max-count")]
        max_count: Option<usize>,
    },
    Exists {
        reference: String,
    },
    Delete {
        entries: Vec<String>,
    },
    Expire {
        #[arg(long = "all", conflicts_with = "reference")]
        all: bool,
        #[arg(long = "expire", default_value = "90")]
        expire: String,
        reference: Option<String>,
    },
}
