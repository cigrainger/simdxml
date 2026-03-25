use clap::{Parser, Subcommand};
use std::fs;

#[derive(Parser)]
#[command(name = "simdxml")]
#[command(about = "SIMD-accelerated XML parser with XPath 1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate an XPath expression against XML file(s)
    Query {
        /// XPath expression
        #[arg(short, long)]
        expr: String,
        /// XML file(s)
        files: Vec<String>,
    },
    /// Show structural index info for an XML file
    Info {
        /// XML file
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Query { expr, files } => {
            for file in &files {
                let data = match fs::read(file) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Error reading {}: {}", file, e);
                        continue;
                    }
                };

                let index = match simdxml::parse(&data) {
                    Ok(idx) => idx,
                    Err(e) => {
                        eprintln!("Error parsing {}: {}", file, e);
                        continue;
                    }
                };

                match index.xpath_text(&expr) {
                    Ok(results) => {
                        for text in results {
                            println!("{}", text);
                        }
                    }
                    Err(e) => {
                        eprintln!("XPath error on {}: {}", file, e);
                    }
                }
            }
        }
        Commands::Info { file } => {
            let data = match fs::read(&file) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error reading {}: {}", file, e);
                    return;
                }
            };

            let index = match simdxml::parse(&data) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error parsing {}: {}", file, e);
                    return;
                }
            };

            println!("File: {}", file);
            println!("Size: {} bytes", data.len());
            println!("Tags: {}", index.tag_count());
            println!("Text ranges: {}", index.text_count());
        }
    }
}
