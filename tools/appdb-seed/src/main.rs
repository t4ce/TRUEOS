//! Create the normal, kernel-owned app.db on the host. No publication step.
use redb::{Database, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const HASHES: TableDefinition<&str, &str> = TableDefinition::new("hashes");
const UPDATED: TableDefinition<&str, &str> = TableDefinition::new("updated");

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: trueos-appdb-seed BUILDINS_JSON PACKAGE_DIR OUTPUT_DB UPDATED_UTC".into());
    }
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let names = manifest["buildins"].as_array().ok_or("missing buildins array")?;
    let packages = PathBuf::from(&args[1]);
    let destination = PathBuf::from(&args[2]);
    let updated = args[3].to_str().ok_or("invalid updated text")?;
    // Never mutate the last complete image. A failed transaction leaves a temp file.
    let temporary = destination.with_extension("db.new");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let mut database = Database::builder().set_cache_size(64 * 1024 * 1024).create(&temporary)?;
    {
        let transaction = database.begin_write()?;
        {
            let mut apps = transaction.open_table(APPS)?;
            let mut hashes = transaction.open_table(HASHES)?;
            let mut dates = transaction.open_table(UPDATED)?;
            for name in names {
                let name = name.as_str().ok_or("invalid app name")?;
                let archive = format!("{name}.bp");
                let bytes = fs::read(packages.join(&archive))?;
                if bytes.len() < 24 || &bytes[..4] != b"TRBP" {
                    return Err(format!("invalid Blueprint package: {archive}").into());
                }
                let hash = format!("{:x}", Sha256::digest(&bytes));
                apps.insert(archive.as_str(), bytes.as_slice())?;
                hashes.insert(archive.as_str(), hash.as_str())?;
                dates.insert(archive.as_str(), updated)?;
            }
        }
        transaction.commit()?;
    }
    database.compact()?;
    {
        let transaction = database.begin_read()?;
        let apps = transaction.open_table(APPS)?;
        let hashes = transaction.open_table(HASHES)?;
        let dates = transaction.open_table(UPDATED)?;
        if apps.len()? != names.len() as u64 || hashes.len()? != apps.len()? || dates.len()? != apps.len()? {
            return Err("seed tables do not contain the complete app set".into());
        }
        for name in names {
            let archive = format!("{}.bp", name.as_str().unwrap());
            let bytes = apps.get(archive.as_str())?.ok_or("missing app")?;
            let expected = format!("{:x}", Sha256::digest(bytes.value()));
            if hashes.get(archive.as_str())?.ok_or("missing hash")?.value() != expected {
                return Err(format!("seed hash mismatch: {archive}").into());
            }
        }
    }
    drop(database);
    fs::rename(temporary, &destination)?;
    println!("app.db: {} apps, {} bytes", names.len(), fs::metadata(destination)?.len());
    Ok(())
}
