//! Volume-delay functions: cost of a link (or turn) as a function of its flow.
//!
//! A `Vdf` works on whole arrays at once. Parameters live inside the
//! implementation, one entry per link, so the hot loops stay free of per-link
//! dynamic calls and the same type serves links today and turns later.
//! Flows must be non-negative.

/// Cost function for a set of elements (links or turns), indexed 0..len.
pub trait Vdf {
    /// Number of elements this function was built for.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `c_i(flow)` for one element. Path-based solvers update a few elements at a
    /// time and need this.
    fn cost_at(&self, index: usize, flow: f64) -> f64;

    /// `c_i'(flow)` for one element. Gradient projection needs this for its Newton step.
    fn derivative_at(&self, index: usize, flow: f64) -> f64;

    /// `(c_i(flow), c_i'(flow))` for one element. Implementations that share work between
    /// the two should override this; the path-based solver calls it for every element whose
    /// flow changes.
    fn cost_and_derivative_at(&self, index: usize, flow: f64) -> (f64, f64) {
        (self.cost_at(index, flow), self.derivative_at(index, flow))
    }

    /// `out[i] = c_i(flows[i])`.
    fn cost(&self, flows: &[f64], out: &mut [f64]) {
        debug_assert!(flows.len() == self.len() && out.len() == self.len());
        for (i, (&flow, slot)) in flows.iter().zip(out.iter_mut()).enumerate() {
            *slot = self.cost_at(i, flow);
        }
    }

    /// `out[i] = c_i'(flows[i])`.
    fn derivative(&self, flows: &[f64], out: &mut [f64]) {
        debug_assert!(flows.len() == self.len() && out.len() == self.len());
        for (i, (&flow, slot)) in flows.iter().zip(out.iter_mut()).enumerate() {
            *slot = self.derivative_at(i, flow);
        }
    }

    /// `Σ_i ∫_0^{flows[i]} c_i(x) dx`: the Beckmann objective, which Frank-Wolfe's
    /// line search and the equilibrium checks use.
    fn integral(&self, flows: &[f64]) -> f64;
}

/// Zero cost whatever the flow: "this set of elements has no delay function".
/// Used for turns when only fixed turn penalties (or nothing) apply.
#[derive(Debug, Clone, Copy)]
pub struct NoDelay {
    len: usize,
}

impl NoDelay {
    pub fn new(len: usize) -> NoDelay {
        NoDelay { len }
    }
}

impl Vdf for NoDelay {
    fn len(&self) -> usize {
        self.len
    }

    fn cost_at(&self, _index: usize, _flow: f64) -> f64 {
        0.0
    }

    fn derivative_at(&self, _index: usize, _flow: f64) -> f64 {
        0.0
    }

    fn integral(&self, _flows: &[f64]) -> f64 {
        0.0
    }
}

#[derive(Debug, PartialEq)]
pub enum VdfError {
    MismatchedLengths,
    /// A parameter is out of its valid range; `index` is the element, `name` the parameter.
    InvalidParameter {
        index: usize,
        name: &'static str,
    },
}

/// BPR: `c(v) = t0 * (1 + b * (v / capacity)^power)`.
///
/// Valid parameters: `free_flow_time >= 0`, `capacity > 0`, `b >= 0`, `power >= 1`,
/// all finite.
#[derive(Debug, Clone)]
pub struct Bpr {
    params: Vec<BprParams>,
}

/// The four parameters of one element, side by side so evaluating it touches one cache
/// line, and with `1 / capacity` precomputed so evaluation needs no division.
#[derive(Debug, Clone, Copy)]
struct BprParams {
    free_flow_time: f64,
    inv_capacity: f64,
    b: f64,
    power: f64,
}

impl Bpr {
    pub fn new(
        free_flow_time: Vec<f64>,
        capacity: Vec<f64>,
        b: Vec<f64>,
        power: Vec<f64>,
    ) -> Result<Bpr, VdfError> {
        let n = free_flow_time.len();
        if capacity.len() != n || b.len() != n || power.len() != n {
            return Err(VdfError::MismatchedLengths);
        }
        let mut params = Vec::with_capacity(n);
        for i in 0..n {
            let checks = [
                (
                    "free_flow_time",
                    free_flow_time[i].is_finite() && free_flow_time[i] >= 0.0,
                ),
                ("capacity", capacity[i].is_finite() && capacity[i] > 0.0),
                ("b", b[i].is_finite() && b[i] >= 0.0),
                ("power", power[i].is_finite() && power[i] >= 1.0),
            ];
            if let Some(&(name, _)) = checks.iter().find(|(_, ok)| !ok) {
                return Err(VdfError::InvalidParameter { index: i, name });
            }
            params.push(BprParams {
                free_flow_time: free_flow_time[i],
                inv_capacity: 1.0 / capacity[i],
                b: b[i],
                power: power[i],
            });
        }
        Ok(Bpr { params })
    }
}

/// A delay function whose elements already carry some fixed volume that is not part of the
/// assignment: `c_i(flow) = inner.c_i(flow + preload_i)`. Use it to load the network with
/// fixed traffic, such as buses or through traffic from another model, by passing it to a
/// solver in place of `inner`.
///
/// The objective counts only the part above the preload: `Σ ∫_{preload}^{preload+flow} c_i`.
#[derive(Debug, Clone, Copy)]
pub struct Preloaded<'a, V> {
    inner: &'a V,
    preload: &'a [f64],
}

impl<'a, V: Vdf> Preloaded<'a, V> {
    /// `preload` has one non-negative entry per element of `inner`.
    pub fn new(inner: &'a V, preload: &'a [f64]) -> Preloaded<'a, V> {
        assert_eq!(inner.len(), preload.len(), "one preload per element");
        debug_assert!(preload.iter().all(|&p| p.is_finite() && p >= 0.0));
        Preloaded { inner, preload }
    }
}

impl<V: Vdf> Vdf for Preloaded<'_, V> {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn cost_at(&self, i: usize, flow: f64) -> f64 {
        self.inner.cost_at(i, flow + self.preload[i])
    }

    fn derivative_at(&self, i: usize, flow: f64) -> f64 {
        self.inner.derivative_at(i, flow + self.preload[i])
    }

    fn cost_and_derivative_at(&self, i: usize, flow: f64) -> (f64, f64) {
        self.inner.cost_and_derivative_at(i, flow + self.preload[i])
    }

    fn integral(&self, flows: &[f64]) -> f64 {
        let total: Vec<f64> = flows.iter().zip(self.preload).map(|(f, p)| f + p).collect();
        self.inner.integral(&total) - self.inner.integral(self.preload)
    }
}

/// `x^p`, using repeated multiplication for small whole powers (BPR almost always
/// has power 4, and `powf` is several times slower).
fn pow(x: f64, p: f64) -> f64 {
    if p == p.trunc() && p <= 8.0 {
        x.powi(p as i32)
    } else {
        x.powf(p)
    }
}

impl Vdf for Bpr {
    fn len(&self) -> usize {
        self.params.len()
    }

    fn cost_at(&self, i: usize, flow: f64) -> f64 {
        let q = &self.params[i];
        let x = flow * q.inv_capacity;
        q.free_flow_time * (1.0 + q.b * pow(x, q.power))
    }

    fn derivative_at(&self, i: usize, flow: f64) -> f64 {
        let q = &self.params[i];
        let x = flow * q.inv_capacity;
        // d/dv t0 b (v/c)^p = t0 b p (v/c)^(p-1) / c; p >= 1 so the power is never negative.
        q.free_flow_time * q.b * q.power * pow(x, q.power - 1.0) * q.inv_capacity
    }

    fn cost_and_derivative_at(&self, i: usize, flow: f64) -> (f64, f64) {
        let q = &self.params[i];
        let x = flow * q.inv_capacity;
        let x_pm1 = pow(x, q.power - 1.0); // (v/c)^(p-1), shared by both
        let cost = q.free_flow_time * (1.0 + q.b * (x_pm1 * x));
        let derivative = q.free_flow_time * q.b * q.power * x_pm1 * q.inv_capacity;
        (cost, derivative)
    }

    fn integral(&self, flows: &[f64]) -> f64 {
        debug_assert_eq!(flows.len(), self.len());
        let mut total = 0.0;
        for (i, &v) in flows.iter().enumerate() {
            let q = &self.params[i];
            let x = v * q.inv_capacity;
            // ∫ t0 (1 + b (u/c)^p) du from 0 to v = t0 (v + b v (v/c)^p / (p + 1))
            total += q.free_flow_time * (v + q.b * v * pow(x, q.power) / (q.power + 1.0));
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(t0: f64, cap: f64, b: f64, p: f64) -> Bpr {
        Bpr::new(vec![t0], vec![cap], vec![b], vec![p]).unwrap()
    }

    fn cost_at(f: &Bpr, v: f64) -> f64 {
        let mut out = [0.0];
        f.cost(&[v], &mut out);
        out[0]
    }

    fn derivative_at(f: &Bpr, v: f64) -> f64 {
        let mut out = [0.0];
        f.derivative(&[v], &mut out);
        out[0]
    }

    #[test]
    fn known_values() {
        let f = single(6.0, 100.0, 0.15, 4.0);
        assert_eq!(cost_at(&f, 0.0), 6.0);
        assert!((cost_at(&f, 100.0) - 6.9).abs() < 1e-12); // 6 * 1.15
        assert!((cost_at(&f, 50.0) - 6.05625).abs() < 1e-12); // 6 * (1 + 0.15 / 16)
        assert_eq!(derivative_at(&f, 0.0), 0.0);
        assert!((derivative_at(&f, 100.0) - 0.036).abs() < 1e-12); // 6 * 0.15 * 4 / 100
    }

    #[test]
    fn linear_power_has_nonzero_slope_at_zero() {
        let f = single(2.0, 50.0, 0.5, 1.0);
        assert!((derivative_at(&f, 0.0) - 2.0 * 0.5 / 50.0).abs() < 1e-15);
    }

    #[test]
    fn derivative_and_integral_agree_with_finite_differences() {
        for (t0, cap, b, p) in [
            (6.0, 100.0, 0.15, 4.0),
            (3.0, 20.0, 0.8, 2.5),
            (1.0, 7.0, 1.0, 1.0),
        ] {
            let f = single(t0, cap, b, p);
            for v in [0.0, 1.0, 13.7, 100.0, 250.0] {
                let h = 1e-5 * (1.0 + v);
                // Central differences, one-sided at v = 0 would need v - h < 0.
                if v > 0.0 {
                    let slope = (cost_at(&f, v + h) - cost_at(&f, v - h)) / (2.0 * h);
                    let tolerance = 1e-6 * (1.0 + slope.abs());
                    assert!(
                        (derivative_at(&f, v) - slope).abs() < tolerance,
                        "derivative at {v}"
                    );
                    let d_integral = (f.integral(&[v + h]) - f.integral(&[v - h])) / (2.0 * h);
                    assert!(
                        (cost_at(&f, v) - d_integral).abs() < 1e-6 * cost_at(&f, v),
                        "integral at {v}"
                    );
                }
            }
            assert_eq!(f.integral(&[0.0]), 0.0);
        }
    }

    #[test]
    fn works_on_whole_arrays() {
        let f = Bpr::new(
            vec![6.0, 4.0],
            vec![100.0, 200.0],
            vec![0.15, 0.15],
            vec![4.0, 4.0],
        )
        .unwrap();
        let mut out = [0.0; 2];
        f.cost(&[100.0, 0.0], &mut out);
        assert!((out[0] - 6.9).abs() < 1e-12);
        assert_eq!(out[1], 4.0);
        assert_eq!(f.len(), 2);
        let sum = f.integral(&[100.0, 0.0]);
        assert!((sum - 6.0 * (100.0 + 0.15 * 100.0 / 5.0)).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_parameters() {
        let bad = |t0, cap, b, p| Bpr::new(vec![t0], vec![cap], vec![b], vec![p]).unwrap_err();
        assert_eq!(
            bad(-1.0, 1.0, 0.15, 4.0),
            VdfError::InvalidParameter {
                index: 0,
                name: "free_flow_time"
            }
        );
        assert_eq!(
            bad(1.0, 0.0, 0.15, 4.0),
            VdfError::InvalidParameter {
                index: 0,
                name: "capacity"
            }
        );
        assert_eq!(
            bad(1.0, 1.0, -0.1, 4.0),
            VdfError::InvalidParameter {
                index: 0,
                name: "b"
            }
        );
        assert_eq!(
            bad(1.0, 1.0, 0.15, 0.5),
            VdfError::InvalidParameter {
                index: 0,
                name: "power"
            }
        );
        assert_eq!(
            bad(1.0, f64::NAN, 0.15, 4.0),
            VdfError::InvalidParameter {
                index: 0,
                name: "capacity"
            }
        );
        assert_eq!(
            Bpr::new(vec![1.0, 2.0], vec![1.0], vec![0.15], vec![4.0]).unwrap_err(),
            VdfError::MismatchedLengths
        );
    }

    #[test]
    fn combined_evaluation_matches_the_separate_calls() {
        for (t0, cap, b, p) in [
            (6.0, 100.0, 0.15, 4.0),
            (3.0, 20.0, 0.8, 2.5),
            (1.0, 7.0, 1.0, 1.0),
        ] {
            let f = single(t0, cap, b, p);
            for v in [0.0, 1.0, 13.7, 100.0, 250.0] {
                let (cost, derivative) = f.cost_and_derivative_at(0, v);
                assert!((cost - f.cost_at(0, v)).abs() <= 1e-12 * cost.abs().max(1.0));
                assert!(
                    (derivative - f.derivative_at(0, v)).abs() <= 1e-12 * derivative.abs().max(1.0)
                );
            }
        }
        // The default implementation (used by NoDelay) is just the two separate calls.
        assert_eq!(NoDelay::new(3).cost_and_derivative_at(1, 5.0), (0.0, 0.0));
    }

    #[test]
    fn preload_shifts_the_flow_the_function_sees() {
        let bpr = Bpr::new(
            vec![6.0, 3.0],
            vec![100.0, 20.0],
            vec![0.15, 0.8],
            vec![4.0, 2.0],
        )
        .unwrap();
        let preload = [40.0, 0.0];
        let f = Preloaded::new(&bpr, &preload);
        assert_eq!(f.len(), 2);
        assert_eq!(f.cost_at(0, 10.0), bpr.cost_at(0, 50.0));
        assert_eq!(f.derivative_at(0, 10.0), bpr.derivative_at(0, 50.0));
        assert_eq!(
            f.cost_and_derivative_at(0, 10.0),
            bpr.cost_and_derivative_at(0, 50.0)
        );
        // No preload: identical to the inner function.
        assert_eq!(f.cost_at(1, 7.0), bpr.cost_at(1, 7.0));
    }

    #[test]
    fn preloaded_integral_is_the_area_above_the_preload() {
        let bpr = Bpr::new(vec![6.0], vec![100.0], vec![0.15], vec![4.0]).unwrap();
        let preload = [40.0];
        let f = Preloaded::new(&bpr, &preload);
        // Numerical integral of cost from preload to preload + flow.
        let flow = 30.0;
        let steps = 20_000;
        let h = flow / steps as f64;
        let area: f64 = (0..steps)
            .map(|k| bpr.cost_at(0, 40.0 + (k as f64 + 0.5) * h) * h)
            .sum();
        assert!(
            (f.integral(&[flow]) - area).abs() < 1e-6,
            "{} vs {area}",
            f.integral(&[flow])
        );
        assert!(f.integral(&[0.0]).abs() < 1e-12);
    }
}
