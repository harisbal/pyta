# TNTP benchmark data

Files in this folder come from the public repository
https://github.com/bstabler/TransportationNetworks (Transportation Networks for Research).

- `SiouxFalls/SiouxFalls_net.tntp`   network (24 zones, 76 links)
- `SiouxFalls/SiouxFalls_trips.tntp` OD demand (total 360600)
- `SiouxFalls/SiouxFalls_flow.tntp`  published user-equilibrium flows, used later to validate the solvers
- `Anaheim/`        38 zones, 416 nodes, 914 links; zones 1-38 are centroids (`FIRST THRU NODE 39`)
- `ChicagoSketch/`  larger sketch network; source folder is `Chicago-Sketch` in the upstream repository

Larger networks are not stored in the repository. Fetch them with

```sh
python data/tntp/fetch.py ChicagoRegional Philadelphia
```

(`ChicagoRegional` is 1 790 zones, 12 982 nodes and 39 018 links; `Philadelphia` is 1 525 zones,
13 389 nodes and 40 003 links.)
