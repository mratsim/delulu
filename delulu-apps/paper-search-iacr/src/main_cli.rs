//!  Delulu IACR Paper Search — CLI
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! # CLI Entry Point
//!
//! Command-line interface for searching IACR ePrint papers.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_iacr::IacrClient;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "delulu-iacr")]
#[command(
    author,
    version,
    about = "Search papers on the IACR ePrint Archive"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List recent papers from the IACR RSS feed
    ListRecent,
    /// Get full details for a specific paper by year and number
    GetDetails {
        /// Publication year (e.g. 2024)
        year: u32,
        /// Paper number within the year (e.g. 123)
        number: u32,
    },
    /// Get the PDF download URL for a specific paper
    GetPdf {
        /// Publication year (e.g. 2024)
        year: u32,
        /// Paper number within the year (e.g. 123)
        number: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let client = Arc::new(IacrClient::new(30).context("Failed to create IACR client")?);

    match args.command {
        Command::ListRecent => {
            let papers = client.list_recent_papers().await?;
            println!("Found {} recent papers:\n", papers.len());
            for paper in &papers {
                println!("ID:      {}", paper.id);
                println!("Title:   {}", paper.title);
                println!("Authors: {}", paper.authors.join(", "));
                println!("URL:     {}", paper.html_url);
                println!();
            }
        }
        Command::GetDetails { year, number } => {
            let paper = client.get_paper_details(year, number).await?;
            println!("ID:       {}", paper.id);
            println!("Title:    {}", paper.title);
            println!("Authors:  {}", paper.authors.join(", "));
            println!("Abstract: {}", paper.abstract_text);
            println!("URL:      {}", paper.html_url);
            println!("PDF:      {}", paper.pdf_url);
        }
        Command::GetPdf { year, number } => {
            let url = client.download_paper_pdf(year, number);
            println!("{}", url);
        }
    }

    Ok(())
}
