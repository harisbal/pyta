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
    demand = pyta.Demand.from_tntp(str(DATA / "SiouxFalls_trips.tntp"), net)
    return net, demand


@pytest.fixture(scope="module")
def published():
    """Published equilibrium volumes; the flow file lists links in network-file order."""
    rows = [line.split() for line in (DATA / "SiouxFalls_flow.tntp").read_text().splitlines()]
    return np.array([float(r[2]) for r in rows if len(r) >= 4 and r[0].isdigit()])


# ------------------------------------------------------------ single class


def test_published_equilibrium(sioux, published):
    net, demand = sioux
    result = pyta.assign(net, demand, gap=1e-8)
    assert result.converged and result.relative_gap <= 1e-8
    assert abs(result.objective - PUBLISHED_OBJECTIVE) < 0.01
    assert np.abs(result.link_flow - published).max() < 0.1
    assert result.link_flow.dtype == np.float64
    assert result.turn_flow.shape == (net.num_turns,)
    assert len(result.sweep_gaps) == result.iterations
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
    rebuilt = pyta.network_from_tables(links)
    assert (rebuilt.num_nodes, rebuilt.num_links, rebuilt.num_turns) == (24, 76, 178)

    o, d, q = read_trip_table()
    demand = pyta.demand_from_table(pd.DataFrame({"origin": o, "destination": d, "flow": q}), 24)
    assert demand.total() == 360_600.0
    a = pyta.assign(rebuilt, demand, gap=1e-9)
    b = pyta.assign(net, demand, gap=1e-9)
    assert np.allclose(a.link_flow, b.link_flow, atol=1e-6)


def test_paths_table_is_consistent(sioux):
    net, demand = sioux
    result = pyta.assign(net, demand, gap=1e-6)
    paths = result.paths()
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
    assert np.allclose(rebuilt, result.link_flow, atol=1e-6)


@pytest.mark.parametrize("order", ["fixed", "shuffle", "alternate", "largest_gap",
                                   "largest_demand", "smallest_demand"])
def test_every_origin_order_converges(sioux, published, order):
    net, demand = sioux
    result = pyta.assign(net, demand, gap=1e-6, order=order, seed=3)
    assert result.converged
    assert np.abs(result.link_flow - published).max() < 5.0  # 1e-6 gap: a few vehicles


def test_iteration_limit_is_reported(sioux):
    net, demand = sioux
    result = pyta.assign(net, demand, gap=1e-12, max_iter=3)
    assert not result.converged and result.iterations == 3


def test_chunk_size_and_threads(sioux, published):
    net, demand = sioux
    sequential = pyta.assign(net, demand, gap=1e-8, chunk_size=1)
    chunked = pyta.assign(net, demand, gap=1e-8, chunk_size=8)
    assert sequential.converged and chunked.converged
    assert np.abs(chunked.link_flow - published).max() < 0.1
    # The same chunk size gives bit-identical answers whatever the number of threads.
    one = pyta.assign(net, demand, gap=1e-7, chunk_size=8, threads=1)
    four = pyta.assign(net, demand, gap=1e-7, chunk_size=8, threads=4)
    assert np.array_equal(one.link_flow, four.link_flow)
    with pytest.raises(ValueError, match="threads must be at least 1"):
        pyta.assign(net, demand, threads=0)


def test_phase_times(sioux):
    net, demand = sioux
    times = pyta.assign(net, demand, gap=1e-6).times
    assert set(times) == {"initial_assignment", "shortest_paths", "flow_updates",
                          "sweep_rebuild", "gap_checks"}
    assert all(t >= 0.0 for t in times.values())
    assert times["shortest_paths"] > 0.0


def test_assign_releases_the_gil(sioux):
    net, demand = sioux
    results = []

    def work():
        results.append(pyta.assign(net, demand, gap=1e-7).relative_gap)

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
    result = pyta.assign(net, [cars, vans], gap=1e-8)
    assert result.converged and len(result.class_link_flow) == 2
    total = result.class_link_flow[0] + result.class_link_flow[1]
    assert np.abs(total - published).max() < 0.1
    assert np.abs(result.link_volume - published).max() < 0.1
    assert result.objective is None  # not defined for several classes


def test_objective_only_for_a_single_unit_pce_class(sioux):
    net, demand = sioux
    assert pyta.assign(net, demand, gap=1e-4).objective is not None
    heavy = pyta.TrafficClass(demand, pce=2.0)
    assert pyta.assign(net, heavy, gap=1e-4).objective is None


def two_parallel_links():
    """c1 = 10 + v1 and c2 = 20 + v2: capacity equals free-flow time and b = 1, power = 1."""
    links = {"from_node": [0, 0], "to_node": [1, 1], "free_flow_time": [10.0, 20.0],
             "capacity": [10.0, 20.0], "b": [1.0, 1.0], "power": [1.0, 1.0]}
    return pyta.network_from_tables(links)


def test_toll_and_ban_have_closed_form_equilibria():
    net = two_parallel_links()
    ten = pyta.demand_from_table({"origin": [0], "destination": [1], "flow": [10.0]}, 2)
    toll = pyta.TrafficClass(ten, extra_link_cost=np.array([5.0, 0.0]))
    result = pyta.assign(net, [ten, toll], gap=1e-10)
    a, b = result.class_link_flow
    assert np.allclose(a, [10.0, 0.0], atol=1e-6)
    assert np.allclose(b, [2.5, 7.5], atol=1e-6)

    twenty_five = pyta.demand_from_table({"origin": [0], "destination": [1], "flow": [25.0]}, 2)
    banned = pyta.TrafficClass(ten, allowed_links=np.array([False, True]))
    result = pyta.assign(net, [twenty_five, banned], gap=1e-10)
    a, b = result.class_link_flow
    assert np.allclose(a, [22.5, 2.5], atol=1e-6)
    assert np.allclose(b, [0.0, 10.0], atol=1e-6)


def test_pce_loads_the_network_by_its_weight():
    net = two_parallel_links()
    fifteen = pyta.demand_from_table({"origin": [0], "destination": [1], "flow": [15.0]}, 2)
    result = pyta.assign(net, pyta.TrafficClass(fifteen, pce=2.0), gap=1e-10)
    assert np.allclose(result.link_volume, [20.0, 10.0], atol=1e-6)
    assert np.allclose(result.link_flow, [10.0, 5.0], atol=1e-6)


# --------------------------------------------------------------------- turns


def two_turn_routes(turn_rows):
    """Routes 0-1-3 (links 0, 1) and 0-2-3 (links 2, 3); every link costs 3."""
    links = {"from_node": [0, 1, 0, 2], "to_node": [1, 3, 2, 3], "free_flow_time": [3.0] * 4,
             "capacity": [1.0] * 4, "b": [0.0] * 4, "power": [1.0] * 4}
    net = pyta.network_from_tables(links, turn_rows)
    demand = pyta.demand_from_table({"origin": [0], "destination": [3], "flow": [30.0]}, 4)
    return net, demand


def test_turn_delays_and_penalties_have_closed_form_equilibria():
    # Turn (0->1) has delay 2 + 0.2 v, turn (2->3) a constant delay of 5: vA = 15.
    delays = {"from_link": [0, 2], "to_link": [1, 3],
              "delay_free_flow": [2.0, 5.0], "delay_capacity": [10.0, 1.0],
              "delay_b": [1.0, 0.0], "delay_power": [1.0, 1.0]}
    net, demand = two_turn_routes(delays)
    from_link, to_link = net.turn_links()
    assert list(zip(from_link, to_link)) == [(0, 1), (2, 3)]
    result = pyta.assign(net, demand, gap=1e-10)
    assert np.allclose(result.turn_flow, [15.0, 15.0], atol=1e-6)

    # A fixed penalty of 2 on the second turn moves the split to 25 / 5.
    delays["penalty"] = [np.nan, 2.0]
    net, demand = two_turn_routes(delays)
    result = pyta.assign(net, demand, gap=1e-10)
    assert np.allclose(result.turn_flow, [25.0, 5.0], atol=1e-6)


def test_banned_turn_forces_the_other_route():
    net, demand = two_turn_routes({"from_link": [0], "to_link": [1], "banned": [True]})
    assert net.num_turns == 1
    result = pyta.assign(net, demand, gap=1e-10)
    assert np.allclose(result.link_flow, [0.0, 0.0, 30.0, 30.0])


def test_endpoint_nodes_are_not_passed_through():
    # Node 1 sits on the cheap route; making it an endpoint forces the long way round.
    links = {"from_node": [0, 1, 0, 2], "to_node": [1, 3, 2, 3], "free_flow_time": [1.0, 1.0, 5.0, 5.0],
             "capacity": [100.0] * 4}
    demand = pyta.demand_from_table({"origin": [0], "destination": [3], "flow": [10.0]}, 4)
    open_net = pyta.network_from_tables(links)
    closed = pyta.network_from_tables(links, is_endpoint=[False, True, False, False])
    assert pyta.assign(open_net, demand).link_flow[0] > 9.0
    assert pyta.assign(closed, demand).link_flow[0] == 0.0
    assert closed.is_endpoint.tolist() == [False, True, False, False]


# -------------------------------------------------------------------- errors


def test_bad_input_raises_value_error(sioux):
    net, demand = sioux
    with pytest.raises(ValueError, match="unknown order"):
        pyta.assign(net, demand, order="sideways")
    with pytest.raises(ValueError, match="at least one class"):
        pyta.assign(net, [])
    with pytest.raises(ValueError, match="must be a Demand"):
        pyta.assign(net, "demand")
    with pytest.raises(ValueError, match="missing column 'capacity'"):
        pyta.network_from_tables({"from_node": [0], "to_node": [1], "free_flow_time": [1.0]})
    with pytest.raises(ValueError):  # node id out of range
        pyta.network_from_tables({"from_node": [0], "to_node": [5], "free_flow_time": [1.0],
                                  "capacity": [1.0]}, num_nodes=2)
    with pytest.raises(ValueError, match="not an allowed turn|NotAdjacent"):
        two_turn_routes({"from_link": [0], "to_link": [3], "penalty": [1.0]})
    with pytest.raises(ValueError, match="all of delay_free_flow"):
        two_turn_routes({"from_link": [0], "to_link": [1], "delay_free_flow": [1.0]})
    with pytest.raises(ValueError, match="InvalidClass|pce"):
        pyta.assign(net, pyta.TrafficClass(demand, pce=0.0))
    with pytest.raises(IndexError):
        pyta.assign(net, demand, gap=1e-3).paths(class_index=4)


def test_unreachable_demand_is_an_error():
    net = pyta.network_from_tables({"from_node": [0], "to_node": [1], "free_flow_time": [1.0],
                                    "capacity": [10.0]}, num_nodes=3)
    demand = pyta.demand_from_table({"origin": [0], "destination": [2], "flow": [1.0]}, 3)
    with pytest.raises(ValueError, match="Unreachable"):
        pyta.assign(net, demand)


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
    demand = pyta.Demand.from_gmns(str(tmp_path / "demand.csv"), gmns)
    assert demand.total() == pytest.approx(360_600)
    result = pyta.assign(gmns, demand, gap=1e-6)
    assert result.converged
    assert result.objective == pytest.approx(PUBLISHED_OBJECTIVE, rel=1e-6)


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
        pyta.Demand.from_gmns(str(tmp_path / "demand.csv"), net)
    demand = pyta.Demand.from_gmns(str(tmp_path / "demand.csv"), net, skip_unknown_zones=True)
    assert demand.total() == pytest.approx(60)
    assert demand.skipped_flow == 5


def test_gmns_needs_units_and_a_gmns_network(tmp_path):
    folder = write_gmns(tmp_path / "g", "node_id\na\nb\n", GMNS_LINK_HEAD + "ab,a,b,true,1,60,900,1\n")
    with pytest.raises(ValueError, match="no length unit"):
        pyta.Network.from_gmns(str(folder))
    sioux = pyta.Network.from_tntp(str(DATA / "SiouxFalls_net.tntp"))
    (tmp_path / "d.csv").write_text("o_zone_id,d_zone_id,volume\n1,2,1\n")
    with pytest.raises(ValueError, match="from_gmns"):
        pyta.Demand.from_gmns(str(tmp_path / "d.csv"), sioux)
    assert sioux.node_ids is None


# --------------------------------------------------------------- periods


def line_network():
    """a -> b -> c, 10 and 20 minutes, never congested."""
    return pyta.network_from_tables({
        "from_node": [0, 1], "to_node": [1, 2],
        "free_flow_time": [10.0, 20.0], "capacity": [1e6, 1e6], "b": [0.0, 0.0],
    })


def test_unfinished_trips_preload_the_links_still_ahead_of_them():
    net = line_network()
    busy = pyta.demand_from_table({"origin": [0], "destination": [2], "flow": [60.0]}, 3)
    quiet = pyta.demand_from_table({"origin": [], "destination": [], "flow": []}, 3)
    first, second, third = pyta.assign_periods(
        net, [pyta.Period(60, busy), pyta.Period(60, quiet), pyta.Period(60, quiet)])
    # Path time 30 of 60 minutes; a third of the trips still travelling are on the
    # 10-minute link and still have to enter the second link.
    np.testing.assert_allclose(first.link_preload, [0.0, 0.0])
    np.testing.assert_allclose(second.link_preload, [0.0, 10.0])
    np.testing.assert_allclose(second.turn_preload, [10.0])
    np.testing.assert_allclose(third.link_preload, [0.0, 0.0])
    # The assignment itself does not include the preload, and there is no objective.
    np.testing.assert_allclose(first.link_volume, [60.0, 60.0])
    np.testing.assert_allclose(second.link_volume, [0.0, 0.0])
    assert first.objective is None


def test_periods_are_validated_and_sioux_falls_period_matches_assign(sioux):
    net, demand = sioux
    with pytest.raises(ValueError, match="duration"):
        pyta.assign_periods(net, [pyta.Period(0, demand)])
    single = pyta.assign(net, demand, gap=1e-6)
    assert single.link_preload is None
    (period,) = pyta.assign_periods(net, [pyta.Period(60, demand)], gap=1e-6)
    np.testing.assert_allclose(period.link_volume, single.link_volume)
    np.testing.assert_allclose(period.link_preload, 0.0)
