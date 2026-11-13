//! Create the normal, kernel-owned app.db on the host. No publication step.
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::PathBuf};

const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const HASHES: TableDefinition<&str, &str> = TableDefinition::new("hashes");
const UPDATED: TableDefinition<&str, &str> = TableDefinition::new("updated");
const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: trueos-appdb-seed BUILDINS_JSON PACKAGE_DIR OUTPUT_DB UPDATED_UTC APPBUNDLE_JSON".into());
    }
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let names = manifest["buildins"]
        .as_array()
        .ok_or("missing buildins array")?;
    let packages = PathBuf::from(&args[1]);
    let destination = PathBuf::from(&args[2]);
    let updated = args[3].to_str().ok_or("invalid updated text")?;
    let bundle_json = fs::read(&args[4])?;
    let bundles = trueos_appbundle::AppBundles::parse(&bundle_json)?;
    // Never mutate the last complete image. A failed transaction leaves a temp file.
    let temporary = destination.with_extension("db.new");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let mut database = Database::builder()
        .set_cache_size(64 * 1024 * 1024)
        .create(&temporary)?;
    {
        let transaction = database.begin_write()?;
        {
            let mut apps = transaction.open_table(APPS)?;
            let mut hashes = transaction.open_table(HASHES)?;
            let mut dates = transaction.open_table(UPDATED)?;
            let mut metadata = transaction.open_table(METADATA)?;
            metadata.insert(trueos_appbundle::METADATA_KEY, bundle_json.as_slice())?;
            for name in names {
                let name = name.as_str().ok_or("invalid app name")?;
                let archive = format!("{name}.bp");
                let bytes = fs::read(packages.join(&archive))?;
                if bytes.len() < 24 || &bytes[..4] != b"TRBP" {
                    return Err(format!("invalid Blueprint package: {archive}").into());
                }
                let hash = sha256_hex(&bytes);
                apps.insert(archive.as_str(), bytes.as_slice())?;
                hashes.insert(archive.as_str(), hash.as_str())?;
                dates.insert(archive.as_str(), updated)?;
            }
            for name in names {
                let root = format!("{}.bp", name.as_str().unwrap());
                for member in bundles.members(&root)? {
                    if apps.get(member.as_str())?.is_none() {
                        return Err(format!(
                            "{root}: required Blueprint {member} is missing from the seed"
                        )
                        .into());
                    }
                }
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
        if apps.len()? != names.len() as u64
            || hashes.len()? != apps.len()?
            || dates.len()? != apps.len()?
        {
            return Err("seed tables do not contain the complete app set".into());
        }
        for name in names {
            let archive = format!("{}.bp", name.as_str().unwrap());
            let bytes = apps.get(archive.as_str())?.ok_or("missing app")?;
            let expected = sha256_hex(bytes.value());
            if hashes.get(archive.as_str())?.ok_or("missing hash")?.value() != expected {
                return Err(format!("seed hash mismatch: {archive}").into());
            }
        }
    }
    drop(database);
    fs::rename(temporary, &destination)?;
    println!(
        "app.db: {} apps, {} bytes",
        names.len(),
        fs::metadata(destination)?.len()
    );
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in Sha256::digest(bytes) {
        hex.push(DIGITS[(byte >> 4) as usize] as char);
        hex.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    hex
}
