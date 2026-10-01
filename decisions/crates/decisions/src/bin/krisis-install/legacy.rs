//! Exact readers for retained Decisions and Krisis shell releases.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use cell_install::{Error, Result, file_digest};

#[derive(Clone, Debug)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub format: u32,
    pub files: BTreeMap<String, String>,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(message))
    }
}

pub fn hexadecimal(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn pairs(path: &Path) -> Result<Vec<(String, String)>> {
    file_digest(path)?;
    let text = fs::read_to_string(path)?;
    require(text.ends_with('\n'), "release receipt is not canonical")?;
    let mut seen = BTreeSet::new();
    text.lines()
        .map(|line| {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::new("invalid release receipt"))?;
            require(
                seen.insert(key.to_owned()),
                "duplicate release receipt field",
            )?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn inventory(root: &Path, directory: &Path, output: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            inventory(root, &path, output)?;
        } else {
            let name = path
                .strip_prefix(root)
                .map_err(|_| Error::new("invalid release path"))?
                .to_str()
                .ok_or_else(|| Error::new("release path is not UTF-8"))?
                .to_owned();
            output.insert(name, file_digest(&path)?);
        }
    }
    Ok(())
}

pub fn read(root: &Path, _uid: u32) -> Result<Release> {
    let fields: BTreeMap<_, _> = pairs(&root.join("manifest.txt"))?.into_iter().collect();
    let field = |name: &str| {
        fields
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new(format!("missing release {name}")))
    };
    let mut files = BTreeMap::new();
    inventory(root, root, &mut files)?;
    Ok(Release {
        id: field("release_id")?,
        version: field("version")?,
        format: field("format")?
            .parse()
            .map_err(|_| Error::new("invalid retained release format"))?,
        files,
    })
}
