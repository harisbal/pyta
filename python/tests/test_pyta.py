"""Tests for the Python API. Sioux Falls is checked against its published equilibrium."""

import threading
from pathlib import Path

import numpy as np
import pandas as pd
import pytest

import pyta

DATA = Path(__file__).resolve().parents[2] / "data" / "tntp" / "SiouxFalls"
PUBLISHED_OBJECTIVE = 4_231_335.29


def read_trip_table():
    """The Sioux Falls trip table as zero-based (origin, destination, flow) arrays."""
    origin, o, d, q = None, [], [], []
    for line in (DATA / "SiouxFalls_trips.tntp").read_text().splitlines():
        line = line.strip()
        if line.startswith("Origin"):
            origin = int(line.split()[1]) - 1
        elif origin is not None and ":" in line:
            for part in line.split(";"):
                if ":" in part:
                    dest, flow = part.split(":")
                    if float(flow) > 0:
                        o.append(origin)
                        d.append(int(dest) - 1)
                        q.append(float(flow))
    return np.array(o), np.array(d), np.array(q)


@pytest.fixture(scope="module")
def sioux():
    net = pyta.Network.from_tntp(str(DATA / "SiouxFalls_net.tntp"))
    demand = pyta.Demand.from_tntp(net, str(DATA / "SiouxFalls_trips.tntp"))
    return net, demand


@pytest.fixture(scope="module")
def published():
    """Published equilibrium volumes; the flow file lists links in network-file order."""
    rows = [line.split() for line in (DATA / "SiouxFalls_flow.tntp").read_text().splitlines()]
    return np.array([float(r[2]) for r in rows if len(r) >= 4 and r[0].isdigit()])


# ------------------------------------------------------------ single class


def test_published_equilibrium(sioux, published):
    net, demand = sioux
    result = net.assign(demand, gap=1e-8)
    assert result.converged and result.relative_gap <= 1e-8
    assert abs(result.diagnostics.objective - PUBLISHED_OBJECTIVE) < 0.01
    assert np.abs(result.links.by_class[0] - published).max() < 0.1
    assert result.links.by_class[0].dtype == np.float64
    assert result.turns.by_class[0].shape == (net.num_turns,)
    assert len(result.diagnostics.sweep_gaps) == result.iterations
    assert "converged=True" in repr(result)


def test_tables_and_dataframes_build_the_same_model(sioux):
    net, _ = sioux
    from_node, to_node = net.link_nodes()
    raw = pd.read_csv(
        DATA / "SiouxFalls_net.tntp", sep=r"\s+", skiprows=8, comment="~",
        names=["init", "term", "capacity", "length", "fftt", "b", "power", "speed", "toll", "type", "end"],
    )
    links = pd.DataFrame({
        "from_node": raw["init"] - 1, "to_node": raw["term"] - 1,
        "free_flow_time": raw["fftt"], "capacity": raw["capacity"],
        "b": raw["b"], "power": raw["power"],
    })
    assert (links["from_node"].to_numpy() == from_node).all()
    assert (links["to_node"].to_numpy() == to_node).all()
    rebuilt = pyta.Network.from_tables(links)
    assert (rebuilt.num_nodes, rebuilt.num_links, rebuilt.num_turns) == (24, 76, 178)

    o, d, q = read_trip_table()
    demand = pyta.Demand.from_table(rebuilt, pd.DataFrame({"origin": o, "destination": d, "flow": q}))
    assert demand.total() == 360_600.0
    a = rebuilt.assign(demand, gap=1e-9)
    b = net.assign(demand, gap=1e-9)
    assert np.allclose(a.links.by_class[0], b.links.by_class[0], atol=1e-6)


def test_paths_table_is_consistent(sioux):
    net, demand = sioux
    result = net.assign(demand, gap=1e-6)
    paths = result.paths.arrays()
    n = len(paths["flow"])
    assert len(paths["origin"]) == len(paths["destination"]) == n
    assert len(paths["link_offsets"]) == n + 1
    assert paths["link_offsets"][-1] == len(paths["links"])
    assert (paths["flow"] > 0).all()
    assert abs(paths["flow"].sum() - 360_600.0) < 1e-6
    # Link flows rebuilt from the paths match the reported ones.
    rebuilt = np.zeros(net.num_links)
    counts = np.diff(paths["link_offsets"])
    np.add.at(rebuilt, paths["links"], np.repeat(paths["flow"], counts))
    assert np.allclose(rebuilt, result.links.by_class[0], atol=1e-6)


@pytest.mark.parametrize("order", ["fixed", "shuffle", "alternate", "largest_gap",
                                   "largest_demand", "smallest_demand"])
def test_every_origin_order_converges(sioux, published, order):
    net, demand = sioux
    result = net.assign(demand, gap=1e-6, order=order, seed=3)
    assert result.converged
    assert np.abs(result.links.by_class[0] - published).max() < 5.0  # 1e-6 gap: a few vehicles


def test_iteration_limit_is_reported(sioux):
    net, demand = sioux
    result = net.assign(demand, gap=1e-12, max_iter=3)
    assert not result.converged and result.iterations == 3


def test_chunk_size_and_threads(sioux, published):
    net, demand = sioux
    sequential = net.assign(demand, gap=1e-8, chunk_size=1)
    chunked = net.assign(demand, gap=1e-8, chunk_size=8)
    assert sequential.converged and chunked.converged
    assert np.abs(chunked.links.by_class[0] - published).max() < 0.1
    # The same chunk size gives bit-identical answers whatever the number of threads.
    one = net.assign(demand, gap=1e-7, chunk_size=8, threads=1)
    four = net.assign(demand, gap=1e-7, chunk_size=8, threads=4)
    assert np.array_equal(one.links.by_class[0], four.links.by_class[0])
    with pytest.raises(ValueError, match="threads must be at least 1"):
        net.assign(demand, threads=0)


def test_phase_times(sioux):
    net, demand = sioux
    times = net.assign(demand, gap=1e-6).diagnostics.times
    assert set(times) == {"initial_assignment", "shortest_paths", "flow_updates",
                          "sweep_rebuild", "gap_checks"}
    assert all(t >= 0.0 for t in times.values())
    assert times["shortest_paths"] > 0.0


def test_assign_releases_the_gil(sioux):
    net, demand = sioux
    results = []

    def work():
        results.append(net.assign(demand, gap=1e-7).relative_gap)

    threads = [threading.Thread(target=work) for _ in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(results) == 4 and all(g <= 1e-7 for g in results)


# --------------------------------------------------------------- multi-class


def test_two_classes_split_reproduce_the_published_equilibrium(sioux, published):
    net, _ = sioux
    o, d, q = read_trip_table()
    cars = pyta.Demand(24, o, d, 0.6 * q)
    vans = pyta.Demand(24, o, d, 0.4 * q)
    result = net.assign([cars, vans], gap=1e-8)
    assert result.converged and len(result.links.by_class) == 2
    total = result.links.by_class[0] + result.links.by_class[1]
    assert np.abs(total - published).max() < 0.1
    assert np.abs(result.links.volume - published).max() < 0.1
    assert np.isnan(result.diagnostics.objective)  # not defined for several classes


def test_objective_only_for_a_single_unit_pce_class(sioux):
    net, demand = sioux
    assert not np.isnan(net.assign(demand, gap=1e-4).diagnostics.objective)
    heavy = pyta.TrafficClass(demand, pce=2.0)
    assert np.isnan(net.assign(heavy, gap=1e-4).diagnostics.objective)


def two_parallel_links():
    """c1 = 10 + v1 and c2 = 20 + v2: capacity equals free-flow time and b = 1, power = 1."""
    links = {"from_node": [0, 0], "to_node": [1, 1], "free_flow_time": [10.0, 20.0],
             "capacity": [10.0, 20.0], "b": [1.0, 1.0], "power": [1.0, 1.0]}
    return pyta.Network.from_tables(links)


def test_toll_and_ban_have_closed_form_equilibria():
    net = two_parallel_links()
    ten = pyta.Demand.from_table(net, {"origin": [0], "destination": [1], "flow": [10.0]})
    toll = pyta.TrafficClass(ten, extra_link_cost=np.array([5.0, 0.0]))
    result = net.assign([ten, toll], gap=1e-10)
    a, b = result.links.by_class
    assert np.allclose(a, [10.0, 0.0], atol=1e-6)
    assert np.allclose(b, [2.5, 7.5], atol=1e-6)

    twenty_five = pyta.Demand.from_table(net, {"origin": [0], "destination": [1], "flow": [25.0]})
    banned = pyta.TrafficClass(ten, allowed_links=np.array([False, True]))
    result = net.assign([twenty_five, banned], gap=1e-10)
    a, b = result.links.by_class
    assert np.allclose(a, [22.5, 2.5], atol=1e-6)
    assert np.allclose(b, [0.0, 10.0], atol=1e-6)


def test_pce_loads_the_network_by_its_weight():
    net = two_parallel_links()
    fifteen = pyta.Demand.from_table(net, {"origin": [0], "destination": [1], "flow": [15.0]})
    result = net.assign(pyta.TrafficClass(fifteen, pce=2.0), gap=1e-10)
    assert np.allclose(result.links.volume, [20.0, 10.0], atol=1e-6)
    assert np.allclose(result.links.by_class[0], [10.0, 5.0], atol=1e-6)


# --------------------------------------------------------------------- turns


def two_turn_routes(turn_rows):
    """Routes 0-1-3 (links 0, 1) and 0-2-3 (links 2, 3); every link costs 3."""
    links = {"from_node": [0, 1, 0, 2], "to_node": [1, 3, 2, 3], "free_flow_time": [3.0] * 4,
             "capacity": [1.0] * 4, "b": [0.0] * 4, "power": [1.0] * 4}
    net = pyta.Network.from_tables(links, turn_rows)
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [3], "flow": [30.0]})
    return net, demand


def test_turn_delays_and_penalties_have_closed_form_equilibria():
    # Turn (0->1) has delay 2 + 0.2 v, turn (2->3) a constant delay of 5: vA = 15.
    delays = {"from_link": [0, 2], "to_link": [1, 3],
              "delay_free_flow": [2.0, 5.0], "delay_capacity": [10.0, 1.0],
              "delay_b": [1.0, 0.0], "delay_power": [1.0, 1.0]}
    net, demand = two_turn_routes(delays)
    from_link, to_link = net.turn_links()
    assert list(zip(from_link, to_link)) == [(0, 1), (2, 3)]
    result = net.assign(demand, gap=1e-10)
    assert np.allclose(result.turns.by_class[0], [15.0, 15.0], atol=1e-6)

    # A fixed penalty of 2 on the second turn moves the split to 25 / 5.
    delays["penalty"] = [np.nan, 2.0]
    net, demand = two_turn_routes(delays)
    result = net.assign(demand, gap=1e-10)
    assert np.allclose(result.turns.by_class[0], [25.0, 5.0], atol=1e-6)


def test_banned_turn_forces_the_other_route():
    net, demand = two_turn_routes({"from_link": [0], "to_link": [1], "banned": [True]})
    assert net.num_turns == 1
    result = net.assign(demand, gap=1e-10)
    assert np.allclose(result.links.by_class[0], [0.0, 0.0, 30.0, 30.0])


def test_centroids_are_not_passed_through():
    # Node 1 sits on the cheap route; making it a centroid forces the long way round.
    links = {"from_node": [0, 1, 0, 2], "to_node": [1, 3, 2, 3], "free_flow_time": [1.0, 1.0, 5.0, 5.0],
             "capacity": [100.0] * 4}
    open_net = pyta.Network.from_tables(links)
    demand = pyta.Demand.from_table(open_net, {"origin": [0], "destination": [3], "flow": [10.0]})
    closed = pyta.Network.from_tables(links, is_centroid=[False, True, False, False])
    assert open_net.assign(demand).links.by_class[0][0] > 9.0
    assert closed.assign(demand).links.by_class[0][0] == 0.0
    assert closed.is_centroid.tolist() == [False, True, False, False]


# -------------------------------------------------------------------- errors


def test_bad_input_raises_value_error(sioux):
    net, demand = sioux
    with pytest.raises(ValueError, match="unknown order"):
        net.assign(demand, order="sideways")
    with pytest.raises(ValueError, match="at least one class"):
        net.assign([])
    with pytest.raises(ValueError, match="must be a Demand"):
        net.assign("demand")
    with pytest.raises(ValueError, match="missing column 'capacity'"):
        pyta.Network.from_tables({"from_node": [0], "to_node": [1], "free_flow_time": [1.0]})
    with pytest.raises(ValueError):  # node id out of range
        pyta.Network.from_tables({"from_node": [0], "to_node": [5], "free_flow_time": [1.0],
                                  "capacity": [1.0]}, num_nodes=2)
    with pytest.raises(ValueError, match="not an allowed turn|NotAdjacent"):
        two_turn_routes({"from_link": [0], "to_link": [3], "penalty": [1.0]})
    with pytest.raises(ValueError, match="all of delay_free_flow"):
        two_turn_routes({"from_link": [0], "to_link": [1], "delay_free_flow": [1.0]})
    with pytest.raises(ValueError, match="InvalidClass|pce"):
        net.assign(pyta.TrafficClass(demand, pce=0.0))
    with pytest.raises(IndexError):
        net.assign(demand, gap=1e-3).paths.arrays(class_index=4)


def test_unreachable_demand_is_an_error():
    net = pyta.Network.from_tables({"from_node": [0], "to_node": [1], "free_flow_time": [1.0],
                                    "capacity": [10.0]}, num_nodes=3)
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [1.0]})
    with pytest.raises(ValueError, match="no allowed path serves"):
        net.assign(demand)


def two_zone_network():
    # 0 -> 1 -> 2; node 3 is isolated and nothing leaves node 2.
    return pyta.Network.from_tables({"from_node": [0, 1], "to_node": [1, 2],
                                     "free_flow_time": [1.0, 1.0], "capacity": [100.0, 100.0]},
                                    num_nodes=4)


def test_skip_unreachable_assigns_the_rest_and_reports_the_rest():
    net = two_zone_network()
    trips = {"origin": [0, 2, 0], "destination": [2, 0, 3], "flow": [10.0, 4.0, 1.5]}
    demand = pyta.Demand.from_table(net, trips)
    result = net.assign(demand, unreachable="skip")
    assert result.converged
    assert result.links.by_class[0].tolist() == [10.0, 10.0]
    assert result.unrouted.flow == [5.5]
    table = result.unrouted.table().sort_values("origin").reset_index(drop=True)
    assert table.to_dict("list") == {"origin": [0, 2], "destination": [3, 0], "flow": [1.5, 4.0]}


def test_skip_unreachable_reports_per_class_and_honours_allowed_links():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [3.0]})
    cars = pyta.TrafficClass(demand, name="cars")
    trucks = pyta.TrafficClass(demand, name="trucks", allowed_links=np.array([True, False]))
    result = net.assign([cars, trucks], unreachable="skip")
    assert result.unrouted.flow == [0.0, 3.0]
    assert result.links.by_class[0].tolist() == [3.0, 3.0]
    assert result.links.by_class[1].tolist() == [0.0, 0.0]
    table = result.unrouted.table()
    assert table["class"].tolist() == ["trucks"]
    with pytest.raises(ValueError, match="no allowed path serves"):
        net.assign([cars, trucks])


def test_skip_unreachable_with_nothing_left_to_route():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [2], "destination": [0], "flow": [4.0]})
    result = net.assign(demand, unreachable="skip")
    assert result.converged
    assert result.links.by_class[0].tolist() == [0.0, 0.0]
    assert result.unrouted.flow == [4.0]


def test_skip_unreachable_changes_nothing_when_all_demand_is_routable():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [10.0]})
    plain = net.assign(demand)
    skipped = net.assign(demand, unreachable="skip")
    assert skipped.links.by_class[0].tolist() == plain.links.by_class[0].tolist()
    assert skipped.unrouted.flow == [0.0]
    assert len(skipped.unrouted.table()) == 0


def test_unreachable_option_is_validated():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [1.0]})
    with pytest.raises(ValueError, match="unknown unreachable .ignore.: use one of"):
        net.assign(demand, unreachable="ignore")


# ------------------------------------------------------------------ GMNS


def write_gmns(folder: Path, nodes: str, links: str, movements: str = "", config: str = "") -> Path:
    folder.mkdir(exist_ok=True)
    (folder / "node.csv").write_text(nodes)
    (folder / "link.csv").write_text(links)
    if movements:
        (folder / "movement.csv").write_text(movements)
    if config:
        (folder / "config.csv").write_text(config)
    return folder


GMNS_LINK_HEAD = "link_id,from_node_id,to_node_id,directed,length,free_speed,capacity,lanes\n"


def test_gmns_sioux_falls_matches_published_equilibrium(tmp_path):
    """Sioux Falls written as GMNS (km at 60 km/h makes length equal minutes)."""
    net = pyta.Network.from_tntp(str(DATA / "SiouxFalls_net.tntp"))
    table = pd.read_csv(
        DATA / "SiouxFalls_net.tntp", sep=r"\s+", skiprows=8, comment="~", header=None,
        usecols=range(10), names=["f", "t", "cap", "len", "fft", "b", "power", "speed", "toll", "type"],
    )
    nodes = "node_id,zone_id\n" + "".join(f"{n},{n}\n" for n in range(1, 25))
    links = "link_id,from_node_id,to_node_id,directed,length,free_speed,capacity,lanes,vdf_alpha,vdf_beta\n"
    for i, r in table.iterrows():
        links += f"{i},{int(r.f)},{int(r.t)},1,{r.fft},60,{r.cap},1,{r.b},{r.power}\n"
    folder = write_gmns(tmp_path / "sf", nodes, links)
    o, d, q = read_trip_table()
    pd.DataFrame({"o_zone_id": o + 1, "d_zone_id": d + 1, "volume": q}).to_csv(
        tmp_path / "demand.csv", index=False)

    gmns = pyta.Network.from_gmns(str(folder), length_unit="km", speed_unit="kph")
    assert gmns.num_links == net.num_links and gmns.node_ids[0] == "1"
    demand = pyta.Demand.from_gmns(gmns, str(tmp_path / "demand.csv"))
    assert demand.total() == pytest.approx(360_600)
    result = gmns.assign(demand, gap=1e-6)
    assert result.converged
    assert result.diagnostics.objective == pytest.approx(PUBLISHED_OBJECTIVE, rel=1e-6)


def test_gmns_zones_movements_and_unknown_zones(tmp_path):
    # Zone 7 spans nodes a and c; zone 8 is node b. One undirected link a-b.
    nodes = "node_id,zone_id\na,7\nb,8\nc,7\nd,\n"
    links = GMNS_LINK_HEAD + "ab,a,b,false,6,60,900,2\nbc,b,c,true,6,60,900,2\ncd,c,d,true,1,60,900,1\nbd,b,d,true,1,60,900,1\n"
    movements = "mvmt_id,node_id,ib_link_id,ob_link_id,penalty\n1,b,ab,bd,30\n"
    folder = write_gmns(tmp_path / "g", nodes, links, movements, "long_length,speed\nkm,kph\n")
    net = pyta.Network.from_gmns(str(folder))
    assert net.num_links == 5  # the undirected link is two links
    assert net.link_ids == ["ab", "ab", "bc", "cd", "bd"]
    assert net.node_ids == ["a", "b", "c", "d"]
    # Entering b on ab (link 0) only bd is listed, so bc is banned for that link.
    from_link, to_link = (x.tolist() for x in net.turn_links())
    assert [t for f, t in zip(from_link, to_link) if f == 0] == [4]

    (tmp_path / "demand.csv").write_text("o_zone_id,d_zone_id,volume\n7,8,60\n9,8,5\n")
    with pytest.raises(ValueError, match="unknown zone '9'"):
        pyta.Demand.from_gmns(net, str(tmp_path / "demand.csv"))
    demand = pyta.Demand.from_gmns(net, str(tmp_path / "demand.csv"), skip_unknown_zones=True)
    assert demand.total() == pytest.approx(60)
    assert demand.unknown_zone_flow == 5


def test_gmns_needs_units_and_a_gmns_network(tmp_path):
    folder = write_gmns(tmp_path / "g", "node_id\na\nb\n", GMNS_LINK_HEAD + "ab,a,b,true,1,60,900,1\n")
    with pytest.raises(ValueError, match="no length unit"):
        pyta.Network.from_gmns(str(folder))
    sioux = pyta.Network.from_tntp(str(DATA / "SiouxFalls_net.tntp"))
    (tmp_path / "d.csv").write_text("o_zone_id,d_zone_id,volume\n1,2,1\n")
    with pytest.raises(ValueError, match="from_gmns"):
        pyta.Demand.from_gmns(sioux, str(tmp_path / "d.csv"))
    assert sioux.node_ids is None


# --------------------------------------------------------------- preloads


def test_preload_acts_like_fixed_traffic():
    # c1 = 10 + v1, c2 = 20 + v2 with 10 trips and 5 fixed vehicles on link 1:
    # 15 + x = 20 + (10 - x) gives x = 7.5.
    net = two_parallel_links()
    ten = pyta.Demand.from_table(net, {"origin": [0], "destination": [1], "flow": [10.0]})
    result = net.assign(ten, gap=1e-10, link_preload=np.array([5.0, 0.0]))
    np.testing.assert_allclose(result.links.by_class[0], [7.5, 2.5], atol=1e-6)
    np.testing.assert_allclose(result.links.time, [22.5, 22.5], atol=1e-6)
    np.testing.assert_allclose(result.links.preload, [5.0, 0.0])
    assert result.turns.preload is None

    # The same as five vehicles that can only use link 1.
    five = pyta.Demand.from_table(net, {"origin": [0], "destination": [1], "flow": [5.0]})
    fixed = pyta.TrafficClass(five, allowed_links=np.array([True, False]))
    both = net.assign([ten, fixed], gap=1e-10)
    np.testing.assert_allclose(both.links.by_class[0], result.links.by_class[0], atol=1e-6)


def test_zero_preload_changes_nothing_and_bad_preloads_are_rejected(sioux):
    net, demand = sioux
    plain = net.assign(demand, gap=1e-6)
    zero = net.assign(demand, gap=1e-6, link_preload=np.zeros(net.num_links),
                       turn_preload=np.zeros(net.num_turns))
    np.testing.assert_array_equal(zero.links.by_class[0], plain.links.by_class[0])
    assert zero.diagnostics.objective == plain.diagnostics.objective
    with pytest.raises(ValueError, match="link_preload has 3 entries"):
        net.assign(demand, link_preload=np.zeros(3))
    with pytest.raises(ValueError, match="non-negative"):
        net.assign(demand, turn_preload=np.full(net.num_turns, -1.0))


# ------------------------------------------------------------------ tables


def home_to_work():
    """The getting-started example: three routes from node 0 to node 3."""
    links = pd.DataFrame({"from_node": [0, 1, 0, 2, 2], "to_node": [1, 3, 2, 3, 1],
                          "free_flow_time": [6.0, 6.0, 4.0, 4.0, 1.0],
                          "capacity": [2000, 2000, 1500, 800, 1000]})
    net = pyta.Network.from_tables(links)
    return net, pyta.Demand.from_table(net, {"origin": [0], "destination": [3], "flow": [3000.0]})


def test_tables_match_the_arrays():
    net, demand = home_to_work()
    result = net.assign(demand, gap=1e-10)

    links = result.links.table()
    assert list(links.columns) == ["from_node", "to_node", "flow", "volume", "voc", "time"]
    np.testing.assert_array_equal(links["flow"], result.links.by_class[0])
    np.testing.assert_allclose(links["voc"], result.links.volume / net.capacity)

    turns = result.turns.table()
    assert list(turns.columns) == ["from_link", "to_link", "node", "flow", "volume", "time"]
    np.testing.assert_array_equal(turns["node"], [1, 2, 2, 1])

    # Every used path has the same time at equilibrium, and the times add up.
    paths = result.paths.table()
    assert sorted(paths["links"]) == [(0, 1), (2, 3), (2, 4, 1)]
    np.testing.assert_allclose(paths["time"], paths["time"].iloc[0], rtol=1e-6)
    expected = [sum(result.links.time[list(p)]) for p in paths["links"]]
    np.testing.assert_allclose(paths["time"], expected)
    long = paths.explode("links")
    assert long.loc[long["links"] == 4, "flow"].sum() == pytest.approx(result.links.by_class[0][4])


def test_tables_label_classes_and_preloads():
    net, demand = home_to_work()
    trucks = pyta.TrafficClass(
        pyta.Demand.from_table(net, {"origin": [0], "destination": [3], "flow": [200.0]}),
        pce=2.5, allowed_links=np.array([True, True, False, False, False]), name="trucks")
    result = net.assign([demand, trucks], link_preload=np.full(5, 10.0))
    assert result.class_names == ["0", "trucks"]

    links = result.links.table()
    assert {"flow_0", "flow_trucks", "preload"} <= set(links.columns)
    np.testing.assert_allclose(links["flow"], links["flow_0"] + links["flow_trucks"])
    np.testing.assert_allclose(links["voc"], (result.links.volume + 10.0) / net.capacity)

    paths = result.paths.table()
    assert set(paths["class"]) == {"0", "trucks"}
    assert list(result.paths.table(1)["links"]) == [(0, 1)]


# ---------------------------------------------------------------- grouped result and options


def two_class_result():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [3.0]})
    cars = pyta.TrafficClass(demand, name="cars")
    trucks = pyta.TrafficClass(demand, pce=2.0, name="trucks")
    return net, net.assign([cars, trucks], gap=1e-8)


def test_result_groups_agree_with_each_other():
    net, result = two_class_result()
    links = result.links
    assert links.by_class["cars"] is links.by_class[0]
    assert links.by_class.names == ["cars", "trucks"]
    np.testing.assert_allclose(links.flow, links.by_class[0] + links.by_class[1])
    np.testing.assert_allclose(links.volume, links.by_class[0] + 2.0 * links.by_class[1])
    np.testing.assert_allclose(links.voc, links.volume / net.capacity)
    assert links.preload is None and result.turns.preload is None
    with pytest.raises(KeyError, match="no class named 'vans'"):
        links.by_class["vans"]
    assert not links.flow.flags.writeable  # cached arrays cannot be edited by accident
    table = links.table()
    assert list(table["flow_trucks"]) == list(links.by_class["trucks"])
    assert result.paths.table()["class"].tolist() == ["cars", "trucks"]
    assert result.paths.table(1)["class"].tolist() == ["trucks"]


def test_diagnostics_and_summary():
    _, result = two_class_result()
    d = result.diagnostics
    assert len(d.sweep_gaps) == result.iterations
    assert set(d.times) >= {"shortest_paths", "flow_updates"}
    assert np.isnan(d.objective)  # several classes
    text = result.summary()
    assert "converged after" in text and "2 classes" in text
    assert "cars" in text and "trucks" in text and "busiest link" in text
    assert "left out" not in text
    assert repr(result).startswith("AssignResult(converged=True")


def test_summary_reports_demand_left_out():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0, 2], "destination": [2, 0], "flow": [3.0, 4.0]})
    result = net.assign(demand, unreachable="skip")
    assert result.unrouted.total == 4.0 and len(result.unrouted) == 1
    assert "4.0 trips left out" in result.summary()


def test_options_are_string_enums_that_check_their_values():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [3.0]})
    assert pyta.Order.SHUFFLE == "shuffle" and str(pyta.Order.LARGEST_GAP) == "largest_gap"
    by_enum = net.assign(demand, order=pyta.Order.SHUFFLE, seed=2)
    by_name = net.assign(demand, order="shuffle", seed=2)
    np.testing.assert_array_equal(by_enum.links.flow, by_name.links.flow)
    with pytest.raises(ValueError, match=r"unknown order 'sideways': use one of 'fixed', 'shuffle'"):
        net.assign(demand, order="sideways")
    with pytest.raises(ValueError, match="unknown unreachable"):
        net.assign(demand, unreachable="ignore")
    assert net.assign(demand, unreachable=pyta.Unreachable.SKIP).unrouted.total == 0.0


def test_options_after_classes_are_keyword_only():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [3.0]})
    with pytest.raises(TypeError):
        net.assign(demand, 1e-6)
    with pytest.raises(TypeError):
        pyta.assign(net, demand, 1e-6)


def test_function_and_method_are_the_same():
    net = two_zone_network()
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [2], "flow": [3.0]})
    np.testing.assert_array_equal(pyta.assign(net, demand).links.flow, net.assign(demand).links.flow)


def test_unreachable_error_names_the_pairs():
    net = pyta.Network.from_tables({"from_node": [0], "to_node": [1], "free_flow_time": [1.0],
                                    "capacity": [10.0]}, num_nodes=8)
    pairs = [(0, d) for d in range(2, 8)]  # six pairs have no path
    demand = pyta.Demand.from_table(
        net, {"origin": [o for o, _ in pairs], "destination": [d for _, d in pairs], "flow": [2.0] * 6})
    with pytest.raises(ValueError) as error:
        net.assign(demand)
    message = str(error.value)
    assert "6 origin-destination pair(s) carrying 12 trips" in message
    assert "0 -> 2 (class 0, 2 trips)" in message and "and 1 more" in message
    assert "unreachable='skip'" in message


def test_old_names_still_work_but_warn():
    net, result = two_class_result()
    old = {
        "link_volume": result.links.volume,
        "turn_volume": result.turns.volume,
        "link_time": result.links.time,
        "link_flow": result.links.by_class[0],
        "unrouted_flow": result.unrouted.flow,
        "sweep_gaps": result.diagnostics.sweep_gaps,
        "times": result.diagnostics.times,
    }
    for name, value in old.items():
        with pytest.warns(DeprecationWarning, match=f"AssignResult.{name} is deprecated"):
            got = getattr(result, name)
        np.testing.assert_array_equal(got, value)
    with pytest.warns(DeprecationWarning, match="class_link_flow"):
        assert len(result.class_link_flow) == 2
    with pytest.warns(DeprecationWarning):
        assert result.objective is None
    with pytest.warns(DeprecationWarning, match=r"link_table\(\) is deprecated"):
        assert len(result.link_table()) == net.num_links
    with pytest.warns(DeprecationWarning, match="path_table"):
        assert len(result.path_table(0)) > 0
    with pytest.warns(DeprecationWarning, match=r"result.paths\(class_index\)"):
        assert len(result.paths(1)["flow"]) > 0
    with pytest.warns(DeprecationWarning, match=r"result.unrouted\(\)"):
        assert len(result.unrouted()["flow"]) == 0
    with pytest.warns(DeprecationWarning, match="unrouted_table"):
        assert len(result.unrouted_table()) == 0


def test_old_centroid_and_intrazonal_names_still_work_but_warn():
    links = {"from_node": [0, 1], "to_node": [1, 2], "free_flow_time": [1.0, 1.0], "capacity": [10.0, 10.0]}
    flags = [False, True, False]
    with pytest.warns(DeprecationWarning, match="is_endpoint is deprecated"):
        net = pyta.Network.from_tables(links, is_endpoint=flags)
    assert net.is_centroid.tolist() == flags
    with pytest.warns(DeprecationWarning, match="Network.is_endpoint is deprecated"):
        assert net.is_endpoint.tolist() == flags
    with pytest.warns(DeprecationWarning, match="is_endpoint is deprecated"):
        pyta.Network(3, np.array([0, 1]), np.array([1, 2]), np.ones(2), np.full(2, 10.0),
                     is_endpoint=np.array(flags))
    demand = pyta.Demand.from_table(net, {"origin": [0], "destination": [0], "flow": [5.0]})
    assert demand.unknown_zone_flow == 0.0
    with pytest.warns(DeprecationWarning, match="Demand.skipped_flow is deprecated"):
        assert demand.skipped_flow == demand.unknown_zone_flow
