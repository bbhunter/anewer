use anewer::cli::Args;
use anewer::filter::{HashFilter, LineOptions};
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut filter = HashFilter::new(
        args.filename,
        args.quiet,
        args.invert,
        args.dry_run,
        LineOptions {
            null: args.null,
            line_buffered: args.line_buffered,
            trim: args.trim,
            skip_fields: args.skip_fields.unwrap_or(0),
            field_delimiter: args.field_delimiter,
        },
    )?;

    filter.process_input()
}
