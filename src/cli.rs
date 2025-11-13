use clap::{arg, command, Parser};

#[derive(Parser, Debug)]
#[command(name = "drill", version, about = "HTTP load testing application written in Rust inspired by Ansible syntax")]
pub(crate) struct Args {
    /// Sets the benchmark file
    #[arg(short, long, required = true)]
    pub benchmark: String,

    /// Shows request statistics
    #[arg(short, long, conflicts_with = "compare")]
    pub stats: bool,

    /// Sets a report file
    #[arg(short, long, conflicts_with = "compare")]
    pub report: Option<String>,

    /// Sets a compare file
    #[arg(short, long, conflicts_with = "report")]
    pub compare: Option<String>,

    /// Sets a threshold value in ms amongst the compared file
    #[arg(short, long, conflicts_with = "report")]
    pub threshold: Option<f64>,

    /// Do not fail if an interpolation is not present. (Not recommended)
    #[arg(long = "relaxed-interpolations")]
    pub relaxed_interpolations: bool,

    /// Disables SSL certification check. (Not recommended)
    #[arg(long = "no-check-certificate")]
    pub no_check_certificate: bool,

    /// Tags to include
    #[arg(long)]
    pub tags: Option<String>,

    /// Tags to exclude
    #[arg(long = "skip-tags")]
    pub skip_tags: Option<String>,

    /// List all benchmark tags
    #[arg(long = "list-tags", conflicts_with_all = ["tags", "skip_tags"])]
    pub list_tags: bool,

    /// List benchmark tasks (executes --tags/--skip-tags filter)
    #[arg(long = "list-tasks")]
    pub list_tasks: bool,

    /// Disables output
    #[arg(short, long)]
    pub quiet: bool,

    /// Set timeout in seconds for all requests
    #[arg(short = 'o', long)]
    pub timeout: Option<u64>,

    /// Shows statistics in nanoseconds
    #[arg(short, long)]
    pub nanosec: bool,

    /// Toggle verbose output
    #[arg(short, long)]
    pub verbose: bool,
}
