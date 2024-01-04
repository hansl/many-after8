use clap::Parser;
use rand::{thread_rng, Rng};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const DENOMINATOR: f64 = 1_000_000_000.0;

#[derive(Debug, Parser)]
struct Opt {
    /// The directory that contains the JSON files and the PEM file.
    #[clap(long, global = true)]
    dir: Option<PathBuf>,

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

    /// Whether to randomize the amount, within 10% of the maximum. Each id
    /// will have a different randomized maximum.
    #[clap(long)]
    randomize: bool,

    /// A memo to pass to the minting command.
    #[clap(long)]
    memo: Option<String>,
}

#[derive(Debug, Parser)]
pub struct BalancesOpt {}

fn read_all_jsons(root: impl AsRef<Path>) -> Result<BTreeMap<String, f64>, anyhow::Error> {
    // Read all the JSON files.
    let mut to_mint = BTreeMap::<String, f64>::new();

    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap() == "json" {
            let data = std::fs::read_to_string(&path).unwrap();
            let data: BTreeMap<String, Value> = serde_json::from_str(&data).unwrap();
            for (name, value) in data {
                let tokens = match &value {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => {
                        let s = s.replace(",", "");
                        s.parse::<f64>().ok()
                    }
                    x => {
                        panic!("Invalid value type '{}' in file '{:?}'", x, path);
                    }
                };
                if let Some(tokens) = tokens {
                    let curr = to_mint.entry(name).or_default();
                    *curr += tokens;
                } else {
                    panic!("Invalid token amount '{}' in file '{:?}'", value, path);
                }
            }
        }
    }

    Ok(to_mint)
}

fn mint(
    root: impl AsRef<Path>,
    balances: BTreeMap<String, f64>,
    opts: &MintOpt,
) -> Result<(), anyhow::Error> {
    let mut rand = thread_rng();
    let to_mint = balances
        .into_iter()
        .map(|(id, balance)| {
            let max = if opts.randomize {
                opts.max * rand.gen_range(0.9..1.1)
            } else {
                opts.max
            };
            (id, balance.min(max))
        })
        .filter(|(_, balance)| *balance > 0.0)
        .map(|(id, amount)| {
            eprintln!("{}: {}", id, amount);
            // A small sanity check.
            if amount > DENOMINATOR {
                panic!("Invalid amount '{}' for id '{}'", amount, id);
            }

            let amount = (amount * DENOMINATOR) as u64;
            (id, amount)
        })
        .collect::<BTreeMap<_, _>>();

    // Commit a new file to disk.
    let now = chrono::Local::now();
    let output = root
        .as_ref()
        .join(format!("mint-{}.json", now.format("%Y%m%d-%H%M%S")));
    if !opts.dry_run {
        std::fs::write(
            output,
            serde_json::to_string_pretty(
                &to_mint
                    .iter()
                    .map(|(id, amount)| (id.clone(), -((*amount as f64) / DENOMINATOR)))
                    .collect::<BTreeMap<_, _>>(),
            )?,
        )?;
    }

    // Output the command line to run.
    println!("ledger --pem {}/MFX_Token.pem https://alberto.app/api token mint mqbh742x4s356ddaryrxaowt4wxtlocekzpufodvowrirfrqaaaaa3l '{:#?}'", root.as_ref().display(), to_mint );

    Ok(())
}

fn balances(
    _root: impl AsRef<Path>,
    balances: BTreeMap<String, f64>,
    _opts: &BalancesOpt,
) -> Result<(), anyhow::Error> {
    for (id, balance) in balances {
        if balance > 0.0 {
            println!("{}: {:0.9}", id, balance);
        }
    }
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let opts = Opt::parse();
    let root = &opts.dir.unwrap_or_else(|| std::env::current_dir().unwrap());
    let b = read_all_jsons(root)?;

    match opts.subcommand {
        Subcommand::Mint(opts) => mint(root, b, &opts),
        Subcommand::Balances(opts) => balances(root, b, &opts),
    }
}
