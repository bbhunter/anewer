use ahash::RandomState as ARandomState;
use anyhow::{Context, Result};
use memchr::{memchr, memchr2, memchr3};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::hash::BuildHasherDefault;
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::path::PathBuf;

use crate::hasher;

fn trim_line(line: &[u8], trim: bool) -> &[u8] {
    if !trim {
        return line;
    }

    let mut start = 0;
    let mut end = line.len();

    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }

    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    &line[start..end]
}

fn find_whitespace(bytes: &[u8]) -> Option<usize> {
    let sp_tab = memchr3(b' ', b'\t', b'\r', bytes);
    let nl_ff = memchr2(b'\n', 0x0c, &bytes[..sp_tab.unwrap_or(bytes.len())]);
    nl_ff.or(sp_tab)
}

fn skip_ascii(line: &[u8], num: usize) -> &[u8] {
    let mut pos = 0;

    while pos < line.len() && line[pos].is_ascii_whitespace() {
        pos += 1;
    }

    if num == 1 {
        pos += match find_whitespace(&line[pos..]) {
            Some(relative) => relative,
            None => return &line[line.len()..],
        };

        while pos < line.len() && line[pos].is_ascii_whitespace() {
            pos += 1;
        }

        return &line[pos..];
    }

    for _ in 0..num {
        if pos == line.len() {
            return &line[line.len()..];
        }

        while pos < line.len() && !line[pos].is_ascii_whitespace() {
            pos += 1;
        }

        while pos < line.len() && line[pos].is_ascii_whitespace() {
            pos += 1;
        }
    }

    &line[pos..]
}

fn skip_delim(mut line: &[u8], num: usize, delim: u8) -> &[u8] {
    for _ in 0..num {
        line = match memchr(delim, line) {
            Some(pos) => &line[pos + 1..],
            None => return &[],
        };
    }

    line
}

fn extract_key(line: &[u8], num: usize, field_delimiter: Option<u8>) -> &[u8] {
    if num == 0 {
        line
    } else if let Some(delim) = field_delimiter {
        skip_delim(line, num, delim)
    } else {
        skip_ascii(line, num)
    }
}

enum ScanOutcome {
    Finished { end_delim: bool },
    Stopped,
}

fn scan_lines(
    reader: &mut impl BufRead,
    delim: u8,
    partial: &mut Vec<u8>,
    mut process: impl FnMut(&[u8]) -> Result<bool>,
) -> Result<ScanOutcome> {
    let mut end_delim = true;

    loop {
        let consumed;
        {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                if !partial.is_empty() {
                    partial.push(delim);
                    if !process(partial)? {
                        return Ok(ScanOutcome::Stopped);
                    }
                }
                return Ok(ScanOutcome::Finished { end_delim });
            }

            if let Some(delimiter_index) = memchr(delim, chunk) {
                end_delim = true;
                let keep_going = if partial.is_empty() {
                    process(&chunk[..=delimiter_index])?
                } else {
                    partial.extend_from_slice(&chunk[..=delimiter_index]);
                    let keep_going = process(partial)?;
                    partial.clear();
                    keep_going
                };
                if !keep_going {
                    return Ok(ScanOutcome::Stopped);
                }
                consumed = delimiter_index + 1;
            } else {
                partial.extend_from_slice(chunk);
                end_delim = false;
                consumed = chunk.len();
            }
        }
        reader.consume(consumed);
    }
}

fn write_line(
    output: &mut impl Write,
    raw: &[u8],
    line: &[u8],
    delim: u8,
    trim: bool,
) -> io::Result<()> {
    if trim {
        output.write_all(line)?;
        output.write_all(&[delim])
    } else {
        output.write_all(raw)
    }
}

#[derive(Default)]
pub struct LineOptions {
    pub null: bool,
    pub trim: bool,
    pub skip_fields: usize,
    pub field_delimiter: Option<u8>,
}

pub struct HashFilter {
    hasher: ARandomState,
    set: HashSet<u64, BuildHasherDefault<hasher::IdentityHasher>>,
    out_file: Option<BufWriter<File>>,
    quiet: bool,
    invert: bool,
    delim: u8,
    trim: bool,
    skip_fields: usize,
    field_delimiter: Option<u8>,
}

impl HashFilter {
    pub fn new(
        filename: Option<PathBuf>,
        quiet: bool,
        invert: bool,
        dry_run: bool,
        options: LineOptions,
    ) -> Result<Self> {
        let delim = if options.null { b'\0' } else { b'\n' };

        if options.skip_fields > 0 && options.field_delimiter == Some(delim) {
            anyhow::bail!("field separator must differ from line separator");
        }

        let mut filter = HashFilter {
            hasher: ARandomState::new(),
            set: HashSet::default(),
            out_file: None,
            quiet,
            invert,
            delim,
            trim: options.trim,
            skip_fields: options.skip_fields,
            field_delimiter: options.field_delimiter,
        };

        if let Some(filename) = filename {
            filter.load_state(&filename, dry_run)?;
        }

        Ok(filter)
    }

    fn load_state(&mut self, filename: &PathBuf, dry_run: bool) -> Result<()> {
        let mut f = if dry_run {
            match File::open(filename) {
                Ok(f) => f,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to open file: {:?}", filename));
                }
            }
        } else {
            OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(filename)
                .context("could not create/write/open file")?
        };

        let mut partial = Vec::new();
        let delim = self.delim;
        let trim = self.trim;

        let out = {
            let mut reader = BufReader::new(&mut f);

            scan_lines(&mut reader, delim, &mut partial, |raw| {
                let line = trim_line(&raw[..raw.len() - 1], trim);
                self.set.insert(hasher::hash(&self.hasher, line));
                Ok(true)
            })
            .with_context(|| format!("failed to read file: {:?}", filename))?
        };

        if !dry_run {
            if matches!(out, ScanOutcome::Finished { end_delim: false }) {
                f.write_all(&[delim])?;
            }

            self.out_file = Some(BufWriter::new(f));
        }
        Ok(())
    }

    fn process_line(&mut self, raw: &[u8], stdout: &mut impl Write) -> Result<bool> {
        debug_assert_eq!(raw.last(), Some(&self.delim));

        let line = trim_line(&raw[..raw.len() - 1], self.trim);
        let key = extract_key(line, self.skip_fields, self.field_delimiter);
        let is_new_line = self.set.insert(hasher::hash(&self.hasher, key));

        if is_new_line {
            if let Some(f) = &mut self.out_file {
                f.write_all(key).context("could not write to file")?;
                f.write_all(&[self.delim])
                    .context("couldn't write to file")?;
            }
        }

        if ((!self.invert && is_new_line) || (self.invert && !is_new_line))
            && !self.quiet
            && write_line(stdout, raw, line, self.delim, self.trim).is_err()
        {
            return Ok(false);
        }

        Ok(true)
    }

    pub fn process_input(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        let mut stdout = io::stdout().lock();
        let mut partial = Vec::new();

        scan_lines(&mut stdin, self.delim, &mut partial, |raw| {
            self.process_line(raw, &mut stdout)
        })?;

        if let Some(f) = &mut self.out_file {
            f.flush().context("error flushing file")?;
        }

        Ok(())
    }
}
