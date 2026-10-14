//! Reader for GMNS (General Modeling Network Specification) datasets.
//!
//! A dataset is a directory of CSV files: `node.csv` and `link.csv` (required),
//! `movement.csv` and `config.csv` (optional). GMNS has no demand table, so demand is
//! read from a separate flat file, see [`read_demand`].
//!
//! Conventions of this reader:
//! - External ids (strings, whatever the file uses) become dense ids: node `i` is
//!   the `i`-th row of `node.csv`; links are numbered in file order. A link with
//!   `directed = false` becomes two links (forward copy, then reverse copy).
//!   A blank `directed` counts as directed, since real files often leave it empty.
//! - Times are in minutes: free-flow time = `length / free_speed`, turn penalties are
//!   converted from seconds. Capacity is `capacity * lanes` (GMNS capacity is per lane).
//! - A zone is the set of nodes that carry its `zone_id` (often one node, ADR 0001, but
//!   often several, ADR 0009). Demand between two zones is split equally over every
//!   (origin node, destination node) pair, see [`read_demand`].
//! - `movement.csv` restricts an inbound link at a node only if it lists at least one
//!   movement for that link: then only the listed movements are allowed, the rest are
//!   banned. Inbound links the table never mentions keep all their turns (the official
//!   Lima example leaves out the links that leave zone nodes, and banning every turn
//!   there would disconnect the network). Movements at endpoint nodes are ignored,
//!   because no turn exists there.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use pyta_core::{Edge, Network, NetworkError, TurnException, TurnRule};

use crate::tntp::Demand;

/// Knobs for reading a dataset. `Default` gives the behaviour described in the module docs.
#[derive(Debug, Clone)]
pub struct GmnsOptions {
    /// Unit of `length`, e.g. `"mile"`, `"km"`, `"foot"`. Overrides `config.csv`.
    /// Needed when there is no config, and when the config is wrong: the official
    /// examples declare miles for lengths that are clearly in feet.
    pub length_unit: Option<String>,
    /// Unit of `free_speed`, e.g. `"mph"`, `"kph"`. Overrides `config.csv`.
    pub speed_unit: Option<String>,
    /// Zone nodes may start or end a path but not be passed through (centroid style).
    /// Off by default: zones are ordinary intersections.
    pub zone_nodes_are_endpoints: bool,
    /// BPR parameters for links without `vdf_alpha` / `vdf_beta` values.
    pub default_b: f64,
    pub default_power: f64,
}

impl Default for GmnsOptions {
    fn default() -> Self {
        GmnsOptions {
            length_unit: None,
            speed_unit: None,
            zone_nodes_are_endpoints: false,
            default_b: 0.15,
            default_power: 4.0,
        }
    }
}

/// How to read the demand file. The default column names follow the DTALite-style
/// `demand.csv`; other files (the Lima example uses `orig_taz`, `dest_taz`, `total`)
/// just need other names.
#[derive(Debug, Clone)]
pub struct DemandOptions {
    pub origin: String,
    pub destination: String,
    pub flow: String,
    /// Skip rows whose zone has no node instead of failing. The skipped flow is
    /// reported in [`GmnsDemand::skipped_flow`].
    pub skip_unknown_zones: bool,
}

impl Default for DemandOptions {
    fn default() -> Self {
        DemandOptions {
            origin: "o_zone_id".to_string(),
            destination: "d_zone_id".to_string(),
            flow: "volume".to_string(),
            skip_unknown_zones: false,
        }
    }
}

/// Demand between nodes, expanded from zone-to-zone rows.
#[derive(Debug)]
pub struct GmnsDemand {
    /// `origins` and `destinations` are node ids. `num_zones` counts the network's zones.
    pub demand: Demand,
    /// Flow of rows dropped because a zone has no node (only with `skip_unknown_zones`).
    pub skipped_flow: f64,
}

/// A GMNS network. The per-link vectors are indexed by dense link id.
#[derive(Debug)]
pub struct GmnsNetwork {
    pub network: Network,
    /// Dense node id -> `node_id` in the file.
    pub node_ids: Vec<String>,
    /// Dense link id -> `link_id` in the file.
    pub link_ids: Vec<String>,
    /// True for the reverse copy of an undirected link.
    pub is_reverse_copy: Vec<bool>,
    /// Per node: paths may start or end here but not pass through.
    pub is_endpoint: Vec<bool>,
    /// Zone id in the file -> the nodes that carry it, in file order.
    pub zone_nodes: HashMap<String, Vec<u32>>,
    /// Bans and penalties derived from `movement.csv` (empty without it), ready
    /// for `TurnTable::build`. Penalties are in minutes.
    pub turn_exceptions: Vec<TurnException>,
    /// Minutes.
    pub free_flow_time: Vec<f64>,
    /// Vehicles per hour on the whole link (per-lane capacity times lanes).
    pub capacity: Vec<f64>,
    pub b: Vec<f64>,
    pub power: Vec<f64>,
    /// Metres.
    pub length: Vec<f64>,
}

#[derive(Debug)]
pub enum GmnsError {
    Io {
        file: String,
        source: std::io::Error,
    },
    Csv {
        file: &'static str,
        source: csv::Error,
    },
    MissingColumn {
        file: &'static str,
        column: &'static str,
    },
    /// `line` is 1-based and counts the header.
    Row {
        file: &'static str,
        line: u64,
        message: String,
    },
    DuplicateId {
        file: &'static str,
        id: String,
    },
    /// Neither the config nor the options say what unit `what` is in.
    MissingUnit(&'static str),
    UnknownUnit(String),
    /// Demand refers to a zone no node carries.
    UnknownZone(String),
    Network(NetworkError),
}

impl fmt::Display for GmnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GmnsError::Io { file, source } => write!(f, "cannot read {file}: {source}"),
            GmnsError::Csv { file, source } => write!(f, "{file}: {source}"),
            GmnsError::MissingColumn { file, column } => {
                write!(f, "{file}: missing column '{column}'")
            }
            GmnsError::Row {
                file,
                line,
                message,
            } => write!(f, "{file}.csv line {line}: {message}"),
            GmnsError::DuplicateId { file, id } => write!(f, "{file}.csv: duplicate id '{id}'"),
            GmnsError::MissingUnit(what) => {
                write!(f, "no {what} unit: set it in config.csv or in the options")
            }
            GmnsError::UnknownUnit(unit) => write!(f, "unknown unit '{unit}'"),
            GmnsError::UnknownZone(zone) => write!(f, "demand refers to unknown zone '{zone}'"),
            GmnsError::Network(e) => write!(f, "invalid network: {e:?}"),
        }
    }
}

impl std::error::Error for GmnsError {}

impl From<NetworkError> for GmnsError {
    fn from(e: NetworkError) -> Self {
        GmnsError::Network(e)
    }
}

// ---------------------------------------------------------------------------
// CSV plumbing
// ---------------------------------------------------------------------------

/// One CSV file: header positions plus all records.
struct Table {
    file: &'static str,
    columns: HashMap<String, usize>,
    records: Vec<csv::StringRecord>,
}

impl Table {
    fn parse(file: &'static str, reader: impl Read) -> Result<Table, GmnsError> {
        let csv_err = |source| GmnsError::Csv { file, source };
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true) // rows may be shorter than the header; missing cells read as blank
            .trim(csv::Trim::All)
            .from_reader(reader);
        let mut columns = HashMap::new();
        for (i, name) in reader.headers().map_err(csv_err)?.iter().enumerate() {
            columns.insert(name.to_string(), i);
        }
        let records = reader
            .records()
            .collect::<Result<Vec<_>, _>>()
            .map_err(csv_err)?;
        Ok(Table {
            file,
            columns,
            records,
        })
    }

    fn optional(&self, column: &str) -> Option<usize> {
        self.columns.get(column).copied()
    }

    fn required(&self, column: &'static str) -> Result<usize, GmnsError> {
        self.optional(column).ok_or(GmnsError::MissingColumn {
            file: self.file,
            column,
        })
    }

    fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.records.iter().map(|record| Row {
            file: self.file,
            record,
        })
    }
}

/// One record, with accessors that know the GMNS rule that `''` and `NaN` mean missing.
struct Row<'a> {
    file: &'static str,
    record: &'a csv::StringRecord,
}

impl Row<'_> {
    fn error(&self, message: String) -> GmnsError {
        GmnsError::Row {
            file: self.file,
            line: self.record.position().map_or(0, |p| p.line()),
            message,
        }
    }

    fn text(&self, column: usize) -> Option<&str> {
        match self.record.get(column) {
            None | Some("") | Some("NaN") => None,
            Some(value) => Some(value),
        }
    }

    fn required_text(&self, column: usize, name: &str) -> Result<&str, GmnsError> {
        self.text(column)
            .ok_or_else(|| self.error(format!("{name} is blank")))
    }

    fn number(&self, column: usize, name: &str) -> Result<Option<f64>, GmnsError> {
        let Some(text) = self.text(column) else {
            return Ok(None);
        };
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(Some(value)),
            _ => Err(self.error(format!("cannot parse {name} '{text}'"))),
        }
    }

    fn required_number(&self, column: usize, name: &str) -> Result<f64, GmnsError> {
        self.number(column, name)?
            .ok_or_else(|| self.error(format!("{name} is blank")))
    }

    /// Like `number`, for a column that may not exist at all.
    fn optional_number(&self, column: Option<usize>, name: &str) -> Result<Option<f64>, GmnsError> {
        match column {
            Some(c) => self.number(c, name),
            None => Ok(None),
        }
    }
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.to_lowercase().as_str() {
        "true" | "t" | "1" | "yes" => Some(true),
        "false" | "f" | "0" | "no" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

fn metres_per_length_unit(unit: &str) -> Result<f64, GmnsError> {
    match unit.trim().to_lowercase().as_str() {
        "m" | "meter" | "meters" | "metre" | "metres" => Ok(1.0),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => Ok(1000.0),
        "ft" | "foot" | "feet" => Ok(0.3048),
        "mi" | "mile" | "miles" => Ok(1609.344),
        _ => Err(GmnsError::UnknownUnit(unit.to_string())),
    }
}

fn metres_per_hour_per_speed_unit(unit: &str) -> Result<f64, GmnsError> {
    match unit.trim().to_lowercase().as_str() {
        "kph" | "kmph" | "kmh" | "km/h" | "km/hr" => Ok(1000.0),
        "mph" | "mi/h" | "mi/hr" => Ok(1609.344),
        "mps" | "m/s" => Ok(3600.0),
        "fps" | "ft/s" => Ok(0.3048 * 3600.0),
        _ => Err(GmnsError::UnknownUnit(unit.to_string())),
    }
}

/// The unit the options give, else the one in `config.csv`, else an error.
fn resolve_unit(
    option: &Option<String>,
    config: Option<(&Table, &'static str)>,
    what: &'static str,
) -> Result<String, GmnsError> {
    if let Some(unit) = option {
        return Ok(unit.clone());
    }
    if let Some((table, column)) = config
        && let (Some(c), Some(record)) = (table.optional(column), table.records.first())
    {
        let row = Row {
            file: table.file,
            record,
        };
        if let Some(unit) = row.text(c) {
            return Ok(unit.to_string());
        }
    }
    Err(GmnsError::MissingUnit(what))
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn open(path: &Path) -> Result<fs::File, GmnsError> {
    fs::File::open(path).map_err(|source| GmnsError::Io {
        file: path.display().to_string(),
        source,
    })
}

/// Reads `node.csv`, `link.csv` and, when present, `movement.csv` and `config.csv`
/// from `dir`.
pub fn read_network(
    dir: impl AsRef<Path>,
    options: &GmnsOptions,
) -> Result<GmnsNetwork, GmnsError> {
    let dir = dir.as_ref();
    let optional = |name: &str| -> Result<Option<fs::File>, GmnsError> {
        let path = dir.join(name);
        if path.exists() {
            Ok(Some(open(&path)?))
        } else {
            Ok(None)
        }
    };
    let nodes = Table::parse("node", open(&dir.join("node.csv"))?)?;
    let links = Table::parse("link", open(&dir.join("link.csv"))?)?;
    let movements = optional("movement.csv")?
        .map(|f| Table::parse("movement", f))
        .transpose()?;
    let config = optional("config.csv")?
        .map(|f| Table::parse("config", f))
        .transpose()?;
    build(&nodes, &links, movements.as_ref(), config.as_ref(), options)
}

/// Same as [`read_network`], from the contents of the files.
pub fn parse_network(
    node_csv: &str,
    link_csv: &str,
    movement_csv: Option<&str>,
    config_csv: Option<&str>,
    options: &GmnsOptions,
) -> Result<GmnsNetwork, GmnsError> {
    let nodes = Table::parse("node", node_csv.as_bytes())?;
    let links = Table::parse("link", link_csv.as_bytes())?;
    let movements = movement_csv
        .map(|t| Table::parse("movement", t.as_bytes()))
        .transpose()?;
    let config = config_csv
        .map(|t| Table::parse("config", t.as_bytes()))
        .transpose()?;
    build(&nodes, &links, movements.as_ref(), config.as_ref(), options)
}

fn build(
    nodes: &Table,
    links: &Table,
    movements: Option<&Table>,
    config: Option<&Table>,
    options: &GmnsOptions,
) -> Result<GmnsNetwork, GmnsError> {
    let metres_per_unit = metres_per_length_unit(&resolve_unit(
        &options.length_unit,
        config.map(|t| (t, "long_length")),
        "length",
    )?)?;
    let metres_per_hour = metres_per_hour_per_speed_unit(&resolve_unit(
        &options.speed_unit,
        config.map(|t| (t, "speed")),
        "speed",
    )?)?;

    // Nodes: external id -> dense id, and zone -> node.
    let id_col = nodes.required("node_id")?;
    let zone_col = nodes.optional("zone_id");
    let mut node_ids = Vec::new();
    let mut node_index: HashMap<String, u32> = HashMap::new();
    let mut zone_nodes: HashMap<String, Vec<u32>> = HashMap::new();
    let mut is_endpoint = Vec::new();
    for row in nodes.rows() {
        let id = row.required_text(id_col, "node_id")?.to_string();
        let dense = node_ids.len() as u32;
        if node_index.insert(id.clone(), dense).is_some() {
            return Err(GmnsError::DuplicateId { file: "node", id });
        }
        node_ids.push(id);
        let zone = zone_col.and_then(|c| row.text(c));
        if let Some(zone) = zone {
            zone_nodes.entry(zone.to_string()).or_default().push(dense);
        }
        is_endpoint.push(zone.is_some() && options.zone_nodes_are_endpoints);
    }

    // Links.
    let link_id_col = links.required("link_id")?;
    let from_col = links.required("from_node_id")?;
    let to_col = links.required("to_node_id")?;
    let length_col = links.required("length")?;
    let speed_col = links.required("free_speed")?;
    let capacity_col = links.required("capacity")?;
    let lanes_col = links.required("lanes")?;
    let directed_col = links.optional("directed");
    let alpha_col = links.optional("vdf_alpha");
    let beta_col = links.optional("vdf_beta");

    let mut edges = Vec::new();
    let mut link_ids = Vec::new();
    let mut is_reverse_copy = Vec::new();
    let mut free_flow_time = Vec::new();
    let mut capacity = Vec::new();
    let mut b = Vec::new();
    let mut power = Vec::new();
    let mut length = Vec::new();
    // External link id -> its dense copies (one, or two when undirected).
    let mut copies: HashMap<String, Vec<u32>> = HashMap::new();
    for row in links.rows() {
        let id = row.required_text(link_id_col, "link_id")?.to_string();
        let node = |col: usize, name: &str| -> Result<u32, GmnsError> {
            let text = row.required_text(col, name)?;
            node_index
                .get(text)
                .copied()
                .ok_or_else(|| row.error(format!("{name} '{text}' is not in node.csv")))
        };
        let from = node(from_col, "from_node_id")?;
        let to = node(to_col, "to_node_id")?;
        let directed = match directed_col.and_then(|c| row.text(c)) {
            None => true,
            Some(text) => parse_bool(text)
                .ok_or_else(|| row.error(format!("cannot parse directed '{text}'")))?,
        };

        let len = row.required_number(length_col, "length")?;
        let speed = row.required_number(speed_col, "free_speed")?;
        let per_lane = row.required_number(capacity_col, "capacity")?;
        let lanes = row.required_number(lanes_col, "lanes")?;
        if len < 0.0 || speed <= 0.0 || per_lane <= 0.0 || lanes <= 0.0 {
            return Err(
                row.error("length must be >= 0 and free_speed, capacity and lanes > 0".to_string())
            );
        }
        let metres = len * metres_per_unit;
        let minutes = metres / (speed * metres_per_hour) * 60.0;
        let alpha = row
            .optional_number(alpha_col, "vdf_alpha")?
            .unwrap_or(options.default_b);
        let beta = row
            .optional_number(beta_col, "vdf_beta")?
            .unwrap_or(options.default_power);

        let first = edges.len() as u32;
        let mut ends = vec![(from, to, false)];
        if !directed {
            ends.push((to, from, true));
        }
        if copies.contains_key(&id) {
            return Err(GmnsError::DuplicateId { file: "link", id });
        }
        for (k, (f, t, reverse)) in ends.into_iter().enumerate() {
            edges.push(Edge {
                link_id: first + k as u32,
                from: f,
                to: t,
            });
            link_ids.push(id.clone());
            is_reverse_copy.push(reverse);
            free_flow_time.push(minutes);
            capacity.push(per_lane * lanes);
            b.push(alpha);
            power.push(beta);
            length.push(metres);
        }
        copies.insert(id, (first..edges.len() as u32).collect());
    }
    let network = Network::from_edges(node_ids.len(), &edges)?;

    let turn_exceptions = match movements {
        Some(table) => movement_exceptions(table, &network, &node_index, &copies, &is_endpoint)?,
        None => Vec::new(),
    };

    Ok(GmnsNetwork {
        network,
        node_ids,
        link_ids,
        is_reverse_copy,
        is_endpoint,
        zone_nodes,
        turn_exceptions,
        free_flow_time,
        capacity,
        b,
        power,
        length,
    })
}

/// Turns `movement.csv` into turn exceptions: listed movements get their penalty
/// (and so are allowed, U-turns included), and every other turn from an inbound link
/// that has at least one listed movement is banned.
fn movement_exceptions(
    table: &Table,
    network: &Network,
    node_index: &HashMap<String, u32>,
    copies: &HashMap<String, Vec<u32>>,
    is_endpoint: &[bool],
) -> Result<Vec<TurnException>, GmnsError> {
    let node_col = table.required("node_id")?;
    let ib_col = table.required("ib_link_id")?;
    let ob_col = table.required("ob_link_id")?;
    let penalty_col = table.optional("penalty");

    // (from_link, to_link) -> penalty in minutes. Several rows can name the same pair
    // (one per lane group); the driver takes the cheapest.
    let mut listed: HashMap<(u32, u32), f64> = HashMap::new();
    let mut junctions: HashSet<u32> = HashSet::new();
    for row in table.rows() {
        let node_text = row.required_text(node_col, "node_id")?;
        let node = *node_index
            .get(node_text)
            .ok_or_else(|| row.error(format!("node_id '{node_text}' is not in node.csv")))?;
        // A copy of an undirected link is picked by which end touches the junction.
        let pick = |col: usize, name: &str, inbound: bool| -> Result<u32, GmnsError> {
            let text = row.required_text(col, name)?;
            copies
                .get(text)
                .and_then(|ids| {
                    ids.iter().copied().find(|&l| {
                        let end = if inbound {
                            network.to_node(l)
                        } else {
                            network.from_node(l)
                        };
                        end == node
                    })
                })
                .ok_or_else(|| {
                    row.error(format!(
                        "link '{text}' ({name}) does not touch node '{node_text}'"
                    ))
                })
        };
        let from_link = pick(ib_col, "ib_link_id", true)?;
        let to_link = pick(ob_col, "ob_link_id", false)?;
        let seconds = row.optional_number(penalty_col, "penalty")?.unwrap_or(0.0);
        if seconds < 0.0 {
            return Err(row.error("penalty must be >= 0".to_string()));
        }
        if is_endpoint[node as usize] {
            continue; // no turns exist at an endpoint node
        }
        junctions.insert(node);
        let minutes = seconds / 60.0;
        listed
            .entry((from_link, to_link))
            .and_modify(|p| *p = p.min(minutes))
            .or_insert(minutes);
    }

    let mut exceptions: Vec<TurnException> = listed
        .iter()
        .map(|(&(from_link, to_link), &penalty)| TurnException {
            from_link,
            to_link,
            rule: TurnRule::Penalty(penalty),
        })
        .collect();

    // Ban every unlisted turn from an inbound link that appears in the table.
    let restricted: HashSet<u32> = listed.keys().map(|&(from_link, _)| from_link).collect();
    let mut incoming: HashMap<u32, Vec<u32>> = HashMap::new();
    for link in 0..network.num_links() as u32 {
        let node = network.to_node(link);
        if junctions.contains(&node) {
            incoming.entry(node).or_default().push(link);
        }
    }
    for (&node, in_links) in &incoming {
        for &from_link in in_links.iter().filter(|l| restricted.contains(l)) {
            for pos in network.out_links(node) {
                let to_link = network.link_id(pos);
                if !listed.contains_key(&(from_link, to_link)) {
                    exceptions.push(TurnException {
                        from_link,
                        to_link,
                        rule: TurnRule::Ban,
                    });
                }
            }
        }
    }
    // HashMap iteration order is random; sort so results are reproducible.
    exceptions.sort_by_key(|e| (e.from_link, e.to_link));
    Ok(exceptions)
}

/// Reads a demand file with one `origin, destination, flow` row per zone pair.
///
/// Zones are mapped to nodes through `net.zone_nodes`. A row of `q` trips from zone `o`
/// to zone `d` becomes `q / (n_o * n_d)` trips for every pair of one of the `n_o` nodes of
/// `o` and one of the `n_d` nodes of `d`, so the total is preserved. Zero flows are skipped
/// and repeated pairs are kept. Pairs whose origin and destination node coincide are
/// emitted too, and the assignment ignores them: a zone-to-zone row with a single node
/// on each side and `o == d` carries no flow at all.
pub fn read_demand(
    path: impl AsRef<Path>,
    net: &GmnsNetwork,
    options: &DemandOptions,
) -> Result<GmnsDemand, GmnsError> {
    read_zone_demand(path, &net.zone_nodes, options)
}

pub fn parse_demand(
    reader: impl Read,
    net: &GmnsNetwork,
    options: &DemandOptions,
) -> Result<GmnsDemand, GmnsError> {
    parse_zone_demand(reader, &net.zone_nodes, options)
}

/// [`read_demand`] given only the zone-to-nodes map (`GmnsNetwork::zone_nodes`), for
/// callers that keep that map but not the whole network.
pub fn read_zone_demand(
    path: impl AsRef<Path>,
    zone_nodes: &HashMap<String, Vec<u32>>,
    options: &DemandOptions,
) -> Result<GmnsDemand, GmnsError> {
    parse_zone_demand(open(path.as_ref())?, zone_nodes, options)
}

pub fn parse_zone_demand(
    reader: impl Read,
    zone_nodes: &HashMap<String, Vec<u32>>,
    options: &DemandOptions,
) -> Result<GmnsDemand, GmnsError> {
    let table = Table::parse("demand", reader)?;
    let column = |name: &str, what: &'static str| {
        table
            .columns
            .get(name)
            .copied()
            .ok_or(GmnsError::MissingColumn {
                file: "demand",
                column: what,
            })
    };
    let origin_col = column(&options.origin, "origin zone column")?;
    let destination_col = column(&options.destination, "destination zone column")?;
    let flow_col = column(&options.flow, "flow column")?;

    let mut result = GmnsDemand {
        demand: Demand {
            num_zones: zone_nodes.len(),
            origins: Vec::new(),
            destinations: Vec::new(),
            flows: Vec::new(),
        },
        skipped_flow: 0.0,
    };
    for row in table.rows() {
        let flow = row.required_number(flow_col, "flow")?;
        if flow < 0.0 {
            return Err(row.error("flow must be >= 0".to_string()));
        }
        if flow == 0.0 {
            continue;
        }
        let origin = row.required_text(origin_col, "origin")?;
        let destination = row.required_text(destination_col, "destination")?;
        let (Some(origins), Some(destinations)) =
            (zone_nodes.get(origin), zone_nodes.get(destination))
        else {
            if options.skip_unknown_zones {
                result.skipped_flow += flow;
                continue;
            }
            let unknown = if zone_nodes.contains_key(origin) {
                destination
            } else {
                origin
            };
            return Err(GmnsError::UnknownZone(unknown.to_string()));
        };
        let share = flow / (origins.len() * destinations.len()) as f64;
        for &o in origins {
            for &d in destinations {
                result.demand.origins.push(o);
                result.demand.destinations.push(d);
                result.demand.flows.push(share);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyta_core::{TurnOptions, TurnTable};

    const CONFIG: &str = "long_length,speed\nkm,kph\n";

    fn net(node: &str, link: &str, movement: Option<&str>) -> Result<GmnsNetwork, GmnsError> {
        parse_network(node, link, movement, Some(CONFIG), &GmnsOptions::default())
    }

    const NODES: &str = "node_id,zone_id\n10,1\nB,2\n30,\n";

    #[test]
    fn derives_time_and_capacity() {
        // 6 km at 60 km/h is 6 minutes; 900/lane on 2 lanes is 1800.
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n\
                     a,10,B,6,60,900,2\n";
        let g = net(NODES, links, None).unwrap();
        assert_eq!(g.network.num_links(), 1);
        assert!((g.free_flow_time[0] - 6.0).abs() < 1e-12);
        assert_eq!(g.capacity[0], 1800.0);
        assert_eq!((g.b[0], g.power[0]), (0.15, 4.0));
        assert_eq!(g.length[0], 6000.0);
        assert_eq!(g.node_ids, ["10", "B", "30"]);
    }

    #[test]
    fn units_come_from_the_options_when_given() {
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n\
                     a,10,B,1,30,900,1\n";
        let options = GmnsOptions {
            length_unit: Some("mile".to_string()),
            speed_unit: Some("mph".to_string()),
            ..GmnsOptions::default()
        };
        // No config at all: 1 mile at 30 mph is 2 minutes.
        let g = parse_network(NODES, links, None, None, &options).unwrap();
        assert!((g.free_flow_time[0] - 2.0).abs() < 1e-12);
        // Without options either, it is an error rather than a guess.
        assert!(matches!(
            parse_network(NODES, links, None, None, &GmnsOptions::default()),
            Err(GmnsError::MissingUnit("length"))
        ));
    }

    #[test]
    fn vdf_columns_override_the_defaults() {
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes,vdf_alpha,vdf_beta\n\
                     a,10,B,1,60,900,1,0.5,2\n\
                     b,B,10,1,60,900,1,,\n";
        let g = net(NODES, links, None).unwrap();
        assert_eq!((g.b[0], g.power[0]), (0.5, 2.0));
        assert_eq!((g.b[1], g.power[1]), (0.15, 4.0));
    }

    #[test]
    fn undirected_links_become_two_links_and_blank_means_directed() {
        let links = "link_id,from_node_id,to_node_id,directed,length,free_speed,capacity,lanes\n\
                     a,10,B,false,1,60,900,1\n\
                     b,B,30,,1,60,900,1\n\
                     c,30,10,TRUE,1,60,900,1\n";
        let g = net(NODES, links, None).unwrap();
        assert_eq!(g.network.num_links(), 4);
        assert_eq!(g.link_ids, ["a", "a", "b", "c"]);
        assert_eq!(g.is_reverse_copy, [false, true, false, false]);
        // The reverse copy runs B -> 10.
        assert_eq!((g.network.from_node(1), g.network.to_node(1)), (1, 0));
    }

    #[test]
    fn bad_links_are_reported_with_their_line() {
        let head = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n";
        let err = net(NODES, &format!("{head}a,10,nope,1,60,900,1\n"), None).unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        assert!(err.to_string().contains("nope"), "{err}");
        // A missing capacity is an error, not a silent default.
        let err = net(NODES, &format!("{head}a,10,B,1,60,,1\n"), None).unwrap_err();
        assert!(err.to_string().contains("capacity is blank"), "{err}");
        let dup = format!("{head}a,10,B,1,60,900,1\na,B,10,1,60,900,1\n");
        assert!(matches!(
            net(NODES, &dup, None),
            Err(GmnsError::DuplicateId { file: "link", .. })
        ));
    }

    #[test]
    fn a_zone_collects_all_its_nodes_in_file_order() {
        let nodes = "node_id,zone_id\n1,7\n2,8\n3,7\n4,\n";
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n";
        let g = net(nodes, links, None).unwrap();
        assert_eq!(g.zone_nodes["7"], [0, 2]);
        assert_eq!(g.zone_nodes["8"], [1]);
        assert_eq!(g.zone_nodes.len(), 2);
    }

    #[test]
    fn zone_nodes_become_endpoints_on_request() {
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n";
        let off = net(NODES, links, None).unwrap();
        assert_eq!(off.is_endpoint, [false, false, false]);
        let options = GmnsOptions {
            zone_nodes_are_endpoints: true,
            ..GmnsOptions::default()
        };
        let on = parse_network(NODES, links, None, Some(CONFIG), &options).unwrap();
        assert_eq!(on.is_endpoint, [true, true, false]);
        assert_eq!(on.zone_nodes["2"], [1]);
    }

    /// A cross at node X: links w, e, s, n come in; links W, E, S, N go out.
    fn cross() -> (String, String) {
        let nodes = "node_id\nX\nw\ne\ns\nn\n".to_string();
        let mut links =
            String::from("link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n");
        for (inbound, outbound, node) in [
            ("w", "W", "w"),
            ("e", "E", "e"),
            ("s", "S", "s"),
            ("n", "N", "n"),
        ] {
            links += &format!("{inbound},{node},X,1,60,900,1\n");
            links += &format!("{outbound},X,{node},1,60,900,1\n");
        }
        (nodes, links)
    }

    #[test]
    fn movements_allow_only_the_listed_turns_of_a_listed_inbound_link() {
        let (nodes, links) = cross();
        // From the west: straight on (E) and a left (N) with 30 s penalty. Nothing else.
        let movements = "mvmt_id,node_id,ib_link_id,ob_link_id,penalty\n\
                         1,X,w,E,\n\
                         2,X,w,N,30\n";
        let g = net(&nodes, &links, Some(movements)).unwrap();
        let table = TurnTable::build(
            &g.network,
            &g.is_endpoint,
            &g.turn_exceptions,
            TurnOptions::default(),
        )
        .unwrap();
        let link = |id: &str| g.link_ids.iter().position(|l| l == id).unwrap() as u32;
        let from_west: Vec<(u32, f64)> = table
            .turns_from(link("w"))
            .map(|t| (table.to_link(t), table.penalty(t)))
            .collect();
        // Turns are listed in out-link order of the junction; links E, N were created
        // in the order w, e, s, n, so E precedes N.
        assert_eq!(from_west, vec![(link("E"), 0.0), (link("N"), 0.5)]);
        // Nothing is listed for the inbound link from the east, so it is unrestricted:
        // W, S and N are allowed and only the U-turn (E) stays banned.
        assert_eq!(table.turns_from(link("e")).count(), 3);
    }

    #[test]
    fn duplicate_movements_take_the_cheapest_penalty() {
        let (nodes, links) = cross();
        let movements = "mvmt_id,node_id,ib_link_id,ob_link_id,penalty\n\
                         1,X,w,E,60\n\
                         2,X,w,E,12\n";
        let g = net(&nodes, &links, Some(movements)).unwrap();
        let penalty = g
            .turn_exceptions
            .iter()
            .find_map(|e| match e.rule {
                TurnRule::Penalty(p) => Some(p),
                TurnRule::Ban => None,
            })
            .unwrap();
        assert!((penalty - 0.2).abs() < 1e-12);
    }

    #[test]
    fn a_listed_u_turn_is_allowed() {
        let (nodes, links) = cross();
        let movements = "mvmt_id,node_id,ib_link_id,ob_link_id,type\n\
                         1,X,w,W,uturn\n";
        let g = net(&nodes, &links, Some(movements)).unwrap();
        let table = TurnTable::build(
            &g.network,
            &g.is_endpoint,
            &g.turn_exceptions,
            TurnOptions::default(),
        )
        .unwrap();
        let link = |id: &str| g.link_ids.iter().position(|l| l == id).unwrap() as u32;
        let to: Vec<u32> = table
            .turns_from(link("w"))
            .map(|t| table.to_link(t))
            .collect();
        assert_eq!(to, [link("W")]);
    }

    #[test]
    fn movements_on_undirected_links_pick_the_copy_by_junction() {
        // One undirected street a between X and Y; movement into it at X must leave X.
        let nodes = "node_id\nX\nY\nz\n";
        let links = "link_id,from_node_id,to_node_id,directed,length,free_speed,capacity,lanes\n\
                     a,X,Y,false,1,60,900,1\n\
                     b,z,X,true,1,60,900,1\n";
        let movements = "mvmt_id,node_id,ib_link_id,ob_link_id\n1,X,b,a\n";
        let g = net(nodes, links, Some(movements)).unwrap();
        let e = g
            .turn_exceptions
            .iter()
            .find(|e| e.rule != TurnRule::Ban)
            .unwrap();
        // Dense links: a = 0 (X->Y), a reversed = 1 (Y->X), b = 2.
        assert_eq!((e.from_link, e.to_link), (2, 0));
    }

    #[test]
    fn movements_that_do_not_touch_their_node_are_rejected() {
        let (nodes, links) = cross();
        let movements = "mvmt_id,node_id,ib_link_id,ob_link_id\n1,X,E,W\n";
        let err = net(&nodes, &links, Some(movements)).unwrap_err();
        assert!(err.to_string().contains("does not touch"), "{err}");
    }

    #[test]
    fn demand_maps_zones_to_nodes() {
        let links = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n";
        let g = net(NODES, links, None).unwrap();
        let text = "o_zone_id,d_zone_id,volume\n1,2,100\n2,1,0\n2,1,5.5\n";
        let d = parse_demand(text.as_bytes(), &g, &DemandOptions::default()).unwrap();
        let d = d.demand;
        assert_eq!(d.origins, [0, 1]);
        assert_eq!(d.destinations, [1, 0]);
        assert_eq!(d.flows, [100.0, 5.5]);
        assert_eq!(d.num_zones, 2);

        let columns = DemandOptions {
            origin: "orig_taz".to_string(),
            destination: "dest_taz".to_string(),
            flow: "total".to_string(),
            ..DemandOptions::default()
        };
        let d = parse_demand("orig_taz,dest_taz,total\n1,2,3\n".as_bytes(), &g, &columns).unwrap();
        assert_eq!(d.demand.flows, [3.0]);
    }

    /// Zone 7 has nodes 0 and 2, zone 8 has nodes 1, 3 and 4.
    const MULTI_NODES: &str = "node_id,zone_id\na,7\nb,8\nc,7\nd,8\ne,8\n";
    const NO_LINKS: &str = "link_id,from_node_id,to_node_id,length,free_speed,capacity,lanes\n";

    #[test]
    fn demand_is_split_over_every_node_pair_and_keeps_its_total() {
        let g = net(MULTI_NODES, NO_LINKS, None).unwrap();
        let text = "o_zone_id,d_zone_id,volume\n7,8,60\n";
        let d = parse_demand(text.as_bytes(), &g, &DemandOptions::default())
            .unwrap()
            .demand;
        // 2 origin nodes x 3 destination nodes, 10 trips each.
        assert_eq!(d.origins, [0, 0, 0, 2, 2, 2]);
        assert_eq!(d.destinations, [1, 3, 4, 1, 3, 4]);
        assert!(d.flows.iter().all(|&q| (q - 10.0).abs() < 1e-12));
        assert!((d.total() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn intrazonal_demand_includes_same_node_pairs_for_the_assignment_to_drop() {
        let g = net(MULTI_NODES, NO_LINKS, None).unwrap();
        let text = "o_zone_id,d_zone_id,volume\n7,7,40\n";
        let d = parse_demand(text.as_bytes(), &g, &DemandOptions::default())
            .unwrap()
            .demand;
        assert_eq!(d.origins, [0, 0, 2, 2]);
        assert_eq!(d.destinations, [0, 2, 0, 2]);
        assert!(d.flows.iter().all(|&q| (q - 10.0).abs() < 1e-12));
    }

    #[test]
    fn unknown_zones_are_an_error_unless_skipping_is_requested() {
        let g = net(MULTI_NODES, NO_LINKS, None).unwrap();
        let text = "o_zone_id,d_zone_id,volume\n7,8,60\n7,99,5\n99,8,2\n";
        let err = parse_demand(text.as_bytes(), &g, &DemandOptions::default()).unwrap_err();
        assert!(matches!(err, GmnsError::UnknownZone(z) if z == "99"));

        let options = DemandOptions {
            skip_unknown_zones: true,
            ..DemandOptions::default()
        };
        let d = parse_demand(text.as_bytes(), &g, &options).unwrap();
        assert_eq!(d.skipped_flow, 7.0);
        assert!((d.demand.total() - 60.0).abs() < 1e-9);
    }
}
