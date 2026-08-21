use clap::Parser;
use std::path::PathBuf;
fn parse_byte(value: &str) -> Result<u8, String> {
    match value.as_bytes() {
        [byte] => Ok(*byte),
        _ => Err("field separator must be exactly one byte".into()),
    }
}

/// anewer appends only new lines from stdin to a file.
#[derive(Debug, Parser)]
#[command(name = "anewer")]
#[command(author, version, about)]
pub struct Args {
    /// path to file
    pub filename: Option<PathBuf>,
    /// use NUL instead of newline as separator.
    #[arg(short = '0', long)]
    pub null: bool,

    /// quiet, won't print to stdout.
    #[arg(short, long)]
    pub quiet: bool,

    /// dry run, will leave the file as is.
    #[arg(short = 'd', long)]
    pub dry_run: bool,
    /// remove leading and trailing ASCII whitespace from line.
    #[arg(short = 't', long)]
    pub trim: bool,
    /// ignore the first NUM fields from stdin when building the compare string.
    #[arg(long, value_name = "NUM")]
    pub skip_fields: Option<usize>,

    /// separate fields with BYTE instead of whitespace.
    #[arg(
        short = 'F',
        long,
        value_name = "BYTE",
        value_parser = parse_byte,
        requires = "skip_fields"
    )]
    pub field_delimiter: Option<u8>,

    /// invert matching
    #[arg(short = 'v', long)]
    pub invert: bool,
}
