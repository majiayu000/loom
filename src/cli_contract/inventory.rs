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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceInventory {
    pub surfaces: Vec<SurfaceSpec>,
    pub examples: Vec<SurfaceExample>,
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
    if surfaces.is_empty() || examples.is_empty() {
        return Err(InventoryError::new(format!(
            "{}: surface and example inventories must not be empty",
            inventory_path.display()
        )));
    }
    validate_unique_ids(&surfaces, &examples)?;
    Ok(SurfaceInventory { surfaces, examples })
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
            let start_line = required_integer(table, "start_line", &location)?;
            let end_line = required_integer(table, "end_line", &location)?;
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

fn required_string(table: &Table, key: &str, location: &str) -> Result<String, InventoryError> {
    table
        .get(key)
        .and_then(Item::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| InventoryError::new(format!("{location}: missing non-empty '{key}'")))
}

fn required_integer(table: &Table, key: &str, location: &str) -> Result<usize, InventoryError> {
    let value = table
        .get(key)
        .and_then(Item::as_integer)
        .ok_or_else(|| InventoryError::new(format!("{location}: missing integer '{key}'")))?;
    usize::try_from(value)
        .map_err(|_| InventoryError::new(format!("{location}: '{key}' must be positive")))
}

fn validate_unique_ids(
    surfaces: &[SurfaceSpec],
    examples: &[SurfaceExample],
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
    Ok(())
}
