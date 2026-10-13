# 0004. Multi-class model: PCE, extra costs, allowed links

**Decision.** A class has its own demand, a PCE, an optional per-link extra cost and an optional
allowed-links mask. Delays depend on the PCE-weighted total volume. Only gradient projection supports classes.

**Why this shape.** It covers the usual needs (cars and trucks, tolls with different values of time,
truck bans) with data the user already has, and without per-class delay functions. Extra costs are in
time units so the core needs no value-of-time model.

**Implementation.** Disallowed links get infinite cost in that class's cost array, which the
shortest-path search treats as unusable. The single-class functions are wrappers over the multi-class
core, so the existing single-class tests are regression tests for it.

**Asymmetry.** With different PCEs, \(\partial g_k / \partial x_{k'} \ne \partial g_{k'} / \partial x_k\), so there is no objective to minimise.
Gradient projection still works: shifting class-\(k\) flow changes volume by \(\mathrm{pce}_k\) times
the shift, and the Newton step scales its slope accordingly. Convergence is judged by the relative gap,
and Frank-Wolfe (which needs an objective for its line search) is not offered.

**Validation.** Identical classes reproduce the published single-class solution; tolls, bans and PCE
match closed forms.
