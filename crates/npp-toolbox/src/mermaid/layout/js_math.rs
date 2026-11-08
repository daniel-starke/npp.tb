//! `Math.sin`, `Math.cos`, `Math.log` and `Math.pow` as Edge computes them: correctly rounded.
//! The C runtime rounds differently in the last bit now and then, which the force-directed
//! layouts amplify into visibly different drawings. The functions are evaluated in
//! double-double arithmetic (about 106 bits) and rounded once, without fused multiply-adds, so
//! that every target computes the same bits.

/// An unevaluated sum `hi + lo` with `|lo| <= ulp(hi) / 2`.
#[derive(Clone, Copy, Debug)]
struct Dd {
	hi: f64,
	lo: f64,
}

fn two_sum(a: f64, b: f64) -> Dd {
	let s = a + b;
	let bb = s - a;
	Dd { hi: s, lo: (a - (s - bb)) + (b - bb) }
}

fn quick_two_sum(a: f64, b: f64) -> Dd {
	let s = a + b;
	Dd { hi: s, lo: b - (s - a) }
}

/// Veltkamp's split into two halves of 26 bits.
fn split(a: f64) -> (f64, f64) {
	let t = 134_217_729.0 * a;
	let hi = t - (t - a);
	(hi, a - hi)
}

/// Dekker's exact product.
fn two_prod(a: f64, b: f64) -> Dd {
	let p = a * b;
	let (ah, al) = split(a);
	let (bh, bl) = split(b);
	Dd { hi: p, lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl }
}

impl Dd {
	fn from(value: f64) -> Dd {
		Dd { hi: value, lo: 0.0 }
	}

	fn add(self, other: Dd) -> Dd {
		let s = two_sum(self.hi, other.hi);
		let t = two_sum(self.lo, other.lo);
		let s = quick_two_sum(s.hi, s.lo + t.hi);
		quick_two_sum(s.hi, s.lo + t.lo)
	}

	fn neg(self) -> Dd {
		Dd { hi: -self.hi, lo: -self.lo }
	}

	fn sub(self, other: Dd) -> Dd {
		self.add(other.neg())
	}

	fn mul(self, other: Dd) -> Dd {
		let p = two_prod(self.hi, other.hi);
		quick_two_sum(p.hi, p.lo + (self.hi * other.lo + self.lo * other.hi))
	}

	fn mul_f64(self, other: f64) -> Dd {
		let p = two_prod(self.hi, other);
		quick_two_sum(p.hi, p.lo + self.lo * other)
	}

	fn div(self, other: Dd) -> Dd {
		let q1 = self.hi / other.hi;
		let r = self.sub(other.mul_f64(q1));
		let q2 = r.hi / other.hi;
		let r = r.sub(other.mul_f64(q2));
		let q3 = r.hi / other.hi;
		quick_two_sum(q1, q2).add(Dd::from(q3))
	}

	fn value(self) -> f64 {
		self.hi + self.lo
	}
}

/// pi / 2 and ln 2 in three parts.
const PIO2: [f64; 3] =
	[std::f64::consts::FRAC_PI_2, 6.123_233_995_736_766e-17, -1.497_384_904_859_169_8e-33];
const LN2: [f64; 3] =
	[std::f64::consts::LN_2, 2.319_046_813_846_299_6e-17, 5.707_708_438_416_212e-34];

/// `k * c` for a constant `c` in three parts.
fn times_constant(k: f64, constant: &[f64; 3]) -> Dd {
	two_prod(k, constant[0]).add(two_prod(k, constant[1])).add(Dd::from(k * constant[2]))
}

/// `sum(coefficients[n] * z^n)` by Horner's rule.
fn polynomial(coefficients: &[Dd], z: Dd) -> Dd {
	coefficients.iter().rev().fold(Dd::from(0.0), |sum, &coefficient| sum.mul(z).add(coefficient))
}

/// `(-1)^n / (2n + offset)!` for `n` below `count`.
fn alternating_inverse_factorials(offset: usize, count: usize) -> Vec<Dd> {
	let mut factorial = Dd::from(1.0);
	for n in 2..=offset {
		factorial = factorial.mul_f64(n as f64);
	}
	let one = Dd::from(1.0);
	(0..count)
		.map(|n| {
			if n > 0 {
				let k = 2 * n + offset;
				factorial = factorial.mul_f64(((k - 1) * k) as f64);
			}
			let term = one.div(factorial);
			if n % 2 == 0 { term } else { term.neg() }
		})
		.collect()
}

/// `x` reduced by the nearest multiple `n` of pi/2, for `|x| < 2^20`.
fn reduce(x: f64) -> (i64, Dd) {
	let n = (x * (2.0 / std::f64::consts::PI)).round();
	(n as i64, Dd::from(x).sub(times_constant(n, &PIO2)))
}

/// sin and cos of a reduced argument.
fn sin_dd(r: Dd) -> Dd {
	r.mul(polynomial(&alternating_inverse_factorials(1, 16), r.mul(r)))
}

fn cos_dd(r: Dd) -> Dd {
	polynomial(&alternating_inverse_factorials(0, 16), r.mul(r))
}

const REDUCIBLE: f64 = 1_048_576.0;

pub fn sin(x: f64) -> f64 {
	if !x.is_finite() {
		return f64::NAN;
	}
	if x == 0.0 || x.abs() >= REDUCIBLE {
		return if x == 0.0 { x } else { x.sin() };
	}
	let (n, r) = reduce(x);
	match n.rem_euclid(4) {
		0 => sin_dd(r).value(),
		1 => cos_dd(r).value(),
		2 => -sin_dd(r).value(),
		_ => -cos_dd(r).value(),
	}
}

pub fn cos(x: f64) -> f64 {
	if !x.is_finite() {
		return f64::NAN;
	}
	if x.abs() >= REDUCIBLE {
		return x.cos();
	}
	let (n, r) = reduce(x);
	match n.rem_euclid(4) {
		0 => cos_dd(r).value(),
		1 => -sin_dd(r).value(),
		2 => -cos_dd(r).value(),
		_ => sin_dd(r).value(),
	}
}

/// The natural logarithm of a positive finite `x` as a double-double.
fn log_dd(x: f64) -> Dd {
	let (mut mantissa, mut exponent) = (x, 0i64);
	if mantissa < f64::MIN_POSITIVE {
		mantissa *= 2f64.powi(54);
		exponent -= 54;
	}
	let bits = mantissa.to_bits();
	exponent += ((bits >> 52) & 0x7ff) as i64 - 1023;
	mantissa = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
	if mantissa > std::f64::consts::SQRT_2 {
		mantissa /= 2.0;
		exponent += 1;
	}
	// log(m) = 2 atanh(s) with s = (m - 1) / (m + 1); m - 1 is exact.
	let s = Dd::from(mantissa - 1.0).div(two_sum(mantissa, 1.0));
	let one = Dd::from(1.0);
	let coefficients: Vec<Dd> = (0..24).map(|n| one.div(Dd::from((2 * n + 1) as f64))).collect();
	let atanh = s.mul(polynomial(&coefficients, s.mul(s)));
	times_constant(exponent as f64, &LN2).add(atanh.mul_f64(2.0))
}

pub fn log(x: f64) -> f64 {
	if x.is_nan() || x < 0.0 {
		return f64::NAN;
	}
	if x == 0.0 {
		return f64::NEG_INFINITY;
	}
	if x == f64::INFINITY {
		return x;
	}
	log_dd(x).value()
}

/// `exp` of a double-double, scaled by `2^k` at the end.
fn exp_dd(t: Dd) -> f64 {
	if t.hi > 710.0 {
		return f64::INFINITY;
	}
	if t.hi < -746.0 {
		return 0.0;
	}
	let k = (t.hi / LN2[0]).round();
	let r = t.sub(times_constant(k, &LN2));
	let mut factorial = Dd::from(1.0);
	let one = Dd::from(1.0);
	let coefficients: Vec<Dd> = (0..27)
		.map(|n| {
			if n > 1 {
				factorial = factorial.mul_f64(n as f64);
			}
			one.div(factorial)
		})
		.collect();
	let value = polynomial(&coefficients, r);
	// Scales in two steps so that neither factor overflows or underflows on its own.
	let k = k as i32;
	let half = k / 2;
	value.value() * 2f64.powi(half) * 2f64.powi(k - half)
}

pub fn pow(x: f64, y: f64) -> f64 {
	if y == 0.0 {
		return 1.0;
	}
	if x.is_nan() || y.is_nan() || (y.is_infinite() && (x == 1.0 || x == -1.0)) {
		return f64::NAN;
	}
	let y_is_integer = y.fract() == 0.0;
	let y_is_odd = y_is_integer && (y / 2.0).fract() != 0.0;
	if y.is_infinite() {
		return match (x.abs() > 1.0) == (y > 0.0) {
			true => f64::INFINITY,
			false => 0.0,
		};
	}
	if x == 0.0 || x.is_infinite() {
		let magnitude = if (x == 0.0) == (y < 0.0) { f64::INFINITY } else { 0.0 };
		return if x.is_sign_negative() && y_is_odd { -magnitude } else { magnitude };
	}
	if x < 0.0 && !y_is_integer {
		return f64::NAN;
	}
	let magnitude = exp_dd(log_dd(x.abs()).mul_f64(y));
	if x < 0.0 && y_is_odd { -magnitude } else { magnitude }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn computes_like_edge() {
		// Edge's results where the C runtime or fdlibm round differently.
		assert_eq!(sin(0.0), 0.0);
		assert_eq!(cos(0.0), 1.0);
		assert_eq!(log(1.0), 0.0);
		assert!(log(-1.0).is_nan());
		assert_eq!(log(0.0), f64::NEG_INFINITY);
		assert_eq!(pow(2.0, 10.0), 1024.0);
		assert_eq!(pow(-2.0, 3.0), -8.0);
		assert!(pow(-2.0, 0.5).is_nan());
		assert!(pow(1.0, f64::INFINITY).is_nan());
		assert_eq!(pow(0.0, -1.0), f64::INFINITY);
		assert_eq!(pow(10.0, 400.0), f64::INFINITY);
		let exponent = log(96.0) / log(25.0);
		assert_eq!(exponent, 1.4179944924264754);
		assert_eq!(pow(15.0, exponent), 46.52548701922506);
		assert_eq!(pow(17.0, exponent), 55.56096674434149);
		assert_eq!(pow(22.0, exponent), 80.08440770139889);
	}

	/// `JS_MATH_SAMPLES=<file> cargo test -p npp-toolbox js_math_samples -- --ignored` compares
	/// with the JSON array of `[x, sin, cos, y, log, pow(y, e), e]` written by Edge.
	#[test]
	#[ignore]
	fn js_math_samples() {
		use super::super::super::value::{Value, parse_json};
		let path = std::env::var("JS_MATH_SAMPLES").expect("JS_MATH_SAMPLES");
		let text = std::fs::read_to_string(path).expect("sample file");
		let Some(Value::Array(samples)) = parse_json(&text) else { panic!("no array") };
		let mut failures = [0; 4];
		for sample in &samples {
			let Value::Array(v) = sample else { continue };
			let n: Vec<f64> = v.iter().filter_map(Value::as_number).collect();
			failures[0] += usize::from(sin(n[0]) != n[1]);
			failures[1] += usize::from(cos(n[0]) != n[2]);
			failures[2] += usize::from(log(n[3]) != n[4]);
			failures[3] += usize::from(pow(n[3], n[6]) != n[5]);
		}
		// Edge's `pow` is the C runtime's, which misrounds about one result in 3000.
		assert_eq!(failures[..3], [0; 3], "of {}", samples.len());
		assert!(failures[3] * 1000 < samples.len(), "pow: {} of {}", failures[3], samples.len());
	}
}
