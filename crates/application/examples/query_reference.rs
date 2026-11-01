//! Small executable boundary for an independent substructure comparison.
//!
//! Reads one JSON request from standard input, runs the bounded matcher, and prints one JSON
//! response with 0-based atom positions (creation order) instead of generated atom IDs, so an
//! outside reader needs no knowledge of the ID format. No reference code is linked.
use std::error::Error;
use std::io::{self, Read};

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::query::{self, AtomPredicate, MatchModel, QueryError};
use chemifree_domain::states::{AtomKind, BondQuery, QueryAtom, QueryBondOrder};
use serde_json::{Value, json};

fn element(symbol: &Value) -> Result<Element, Box<dyn Error>> {
    let symbol = symbol.as_str().ok_or("element symbol must be a string")?;
    Element::from_symbol(symbol).ok_or_else(|| format!("unknown element {symbol}").into())
}

fn atom(spec: &Value) -> Result<Atom, Box<dyn Error>> {
    if let Some(kind) = spec.get("query").and_then(Value::as_str) {
        let query_atom = match kind {
            "any" => QueryAtom::Any,
            "list" => QueryAtom::List {
                elements: spec["elements"]
                    .as_array()
                    .ok_or("list needs elements")?
                    .iter()
                    .map(element)
                    .collect::<Result<_, _>>()?,
                negated: false,
            },
            other => return Err(format!("unknown query atom {other}").into()),
        };
        return Ok(Atom::new(AtomKind::Query(query_atom)));
    }
    let mut atom = Atom::from(element(&spec["el"])?);
    atom.charge = i8::try_from(spec["charge"].as_i64().unwrap_or(0))?;
    Ok(atom)
}

fn order(name: &str) -> Result<BondOrder, Box<dyn Error>> {
    Ok(match name {
        "single" => BondOrder::Single,
        "double" => BondOrder::Double,
        "triple" => BondOrder::Triple,
        "aromatic" => BondOrder::Aromatic,
        "any" => BondOrder::Query(BondQuery::Any),
        "single_or_double" => BondOrder::Query(BondQuery::OneOf(vec![
            QueryBondOrder::Single,
            QueryBondOrder::Double,
        ])),
        "unknown" => BondOrder::Unknown,
        other => return Err(format!("unknown bond order {other}").into()),
    })
}

fn molecule(name: &str, spec: &Value) -> Result<MoleculeGraph, Box<dyn Error>> {
    let mut document = Document::new(DocumentId::parse(name).ok_or("bad id")?, name)?;
    let mut commands = Vec::new();
    for item in spec["atoms"].as_array().ok_or("atoms must be a list")? {
        commands.push(Command::AddAtom {
            atom: atom(item)?,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0)?),
            bond_to: None,
        });
    }
    for bond in spec["bonds"].as_array().ok_or("bonds must be a list")? {
        let index = |slot: usize| -> Result<usize, Box<dyn Error>> {
            Ok(usize::try_from(bond[slot].as_u64().ok_or("bond index")?)?)
        };
        commands.push(Command::AddBond {
            begin: AtomRef::Created(index(0)?),
            end: AtomRef::Created(index(1)?),
            order: order(bond[2].as_str().ok_or("bond order")?)?,
        });
    }
    // An empty drawing is a valid input for the empty-query refusal; a transaction cannot be.
    if !commands.is_empty() {
        document.apply(&commands)?;
    }
    Ok(MoleculeGraph::from_document(&document))
}

fn refusal_code(error: QueryError) -> &'static str {
    match error {
        QueryError::EmptyQuery => "empty_query",
        QueryError::QueryLimit => "query_limit",
        QueryError::InvalidBounds => "invalid_bounds",
        QueryError::InvalidPredicate => "invalid_predicate",
        QueryError::UnsupportedQuery => "unsupported_query",
        QueryError::UnsupportedTarget => "unsupported_target",
        QueryError::AromaticTargetUnsupported(_) => "aromatic_target_unsupported",
    }
}

fn position(graph: &MoleculeGraph, id: AtomId) -> usize {
    graph
        .atoms()
        .position(|(candidate, _)| candidate == id)
        .expect("mapped ID is in its graph")
}

fn run(request: &Value) -> Result<Value, Box<dyn Error>> {
    let query_graph = molecule("query", &request["query"])?;
    let target_graph = molecule("target", &request["target"])?;
    let query_ids: Vec<AtomId> = query_graph.atoms().map(|(id, _)| id).collect();
    let mut predicates = Vec::new();
    for item in request["predicates"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
    {
        let index = usize::try_from(item["atom"].as_u64().ok_or("predicate atom")?)?;
        predicates.push(AtomPredicate {
            // An index past the query yields an ID the query lacks, exercising the refusal.
            atom: query_ids
                .get(index)
                .copied()
                .unwrap_or(AtomId::from_serial(9999).ok_or("serial")?),
            ring: item["ring"].as_bool(),
            min_neighbors: item["min_neighbors"]
                .as_u64()
                .map(usize::try_from)
                .transpose()?,
            max_neighbors: item["max_neighbors"]
                .as_u64()
                .map(usize::try_from)
                .transpose()?,
            charge: item["charge"].as_i64().map(i8::try_from).transpose()?,
        });
    }
    let model = match request["mode"].as_str() {
        Some("recorded") => MatchModel::Recorded,
        Some("aromatic_target") => MatchModel::AromaticTarget,
        _ => return Err("mode must be recorded or aromatic_target".into()),
    };
    let bound = |key: &str| -> Result<usize, Box<dyn Error>> {
        Ok(usize::try_from(request[key].as_u64().ok_or("bound")?)?)
    };
    match query::find_graphs(
        &query_graph,
        &target_graph,
        &predicates,
        bound("max_matches")?,
        bound("max_states")?,
        model,
    ) {
        Ok(result) => {
            let mappings: Vec<Vec<usize>> = result
                .mappings
                .iter()
                .map(|mapping| {
                    // Entries arrive in query-atom order; report the target position of each.
                    mapping
                        .iter()
                        .map(|(_, target)| position(&target_graph, *target))
                        .collect()
                })
                .collect();
            Ok(json!({"outcome": "ok", "complete": result.complete, "mappings": mappings}))
        }
        Err(error) => Ok(json!({"outcome": "refused", "code": refusal_code(error)})),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    println!("{}", run(&serde_json::from_str(&text)?)?);
    Ok(())
}
