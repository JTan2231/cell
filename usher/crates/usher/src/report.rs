use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::evidence::{self, Finding, Problem, Status};
use crate::inventory;

#[derive(Serialize)]
pub(crate) struct ProductReport {
    id: String,
    name: String,
    root: Option<String>,
    aliases: Vec<String>,
    descriptor: String,
    identity: Finding,
    semantics: Finding,
    chancery: Finding,
    complete: bool,
}

impl ProductReport {
    fn load(root: &Path, descriptor: String, id: String) -> Self {
        let mut identity = Finding::new();
        identity.evidence.push(descriptor.clone());
        match inventory::load(root, &descriptor, &id) {
            Ok(product) => {
                identity.identities.push(product.id.clone());
                identity.evidence.push(product.root.clone());
                Self {
                    semantics: evidence::semantics(root, &product),
                    chancery: evidence::chancery(root, &product, &descriptor),
                    id: product.id,
                    name: product.name,
                    root: Some(product.root),
                    aliases: product.aliases,
                    descriptor,
                    identity,
                    complete: false,
                }
            }
            Err(problem) => {
                identity.fail(&descriptor, problem);
                let mut semantics = Finding::new();
                semantics.fail(
                    &descriptor,
                    Problem::new(
                        Status::Unassessed,
                        "product identity/root could not be established",
                    ),
                );
                let mut chancery = Finding::new();
                chancery.fail(
                    &descriptor,
                    Problem::new(
                        Status::Unassessed,
                        "product identity/root could not be established",
                    ),
                );
                Self {
                    name: id.clone(),
                    id,
                    root: None,
                    aliases: vec![],
                    descriptor,
                    identity,
                    semantics,
                    chancery,
                    complete: false,
                }
            }
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Report {
    schema_version: u32,
    scope: &'static str,
    products: Vec<ProductReport>,
    complete: usize,
    pub(crate) incomplete: usize,
}

pub(crate) fn inspect(root: &Path, selection: Option<&str>) -> Result<Report, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("cannot open checkout root: {e}"))?;
    let inventory_path = evidence::directory(&root, "pipeline/products")
        .map_err(|e| format!("cannot read product inventory: {}", e.message))?;
    let mut paths = fs::read_dir(inventory_path)
        .map_err(|e| format!("cannot enumerate inventory: {e}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot enumerate inventory: {e}"))?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "sh"));
    paths.sort();
    if paths.is_empty() || paths.len() > 256 {
        return Err("inventory must contain 1 through 256 product descriptors".to_owned());
    }
    let mut products = Vec::new();
    for path in paths {
        let id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or("descriptor filenames must be UTF-8")?
            .to_owned();
        products.push(ProductReport::load(
            &root,
            format!("pipeline/products/{id}.sh"),
            id,
        ));
    }
    check_collisions(&mut products);
    for product in &mut products {
        product.complete = [&product.identity, &product.semantics, &product.chancery]
            .iter()
            .all(|finding| finding.status == Status::Declared);
    }
    if let Some(selection) = selection {
        products.retain(|product| {
            product.id == selection || product.aliases.iter().any(|alias| alias == selection)
        });
        if products.len() != 1 {
            return Err(format!(
                "product selection {selection:?} is unknown or ambiguous"
            ));
        }
    }
    let complete = products.iter().filter(|product| product.complete).count();
    Ok(Report {
        schema_version: 1,
        scope: "repository_declarations",
        incomplete: products.len() - complete,
        products,
        complete,
    })
}

// Check the complete inventory even when reporting one product. A selected
// product must not become recognized by hiding another claimant to its IDs.
fn check_collisions(products: &mut [ProductReport]) {
    let mut claims: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    for (index, product) in products.iter().enumerate() {
        if let Some(root) = &product.root {
            claims.entry((0, root.clone())).or_default().push(index);
        }
        let names: BTreeSet<_> = std::iter::once(&product.id)
            .chain(&product.aliases)
            .collect();
        for name in names {
            claims.entry((1, name.clone())).or_default().push(index);
        }
        for id in &product.semantics.identities {
            claims.entry((2, id.clone())).or_default().push(index);
        }
        for id in &product.chancery.identities {
            claims.entry((3, id.clone())).or_default().push(index);
        }
    }
    for ((kind, claim), owners) in claims {
        if owners.len() < 2 {
            continue;
        }
        let owner_names = owners
            .iter()
            .map(|index| products[*index].id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let label = [
            "product root",
            "product ID/alias",
            "Semantics project",
            "Chancery provider",
        ][kind];
        for owner in owners {
            let product = &mut products[owner];
            let finding = match kind {
                2 => &mut product.semantics,
                3 => &mut product.chancery,
                _ => &mut product.identity,
            };
            finding.fail(
                &product.descriptor,
                Problem::invalid(format!("duplicate {label} {claim:?}: {owner_names}")),
            );
        }
    }
}

impl Report {
    pub(crate) fn render(&self, output: &mut impl Write) -> io::Result<()> {
        writeln!(output, "Cell recognition — repository declarations\n")?;
        for product in &self.products {
            writeln!(
                output,
                "{} ({}) — {}",
                product.name,
                product.id,
                if product.complete {
                    "complete"
                } else {
                    "incomplete"
                }
            )?;
            for (name, finding) in [
                ("identity", &product.identity),
                ("semantics", &product.semantics),
                ("chancery", &product.chancery),
            ] {
                writeln!(
                    output,
                    "  {name:10} {:10} {}",
                    finding.status.label(),
                    finding.identities.join(", ")
                )?;
                for path in &finding.evidence {
                    writeln!(output, "    {path}")?;
                }
                for description in finding.descriptions() {
                    writeln!(output, "    ! {description}")?;
                }
            }
            writeln!(output)?;
        }
        writeln!(
            output,
            "{} properly ushered; {} incomplete.",
            self.complete, self.incomplete
        )?;
        writeln!(
            output,
            "Registration, installation, quality, readiness, and other relationships are outside this report."
        )
    }
}
