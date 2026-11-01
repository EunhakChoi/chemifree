//! Test utility: force a safe rewrite of an authored CDXML fixture on standard input.
use std::io::{self, Read};

use chemifree_domain::interchange::{Content, ObjectKind};
use chemifree_providers::cdxml;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut document = cdxml::read(&input)?;
    document.document_mut().edit(|root| {
        let first_page = root.children.iter_mut().find_map(|child| match child {
            Content::Object(page) if page.kind == ObjectKind::Page => Some(page),
            _ => None,
        });
        if let Some(page) = first_page {
            page.id = Some(11);
        }
    })?;
    print!("{}", cdxml::write(&document)?);
    Ok(())
}
