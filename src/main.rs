use ahash::RandomState as ARandomState;
use anyhow::{Context, Error, Result};
use clap::Parser;
use memchr::memchr;
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::hash::BuildHasherDefault;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, ErrorKind, Write};
use std::path::PathBuf;

// straight from huniq
#[derive(Default)]
struct IdentityHasher {
    off: u8,
    buf: [u8; 8],
}

impl Hasher for IdentityHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.off += (&mut self.buf[self.off as usize..])
            .write(bytes)
            .unwrap_or(0) as u8;
    }

    fn finish(&self) -> u64 {
        u64::from_ne_bytes(self.buf)
    }
}

fn hash<T: BuildHasher, U: std::hash::Hash + ?Sized>(build: &T, v: &U) -> u64 {
    let mut s = build.build_hasher();
    v.hash(&mut s);
    s.finish()
}

/// Appends lines from stdin to a file, if they don't already exist in the file.
#[derive(Debug, Parser)]
#[command(name = "anewer")]
#[command(author, version, about)]
struct Args {
    /// path to file, will be created if needed
    filename: Option<PathBuf>,

    /// Quiet, won't print to stdout.
    #[arg(short, long)]
    quiet: bool,

    /// Dry run, will leave the file as it is.
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Invert the sense of matching
    #[arg(short = 'v', long)]
    invert: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut file = None;
    let hasher = ARandomState::new();
    let mut set = HashSet::<u64, BuildHasherDefault<IdentityHasher>>::default();

    if let Some(filename) = args.filename {
        let mut has_newline = true;

        match fs::read(&filename) {
            Ok(content) => {
                has_newline = content.is_empty() || content[content.len() - 1] == b'\n';

                let mut remaining = &content[..];
                loop {
                    if let Some(idx) = memchr(b'\n', remaining) {
                        set.insert(hash(&hasher, &remaining[..idx]));
                        remaining = &remaining[idx + 1..];
                    } else {
                        if !remaining.is_empty() {
                            set.insert(hash(&hasher, &remaining));
                        }
                        break;
                    }
                }
            }
            Err(error) => match error.kind() {
                ErrorKind::NotFound => (),
                _ => {
                    return Err(
                        Error::new(error).context(format!("Failed to read file: {:?}", filename))
                    )
                }
            },
        }

        if !args.dry_run {
            let mut file2 = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(filename)
                .context("Could not create/write/open file")?;
            if !has_newline {
                file2.write_all(b"\n")?;
            }

            file = Some(file2);
        }
    }

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout();

    loop {
        let mut line = Vec::new();
        let mut n = stdin.read_until(b'\n', &mut line)?;

        if n == 0 {
            break;
        }

        if !line[n - 1] == b'\n' {
            n += 1;
            line.push(b'\n')
        }

        let known_line = set.insert(hash(&hasher, &line[..n - 1]));

        if (!args.invert && known_line) || (args.invert && !known_line) {
            if let Some(file) = &mut file {
                file.write_all(&line).context("Could not write to file")?;
            }

            if !args.quiet {
                if stdout.write_all(&line).is_err() {
                    break;
                }
            }
        }
    }

    Ok(())
}
