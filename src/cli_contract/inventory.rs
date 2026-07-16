use std::{collections::BTreeSet, fmt, fs, path::Path};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

pub const INVENTORY_PATH: &str = "docs/agent-command-surfaces.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleClassification {
    Executable,
    OutputExample,
    Legacy,
    NonCommand,
}

impl ExampleClassification {
    fn parse(value: &str, location: &str) -> Result<Self, InventoryError> {
        match value {
            "executable" => Ok(Self::Executable),
            "output_example" => Ok(Self::OutputExample),
            "legacy" => Ok(Self::Legacy),
            "non_command" => Ok(Self::NonCommand),
            _ => Err(InventoryError::new(format!(
                "{location}: classification '{value}' is outside the closed classification set"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSpec {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceExample {
    pub id: String,
    pub surface: String,
    pub start_line: usize,
    pub end_line: usize,
    pub classification: ExampleClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextActionShape {
    String,
    Object,
}

impl NextActionShape {
    fn parse(value: &str, location: &str) -> Result<Self, InventoryError> {
        match value {
            "string" => Ok(Self::String),
            "object" => Ok(Self::Object),
            _ => Err(InventoryError::new(format!(
                "{location}: next-action shape '{value}' is outside the closed shape set"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextActionEmitter {
    pub id: String,
    pub source: String,
    pub shape: NextActionShape,
    pub fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelBinding {
    CliEquivalent,
    NoCliEquivalent,
}

impl PanelBinding {
    fn parse(value: &str, location: &str) -> Result<Self, InventoryError> {
        match value {
            "cli_equivalent" => Ok(Self::CliEquivalent),
            "no_cli_equivalent" => Ok(Self::NoCliEquivalent),
            _ => Err(InventoryError::new(format!(
                "{location}: panel binding '{value}' is outside the closed binding set"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelMutation {
    pub id: String,
    pub label_path: String,
    pub action_id: String,
    pub backend_route: String,
    pub handler: String,
    pub binding: PanelBinding,
    pub cli_argv: Vec<String>,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceInventory {
    pub surfaces: Vec<SurfaceSpec>,
    pub examples: Vec<SurfaceExample>,
    pub next_action_emitters: Vec<NextActionEmitter>,
    pub panel_mutations: Vec<PanelMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryError {
    message: String,
}

impl InventoryError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InventoryError {}

pub fn load_surface_inventory(repo_root: &Path) -> Result<SurfaceInventory, InventoryError> {
    let inventory_path = repo_root.join(INVENTORY_PATH);
    let source = fs::read_to_string(&inventory_path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", inventory_path.display())))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{}: {error}", inventory_path.display())))?;
    let surfaces = parse_surfaces(required_tables(&document, "surface")?)?;
    let examples = parse_examples(required_tables(&document, "example")?)?;
    let next_action_emitters =
        parse_next_action_emitters(required_tables(&document, "next_action_emitter")?)?;
    let panel_mutations = parse_panel_mutations(required_tables(&document, "panel_mutation")?)?;
    if surfaces.is_empty() || examples.is_empty() {
        return Err(InventoryError::new(format!(
            "{}: surface and example inventories must not be empty",
            inventory_path.display()
        )));
    }
    if next_action_emitters.is_empty() || panel_mutations.is_empty() {
        return Err(InventoryError::new(format!(
            "{}: next-action emitter and panel mutation inventories must not be empty",
            inventory_path.display()
        )));
    }
    validate_unique_ids(
        &surfaces,
        &examples,
        &next_action_emitters,
        &panel_mutations,
    )?;
    Ok(SurfaceInventory {
        surfaces,
        examples,
        next_action_emitters,
        panel_mutations,
    })
}

fn required_tables<'a>(
    document: &'a DocumentMut,
    key: &str,
) -> Result<&'a ArrayOfTables, InventoryError> {
    document
        .get(key)
        .and_then(Item::as_array_of_tables)
        .ok_or_else(|| InventoryError::new(format!("{INVENTORY_PATH}: missing [[{key}]]")))
}

fn parse_surfaces(tables: &ArrayOfTables) -> Result<Vec<SurfaceSpec>, InventoryError> {
    tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let location = format!("{INVENTORY_PATH}:surface[{}]", index + 1);
            Ok(SurfaceSpec {
                id: required_string(table, "id", &location)?,
                path: required_string(table, "path", &location)?,
            })
        })
        .collect()
}

fn parse_examples(tables: &ArrayOfTables) -> Result<Vec<SurfaceExample>, InventoryError> {
    tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let location = format!("{INVENTORY_PATH}:example[{}]", index + 1);
            let (start_line, end_line) = required_line_range(table, &location)?;
            if start_line == 0 || end_line < start_line {
                return Err(InventoryError::new(format!(
                    "{location}: invalid line range {start_line}..={end_line}"
                )));
            }
            Ok(SurfaceExample {
                id: required_string(table, "id", &location)?,
                surface: required_string(table, "surface", &location)?,
                start_line,
                end_line,
                classification: ExampleClassification::parse(
                    &required_string(table, "classification", &location)?,
                    &location,
                )?,
            })
        })
        .collect()
}

fn parse_next_action_emitters(
    tables: &ArrayOfTables,
) -> Result<Vec<NextActionEmitter>, InventoryError> {
    tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let location = format!("{INVENTORY_PATH}:next_action_emitter[{}]", index + 1);
            Ok(NextActionEmitter {
                id: required_string(table, "id", &location)?,
                source: required_string(table, "source", &location)?,
                shape: NextActionShape::parse(
                    &required_string(table, "shape", &location)?,
                    &location,
                )?,
                fixture_ids: required_string_array(table, "fixture_ids", &location)?,
            })
        })
        .collect()
}

fn parse_panel_mutations(tables: &ArrayOfTables) -> Result<Vec<PanelMutation>, InventoryError> {
    tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let location = format!("{INVENTORY_PATH}:panel_mutation[{}]", index + 1);
            let binding =
                PanelBinding::parse(&required_string(table, "binding", &location)?, &location)?;
            let cli_argv = optional_string_array(table, "cli_argv", &location)?;
            let rationale = optional_string(table, "rationale", &location)?;
            match binding {
                PanelBinding::CliEquivalent if cli_argv.is_empty() => {
                    return Err(InventoryError::new(format!(
                        "{location}: cli_equivalent requires non-empty 'cli_argv'"
                    )));
                }
                PanelBinding::CliEquivalent if rationale.is_some() => {
                    return Err(InventoryError::new(format!(
                        "{location}: cli_equivalent must not declare a rationale"
                    )));
                }
                PanelBinding::NoCliEquivalent if !cli_argv.is_empty() => {
                    return Err(InventoryError::new(format!(
                        "{location}: no_cli_equivalent must not declare 'cli_argv'"
                    )));
                }
                PanelBinding::NoCliEquivalent if rationale.is_none() => {
                    return Err(InventoryError::new(format!(
                        "{location}: no_cli_equivalent requires a review-owned rationale"
                    )));
                }
                _ => {}
            }
            Ok(PanelMutation {
                id: required_string(table, "id", &location)?,
                label_path: required_string(table, "label_path", &location)?,
                action_id: required_string(table, "action_id", &location)?,
                backend_route: required_string(table, "backend_route", &location)?,
                handler: required_string(table, "handler", &location)?,
                binding,
                cli_argv,
                rationale,
            })
        })
        .collect()
}

fn required_string(table: &Table, key: &str, location: &str) -> Result<String, InventoryError> {
    table
        .get(key)
        .and_then(Item::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| InventoryError::new(format!("{location}: missing non-empty '{key}'")))
}

fn required_line_range(table: &Table, location: &str) -> Result<(usize, usize), InventoryError> {
    let values = table
        .get("line_range")
        .and_then(Item::as_array)
        .ok_or_else(|| InventoryError::new(format!("{location}: missing 'line_range' array")))?;
    if values.len() != 2 {
        return Err(InventoryError::new(format!(
            "{location}: 'line_range' must contain exactly two integers"
        )));
    }
    let parse = |index: usize| {
        let value = values
            .get(index)
            .and_then(|value| value.as_integer())
            .ok_or_else(|| {
                InventoryError::new(format!(
                    "{location}: line_range item {} must be an integer",
                    index + 1
                ))
            })?;
        usize::try_from(value).map_err(|_| {
            InventoryError::new(format!(
                "{location}: line_range item {} must be positive",
                index + 1
            ))
        })
    };
    Ok((parse(0)?, parse(1)?))
}

fn required_string_array(
    table: &Table,
    key: &str,
    location: &str,
) -> Result<Vec<String>, InventoryError> {
    let values = optional_string_array(table, key, location)?;
    if values.is_empty() {
        return Err(InventoryError::new(format!(
            "{location}: missing non-empty string array '{key}'"
        )));
    }
    Ok(values)
}

fn optional_string_array(
    table: &Table,
    key: &str,
    location: &str,
) -> Result<Vec<String>, InventoryError> {
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let array = item
        .as_array()
        .ok_or_else(|| InventoryError::new(format!("{location}: '{key}' must be an array")))?;
    array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    InventoryError::new(format!(
                        "{location}: '{key}' item {} must be a non-empty string",
                        index + 1
                    ))
                })
        })
        .collect()
}

fn optional_string(
    table: &Table,
    key: &str,
    location: &str,
) -> Result<Option<String>, InventoryError> {
    let Some(item) = table.get(key) else {
        return Ok(None);
    };
    item.as_str()
        .filter(|value| !value.is_empty())
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| InventoryError::new(format!("{location}: '{key}' must be non-empty")))
}

fn validate_unique_ids(
    surfaces: &[SurfaceSpec],
    examples: &[SurfaceExample],
    next_action_emitters: &[NextActionEmitter],
    panel_mutations: &[PanelMutation],
) -> Result<(), InventoryError> {
    let mut surface_ids = BTreeSet::new();
    for surface in surfaces {
        if !surface_ids.insert(surface.id.as_str()) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: duplicate surface id '{}'",
                surface.id
            )));
        }
    }
    let mut example_ids = BTreeSet::new();
    for example in examples {
        if !example_ids.insert(example.id.as_str()) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: duplicate example id '{}'",
                example.id
            )));
        }
        if !surface_ids.contains(example.surface.as_str()) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: example '{}' references unknown surface '{}'",
                example.id, example.surface
            )));
        }
    }
    let mut emitter_ids = BTreeSet::new();
    for emitter in next_action_emitters {
        if !emitter_ids.insert(emitter.id.as_str()) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: duplicate next-action emitter id '{}'",
                emitter.id
            )));
        }
        let mut fixture_ids = BTreeSet::new();
        for fixture_id in &emitter.fixture_ids {
            if !fixture_ids.insert(fixture_id.as_str()) {
                return Err(InventoryError::new(format!(
                    "{INVENTORY_PATH}: emitter '{}' repeats fixture id '{}'",
                    emitter.id, fixture_id
                )));
            }
        }
    }
    let mut panel_ids = BTreeSet::new();
    let mut panel_routes = BTreeSet::new();
    for mutation in panel_mutations {
        if !panel_ids.insert(mutation.id.as_str()) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: duplicate panel mutation id '{}'",
                mutation.id
            )));
        }
        if !panel_routes.insert((mutation.backend_route.as_str(), mutation.handler.as_str())) {
            return Err(InventoryError::new(format!(
                "{INVENTORY_PATH}: duplicate panel mutation route '{}' and handler '{}'",
                mutation.backend_route, mutation.handler
            )));
        }
    }
    Ok(())
}
