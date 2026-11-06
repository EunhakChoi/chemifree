//! Closed, data-only native clipboard encoding; never a list of executable commands.

use chemifree_domain::clipboard::{
    Fragment, FragmentAtom, FragmentBond, FragmentError, FragmentStereo,
};
use chemifree_domain::document::{Atom, AtomId, Bond, BondId, Document, DocumentId, Point, Vector};
use chemifree_domain::element::Element;
use chemifree_domain::settings::Settings;
use chemifree_domain::states::{AtomKind, QueryAtom};
use chemifree_domain::stereo::{CenterState, DoubleBondState, Slot, StereoKey, StereoState};
use serde::{Deserialize, Serialize};

use crate::contract::{
    self, AtomConfigurationDto, BondConfigurationDto, BondOrderDto, BondQueryDto, HydrogensDto,
    MarkDto, QueryDto, RadicalDto, SettingsDto, SettingsEditDto,
};
use crate::{ApplicationError, ErrorCode};

pub(crate) const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

pub(crate) struct Paste<'a> {
    pub expected_revision: u64,
    pub payload: &'a str,
    pub offset: crate::contract::PointDto,
    pub selection_before: Option<crate::contract::SelectionDto>,
    pub fragment: &'a Fragment,
}

// Read the common header before imposing the closed version-1 body schema.
// Future versions may have different body fields; they remain unsupported.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: String,
    version: u32,
    settings: SettingsDto,
    atoms: Vec<AtomData>,
    bonds: Vec<BondData>,
    stereo: Vec<StereoData>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AtomData {
    id: String,
    kind: KindData,
    charge: i8,
    #[serde(deserialize_with = "required_nullable")]
    isotope: Option<u16>,
    hydrogens: HydrogensDto,
    radical: RadicalDto,
    attachment_points: Vec<u64>,
    position: PointData,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum KindData {
    Element { symbol: String },
    Query { query: QueryData },
}

// Native records carry complete state. The public edit QueryDto intentionally
// defaults list.negated, so it cannot deserialize this whole-record format.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum QueryData {
    Any {},
    AnyHeavy {},
    Heteroatom {},
    Halogen {},
    List {
        elements: Vec<String>,
        negated: bool,
    },
}

impl From<&QueryAtom> for QueryData {
    fn from(query: &QueryAtom) -> Self {
        match query {
            QueryAtom::Any => Self::Any {},
            QueryAtom::AnyHeavy => Self::AnyHeavy {},
            QueryAtom::Heteroatom => Self::Heteroatom {},
            QueryAtom::Halogen => Self::Halogen {},
            QueryAtom::List { elements, negated } => Self::List {
                elements: elements
                    .iter()
                    .map(|element| element.symbol().to_owned())
                    .collect(),
                negated: *negated,
            },
        }
    }
}

impl From<QueryData> for QueryDto {
    fn from(query: QueryData) -> Self {
        match query {
            QueryData::Any {} => Self::Any {},
            QueryData::AnyHeavy {} => Self::AnyHeavy {},
            QueryData::Heteroatom {} => Self::Heteroatom {},
            QueryData::Halogen {} => Self::Halogen {},
            QueryData::List { elements, negated } => Self::List { elements, negated },
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BondData {
    id: String,
    begin: String,
    end: String,
    order: BondOrderDto,
    #[serde(deserialize_with = "required_nullable")]
    query: Option<BondQueryDto>,
    #[serde(deserialize_with = "required_nullable")]
    mark: Option<MarkDto>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StereoData {
    Atom {
        atom: String,
        configuration: AtomConfigurationDto,
    },
    Bond {
        bond: String,
        configuration: BondConfigurationDto,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PointData {
    x: f64,
    y: f64,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(crate) fn refusal(reason: &'static str, message: impl Into<String>) -> ApplicationError {
    ApplicationError {
        invalid_clipboard: Some(Box::new(crate::error::ClipboardRefusal { reason })),
        ..ApplicationError::new(ErrorCode::InvalidCommand, message)
    }
}

pub(crate) fn domain_refusal(error: FragmentError) -> ApplicationError {
    refusal(error.as_str(), error.to_string())
}

fn invalid(message: impl Into<String>) -> ApplicationError {
    refusal("clipboard_fragment_invalid", message)
}

fn atom_id(text: &str) -> Result<AtomId, ApplicationError> {
    AtomId::parse(text)
        .ok_or_else(|| invalid(format!("Invalid clip-local atom identifier {text:?}")))
}

fn bond_id(text: &str) -> Result<BondId, ApplicationError> {
    BondId::parse(text)
        .ok_or_else(|| invalid(format!("Invalid clip-local bond identifier {text:?}")))
}

fn slot(text: &str) -> Result<Slot, ApplicationError> {
    match text {
        "implicit_hydrogen" => Ok(Slot::ImplicitHydrogen),
        "lone_pair" => Ok(Slot::LonePair),
        _ => atom_id(text).map(Slot::Atom),
    }
}

fn slots(values: &[String; 4]) -> Result<[Slot; 4], ApplicationError> {
    Ok([
        slot(&values[0])?,
        slot(&values[1])?,
        slot(&values[2])?,
        slot(&values[3])?,
    ])
}

fn settings(value: SettingsDto) -> Result<Settings, ApplicationError> {
    Settings::default()
        .edited(&contract::to_settings_edit(&SettingsEditDto {
            bond_length_pt: Some(value.bond_length_pt),
            line_width_pt: Some(value.line_width_pt),
            label_font: Some(value.label_font),
            label_weight: Some(value.label_weight),
            label_style: Some(value.label_style),
            label_underline: Some(value.label_underline),
            label_outline: Some(value.label_outline),
            label_shadow: Some(value.label_shadow),
            label_size_pt: Some(value.label_size_pt),
            carbon_labels: Some(value.carbon_labels),
            display_unit: Some(value.display_unit),
        }))
        .map_err(|error| invalid(error.to_string()))
}

pub(crate) fn encode(fragment: &Fragment) -> Result<String, ApplicationError> {
    let atoms = fragment
        .atoms
        .iter()
        .map(|entry| AtomData {
            id: entry.id.to_string(),
            kind: match &entry.atom.kind {
                AtomKind::Element(element) => KindData::Element {
                    symbol: element.symbol().into(),
                },
                AtomKind::Query(query) => KindData::Query {
                    query: query.into(),
                },
            },
            charge: entry.atom.charge,
            isotope: entry.atom.isotope,
            hydrogens: entry.atom.hydrogens.into(),
            radical: entry.atom.radical.into(),
            attachment_points: entry
                .atom
                .attachment_points
                .values()
                .map(u64::from)
                .collect(),
            position: PointData {
                x: entry.position.x(),
                y: entry.position.y(),
            },
        })
        .collect();
    let bonds = fragment
        .bonds
        .iter()
        .map(|entry| BondData {
            id: entry.id.to_string(),
            begin: entry.bond.begin.to_string(),
            end: entry.bond.end.to_string(),
            order: (&entry.bond.order).into(),
            query: match &entry.bond.order {
                chemifree_domain::document::BondOrder::Query(query) => Some(query.into()),
                _ => None,
            },
            mark: entry.bond.mark.map(Into::into),
        })
        .collect();
    let stereo = fragment
        .stereo
        .iter()
        .map(|entry| match (entry.key, entry.state) {
            (StereoKey::Center(atom), StereoState::Center(state)) => Ok(StereoData::Atom {
                atom: atom.to_string(),
                configuration: contract::center_dto(&state),
            }),
            (StereoKey::DoubleBond(bond), StereoState::DoubleBond(state)) => Ok(StereoData::Bond {
                bond: bond.to_string(),
                configuration: contract::double_bond_dto(&state),
            }),
            _ => Err(invalid("A clipboard stereo record has the wrong kind")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let payload = Payload {
        format: "chemifree.clipboard".into(),
        version: 1,
        settings: contract::settings_view(&fragment.settings),
        atoms,
        bonds,
        stereo,
    };
    let text = serde_json::to_string(&payload).map_err(|error| invalid(error.to_string()))?;
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(refusal(
            "clipboard_byte_limit",
            "Native clipboard text exceeds 65536 UTF-8 bytes",
        ));
    }
    Ok(text)
}

pub(crate) fn decode(text: &str) -> Result<Fragment, ApplicationError> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(refusal(
            "clipboard_byte_limit",
            "Native clipboard text exceeds 65536 UTF-8 bytes",
        ));
    }
    let value =
        crate::json::parse_strict(text.as_bytes()).map_err(|error| invalid(error.to_string()))?;
    if !value.is_object() {
        return Err(invalid("Native clipboard data must be a JSON object"));
    }
    let header = Header::deserialize(&value).map_err(|error| invalid(error.to_string()))?;
    if header.format != "chemifree.clipboard" || header.version != 1 {
        return Err(refusal(
            "clipboard_format_unsupported",
            "Expected chemifree.clipboard version 1",
        ));
    }
    let payload: Payload =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    let mut atoms = Vec::with_capacity(payload.atoms.len());
    for entry in payload.atoms {
        let kind = match entry.kind {
            KindData::Element { symbol } => AtomKind::Element(
                Element::from_symbol(&symbol).ok_or_else(|| invalid("Unknown element symbol"))?,
            ),
            KindData::Query { query } => {
                let query: QueryDto = query.into();
                AtomKind::Query(contract::query_atom(&query).map_err(invalid)?)
            }
        };
        atoms.push(FragmentAtom {
            id: atom_id(&entry.id)?,
            position: Point::new(entry.position.x, entry.position.y)
                .map_err(|error| invalid(error.to_string()))?,
            atom: Atom {
                kind,
                charge: entry.charge,
                isotope: entry.isotope,
                hydrogens: contract::to_hydrogens(entry.hydrogens)
                    .map_err(|error| invalid(error.to_string()))?,
                radical: entry.radical.into(),
                attachment_points: contract::attachment_points(&entry.attachment_points)
                    .map_err(|error| invalid(error.to_string()))?,
            },
        });
    }
    let mut bonds = Vec::with_capacity(payload.bonds.len());
    for entry in payload.bonds {
        bonds.push(FragmentBond {
            id: bond_id(&entry.id)?,
            bond: Bond {
                begin: atom_id(&entry.begin)?,
                end: atom_id(&entry.end)?,
                order: contract::bond_order(entry.order, entry.query.as_ref())
                    .map_err(|error| invalid(error.to_string()))?,
                mark: entry.mark.as_ref().map(Into::into),
            },
        });
    }
    let mut stereo = Vec::with_capacity(payload.stereo.len());
    for entry in payload.stereo {
        stereo.push(match entry {
            StereoData::Atom {
                atom,
                configuration,
            } => FragmentStereo {
                key: StereoKey::Center(atom_id(&atom)?),
                state: StereoState::Center(match configuration {
                    AtomConfigurationDto::Tetrahedral { neighbors, turn } => {
                        CenterState::Tetrahedral {
                            slots: slots(&neighbors)?,
                            turn: turn.into(),
                        }
                    }
                    AtomConfigurationDto::Unknown { neighbors } => CenterState::Unknown {
                        slots: slots(&neighbors)?,
                    },
                }),
            },
            StereoData::Bond {
                bond,
                configuration,
            } => FragmentStereo {
                key: StereoKey::DoubleBond(bond_id(&bond)?),
                state: StereoState::DoubleBond(match configuration {
                    BondConfigurationDto::Relative {
                        begin_neighbor,
                        end_neighbor,
                        relation,
                    } => DoubleBondState::Relative {
                        begin_ref: slot(&begin_neighbor)?,
                        end_ref: slot(&end_neighbor)?,
                        relation: relation.into(),
                    },
                    BondConfigurationDto::Unknown {} => DoubleBondState::Unknown,
                }),
            },
        });
    }
    let fragment = Fragment {
        settings: settings(payload.settings)?,
        atoms,
        bonds,
        stereo,
    };
    // Validate untrusted graph/stereo independently of destination state or request replay.
    let scratch = Document::new_with_settings(
        DocumentId::parse("clipboard-validation")
            .ok_or_else(|| invalid("Invalid validation identifier"))?,
        "Clipboard validation",
        fragment.settings.clone(),
    )
    .map_err(|error| invalid(error.to_string()))?;
    fragment
        .paste_commands(&scratch, Vector { dx: 0.0, dy: 0.0 })
        .map_err(domain_refusal)?;
    Ok(fragment)
}
