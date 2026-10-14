//! Reader for the TNTP benchmark format (Transportation Networks for Research).
//!
//! TNTP node numbers are 1-based. Everything returned from here is 0-based, so
//! TNTP node `n` is node `n - 1`. A link's id is its row number in the file.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use pytraf_core::{Edge, Network, NetworkError};

/// A TNTP `*_net.tntp` file. The per-link vectors are indexed by link id.
#[derive(Debug)]
pub struct TntpNetwork {
    pub network: Network,
    pub num_zones: usize,
    /// Per node: paths may start or end here but not pass through. TNTP encodes
    /// this as `<FIRST THRU NODE>`: every node below it is a centroid.
    pub is_endpoint: Vec<bool>,
    pub capacity: Vec<f64>,
    pub length: Vec<f64>,
    pub free_flow_time: Vec<f64>,
    pub b: Vec<f64>,
    pub power: Vec<f64>,
    pub speed: Vec<f64>,
    pub toll: Vec<f64>,
    pub link_type: Vec<u32>,
}

/// A TNTP `*_trips.tntp` file in long format: entry `i` is `flows[i]` trips
/// from zone `origins[i]` to zone `destinations[i]` (0-based). Zero flows are skipped.
#[derive(Debug)]
pub struct Demand {
    pub num_zones: usize,
    pub origins: Vec<u32>,
    pub destinations: Vec<u32>,
    pub flows: Vec<f64>,
}

impl Demand {
    pub fn total(&self) -> f64 {
        self.flows.iter().sum()
    }
}

#[derive(Debug)]
pub enum TntpError {
    Io(std::io::Error),
    MissingMetadata(&'static str),
    /// `line` is 1-based.
    Parse {
        line: usize,
        message: String,
    },
    CountMismatch {
        what: &'static str,
        expected: usize,
        found: usize,
    },
    FlowMismatch {
        expected: f64,
        found: f64,
    },
    Network(NetworkError),
}

impl fmt::Display for TntpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TntpError::Io(e) => write!(f, "I/O error: {e}"),
            TntpError::MissingMetadata(key) => write!(f, "missing metadata <{key}>"),
            TntpError::Parse { line, message } => write!(f, "line {line}: {message}"),
            TntpError::CountMismatch {
                what,
                expected,
                found,
            } => {
                write!(f, "expected {expected} {what}, found {found}")
            }
            TntpError::FlowMismatch { expected, found } => {
                write!(f, "expected total flow {expected}, found {found}")
            }
            TntpError::Network(e) => write!(f, "invalid network: {e:?}"),
        }
    }
}

impl std::error::Error for TntpError {}

// These two impls are what lets `?` convert the inner error automatically.
impl From<std::io::Error> for TntpError {
    fn from(e: std::io::Error) -> Self {
        TntpError::Io(e)
    }
}

impl From<NetworkError> for TntpError {
    fn from(e: NetworkError) -> Self {
        TntpError::Network(e)
    }
}

pub fn read_net(path: impl AsRef<Path>) -> Result<TntpNetwork, TntpError> {
    parse_net(&fs::read_to_string(path)?)
}

pub fn read_trips(path: impl AsRef<Path>) -> Result<Demand, TntpError> {
    parse_trips(&fs::read_to_string(path)?)
}

/// Metadata block: `<KEY> value` lines up to and including `<END OF METADATA>`.
struct Header {
    values: HashMap<String, String>,
    /// Index (0-based) of the first line after the metadata block.
    body_start: usize,
}

impl Header {
    fn parse(text: &str) -> Result<Header, TntpError> {
        let mut values = HashMap::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let Some(rest) = line.strip_prefix('<') else {
                continue; // blank lines before the block, or stray text
            };
            let Some(close) = rest.find('>') else {
                return Err(TntpError::Parse {
                    line: idx + 1,
                    message: "metadata line without closing '>'".to_string(),
                });
            };
            let key = rest[..close].trim().to_uppercase();
            if key == "END OF METADATA" {
                return Ok(Header {
                    values,
                    body_start: idx + 1,
                });
            }
            values.insert(key, rest[close + 1..].trim().to_string());
        }
        Err(TntpError::MissingMetadata("END OF METADATA"))
    }

    fn get<T: FromStr>(&self, key: &'static str) -> Result<T, TntpError> {
        let value = self
            .values
            .get(key)
            .ok_or(TntpError::MissingMetadata(key))?;
        value.parse().map_err(|_| TntpError::Parse {
            line: 0,
            message: format!("cannot parse <{key}> value '{value}'"),
        })
    }
}

/// Parses field `i` of a split line, with a readable error on failure.
fn field<T: FromStr>(fields: &[&str], i: usize, name: &str, line: usize) -> Result<T, TntpError> {
    fields[i].parse().map_err(|_| TntpError::Parse {
        line,
        message: format!("cannot parse {name} '{}'", fields[i]),
    })
}

/// Converts a 1-based TNTP node number to a 0-based node id.
fn to_zero_based(node: u32, line: usize) -> Result<u32, TntpError> {
    node.checked_sub(1).ok_or(TntpError::Parse {
        line,
        message: "node numbers start at 1".to_string(),
    })
}

pub fn parse_net(text: &str) -> Result<TntpNetwork, TntpError> {
    let header = Header::parse(text)?;
    let num_zones: usize = header.get("NUMBER OF ZONES")?;
    let num_nodes: usize = header.get("NUMBER OF NODES")?;
    let num_links: usize = header.get("NUMBER OF LINKS")?;
    let first_thru_node = to_zero_based(header.get("FIRST THRU NODE")?, 0)?;
    let is_endpoint: Vec<bool> = (0..num_nodes as u32).map(|n| n < first_thru_node).collect();

    let mut edges = Vec::with_capacity(num_links);
    let mut capacity = Vec::with_capacity(num_links);
    let mut length = Vec::with_capacity(num_links);
    let mut free_flow_time = Vec::with_capacity(num_links);
    let mut b = Vec::with_capacity(num_links);
    let mut power = Vec::with_capacity(num_links);
    let mut speed = Vec::with_capacity(num_links);
    let mut toll = Vec::with_capacity(num_links);
    let mut link_type = Vec::with_capacity(num_links);

    for (idx, raw) in text.lines().enumerate().skip(header.body_start) {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('~') {
            continue;
        }
        let line = line.strip_suffix(';').unwrap_or(line);
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            return Err(TntpError::Parse {
                line: line_no,
                message: format!("expected 10 columns, found {}", fields.len()),
            });
        }

        edges.push(Edge {
            link_id: edges.len() as u32,
            from: to_zero_based(field(&fields, 0, "init node", line_no)?, line_no)?,
            to: to_zero_based(field(&fields, 1, "term node", line_no)?, line_no)?,
        });
        capacity.push(field(&fields, 2, "capacity", line_no)?);
        length.push(field(&fields, 3, "length", line_no)?);
        free_flow_time.push(field(&fields, 4, "free flow time", line_no)?);
        b.push(field(&fields, 5, "b", line_no)?);
        power.push(field(&fields, 6, "power", line_no)?);
        speed.push(field(&fields, 7, "speed", line_no)?);
        toll.push(field(&fields, 8, "toll", line_no)?);
        link_type.push(field(&fields, 9, "link type", line_no)?);
    }

    if edges.len() != num_links {
        return Err(TntpError::CountMismatch {
            what: "links",
            expected: num_links,
            found: edges.len(),
        });
    }
    let network = Network::from_edges(num_nodes, &edges)?;

    Ok(TntpNetwork {
        network,
        num_zones,
        is_endpoint,
        capacity,
        length,
        free_flow_time,
        b,
        power,
        speed,
        toll,
        link_type,
    })
}

/// A TNTP `*_flow.tntp` file: published equilibrium volume and cost per link,
/// both indexed by link id of the network they were matched against.
#[derive(Debug)]
pub struct PublishedFlows {
    pub volume: Vec<f64>,
    pub cost: Vec<f64>,
}

pub fn read_flows(path: impl AsRef<Path>, net: &TntpNetwork) -> Result<PublishedFlows, TntpError> {
    parse_flows(&fs::read_to_string(path)?, net)
}

/// Rows are `From To Volume Cost` (1-based nodes) and are matched to links by
/// their end nodes, so the row order does not matter. Parallel links between
/// the same two nodes make that match ambiguous and are rejected.
pub fn parse_flows(text: &str, net: &TntpNetwork) -> Result<PublishedFlows, TntpError> {
    let num_links = net.network.num_links();
    let mut by_ends: HashMap<(u32, u32), Option<u32>> = HashMap::new();
    for link in 0..num_links as u32 {
        let ends = (net.network.from_node(link), net.network.to_node(link));
        // A second link with the same ends makes the entry ambiguous (None).
        by_ends
            .entry(ends)
            .and_modify(|e| *e = None)
            .or_insert(Some(link));
    }

    let mut volume = vec![f64::NAN; num_links];
    let mut cost = vec![f64::NAN; num_links];
    let mut rows = 0;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let fields: Vec<&str> = raw.split_whitespace().collect();
        // Skip blank lines and the header row (its first field is not a number).
        if fields.len() < 4 || fields[0].parse::<u32>().is_err() {
            continue;
        }
        let from = to_zero_based(field(&fields, 0, "from node", line_no)?, line_no)?;
        let to = to_zero_based(field(&fields, 1, "to node", line_no)?, line_no)?;
        let link = match by_ends.get(&(from, to)) {
            Some(Some(link)) => *link as usize,
            Some(None) => {
                return Err(TntpError::Parse {
                    line: line_no,
                    message: format!("parallel links between {} and {}", from + 1, to + 1),
                });
            }
            None => {
                return Err(TntpError::Parse {
                    line: line_no,
                    message: format!("no link from {} to {}", from + 1, to + 1),
                });
            }
        };
        if !volume[link].is_nan() {
            return Err(TntpError::Parse {
                line: line_no,
                message: "link listed twice".to_string(),
            });
        }
        volume[link] = field(&fields, 2, "volume", line_no)?;
        cost[link] = field(&fields, 3, "cost", line_no)?;
        rows += 1;
    }
    if rows != num_links {
        return Err(TntpError::CountMismatch {
            what: "flow rows",
            expected: num_links,
            found: rows,
        });
    }
    Ok(PublishedFlows { volume, cost })
}

pub fn parse_trips(text: &str) -> Result<Demand, TntpError> {
    let header = Header::parse(text)?;
    let num_zones: usize = header.get("NUMBER OF ZONES")?;
    let expected_total: f64 = header.get("TOTAL OD FLOW")?;

    let mut origins = Vec::new();
    let mut destinations = Vec::new();
    let mut flows = Vec::new();
    let mut current_origin: Option<u32> = None;

    for (idx, raw) in text.lines().enumerate().skip(header.body_start) {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('~') {
            continue;
        }

        // "Origin 3" starts the block of destinations for zone 3.
        if let Some(rest) = line.strip_prefix("Origin") {
            let zone: u32 = rest.trim().parse().map_err(|_| TntpError::Parse {
                line: line_no,
                message: format!("cannot parse origin '{}'", rest.trim()),
            })?;
            current_origin = Some(to_zero_based(zone, line_no)?);
            continue;
        }

        let origin = current_origin.ok_or(TntpError::Parse {
            line: line_no,
            message: "flow data before the first 'Origin' line".to_string(),
        })?;

        // "1 : 0.0; 2 : 100.0;" holds several `destination : flow` pairs.
        for pair in line.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (dest, flow) = pair.split_once(':').ok_or(TntpError::Parse {
                line: line_no,
                message: format!("expected 'destination : flow', found '{pair}'"),
            })?;
            let dest = to_zero_based(field(&[dest.trim()], 0, "destination", line_no)?, line_no)?;
            let flow: f64 = field(&[flow.trim()], 0, "flow", line_no)?;
            if dest as usize >= num_zones {
                return Err(TntpError::Parse {
                    line: line_no,
                    message: format!("destination {} exceeds {num_zones} zones", dest + 1),
                });
            }
            if flow > 0.0 {
                origins.push(origin);
                destinations.push(dest);
                flows.push(flow);
            }
        }
    }

    let demand = Demand {
        num_zones,
        origins,
        destinations,
        flows,
    };
    let found = demand.total();
    if (found - expected_total).abs() > 1e-6 * expected_total.abs().max(1.0) {
        return Err(TntpError::FlowMismatch {
            expected: expected_total,
            found,
        });
    }
    Ok(demand)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sioux_falls(file: &str) -> String {
        format!(
            "{}/../../data/tntp/SiouxFalls/{file}",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    #[test]
    fn sioux_falls_net() {
        let net = read_net(sioux_falls("SiouxFalls_net.tntp")).unwrap();
        assert_eq!(net.num_zones, 24);
        assert_eq!(net.network.num_nodes(), 24);
        assert_eq!(net.network.num_links(), 76);
        assert!(net.is_endpoint.iter().all(|&e| !e)); // first thru node is 1: nothing is endpoint-only
        assert_eq!(net.capacity.len(), 76);

        // Row 1 of the file: node 1 -> node 2, capacity 25900.20064, length 6, fft 6.
        assert_eq!(net.capacity[0], 25900.20064);
        assert_eq!(net.free_flow_time[0], 6.0);
        assert_eq!(net.b[0], 0.15);
        assert_eq!(net.power[0], 4.0);

        // Node 1 has two outgoing links: to nodes 2 and 3 (0-based 1 and 2), ids 0 and 1.
        let out = net.network.out_links(0);
        assert_eq!(out.len(), 2);
        assert_eq!(net.network.target(out.start), 1);
        assert_eq!(net.network.link_id(out.start), 0);
        assert_eq!(net.network.target(out.start + 1), 2);
        assert_eq!(net.network.link_id(out.start + 1), 1);
    }

    #[test]
    fn sioux_falls_trips() {
        let demand = read_trips(sioux_falls("SiouxFalls_trips.tntp")).unwrap();
        assert_eq!(demand.num_zones, 24);
        assert_eq!(demand.total(), 360600.0);
        assert!(
            demand
                .origins
                .iter()
                .zip(&demand.destinations)
                .all(|(o, d)| o != d)
        );

        // "Origin 1": destination 4 has 500 trips.
        let flow = demand
            .origins
            .iter()
            .zip(&demand.destinations)
            .zip(&demand.flows)
            .find(|((o, d), _)| **o == 0 && **d == 3)
            .map(|(_, f)| *f);
        assert_eq!(flow, Some(500.0));
    }

    const TINY_NET: &str = "\
<NUMBER OF ZONES> 1
<NUMBER OF NODES> 2
<FIRST THRU NODE> 1
<NUMBER OF LINKS> 1
<END OF METADATA>

~ init term cap len fft b power speed toll type ;
 1 2 100 1 2 0.15 4 0 0 1 ;
";

    #[test]
    fn tiny_net_parses() {
        let net = parse_net(TINY_NET).unwrap();
        assert_eq!(net.network.num_links(), 1);
        assert_eq!(net.link_type, [1]);
        // <FIRST THRU NODE> 1: no endpoint-only nodes.
        assert_eq!(net.is_endpoint, [false, false]);
    }

    #[test]
    fn first_thru_node_marks_endpoints() {
        // First thru node 2 (1-based) means node 1 is a centroid.
        let text = TINY_NET.replace("<FIRST THRU NODE> 1", "<FIRST THRU NODE> 2");
        assert_eq!(parse_net(&text).unwrap().is_endpoint, [true, false]);
    }

    #[test]
    fn link_count_must_match_metadata() {
        let text = TINY_NET.replace("<NUMBER OF LINKS> 1", "<NUMBER OF LINKS> 2");
        match parse_net(&text) {
            Err(TntpError::CountMismatch {
                expected: 2,
                found: 1,
                ..
            }) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn bad_field_reports_line_number() {
        let text = TINY_NET.replace(" 100 1 2", " abc 1 2");
        match parse_net(&text) {
            Err(TntpError::Parse { line: 8, .. }) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn missing_end_of_metadata() {
        assert!(matches!(
            parse_net("<NUMBER OF ZONES> 1\n"),
            Err(TntpError::MissingMetadata("END OF METADATA"))
        ));
    }

    #[test]
    fn trips_total_must_match_metadata() {
        let text = "\
<NUMBER OF ZONES> 2
<TOTAL OD FLOW> 999.0
<END OF METADATA>
Origin 1
 1 : 0.0; 2 : 10.0;
";
        assert!(matches!(
            parse_trips(text),
            Err(TntpError::FlowMismatch { .. })
        ));
    }

    #[test]
    fn sioux_falls_published_flows() {
        let net = read_net(sioux_falls("SiouxFalls_net.tntp")).unwrap();
        let flows = read_flows(sioux_falls("SiouxFalls_flow.tntp"), &net).unwrap();
        assert_eq!(flows.volume.len(), 76);
        // First row of the file: 1 -> 2, volume 4494.6576..., cost 6.0008...
        assert!((flows.volume[0] - 4494.6576464564205).abs() < 1e-9);
        assert!((flows.cost[0] - 6.000_816_237_354_32).abs() < 1e-12);
    }

    #[test]
    fn flows_are_matched_by_end_nodes_not_row_order() {
        let net = parse_net(TINY_NET).unwrap();
        let text = "From To Volume Cost\n1 2 5.0 7.0\n";
        let flows = parse_flows(text, &net).unwrap();
        assert_eq!((flows.volume[0], flows.cost[0]), (5.0, 7.0));
        // A row for a link that does not exist is an error with its line number.
        assert!(matches!(
            parse_flows("From To Volume Cost\n2 1 5.0 7.0\n", &net),
            Err(TntpError::Parse { line: 2, .. })
        ));
        // Missing rows are an error too.
        assert!(matches!(
            parse_flows("From To Volume Cost\n", &net),
            Err(TntpError::CountMismatch { .. })
        ));
    }
}
