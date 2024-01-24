use clap::Parser;
use rand::{thread_rng, Rng};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

const DENOMINATOR: f64 = 1_000_000_000.0;

#[derive(Debug, Parser)]
struct Opt {
    /// The directory that contains the JSON files and the PEM file.
    #[clap(long)]
    dir: PathBuf,

    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Debug, Parser)]
enum Subcommand {
    /// Output the minting command to run.
    Mint(MintOpt),

    /// Show remaining balances to mint.
    Balances(BalancesOpt),
}

#[derive(Debug, Parser)]
pub struct MintOpt {
    /// The maximum amount to mint in one run.
    #[clap(long, default_value = "100")]
    max: f64,

    /// Whether to save a new JSON file containing the negatives of the balances
    /// we have minted.
    #[clap(long)]
    dry_run: bool,

    /// Whether to randomize the amount, within 20% of the maximum. Each id
    /// will have a different randomized maximum.
    #[clap(long)]
    randomize: bool,

    /// A memo to pass to the minting command.
    #[clap(long)]
    memo: Option<String>,

    /// Only output JSON, not the full command line.
    #[clap(long)]
    json: bool,

    /// The pem file to use for the command line.
    #[clap(long)]
    pem: PathBuf,
}

#[derive(Debug, Parser)]
pub struct BalancesOpt {}

fn read_all_jsons(root: impl AsRef<Path>) -> Result<BTreeMap<String, u64>, anyhow::Error> {
    // Read all the JSON files.
    let mut balance = BTreeMap::<String, i128>::new();

    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap() == "json" {
            let data = std::fs::read_to_string(&path).unwrap();
            let data: BTreeMap<String, Value> = serde_json::from_str(&data).unwrap();
            for (name, value) in data {
                let tokens = match &value {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.replace(',', "").parse::<f64>().ok(),
                    x => {
                        panic!("Invalid value type '{}' in file '{:?}'", x, path);
                    }
                };
                if let Some(tokens) = tokens {
                    // A small sanity check. This means that a period was missed or
                    // something.
                    if tokens > DENOMINATOR {
                        panic!("Invalid token amount '{}' in file '{:?}'", value, path);
                    }

                    let tokens = (tokens * DENOMINATOR) as i128;
                    let curr = balance.entry(name).or_default();

                    // Make sure we don't end up with a negative or too small balance.
                    let new = *curr + tokens;
                    *curr = new;
                } else {
                    panic!("Invalid token amount '{}' in file '{:?}'", value, path);
                }
            }
        }
    }

    Ok(balance
        .into_iter()
        .filter_map(|(k, v)| {
            if v >= u64::MAX as i128 {
                panic!("Balance for '{}' is too large", k);
            }

            if v > 0 {
                Some((k, v as u64))
            } else {
                None
            }
        })
        .collect())
}

fn mint(
    root: impl AsRef<Path>,
    balances: BTreeMap<String, u64>,
    opts: MintOpt,
) -> Result<(), anyhow::Error> {
    let now = chrono::Local::now();

    let mut rand = thread_rng();
    eprintln!("Minting tokens...");
    eprintln!("Date: {}", now.to_rfc2822());
    eprintln!("Flags: {opts:?}");
    eprintln!();

    let MintOpt {
        dry_run,
        memo,
        randomize,
        max,
        json,
        pem,
    } = opts;
    let max = (max * DENOMINATOR) as u64;

    let to_mint = balances
        .into_iter()
        .map(|(id, balance)| {
            let max = if randomize {
                ((max as f64) * rand.gen_range(0.8..1.2)) as u64
            } else {
                max
            };
            (id, balance.min(max))
        })
        .collect::<BTreeMap<_, _>>();

    let longest = to_mint
        .values()
        .map(|s| format!("{:.09}", *s as f64 / DENOMINATOR).len())
        .max()
        .unwrap_or(0);
    to_mint.iter().for_each(|(id, s)| {
        eprintln!(
            "{}\t{:>longest$}",
            id,
            format!("{:.09}", (*s as f64) / DENOMINATOR),
        );
    });

    eprintln!("--------------------------------------------------");

    if !dry_run {
        // Commit a new file to disk.
        let output = root
            .as_ref()
            .join(format!("mint-{}.json", now.format("%Y%m%d-%H%M%S")));

        writeln!(
            File::create(output).unwrap(),
            "{}",
            serde_json::to_string_pretty(
                &to_mint
                    .iter()
                    .map(|(id, amount)| (
                        id.clone(),
                        (-((*amount as f64) / DENOMINATOR)).to_string()
                    ))
                    .collect::<BTreeMap<_, _>>(),
            )?,
        )?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&to_mint)?);
    } else if !to_mint.is_empty() {
        let to_mint = to_mint
            .iter()
            .map(|(id, amount)| format!(r#"    "{}": {}"#, id, amount))
            .collect::<Vec<_>>()
            .join(",\n");
        let to_mint = format!("{{\n{}\n}}", to_mint);

        // Output the command line to run.
        println!("ledger --pem {} https://alberto.app/api token mint mqbh742x4s356ddaryrxaowt4wxtlocekzpufodvowrirfrqaaaaa3l '{}' {}", pem.display(), to_mint, if let Some(m) = memo {
            format!("--memo '{m}'")
        } else {
            "".to_string()
        });
    }

    Ok(())
}

fn balances(
    _root: impl AsRef<Path>,
    balances: BTreeMap<String, u64>,
    _opts: BalancesOpt,
) -> Result<(), anyhow::Error> {
    for (id, balance) in balances {
        if balance > 0 {
            println!("{}: {:0.9}", id, (balance as f64) / DENOMINATOR);
        }
    }
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let opts = Opt::parse();
    let root = &opts.dir;
    let b = read_all_jsons(root)?;

    match opts.subcommand {
        Subcommand::Mint(opts) => mint(root, b, opts),
        Subcommand::Balances(opts) => balances(root, b, opts),
    }
}
