# cad-vs-cad-bench: twenty worked cases

Twenty cases of the private `cad-vs-cad-bench` data set (drawing in, mass out), each modelled with the
Python authoring layer. Every script builds its part from nothing, writes the `.wpc` file beside
itself (a folder `n.parts/` for a drawing of several items: one file per item and the assembly) and a
drawing sheet, and prints the mass. The drawings they were modelled from are not in this repository.

`python3 run_all.py` runs all of them and compares each mass with the expected one (about one minute).

| script | part | expected | built |
|---|---|---|---|
| first10/1 | Alternator bracket | 0.785 ± 0.001 lb | 0.7851 |
| first10/2 | Curved pass through | 1089 ± 1 g | 1089.00 |
| first10/3 | T handle 2 | 162 ± 0.4 g | 162.157 |
| first10/4 | Triangle block bracket | 424.2 ± 0.4 g | 424.193 |
| first10/5 | Pipe filter | 74.76 ± 0.03 g | 74.761 |
| first10/6 | 3 weight plates | 25.31 ± 0.02 kg | 25.311 |
| first10/7 | Canopy bracket | 26.35 ± 0.02 g | 26.350 |
| first10/8 | Dual alignment tool | 2116 ± 2 g | 2115.92 |
| first10/9 | Tie breaker 1 | 4.5 ± 0.01 lb | 4.540 (miss) |
| first10/10 | Rocking lid | 0.470 ± 0.001 lb | 0.4700 |
| random10/1 | Small box with dividers | 450.0 ± 0.3 g | 450.016 |
| random10/2 | Umbrella stand | 3.061 ± 0.003 lb | 3.0611 |
| random10/3 | Flag pole adapter | 313.6 ± 0.2 g | 313.618 |
| random10/4 | Coupling bracket | 105.8 ± 0.1 g | 105.800 |
| random10/5 | Three shelf bracket | 509 ± 1 g | 509.06 |
| random10/6 | Game stand | 1409 ± 1 g | 1409.00 |
| random10/7 | Tabbed washer | 1985 ± 2 g | 1985.08 |
| random10/8 | Circular grill top | 518 ± 2 g | 518.20 |
| random10/9 | Light mount | 1950 ± 2 g | 1950.05 |
| random10/10 | Notch bracket | 623 ± 1 g | 623.05 |

`first10/9.py` follows the drawing and misses the published answer; `first10/9-as-answered.py` is the
variant that reproduces it (4.507 lb). Mass is one criterion of five: an independent grading of the
sheets against the original drawings accepted only `first10/1` in full when last run.
