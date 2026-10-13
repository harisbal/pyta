# Network model

## Links, nodes and ids

A network is a directed graph. **Nodes** are integers `0..num_nodes`. **Links** are numbered by their
position in the input table: link `i` is row `i`. Per-link data such as costs and flows lives in
arrays indexed by link id, and the graph itself never changes during an assignment.

Internally the graph is stored in *compressed sparse row* form: links sorted by from-node, with an
offsets array. Scanning a node's outgoing links then reads contiguous memory, which is what the shortest
path search spends its time doing.

## Link delay

Each link has a delay function of its volume. The built-in one is BPR:

\[
t(v) = t_0\left(1 + b\left(\frac{v}{c}\right)^{n}\right)
\]

with free-flow time \(t_0\), capacity \(c\), \(b\) (default 0.15) and power \(n\) (default 4).
In Rust you can implement the `Vdf` trait for your own function.

## Zones are nodes

A zone is a node, and demand is given between node ids. PyTA has **no connector concept**: if you want a
zone to have several access points, add a node and ordinary links for them in your data.

Some benchmark networks (Anaheim, for example) have *centroids*: nodes that traffic may start or end
at but must never pass through, or paths would cut across zones through fake connectors. For these,
mark the nodes in `is_endpoint`. A path may then start or end at an endpoint node but not pass
through it. The TNTP reader fills the mask from `<FIRST THRU NODE>`.

If every zone is an ordinary intersection (Sioux Falls), leave `is_endpoint` out.

## Demand

Demand is a long table of `origin, destination, flow`. Repeated pairs add up. Trips with
`origin == destination` use no network and are skipped. A positive flow between nodes that no allowed
path connects is an error.
