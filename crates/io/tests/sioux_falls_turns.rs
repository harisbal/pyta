use pytraf_core::{TurnOptions, TurnTable};
use pytraf_io::tntp::{TntpNetwork, read_net};

fn load() -> TntpNetwork {
    read_net(format!(
        "{}/../../data/tntp/SiouxFalls/SiouxFalls_net.tntp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

// Expected counts computed independently in Python from the raw file:
// sum over nodes of (in-degree * out-degree) = 254; 76 of those are U-turns.
#[test]
fn turn_counts() {
    let net = load();
    let strict = TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default());
    assert_eq!(strict.unwrap().num_turns(), 178);

    let all = TurnOptions {
        allow_u_turns: true,
    };
    let table = TurnTable::build(&net.network, &net.is_endpoint, &[], all).unwrap();
    assert_eq!(table.num_turns(), 254);
}

#[test]
fn every_turn_joins_adjacent_links() {
    let net = load();
    let table =
        TurnTable::build(&net.network, &net.is_endpoint, &[], TurnOptions::default()).unwrap();
    for t in 0..table.num_turns() {
        let (e, f) = (table.from_link(t), table.to_link(t));
        assert_eq!(net.network.to_node(e), net.network.from_node(f));
        assert_ne!(
            net.network.to_node(f),
            net.network.from_node(e),
            "U-turn present"
        );
        assert!(table.turns_from(e).contains(&t));
    }
}
