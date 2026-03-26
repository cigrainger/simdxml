use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use owo_colors::OwoColorize;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// A fast XML/XPath query tool, powered by SIMD.
///
/// Extract data from XML using XPath 1.0 expressions.
/// Reads from files or stdin.
///
///   sxq '//title' book.xml
///   sxq '//claim[@type="independent"]' patents/*.xml
///   curl -s https://example.com/feed.xml | sxq '//item/title'
///   sxq -c '//record' huge.xml
///   sxq -r '//svg:path' drawing.svg
///   sxq info patent.xml
#[derive(Parser)]
#[command(name = "sxq", version)]
struct QueryCli {
    /// XPath 1.0 expression
    #[arg(value_name = "XPATH")]
    xpath: String,

    /// XML files to query (reads stdin if omitted)
    #[arg(value_name = "FILE")]
    files: Vec<String>,

    /// Output raw XML fragments instead of text content
    #[arg(short, long)]
    raw: bool,

    /// Print only the count of matching nodes
    #[arg(short, long)]
    count: bool,

    /// Output results as a JSON array
    #[arg(short, long)]
    json: bool,

    /// Print only filenames that contain matches
    #[arg(short = 'l', long)]
    files_with_matches: bool,

    /// Separate output with NUL bytes instead of newlines
    #[arg(short = '0', long = "null")]
    null_sep: bool,

    /// Suppress filename headers in multi-file output
    #[arg(short = 'H', long = "no-filename")]
    no_filename: bool,

    /// Always show filename headers (even for single file)
    #[arg(long = "with-filename")]
    with_filename: bool,

    /// Include whitespace-only results (stripped by default)
    #[arg(short = 'W', long)]
    whitespace: bool,

    /// Number of threads for parallel processing [default: number of CPUs]
    #[arg(short = 't', long)]
    threads: Option<usize>,

    #[command(flatten)]
    color: colorchoice_clap::Color,
}

/// Show structural index statistics for XML files.
#[derive(Parser)]
#[command(name = "sxq info")]
struct InfoCli {
    /// XML files to inspect
    #[arg(required = true)]
    files: Vec<String>,

    #[command(flatten)]
    color: colorchoice_clap::Color,
}

// ---------------------------------------------------------------------------
// Entrypoint — dispatch "info" subcommand manually, default to query
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Dispatch: `sxq info ...`
    if args.get(1).map(|s| s.as_str()) == Some("info") {
        let info = InfoCli::parse_from(
            std::iter::once("sxq info".to_string()).chain(args[2..].iter().cloned()),
        );
        info.color.write_global();
        return match run_info(&info.files) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}: {:#}", "error".red().bold(), e);
                ExitCode::from(2)
            }
        };
    }

    let cli = QueryCli::parse();
    cli.color.write_global();

    match run_query(&cli) {
        Ok(had_matches) => {
            if had_matches {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{}: {:#}", "error".red().bold(), e);
            ExitCode::from(2)
        }
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

fn run_query(cli: &QueryCli) -> Result<bool> {
    let xpath = &cli.xpath;
    let threads = cli.threads.unwrap_or_else(|| std::thread::available_parallelism()
        .map(|n| n.get()).unwrap_or(1));

    // Validate the XPath expression early, before reading any input.
    let compiled = simdxml::CompiledXPath::compile(xpath);
    if let Err(ref e) = compiled {
        report_xpath_error(xpath, e);
        bail!("invalid XPath expression");
    }
    let compiled = compiled.unwrap();

    let sources = gather_sources(&cli.files)?;
    let multi = sources.len() > 1;
    let show_filename = cli.with_filename || (multi && !cli.no_filename);
    let sep = if cli.null_sep { "\0" } else { "\n" };

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    // --- Parallel batch path for multi-file text extraction ---
    if multi && threads > 1 && !cli.raw && !cli.files_with_matches {
        return run_batch_parallel(cli, &compiled, &sources, show_filename, sep, &mut out, threads);
    }

    let mut any_match = false;
    let mut json_all: Vec<Vec<String>> = Vec::new();

    for source in &sources {
        let data = source.read_bytes()?;
        let name = source.display_name();

        let index = simdxml::parse(&data)
            .with_context(|| format!("parsing {name}"))?;

        let result = index.eval(xpath)
            .with_context(|| format!("evaluating XPath on {name}"))?;

        match result {
            simdxml::XPathResult::NodeSet(ref nodes) => {
                // --- Count mode ---
                if cli.count {
                    let n = nodes.len();
                    if n > 0 { any_match = true; }
                    if show_filename {
                        writeln!(out, "{}:{}", name.bold(), n)?;
                    } else {
                        writeln!(out, "{n}")?;
                    }
                    continue;
                }

                // --- Files-with-matches mode ---
                if cli.files_with_matches {
                    if !nodes.is_empty() {
                        any_match = true;
                        writeln!(out, "{name}")?;
                    }
                    continue;
                }

                // --- Raw XML mode ---
                if cli.raw {
                    let fragments = index.xpath_raw(xpath)
                        .with_context(|| format!("evaluating XPath on {name}"))?;
                    let fragments: Vec<&str> = if cli.whitespace {
                        fragments
                    } else {
                        fragments.into_iter().filter(|s| !s.trim().is_empty()).collect()
                    };
                    if !fragments.is_empty() {
                        any_match = true;
                        if show_filename {
                            writeln!(out, "{}", name.purple().bold())?;
                        }
                    }
                    for fragment in &fragments {
                        write!(out, "{fragment}{sep}")?;
                    }
                    continue;
                }

                // --- Default: text extraction ---
                let texts = index.xpath_text(xpath)
                    .with_context(|| format!("evaluating XPath on {name}"))?;

                let texts: Vec<&str> = if cli.whitespace {
                    texts
                } else {
                    texts.into_iter().filter(|s| !s.trim().is_empty()).collect()
                };

                if !texts.is_empty() {
                    any_match = true;
                }

                if cli.json {
                    let owned: Vec<String> = texts.iter()
                        .map(|s| simdxml::XmlIndex::decode_entities(s).into_owned())
                        .collect();
                    if multi {
                        json_all.push(owned);
                    } else {
                        write_json(&mut out, &owned)?;
                    }
                    continue;
                }

                if show_filename && !texts.is_empty() {
                    writeln!(out, "{}", name.purple().bold())?;
                }
                for text in &texts {
                    let decoded = simdxml::XmlIndex::decode_entities(text);
                    write!(out, "{decoded}{sep}")?;
                }
            }

            // --- Scalar results ---
            simdxml::XPathResult::String(ref s) => {
                any_match = !s.is_empty();
                if cli.json {
                    let items = vec![s.clone()];
                    if multi { json_all.push(items); } else { write_json(&mut out, &items)?; }
                } else if cli.count {
                    if show_filename { writeln!(out, "{}:1", name.bold())?; }
                    else { writeln!(out, "1")?; }
                } else {
                    if show_filename { writeln!(out, "{}", name.purple().bold())?; }
                    write!(out, "{s}{sep}")?;
                }
            }
            simdxml::XPathResult::Number(n) => {
                any_match = true;
                let formatted = result.to_display_string(&index);
                if cli.json {
                    // Numbers go as raw JSON
                    if multi { json_all.push(vec![formatted.clone()]); }
                    else { writeln!(out, "{formatted}")?; }
                } else {
                    if show_filename { writeln!(out, "{}", name.purple().bold())?; }
                    write!(out, "{formatted}{sep}")?;
                }
            }
            simdxml::XPathResult::Boolean(b) => {
                any_match = b;
                let formatted = result.to_display_string(&index);
                if cli.json {
                    // Booleans go as raw JSON
                    if multi { json_all.push(vec![formatted.clone()]); }
                    else { writeln!(out, "{formatted}")?; }
                } else {
                    if show_filename { writeln!(out, "{}", name.purple().bold())?; }
                    write!(out, "{formatted}{sep}")?;
                }
            }
        }
    }

    if cli.json && multi {
        let flat: Vec<String> = json_all.into_iter().flatten().collect();
        write_json(&mut out, &flat)?;
    }

    out.flush()?;
    Ok(any_match)
}

// ---------------------------------------------------------------------------
// Parallel batch processing
// ---------------------------------------------------------------------------

fn run_batch_parallel(
    cli: &QueryCli,
    compiled: &simdxml::CompiledXPath,
    sources: &[Source],
    show_filename: bool,
    sep: &str,
    out: &mut impl Write,
    threads: usize,
) -> Result<bool> {
    // Read all files upfront
    let mut names: Vec<&str> = Vec::with_capacity(sources.len());
    let mut all_data: Vec<Vec<u8>> = Vec::with_capacity(sources.len());
    for source in sources {
        all_data.push(source.read_bytes()?);
        names.push(source.display_name());
    }

    let doc_refs: Vec<&[u8]> = all_data.iter().map(|d| d.as_slice()).collect();

    if cli.count {
        let counts = simdxml::batch::count_batch(&doc_refs, compiled)
            .context("batch count")?;
        let mut any_match = false;
        for (i, count) in counts.iter().enumerate() {
            if *count > 0 { any_match = true; }
            if show_filename {
                writeln!(out, "{}:{}", names[i].bold(), count)?;
            } else {
                writeln!(out, "{count}")?;
            }
        }
        out.flush()?;
        return Ok(any_match);
    }

    // Text extraction with parallelism
    let batch_results = simdxml::batch::eval_batch_parallel(&doc_refs, compiled, threads)
        .context("batch parallel evaluation")?;

    let mut any_match = false;
    let mut json_all: Vec<Vec<String>> = Vec::new();

    for (i, results) in batch_results.iter().enumerate() {
        let results: Vec<&str> = if cli.whitespace {
            results.iter().map(|s| s.as_str()).collect()
        } else {
            results.iter().map(|s| s.as_str()).filter(|s| !s.trim().is_empty()).collect()
        };

        if !results.is_empty() {
            any_match = true;
        }

        if cli.json {
            let owned: Vec<String> = results.iter()
                .map(|s| simdxml::XmlIndex::decode_entities(s).into_owned())
                .collect();
            json_all.push(owned);
            continue;
        }

        if show_filename && !results.is_empty() {
            writeln!(out, "{}", names[i].purple().bold())?;
        }
        for text in &results {
            let decoded = simdxml::XmlIndex::decode_entities(text);
            write!(out, "{decoded}{sep}")?;
        }
    }

    if cli.json {
        let flat: Vec<String> = json_all.into_iter().flatten().collect();
        write_json(out, &flat)?;
    }

    out.flush()?;
    Ok(any_match)
}

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

enum Source {
    File(String),
    Stdin,
}

impl Source {
    fn read_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Source::File(path) => {
                fs::read(path).with_context(|| format!("reading {path}"))
            }
            Source::Stdin => {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)
                    .context("reading stdin")?;
                Ok(buf)
            }
        }
    }

    fn display_name(&self) -> &str {
        match self {
            Source::File(path) => path,
            Source::Stdin => "<stdin>",
        }
    }
}

fn gather_sources(files: &[String]) -> Result<Vec<Source>> {
    if files.is_empty() {
        if io::stdin().is_terminal() {
            bail!(
                "no input. Provide XML files as arguments or pipe via stdin.\n\
                 \n  sxq '//title' file.xml\n  \
                 cat file.xml | sxq '//title'"
            );
        }
        return Ok(vec![Source::Stdin]);
    }

    let mut sources = Vec::new();
    for f in files {
        if f == "-" {
            sources.push(Source::Stdin);
        } else {
            sources.push(Source::File(f.clone()));
        }
    }
    Ok(sources)
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn write_json(out: &mut impl Write, items: &[String]) -> Result<()> {
    write!(out, "[")?;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        write_json_string(out, item)?;
    }
    writeln!(out, "]")?;
    Ok(())
}

fn write_json_string(out: &mut impl Write, s: &str) -> Result<()> {
    write!(out, "\"")?;
    for ch in s.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32)?,
            c => write!(out, "{c}")?,
        }
    }
    write!(out, "\"")?;
    Ok(())
}

fn report_xpath_error(expr: &str, err: &simdxml::SimdXmlError) {
    eprintln!("{}: {}", "error".red().bold(), "invalid XPath expression".bold());
    eprintln!("  {expr}");
    if let simdxml::SimdXmlError::XPathParseError(msg) = err {
        eprintln!("  {}", msg.dimmed());
    }
}

// ---------------------------------------------------------------------------
// Info subcommand
// ---------------------------------------------------------------------------

fn run_info(files: &[String]) -> Result<bool> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for (idx, file) in files.iter().enumerate() {
        let data = fs::read(file)
            .with_context(|| format!("reading {file}"))?;
        let index = simdxml::parse(&data)
            .with_context(|| format!("parsing {file}"))?;

        if files.len() > 1 {
            writeln!(out, "{}", file.bold())?;
        }

        let size = data.len();
        let (size_val, size_unit) = humanize_bytes(size);
        writeln!(out, "  {} {:.1} {} ({} bytes)", "size:".dimmed(), size_val, size_unit, size)?;
        writeln!(out, "  {} {}", "tags:".dimmed(), index.tag_count())?;
        writeln!(out, "  {} {}", "text:".dimmed(), index.text_count())?;

        // Tag name distribution
        let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for i in 0..index.tag_count() {
            if index.tag_types[i] == simdxml::index::TagType::Open
                || index.tag_types[i] == simdxml::index::TagType::SelfClose
            {
                *name_counts.entry(index.tag_name(i)).or_default() += 1;
            }
        }
        let mut sorted: Vec<_> = name_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        let unique = sorted.len();
        writeln!(out, "  {} {}", "unique tags:".dimmed(), unique)?;

        if !sorted.is_empty() {
            let top = sorted.len().min(10);
            writeln!(out, "  {}", "top tags:".dimmed())?;
            for (name, count) in &sorted[..top] {
                writeln!(out, "    {:>6}  {}", count.to_string().cyan(), name)?;
            }
            if sorted.len() > 10 {
                writeln!(out, "    {} ... and {} more", "".dimmed(), sorted.len() - 10)?;
            }
        }

        // Max depth
        let max_depth = index.depths.iter().copied().max().unwrap_or(0);
        writeln!(out, "  {} {}", "max depth:".dimmed(), max_depth)?;

        if files.len() > 1 && idx < files.len() - 1 {
            writeln!(out)?;
        }
    }

    out.flush()?;
    Ok(true)
}

fn humanize_bytes(bytes: usize) -> (f64, &'static str) {
    if bytes >= 1_073_741_824 {
        (bytes as f64 / 1_073_741_824.0, "GiB")
    } else if bytes >= 1_048_576 {
        (bytes as f64 / 1_048_576.0, "MiB")
    } else if bytes >= 1024 {
        (bytes as f64 / 1024.0, "KiB")
    } else {
        (bytes as f64, "B")
    }
}
