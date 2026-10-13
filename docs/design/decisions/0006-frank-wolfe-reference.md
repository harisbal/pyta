# 0006. Frank-Wolfe stays a reference solver

**Decision.** Frank-Wolfe is available (with turn costs, single class) but is not the recommended solver.

**Why keep it.** It is simple, link-based and low on memory, and it shares no code with gradient
projection beyond the delay functions and all-or-nothing loading. That makes it a good cross-check:
the two solvers must approach the same objective.

**Why not the default.** It slows down near the solution: Sioux Falls needs 1 063 iterations for gap
1e-4 and 9 825 for 1e-5. On a five-node test network gradient projection reached a gap of 3e-16 while
Frank-Wolfe was still at 5e-5 after 20 000 iterations, which is why tests compare objectives and
accuracy-adjusted flows when checking one against the other.
