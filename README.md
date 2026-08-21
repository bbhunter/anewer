# anewer [![crates.io][crates-img]][crates]]
anewer appends only new lines from stdin to a file.

This is a Rust reimplementation of [tomnomnom/anew](https://github.com/tomnomnom/anew).

It uses zero-copy to store [aHash](https://github.com/tkaitchuck/aHash) fingerprints in memory instead of complete lines. This keeps memory use nearly constant even if lines get longer.

Running on an Apple M2 benchmark with 500,000 known lines and 500,000 lines as input, `anewer` is 1.6 - 2.2x faster in dry runs and 4.4 - 8.7x faster in quiet append mode while using 73–93% less peak memory.

[crates-img]:   https://img.shields.io/crates/v/anewer.svg
[crates]:       https://crates.io/crates/anewer

## Usage

```
$ anewer --help
anewer appends only new lines from stdin to a file.

Usage: anewer [OPTIONS] [FILENAME]

Arguments:
  [FILENAME]  path to file, will be created if needed

Options:
  -0, --null                    use NUL instead of newline as the record separator
  -q, --quiet                   quiet mode
  -d, --dry-run                 dry run, will leave the file as it is
  -t, --trim                    remove leading and trailing whitespace from line
      --skip-fields <NUM>       ignore leading stdin fields when building the comparision string
  -F, --field-separator <BYTE>  separate fields with BYTE instead of whitespace
  -v, --invert                  invert matching
  -h, --help                    show this help
  -V, --version                 print anewer version
```

## Install

The best way is to install anewer via cargo:

```
cargo install anewer
```

Binary releases are availble via [GitHub Releases](https://github.com/ysf/anewer/releases/latest). I added static x86_64/ARM64 builds to be of use in ctfs or restricted shells.


#### Add unknown elements of newthings.txt to things.txt
```
$ cat things.txt
Zero
One
Two

$ cat newthings.txt
One
Two
Three
Four

$ cat newthings.txt | anewer things.txt
Three
Four

$ cat things.txt
Zero
One
Two
Three
Four
```

#### Or use it as simple uniq without sort
```
$ cat list.txt
One
One
Two
Two
Three
Four
Three
Four

$ cat list.txt | anewer
One
Two
Three
Four
```

#### Trim records before deduplication

```
$ printf '  One  \nOne\n' | anewer --trim
One
```

#### Ignore changing log prefixes

Some tools print timestamps you might want to ignore. `--skip-fields` applies only to stdin but allowes `anewer` to compare the extracted stringt with the lines already stored in the given file. If new, the line is appended to the file, while it passes through the original line to stdout.

With `-F`, each delimiter byte separates a field, including empty fields. If a record line has fewer than `NUM` fields, its considered empty. `--skip-fields 0` keeps the line as it is. Without `-F` whitespace is used as delimiter, and leading whitespace is ignored.

```
$ cat log.txt
ERROR wifi cable broken

$ printf '12:00 ERROR wifi cable broken\n12:01 WARN cpu lost\n' | anewer --skip-fields 1 log.txt
12:01 WARN cpu lost

$ cat log.txt
ERROR wifi cable broken
WARN cpu lost
```

You can use a byte delimiter for structured logs:

```
$ anewer --skip-fields 2 -F $'\t' event.log
```

With `-v`/ `--invert`, matching lines are printed to stdin while new lines are still added to the output file.

Besides that, `--trim` is applied before extraction, and `--null` changes the record separator for stdin, stdout, and the output file.

# License
GPLv3+
