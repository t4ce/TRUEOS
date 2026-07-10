//! A calculator-oriented API for Rust's primitive numeric types.
//!
//! The root contains operations that work through the standard operator traits.
//! Scientific floating-point operations live in [`f32`] and [`f64`], while
//! integer number theory, statistics, algebra, geometry, and common financial
//! formulas have their own namespaces.

use core::iter::{Product, Sum};
use core::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Rem, Shl, Shr, Sub};

// Keep the existing implementations as the canonical public entry points.
pub use crate::{
    Complex, Matrix, Vector, acos_f32, asin_f32, atan2_f64, cos_f32, cos_f64, exp_f32, exp_f64,
    hypot_f32, hypot_f64, log_f32, log_f64, log2_f32, log2_f64, log10_f32, log10_f64, pow_f32,
    pow_f64, sin_f32, sin_f64, tanh_f32, tanh_f64,
};

#[inline]
pub fn add<T: Add<Output = T>>(lhs: T, rhs: T) -> T {
    lhs + rhs
}

#[inline]
pub fn subtract<T: Sub<Output = T>>(lhs: T, rhs: T) -> T {
    lhs - rhs
}

#[inline]
pub fn multiply<T: Mul<Output = T>>(lhs: T, rhs: T) -> T {
    lhs * rhs
}

#[inline]
pub fn divide<T: Div<Output = T>>(lhs: T, rhs: T) -> T {
    lhs / rhs
}

#[inline]
pub fn remainder<T: Rem<Output = T>>(lhs: T, rhs: T) -> T {
    lhs % rhs
}

#[inline]
pub fn negate<T: Neg<Output = T>>(value: T) -> T {
    -value
}

#[inline]
pub fn square<T: Copy + Mul<Output = T>>(value: T) -> T {
    value * value
}

#[inline]
pub fn cube<T: Copy + Mul<Output = T>>(value: T) -> T {
    value * value * value
}

#[inline]
pub fn minimum<T: PartialOrd>(lhs: T, rhs: T) -> T {
    if lhs <= rhs { lhs } else { rhs }
}

#[inline]
pub fn maximum<T: PartialOrd>(lhs: T, rhs: T) -> T {
    if lhs >= rhs { lhs } else { rhs }
}

#[inline]
pub fn clamp<T: PartialOrd>(value: T, minimum: T, maximum: T) -> T {
    if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

#[inline]
pub fn sum<T>(values: &[T]) -> T
where
    T: Copy + Sum<T>,
{
    values.iter().copied().sum()
}

#[inline]
pub fn product<T>(values: &[T]) -> T
where
    T: Copy + Product<T>,
{
    values.iter().copied().product()
}

#[inline]
pub fn bit_and<T: BitAnd<Output = T>>(lhs: T, rhs: T) -> T {
    lhs & rhs
}

#[inline]
pub fn bit_or<T: BitOr<Output = T>>(lhs: T, rhs: T) -> T {
    lhs | rhs
}

#[inline]
pub fn bit_xor<T: BitXor<Output = T>>(lhs: T, rhs: T) -> T {
    lhs ^ rhs
}

#[inline]
pub fn bit_not<T: Not<Output = T>>(value: T) -> T {
    !value
}

#[inline]
pub fn shift_left<T, U>(value: T, places: U) -> T
where
    T: Shl<U, Output = T>,
{
    value << places
}

#[inline]
pub fn shift_right<T, U>(value: T, places: U) -> T
where
    T: Shr<U, Output = T>,
{
    value >> places
}

macro_rules! float_calculator_api {
    (
        $ty:ty,
        abs = $abs:ident,
        floor = $floor:ident,
        ceil = $ceil:ident,
        round = $round:ident,
        trunc = $trunc:ident,
        min = $min:ident,
        max = $max:ident,
        fma = $fma:ident,
        sqrt = $sqrt:ident,
        cbrt = $cbrt:ident,
        exp2 = $exp2:ident,
        exp10 = $exp10:ident,
        expm1 = $expm1:ident,
        log1p = $log1p:ident,
        tan = $tan:ident,
        atan = $atan:ident,
        sinh = $sinh:ident,
        cosh = $cosh:ident,
        asinh = $asinh:ident,
        acosh = $acosh:ident,
        atanh = $atanh:ident,
        erf = $erf:ident,
        erfc = $erfc:ident,
        gamma = $gamma:ident,
        lgamma = $lgamma:ident,
        fmod = $fmod:ident,
        copy_sign = $copy_sign:ident
    ) => {
        #[inline]
        pub fn abs(value: $ty) -> $ty {
            libm::$abs(value)
        }

        #[inline]
        pub fn signum(value: $ty) -> $ty {
            if value.is_nan() {
                value
            } else if value == 0.0 {
                value
            } else if value.is_sign_negative() {
                -1.0
            } else {
                1.0
            }
        }

        #[inline]
        pub fn floor(value: $ty) -> $ty {
            libm::$floor(value)
        }

        #[inline]
        pub fn ceil(value: $ty) -> $ty {
            libm::$ceil(value)
        }

        #[inline]
        pub fn round(value: $ty) -> $ty {
            libm::$round(value)
        }

        #[inline]
        pub fn trunc(value: $ty) -> $ty {
            libm::$trunc(value)
        }

        #[inline]
        pub fn fract(value: $ty) -> $ty {
            value - trunc(value)
        }

        #[inline]
        pub fn min(lhs: $ty, rhs: $ty) -> $ty {
            libm::$min(lhs, rhs)
        }

        #[inline]
        pub fn max(lhs: $ty, rhs: $ty) -> $ty {
            libm::$max(lhs, rhs)
        }

        #[inline]
        pub fn clamp(value: $ty, minimum: $ty, maximum: $ty) -> $ty {
            max(minimum, min(value, maximum))
        }

        #[inline]
        pub fn mul_add(value: $ty, multiplier: $ty, addend: $ty) -> $ty {
            libm::$fma(value, multiplier, addend)
        }

        #[inline]
        pub fn sqrt(value: $ty) -> $ty {
            libm::$sqrt(value)
        }

        #[inline]
        pub fn cbrt(value: $ty) -> $ty {
            libm::$cbrt(value)
        }

        pub fn powi(mut base: $ty, exponent: i32) -> $ty {
            if exponent == 0 {
                return 1.0;
            }

            let invert = exponent < 0;
            let mut power = exponent.unsigned_abs();
            let mut result = 1.0;
            while power != 0 {
                if power & 1 == 1 {
                    result *= base;
                }
                base *= base;
                power >>= 1;
            }

            if invert { 1.0 / result } else { result }
        }

        #[inline]
        pub fn nth_root(value: $ty, degree: u32) -> $ty {
            if degree == 0 {
                return <$ty>::NAN;
            }
            if value < 0.0 && degree & 1 == 1 {
                -pow(-value, 1.0 / degree as $ty)
            } else {
                pow(value, 1.0 / degree as $ty)
            }
        }

        #[inline]
        pub fn exp2(value: $ty) -> $ty {
            libm::$exp2(value)
        }

        #[inline]
        pub fn exp10(value: $ty) -> $ty {
            libm::$exp10(value)
        }

        #[inline]
        pub fn exp_m1(value: $ty) -> $ty {
            libm::$expm1(value)
        }

        #[inline]
        pub fn ln_1p(value: $ty) -> $ty {
            libm::$log1p(value)
        }

        #[inline]
        pub fn log(value: $ty, base: $ty) -> $ty {
            ln(value) / ln(base)
        }

        #[inline]
        pub fn tan(value: $ty) -> $ty {
            libm::$tan(value)
        }

        #[inline]
        pub fn atan(value: $ty) -> $ty {
            libm::$atan(value)
        }

        #[inline]
        pub fn sinh(value: $ty) -> $ty {
            libm::$sinh(value)
        }

        #[inline]
        pub fn cosh(value: $ty) -> $ty {
            libm::$cosh(value)
        }

        #[inline]
        pub fn asinh(value: $ty) -> $ty {
            libm::$asinh(value)
        }

        #[inline]
        pub fn acosh(value: $ty) -> $ty {
            libm::$acosh(value)
        }

        #[inline]
        pub fn atanh(value: $ty) -> $ty {
            libm::$atanh(value)
        }

        #[inline]
        pub fn sec(value: $ty) -> $ty {
            1.0 / cos(value)
        }

        #[inline]
        pub fn csc(value: $ty) -> $ty {
            1.0 / sin(value)
        }

        #[inline]
        pub fn cot(value: $ty) -> $ty {
            1.0 / tan(value)
        }

        #[inline]
        pub fn sinc(value: $ty) -> $ty {
            if value == 0.0 {
                1.0
            } else {
                sin(value) / value
            }
        }

        #[inline]
        pub fn hypot3(x: $ty, y: $ty, z: $ty) -> $ty {
            hypot(hypot(x, y), z)
        }

        #[inline]
        pub fn to_radians(degrees: $ty) -> $ty {
            degrees * PI / 180.0
        }

        #[inline]
        pub fn to_degrees(radians: $ty) -> $ty {
            radians * 180.0 / PI
        }

        #[inline]
        pub fn normalize_radians(angle: $ty) -> $ty {
            let full_turn = 2.0 * PI;
            let wrapped = libm::$fmod(angle, full_turn);
            if wrapped < 0.0 {
                wrapped + full_turn
            } else {
                wrapped
            }
        }

        #[inline]
        pub fn normalize_degrees(angle: $ty) -> $ty {
            let wrapped = libm::$fmod(angle, 360.0);
            if wrapped < 0.0 {
                wrapped + 360.0
            } else {
                wrapped
            }
        }

        #[inline]
        pub fn erf(value: $ty) -> $ty {
            libm::$erf(value)
        }

        #[inline]
        pub fn erfc(value: $ty) -> $ty {
            libm::$erfc(value)
        }

        #[inline]
        pub fn gamma(value: $ty) -> $ty {
            libm::$gamma(value)
        }

        #[inline]
        pub fn ln_gamma(value: $ty) -> $ty {
            libm::$lgamma(value)
        }

        #[inline]
        pub fn modulo(value: $ty, modulus: $ty) -> $ty {
            libm::$fmod(value, modulus)
        }

        #[inline]
        pub fn rem_euclid(value: $ty, modulus: $ty) -> $ty {
            let result = libm::$fmod(value, modulus);
            if result < 0.0 {
                result + abs(modulus)
            } else {
                result
            }
        }

        #[inline]
        pub fn copy_sign(magnitude: $ty, sign: $ty) -> $ty {
            libm::$copy_sign(magnitude, sign)
        }

        #[inline]
        pub fn reciprocal(value: $ty) -> $ty {
            1.0 / value
        }

        #[inline]
        pub fn percent(value: $ty, percentage: $ty) -> $ty {
            value * percentage / 100.0
        }

        #[inline]
        pub fn percentage(value: $ty, total: $ty) -> $ty {
            value * 100.0 / total
        }

        #[inline]
        pub fn percentage_change(from: $ty, to: $ty) -> $ty {
            (to - from) * 100.0 / abs(from)
        }

        #[inline]
        pub fn lerp(start: $ty, end: $ty, amount: $ty) -> $ty {
            start + (end - start) * amount
        }

        #[inline]
        pub fn inverse_lerp(start: $ty, end: $ty, value: $ty) -> $ty {
            (value - start) / (end - start)
        }

        #[inline]
        pub fn map_range(
            value: $ty,
            input_start: $ty,
            input_end: $ty,
            output_start: $ty,
            output_end: $ty,
        ) -> $ty {
            lerp(output_start, output_end, inverse_lerp(input_start, input_end, value))
        }

        #[inline]
        pub fn sigmoid(value: $ty) -> $ty {
            1.0 / (1.0 + exp(-value))
        }

        #[inline]
        pub fn round_to_places(value: $ty, decimal_places: i32) -> $ty {
            let scale = powi(10.0, decimal_places);
            round(value * scale) / scale
        }

        #[inline]
        pub fn approx_eq(lhs: $ty, rhs: $ty, tolerance: $ty) -> bool {
            abs(lhs - rhs) <= tolerance
        }
    };
}

/// Scientific calculator functions operating on `f32`.
pub mod f32 {
    pub use crate::{
        acos_f32 as acos, asin_f32 as asin, cos_f32 as cos, exp_f32 as exp, hypot_f32 as hypot,
        log_f32 as ln, log2_f32 as log2, log10_f32 as log10, pow_f32 as pow, sin_f32 as sin,
        tanh_f32 as tanh,
    };

    pub use core::f32::consts::{
        E, FRAC_1_PI, FRAC_2_PI, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, FRAC_PI_8, LN_2,
        LN_10, LOG2_E, LOG10_E, PI, SQRT_2, TAU,
    };

    float_calculator_api!(
        f32,
        abs = fabsf,
        floor = floorf,
        ceil = ceilf,
        round = roundf,
        trunc = truncf,
        min = fminf,
        max = fmaxf,
        fma = fmaf,
        sqrt = sqrtf,
        cbrt = cbrtf,
        exp2 = exp2f,
        exp10 = exp10f,
        expm1 = expm1f,
        log1p = log1pf,
        tan = tanf,
        atan = atanf,
        sinh = sinhf,
        cosh = coshf,
        asinh = asinhf,
        acosh = acoshf,
        atanh = atanhf,
        erf = erff,
        erfc = erfcf,
        gamma = tgammaf,
        lgamma = lgammaf,
        fmod = fmodf,
        copy_sign = copysignf
    );

    #[inline]
    pub fn atan2(y: f32, x: f32) -> f32 {
        libm::atan2f(y, x)
    }
}

/// Scientific calculator functions operating on `f64`.
pub mod f64 {
    pub use crate::{
        atan2_f64 as atan2, cos_f64 as cos, exp_f64 as exp, hypot_f64 as hypot, log_f64 as ln,
        log2_f64 as log2, log10_f64 as log10, pow_f64 as pow, sin_f64 as sin, tanh_f64 as tanh,
    };

    pub use core::f64::consts::{
        E, FRAC_1_PI, FRAC_2_PI, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, FRAC_PI_8, LN_2,
        LN_10, LOG2_E, LOG10_E, PI, SQRT_2, TAU,
    };

    float_calculator_api!(
        f64,
        abs = fabs,
        floor = floor,
        ceil = ceil,
        round = round,
        trunc = trunc,
        min = fmin,
        max = fmax,
        fma = fma,
        sqrt = sqrt,
        cbrt = cbrt,
        exp2 = exp2,
        exp10 = exp10,
        expm1 = expm1,
        log1p = log1p,
        tan = tan,
        atan = atan,
        sinh = sinh,
        cosh = cosh,
        asinh = asinh,
        acosh = acosh,
        atanh = atanh,
        erf = erf,
        erfc = erfc,
        gamma = tgamma,
        lgamma = lgamma,
        fmod = fmod,
        copy_sign = copysign
    );

    #[inline]
    pub fn asin(value: f64) -> f64 {
        libm::asin(value)
    }

    #[inline]
    pub fn acos(value: f64) -> f64 {
        libm::acos(value)
    }
}

/// Integer and number-theory operations using the widest primitive integers.
pub mod integer {
    #[inline]
    pub const fn abs(value: i128) -> Option<i128> {
        value.checked_abs()
    }

    #[inline]
    pub const fn signum(value: i128) -> i128 {
        if value < 0 {
            -1
        } else if value > 0 {
            1
        } else {
            0
        }
    }

    pub const fn gcd(mut lhs: u128, mut rhs: u128) -> u128 {
        while rhs != 0 {
            let remainder = lhs % rhs;
            lhs = rhs;
            rhs = remainder;
        }
        lhs
    }

    pub const fn lcm(lhs: u128, rhs: u128) -> Option<u128> {
        if lhs == 0 || rhs == 0 {
            Some(0)
        } else {
            (lhs / gcd(lhs, rhs)).checked_mul(rhs)
        }
    }

    pub const fn factorial(value: u32) -> Option<u128> {
        let mut result = 1u128;
        let mut factor = 2u32;
        while factor <= value {
            match result.checked_mul(factor as u128) {
                Some(next) => result = next,
                None => return None,
            }
            factor += 1;
        }
        Some(result)
    }

    pub const fn permutations(n: u32, r: u32) -> Option<u128> {
        if r > n {
            return None;
        }
        let mut result = 1u128;
        let mut index = 0u32;
        while index < r {
            match result.checked_mul((n - index) as u128) {
                Some(next) => result = next,
                None => return None,
            }
            index += 1;
        }
        Some(result)
    }

    pub const fn combinations(n: u32, r: u32) -> Option<u128> {
        if r > n {
            return None;
        }
        let r = if r < n - r { r } else { n - r };
        let mut result = 1u128;
        let mut index = 1u32;
        while index <= r {
            let numerator = (n - r + index) as u128;
            let divisor = index as u128;
            let common = gcd(result, divisor);
            match (result / common).checked_mul(numerator / (divisor / common)) {
                Some(next) => result = next,
                None => return None,
            }
            index += 1;
        }
        Some(result)
    }

    pub const fn fibonacci(index: u32) -> Option<u128> {
        if index == 0 {
            return Some(0);
        }
        let mut previous = 0u128;
        let mut current = 1u128;
        let mut position = 1;
        while position < index {
            let next = match previous.checked_add(current) {
                Some(next) => next,
                None => return None,
            };
            previous = current;
            current = next;
            position += 1;
        }
        Some(current)
    }

    pub const fn is_prime(value: u128) -> bool {
        if value < 2 {
            return false;
        }
        if value.is_multiple_of(2) {
            return value == 2;
        }
        let mut divisor = 3u128;
        while divisor <= value / divisor {
            if value.is_multiple_of(divisor) {
                return false;
            }
            divisor += 2;
        }
        true
    }

    pub const fn next_prime(value: u128) -> Option<u128> {
        let mut candidate = match value.checked_add(1) {
            Some(candidate) => candidate,
            None => return None,
        };
        if candidate <= 2 {
            return Some(2);
        }
        if candidate % 2 == 0 {
            candidate += 1;
        }
        loop {
            if is_prime(candidate) {
                return Some(candidate);
            }
            candidate = match candidate.checked_add(2) {
                Some(candidate) => candidate,
                None => return None,
            };
        }
    }

    const fn add_mod(lhs: u128, rhs: u128, modulus: u128) -> u128 {
        if lhs >= modulus - rhs {
            lhs - (modulus - rhs)
        } else {
            lhs + rhs
        }
    }

    pub const fn modular_multiply(mut lhs: u128, mut rhs: u128, modulus: u128) -> Option<u128> {
        if modulus == 0 {
            return None;
        }
        lhs %= modulus;
        rhs %= modulus;
        let mut result = 0u128;
        while rhs != 0 {
            if rhs & 1 == 1 {
                result = add_mod(result, lhs, modulus);
            }
            rhs >>= 1;
            if rhs != 0 {
                lhs = add_mod(lhs, lhs, modulus);
            }
        }
        Some(result)
    }

    pub const fn modular_power(mut base: u128, mut exponent: u128, modulus: u128) -> Option<u128> {
        if modulus == 0 {
            return None;
        }
        base %= modulus;
        let mut result = 1 % modulus;
        while exponent != 0 {
            if exponent & 1 == 1 {
                result = match modular_multiply(result, base, modulus) {
                    Some(value) => value,
                    None => return None,
                };
            }
            exponent >>= 1;
            if exponent != 0 {
                base = match modular_multiply(base, base, modulus) {
                    Some(value) => value,
                    None => return None,
                };
            }
        }
        Some(result)
    }

    pub const fn integer_sqrt(value: u128) -> u128 {
        if value < 2 {
            return value;
        }
        let mut low = 1u128;
        let mut high = if value < (1u128 << 64) {
            value + 1
        } else {
            1u128 << 64
        };
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            if middle <= value / middle {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }

    pub const fn digit_count(mut value: u128, base: u32) -> Option<u32> {
        if base < 2 {
            return None;
        }
        let mut digits = 1u32;
        while value >= base as u128 {
            value /= base as u128;
            digits += 1;
        }
        Some(digits)
    }

    pub const fn digit_sum(mut value: u128, base: u32) -> Option<u128> {
        if base < 2 {
            return None;
        }
        let mut sum = 0u128;
        while value != 0 {
            sum += value % base as u128;
            value /= base as u128;
        }
        Some(sum)
    }

    pub const fn reverse_digits(mut value: u128, base: u32) -> Option<u128> {
        if base < 2 {
            return None;
        }
        let mut reversed = 0u128;
        while value != 0 {
            reversed = match reversed.checked_mul(base as u128) {
                Some(next) => next,
                None => return None,
            };
            reversed = match reversed.checked_add(value % base as u128) {
                Some(next) => next,
                None => return None,
            };
            value /= base as u128;
        }
        Some(reversed)
    }

    #[inline]
    pub const fn is_palindrome(value: u128, base: u32) -> bool {
        match reverse_digits(value, base) {
            Some(reversed) => value == reversed,
            None => false,
        }
    }
}

/// Descriptive statistics and vector calculations over `f64` slices.
pub mod statistics {
    pub fn sum(values: &[f64]) -> f64 {
        // Neumaier compensated summation is accurate while remaining allocation-free.
        let mut total = 0.0f64;
        let mut correction = 0.0f64;
        for &value in values {
            if !total.is_finite() || !value.is_finite() {
                total += value;
                continue;
            }
            let next = total + value;
            if libm::fabs(total) >= libm::fabs(value) {
                correction += (total - next) + value;
            } else {
                correction += (value - next) + total;
            }
            total = next;
        }
        if total.is_finite() {
            total + correction
        } else {
            total
        }
    }

    #[inline]
    pub fn mean(values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            None
        } else {
            Some(sum(values) / values.len() as f64)
        }
    }

    pub fn geometric_mean(values: &[f64]) -> Option<f64> {
        if values.is_empty() || values.iter().any(|&value| value <= 0.0) {
            return None;
        }
        let log_sum = values
            .iter()
            .fold(0.0, |sum, &value| sum + libm::log(value));
        Some(libm::exp(log_sum / values.len() as f64))
    }

    pub fn harmonic_mean(values: &[f64]) -> Option<f64> {
        if values.is_empty() || values.contains(&0.0) {
            return None;
        }
        let reciprocal_sum = values.iter().fold(0.0, |sum, &value| sum + 1.0 / value);
        Some(values.len() as f64 / reciprocal_sum)
    }

    pub fn weighted_mean(values: &[f64], weights: &[f64]) -> Option<f64> {
        if values.is_empty() || values.len() != weights.len() {
            return None;
        }
        let total_weight = sum(weights);
        if total_weight == 0.0 {
            return None;
        }
        Some(
            values
                .iter()
                .zip(weights)
                .fold(0.0, |total, (&value, &weight)| total + value * weight)
                / total_weight,
        )
    }

    pub fn variance_population(values: &[f64]) -> Option<f64> {
        let mean = mean(values)?;
        Some(
            values
                .iter()
                .fold(0.0, |total, &value| total + (value - mean) * (value - mean))
                / values.len() as f64,
        )
    }

    pub fn variance_sample(values: &[f64]) -> Option<f64> {
        if values.len() < 2 {
            return None;
        }
        let mean = mean(values)?;
        Some(
            values
                .iter()
                .fold(0.0, |total, &value| total + (value - mean) * (value - mean))
                / (values.len() - 1) as f64,
        )
    }

    #[inline]
    pub fn standard_deviation_population(values: &[f64]) -> Option<f64> {
        variance_population(values).map(libm::sqrt)
    }

    #[inline]
    pub fn standard_deviation_sample(values: &[f64]) -> Option<f64> {
        variance_sample(values).map(libm::sqrt)
    }

    pub fn root_mean_square(values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        let squares = values
            .iter()
            .fold(0.0, |total, &value| total + value * value);
        Some(libm::sqrt(squares / values.len() as f64))
    }

    pub fn min(values: &[f64]) -> Option<f64> {
        values.iter().copied().reduce(libm::fmin)
    }

    pub fn max(values: &[f64]) -> Option<f64> {
        values.iter().copied().reduce(libm::fmax)
    }

    #[inline]
    pub fn range(values: &[f64]) -> Option<f64> {
        Some(max(values)? - min(values)?)
    }

    pub fn median_sorted(values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        let middle = values.len() / 2;
        if values.len() & 1 == 1 {
            Some(values[middle])
        } else {
            Some((values[middle - 1] + values[middle]) / 2.0)
        }
    }

    pub fn median(values: &mut [f64]) -> Option<f64> {
        // Insertion sort keeps this usable in no_std builds without allocation.
        for index in 1..values.len() {
            let mut position = index;
            while position > 0 && values[position].total_cmp(&values[position - 1]).is_lt() {
                values.swap(position, position - 1);
                position -= 1;
            }
        }
        median_sorted(values)
    }

    pub fn quantile_sorted(values: &[f64], quantile: f64) -> Option<f64> {
        if values.is_empty() || !(0.0..=1.0).contains(&quantile) {
            return None;
        }
        let position = quantile * (values.len() - 1) as f64;
        let lower = libm::floor(position) as usize;
        let upper = libm::ceil(position) as usize;
        if lower == upper {
            Some(values[lower])
        } else {
            Some(values[lower] + (values[upper] - values[lower]) * (position - lower as f64))
        }
    }

    pub fn dot_product(lhs: &[f64], rhs: &[f64]) -> Option<f64> {
        if lhs.len() != rhs.len() {
            return None;
        }
        Some(
            lhs.iter()
                .zip(rhs)
                .fold(0.0, |total, (&x, &y)| total + x * y),
        )
    }

    #[inline]
    pub fn magnitude(values: &[f64]) -> f64 {
        libm::sqrt(
            values
                .iter()
                .fold(0.0, |total, &value| total + value * value),
        )
    }

    pub fn euclidean_distance(lhs: &[f64], rhs: &[f64]) -> Option<f64> {
        if lhs.len() != rhs.len() {
            return None;
        }
        Some(libm::sqrt(lhs.iter().zip(rhs).fold(0.0, |total, (&x, &y)| {
            let difference = x - y;
            total + difference * difference
        })))
    }

    pub fn covariance_population(lhs: &[f64], rhs: &[f64]) -> Option<f64> {
        if lhs.is_empty() || lhs.len() != rhs.len() {
            return None;
        }
        let lhs_mean = mean(lhs)?;
        let rhs_mean = mean(rhs)?;
        Some(
            lhs.iter()
                .zip(rhs)
                .fold(0.0, |total, (&x, &y)| total + (x - lhs_mean) * (y - rhs_mean))
                / lhs.len() as f64,
        )
    }

    pub fn correlation(lhs: &[f64], rhs: &[f64]) -> Option<f64> {
        let covariance = covariance_population(lhs, rhs)?;
        let lhs_deviation = standard_deviation_population(lhs)?;
        let rhs_deviation = standard_deviation_population(rhs)?;
        let divisor = lhs_deviation * rhs_deviation;
        if divisor == 0.0 {
            None
        } else {
            Some(covariance / divisor)
        }
    }
}

/// Common algebraic formulas over `f64`.
pub mod algebra {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum QuadraticRoots {
        NoRealRoots,
        One(f64),
        Two(f64, f64),
        AllRealNumbers,
    }

    #[inline]
    pub fn evaluate_linear(slope: f64, intercept: f64, x: f64) -> f64 {
        slope * x + intercept
    }

    #[inline]
    pub fn solve_linear(coefficient: f64, constant: f64) -> Option<f64> {
        if coefficient == 0.0 {
            None
        } else {
            Some(-constant / coefficient)
        }
    }

    #[inline]
    pub fn discriminant(a: f64, b: f64, c: f64) -> f64 {
        b * b - 4.0 * a * c
    }

    #[inline]
    pub fn evaluate_quadratic(a: f64, b: f64, c: f64, x: f64) -> f64 {
        (a * x + b) * x + c
    }

    pub fn solve_quadratic(a: f64, b: f64, c: f64) -> QuadraticRoots {
        if a == 0.0 {
            return if b == 0.0 {
                if c == 0.0 {
                    QuadraticRoots::AllRealNumbers
                } else {
                    QuadraticRoots::NoRealRoots
                }
            } else {
                QuadraticRoots::One(-c / b)
            };
        }

        let discriminant = discriminant(a, b, c);
        if discriminant < 0.0 {
            QuadraticRoots::NoRealRoots
        } else if discriminant == 0.0 {
            QuadraticRoots::One(-b / (2.0 * a))
        } else {
            let root = libm::sqrt(discriminant);
            let first = (-b - root) / (2.0 * a);
            let second = (-b + root) / (2.0 * a);
            QuadraticRoots::Two(first, second)
        }
    }

    /// Evaluates coefficients ordered from the highest power to the constant.
    pub fn evaluate_polynomial(coefficients: &[f64], x: f64) -> f64 {
        coefficients
            .iter()
            .fold(0.0, |value, &coefficient| value * x + coefficient)
    }

    /// Evaluates the first derivative without allocating derivative coefficients.
    pub fn evaluate_polynomial_derivative(coefficients: &[f64], x: f64) -> f64 {
        if coefficients.len() < 2 {
            return 0.0;
        }
        let degree = coefficients.len() - 1;
        coefficients[..degree]
            .iter()
            .enumerate()
            .fold(0.0, |value, (index, &coefficient)| {
                value * x + coefficient * (degree - index) as f64
            })
    }

    #[inline]
    pub fn arithmetic_sequence_term(first: f64, difference: f64, index: u64) -> f64 {
        first + index as f64 * difference
    }

    #[inline]
    pub fn arithmetic_sequence_sum(first: f64, difference: f64, terms: u64) -> f64 {
        terms as f64 * (2.0 * first + (terms.saturating_sub(1)) as f64 * difference) / 2.0
    }

    #[inline]
    pub fn geometric_sequence_term(first: f64, ratio: f64, index: i32) -> f64 {
        first * super::f64::powi(ratio, index)
    }

    #[inline]
    pub fn geometric_sequence_sum(first: f64, ratio: f64, terms: u64) -> f64 {
        if ratio == 1.0 {
            first * terms as f64
        } else {
            first * (1.0 - super::f64::pow(ratio, terms as f64)) / (1.0 - ratio)
        }
    }
}

/// Two- and three-dimensional geometry formulas over `f64`.
pub mod geometry {
    #[inline]
    pub fn circle_area(radius: f64) -> f64 {
        super::f64::PI * radius * radius
    }

    #[inline]
    pub fn circle_circumference(radius: f64) -> f64 {
        2.0 * super::f64::PI * radius
    }

    #[inline]
    pub fn rectangle_area(width: f64, height: f64) -> f64 {
        width * height
    }

    #[inline]
    pub fn rectangle_perimeter(width: f64, height: f64) -> f64 {
        2.0 * (width + height)
    }

    #[inline]
    pub fn triangle_area(base: f64, height: f64) -> f64 {
        base * height / 2.0
    }

    pub fn triangle_area_sides(a: f64, b: f64, c: f64) -> Option<f64> {
        if a <= 0.0 || b <= 0.0 || c <= 0.0 || a + b <= c || a + c <= b || b + c <= a {
            return None;
        }
        let semiperimeter = (a + b + c) / 2.0;
        Some(libm::sqrt(
            semiperimeter * (semiperimeter - a) * (semiperimeter - b) * (semiperimeter - c),
        ))
    }

    #[inline]
    pub fn pythagorean_hypotenuse(a: f64, b: f64) -> f64 {
        super::f64::hypot(a, b)
    }

    #[inline]
    pub fn distance_2d(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
        super::f64::hypot(x2 - x1, y2 - y1)
    }

    #[inline]
    pub fn distance_3d(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> f64 {
        super::f64::hypot3(x2 - x1, y2 - y1, z2 - z1)
    }

    #[inline]
    pub fn midpoint_2d(x1: f64, y1: f64, x2: f64, y2: f64) -> (f64, f64) {
        ((x1 + x2) / 2.0, (y1 + y2) / 2.0)
    }

    #[inline]
    pub fn slope(x1: f64, y1: f64, x2: f64, y2: f64) -> Option<f64> {
        if x1 == x2 {
            None
        } else {
            Some((y2 - y1) / (x2 - x1))
        }
    }

    #[inline]
    pub fn sphere_surface_area(radius: f64) -> f64 {
        4.0 * super::f64::PI * radius * radius
    }

    #[inline]
    pub fn sphere_volume(radius: f64) -> f64 {
        4.0 * super::f64::PI * radius * radius * radius / 3.0
    }

    #[inline]
    pub fn cylinder_volume(radius: f64, height: f64) -> f64 {
        circle_area(radius) * height
    }

    #[inline]
    pub fn cone_volume(radius: f64, height: f64) -> f64 {
        circle_area(radius) * height / 3.0
    }
}

/// Percentage, interest, investment, and loan formulas over `f64`.
pub mod finance {
    #[inline]
    pub fn simple_interest(principal: f64, annual_rate_percent: f64, years: f64) -> f64 {
        principal * annual_rate_percent * years / 100.0
    }

    #[inline]
    pub fn compound_value(
        principal: f64,
        annual_rate_percent: f64,
        compounds_per_year: u32,
        years: f64,
    ) -> Option<f64> {
        if compounds_per_year == 0 {
            return None;
        }
        let periods = compounds_per_year as f64;
        Some(
            principal
                * super::f64::pow(1.0 + annual_rate_percent / (100.0 * periods), periods * years),
        )
    }

    #[inline]
    pub fn present_value(future_value: f64, rate_percent: f64, periods: f64) -> f64 {
        future_value / super::f64::pow(1.0 + rate_percent / 100.0, periods)
    }

    #[inline]
    pub fn future_value(present_value: f64, rate_percent: f64, periods: f64) -> f64 {
        present_value * super::f64::pow(1.0 + rate_percent / 100.0, periods)
    }

    pub fn loan_payment(
        principal: f64,
        annual_rate_percent: f64,
        payments_per_year: u32,
        years: f64,
    ) -> Option<f64> {
        if payments_per_year == 0 || years <= 0.0 {
            return None;
        }
        let payment_count = payments_per_year as f64 * years;
        let rate = annual_rate_percent / (100.0 * payments_per_year as f64);
        if rate == 0.0 {
            Some(principal / payment_count)
        } else {
            Some(principal * rate / (1.0 - super::f64::pow(1.0 + rate, -payment_count)))
        }
    }

    #[inline]
    pub fn profit(cost: f64, revenue: f64) -> f64 {
        revenue - cost
    }

    #[inline]
    pub fn profit_margin(cost: f64, revenue: f64) -> f64 {
        (revenue - cost) * 100.0 / revenue
    }

    #[inline]
    pub fn discounted_price(price: f64, discount_percent: f64) -> f64 {
        price * (1.0 - discount_percent / 100.0)
    }

    #[inline]
    pub fn price_with_tax(price: f64, tax_percent: f64) -> f64 {
        price * (1.0 + tax_percent / 100.0)
    }
}

/// Version of the calculator operation protocol shared with the V layer.
pub const CALCULATOR_PROTOCOL_VERSION: u16 = 1;

/// Largest argument list accepted by the kernel calculator protocol.
pub const CALCULATOR_MAX_ARGUMENTS: usize = 8;

/// A calculator operation that can be selected dynamically by a UI.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorOperation {
    Add = 0,
    Subtract = 1,
    Multiply = 2,
    Divide = 3,
    Modulo = 4,
    Power = 5,
    Minimum = 6,
    Maximum = 7,
    Clamp = 8,
    Reciprocal = 9,
    Square = 10,
    Cube = 11,
    SquareRoot = 12,
    CubeRoot = 13,
    NthRoot = 14,
    Absolute = 15,
    Floor = 16,
    Ceiling = 17,
    Round = 18,
    Truncate = 19,
    Fraction = 20,
    MultiplyAdd = 21,
    Sine = 22,
    Cosine = 23,
    Tangent = 24,
    ArcSine = 25,
    ArcCosine = 26,
    ArcTangent = 27,
    ArcTangent2 = 28,
    HyperbolicSine = 29,
    HyperbolicCosine = 30,
    HyperbolicTangent = 31,
    AreaHyperbolicSine = 32,
    AreaHyperbolicCosine = 33,
    AreaHyperbolicTangent = 34,
    Secant = 35,
    Cosecant = 36,
    Cotangent = 37,
    Sinc = 38,
    Hypotenuse = 39,
    Hypotenuse3 = 40,
    ToRadians = 41,
    ToDegrees = 42,
    NormalizeRadians = 43,
    NormalizeDegrees = 44,
    Exponential = 45,
    Exponential2 = 46,
    Exponential10 = 47,
    ExponentialMinus1 = 48,
    NaturalLog = 49,
    Log2 = 50,
    Log10 = 51,
    Logarithm = 52,
    NaturalLog1Plus = 53,
    ErrorFunction = 54,
    ComplementaryErrorFunction = 55,
    Gamma = 56,
    NaturalLogGamma = 57,
    Percent = 58,
    Percentage = 59,
    PercentageChange = 60,
    LinearInterpolation = 61,
    InverseLinearInterpolation = 62,
    MapRange = 63,
    Sigmoid = 64,
    RoundToPlaces = 65,
    CircleArea = 66,
    CircleCircumference = 67,
    RectangleArea = 68,
    RectanglePerimeter = 69,
    TriangleArea = 70,
    TriangleAreaFromSides = 71,
    Distance2d = 72,
    Distance3d = 73,
    SphereSurfaceArea = 74,
    SphereVolume = 75,
    CylinderVolume = 76,
    ConeVolume = 77,
    EvaluateLinear = 78,
    EvaluateQuadratic = 79,
    SimpleInterest = 80,
    CompoundValue = 81,
    PresentValue = 82,
    FutureValue = 83,
    LoanPayment = 84,
    DiscountedPrice = 85,
    PriceWithTax = 86,
    Profit = 87,
    ProfitMargin = 88,
    ArithmeticSequenceTerm = 89,
    ArithmeticSequenceSum = 90,
    GeometricSequenceTerm = 91,
    GeometricSequenceSum = 92,
}

/// Display and input metadata for a dynamically selectable operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalculatorFunctionSpec {
    pub operation: CalculatorOperation,
    pub name: &'static str,
    pub label: &'static str,
    pub arguments: &'static str,
    pub arity: u8,
    pub category: &'static str,
}

macro_rules! calculator_function_specs {
    ($(($operation:ident, $name:literal, $label:literal, $arguments:literal, $arity:literal, $category:literal)),+ $(,)?) => {
        /// Registry used by calculator UIs and other dynamic callers.
        pub const CALCULATOR_FUNCTIONS: &[CalculatorFunctionSpec] = &[
            $(CalculatorFunctionSpec {
                operation: CalculatorOperation::$operation,
                name: $name,
                label: $label,
                arguments: $arguments,
                arity: $arity,
                category: $category,
            }),+
        ];
    };
}

calculator_function_specs!(
    (Add, "add", "+", "left, right", 2, "Arithmetic"),
    (Subtract, "subtract", "-", "left, right", 2, "Arithmetic"),
    (Multiply, "multiply", "x", "left, right", 2, "Arithmetic"),
    (Divide, "divide", "/", "left, right", 2, "Arithmetic"),
    (Modulo, "modulo", "mod", "value, modulus", 2, "Arithmetic"),
    (Power, "power", "pow", "base, exponent", 2, "Arithmetic"),
    (Minimum, "minimum", "min", "left, right", 2, "Arithmetic"),
    (Maximum, "maximum", "max", "left, right", 2, "Arithmetic"),
    (Clamp, "clamp", "clamp", "value, minimum, maximum", 3, "Arithmetic"),
    (Reciprocal, "reciprocal", "1/x", "value", 1, "Arithmetic"),
    (Square, "square", "x^2", "value", 1, "Powers"),
    (Cube, "cube", "x^3", "value", 1, "Powers"),
    (SquareRoot, "square_root", "sqrt", "value", 1, "Powers"),
    (CubeRoot, "cube_root", "cbrt", "value", 1, "Powers"),
    (NthRoot, "nth_root", "root", "value, integer degree", 2, "Powers"),
    (Absolute, "absolute", "abs", "value", 1, "Rounding"),
    (Floor, "floor", "floor", "value", 1, "Rounding"),
    (Ceiling, "ceiling", "ceil", "value", 1, "Rounding"),
    (Round, "round", "round", "value", 1, "Rounding"),
    (Truncate, "truncate", "trunc", "value", 1, "Rounding"),
    (Fraction, "fraction", "fract", "value", 1, "Rounding"),
    (MultiplyAdd, "multiply_add", "mul+", "value, multiplier, addend", 3, "Arithmetic"),
    (Sine, "sine", "sin", "radians", 1, "Trigonometry"),
    (Cosine, "cosine", "cos", "radians", 1, "Trigonometry"),
    (Tangent, "tangent", "tan", "radians", 1, "Trigonometry"),
    (ArcSine, "arc_sine", "asin", "value", 1, "Trigonometry"),
    (ArcCosine, "arc_cosine", "acos", "value", 1, "Trigonometry"),
    (ArcTangent, "arc_tangent", "atan", "value", 1, "Trigonometry"),
    (ArcTangent2, "arc_tangent_2", "atan2", "y, x", 2, "Trigonometry"),
    (HyperbolicSine, "hyperbolic_sine", "sinh", "value", 1, "Trigonometry"),
    (HyperbolicCosine, "hyperbolic_cosine", "cosh", "value", 1, "Trigonometry"),
    (HyperbolicTangent, "hyperbolic_tangent", "tanh", "value", 1, "Trigonometry"),
    (AreaHyperbolicSine, "area_hyperbolic_sine", "asinh", "value", 1, "Trigonometry"),
    (AreaHyperbolicCosine, "area_hyperbolic_cosine", "acosh", "value", 1, "Trigonometry"),
    (AreaHyperbolicTangent, "area_hyperbolic_tangent", "atanh", "value", 1, "Trigonometry"),
    (Secant, "secant", "sec", "radians", 1, "Trigonometry"),
    (Cosecant, "cosecant", "csc", "radians", 1, "Trigonometry"),
    (Cotangent, "cotangent", "cot", "radians", 1, "Trigonometry"),
    (Sinc, "sinc", "sinc", "radians", 1, "Trigonometry"),
    (Hypotenuse, "hypotenuse", "hypot", "x, y", 2, "Geometry"),
    (Hypotenuse3, "hypotenuse_3", "hypot3", "x, y, z", 3, "Geometry"),
    (ToRadians, "to_radians", "to rad", "degrees", 1, "Angles"),
    (ToDegrees, "to_degrees", "to deg", "radians", 1, "Angles"),
    (NormalizeRadians, "normalize_radians", "wrap rad", "radians", 1, "Angles"),
    (NormalizeDegrees, "normalize_degrees", "wrap deg", "degrees", 1, "Angles"),
    (Exponential, "exponential", "exp", "value", 1, "Logarithms"),
    (Exponential2, "exponential_2", "exp2", "value", 1, "Logarithms"),
    (Exponential10, "exponential_10", "exp10", "value", 1, "Logarithms"),
    (ExponentialMinus1, "exponential_minus_1", "exp-1", "value", 1, "Logarithms"),
    (NaturalLog, "natural_log", "ln", "value", 1, "Logarithms"),
    (Log2, "log_2", "log2", "value", 1, "Logarithms"),
    (Log10, "log_10", "log10", "value", 1, "Logarithms"),
    (Logarithm, "logarithm", "log", "value, base", 2, "Logarithms"),
    (NaturalLog1Plus, "natural_log_1_plus", "ln1p", "value", 1, "Logarithms"),
    (ErrorFunction, "error_function", "erf", "value", 1, "Special"),
    (ComplementaryErrorFunction, "complementary_error_function", "erfc", "value", 1, "Special"),
    (Gamma, "gamma", "gamma", "value", 1, "Special"),
    (NaturalLogGamma, "natural_log_gamma", "ln gamma", "value", 1, "Special"),
    (Percent, "percent", "% of", "value, percentage", 2, "Percentages"),
    (Percentage, "percentage", "%", "value, total", 2, "Percentages"),
    (PercentageChange, "percentage_change", "% change", "from, to", 2, "Percentages"),
    (LinearInterpolation, "linear_interpolation", "lerp", "start, end, amount", 3, "Interpolation"),
    (
        InverseLinearInterpolation,
        "inverse_linear_interpolation",
        "inv lerp",
        "start, end, value",
        3,
        "Interpolation"
    ),
    (
        MapRange,
        "map_range",
        "map",
        "value, in start, in end, out start, out end",
        5,
        "Interpolation"
    ),
    (Sigmoid, "sigmoid", "sigmoid", "value", 1, "Special"),
    (RoundToPlaces, "round_to_places", "round n", "value, integer places", 2, "Rounding"),
    (CircleArea, "circle_area", "circle A", "radius", 1, "Geometry"),
    (CircleCircumference, "circle_circumference", "circle C", "radius", 1, "Geometry"),
    (RectangleArea, "rectangle_area", "rect A", "width, height", 2, "Geometry"),
    (RectanglePerimeter, "rectangle_perimeter", "rect P", "width, height", 2, "Geometry"),
    (TriangleArea, "triangle_area", "tri A", "base, height", 2, "Geometry"),
    (
        TriangleAreaFromSides,
        "triangle_area_from_sides",
        "Heron",
        "side a, side b, side c",
        3,
        "Geometry"
    ),
    (Distance2d, "distance_2d", "dist 2D", "x1, y1, x2, y2", 4, "Geometry"),
    (Distance3d, "distance_3d", "dist 3D", "x1, y1, z1, x2, y2, z2", 6, "Geometry"),
    (SphereSurfaceArea, "sphere_surface_area", "sphere A", "radius", 1, "Geometry"),
    (SphereVolume, "sphere_volume", "sphere V", "radius", 1, "Geometry"),
    (CylinderVolume, "cylinder_volume", "cylinder V", "radius, height", 2, "Geometry"),
    (ConeVolume, "cone_volume", "cone V", "radius, height", 2, "Geometry"),
    (EvaluateLinear, "evaluate_linear", "a*x+b", "slope, intercept, x", 3, "Algebra"),
    (EvaluateQuadratic, "evaluate_quadratic", "a*x2+bx+c", "a, b, c, x", 4, "Algebra"),
    (
        SimpleInterest,
        "simple_interest",
        "simple int",
        "principal, annual rate %, years",
        3,
        "Finance"
    ),
    (
        CompoundValue,
        "compound_value",
        "compound",
        "principal, annual rate %, periods/year, years",
        4,
        "Finance"
    ),
    (PresentValue, "present_value", "PV", "future value, rate %, periods", 3, "Finance"),
    (FutureValue, "future_value", "FV", "present value, rate %, periods", 3, "Finance"),
    (
        LoanPayment,
        "loan_payment",
        "payment",
        "principal, annual rate %, payments/year, years",
        4,
        "Finance"
    ),
    (DiscountedPrice, "discounted_price", "discount", "price, discount %", 2, "Finance"),
    (PriceWithTax, "price_with_tax", "+ tax", "price, tax %", 2, "Finance"),
    (Profit, "profit", "profit", "cost, revenue", 2, "Finance"),
    (ProfitMargin, "profit_margin", "margin", "cost, revenue", 2, "Finance"),
    (
        ArithmeticSequenceTerm,
        "arithmetic_sequence_term",
        "arith term",
        "first, difference, integer index",
        3,
        "Sequences"
    ),
    (
        ArithmeticSequenceSum,
        "arithmetic_sequence_sum",
        "arith sum",
        "first, difference, integer terms",
        3,
        "Sequences"
    ),
    (
        GeometricSequenceTerm,
        "geometric_sequence_term",
        "geom term",
        "first, ratio, integer index",
        3,
        "Sequences"
    ),
    (
        GeometricSequenceSum,
        "geometric_sequence_sum",
        "geom sum",
        "first, ratio, integer terms",
        3,
        "Sequences"
    ),
);

impl CalculatorOperation {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw > CalculatorOperation::GeometricSequenceSum as u32 {
            return None;
        }
        // SAFETY: the enum is contiguous from zero through the checked last value.
        Some(unsafe { core::mem::transmute::<u16, Self>(raw as u16) })
    }

    #[inline]
    pub fn spec(self) -> &'static CalculatorFunctionSpec {
        &CALCULATOR_FUNCTIONS[self as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorEvalError {
    UnknownOperation,
    WrongArgumentCount { expected: u8, actual: usize },
    InvalidIntegerArgument(usize),
}

fn integer_u32_argument(arguments: &[f64], index: usize) -> Result<u32, CalculatorEvalError> {
    let value = arguments[index];
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 || f64::fract(value) != 0.0 {
        Err(CalculatorEvalError::InvalidIntegerArgument(index))
    } else {
        Ok(value as u32)
    }
}

fn integer_u64_argument(arguments: &[f64], index: usize) -> Result<u64, CalculatorEvalError> {
    let value = arguments[index];
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 || f64::fract(value) != 0.0 {
        Err(CalculatorEvalError::InvalidIntegerArgument(index))
    } else {
        Ok(value as u64)
    }
}

fn integer_i32_argument(arguments: &[f64], index: usize) -> Result<i32, CalculatorEvalError> {
    let value = arguments[index];
    if !value.is_finite()
        || value < i32::MIN as f64
        || value > i32::MAX as f64
        || f64::fract(value) != 0.0
    {
        Err(CalculatorEvalError::InvalidIntegerArgument(index))
    } else {
        Ok(value as i32)
    }
}

pub fn evaluate_operation_id(raw: u32, arguments: &[f64]) -> Result<f64, CalculatorEvalError> {
    let operation =
        CalculatorOperation::from_raw(raw).ok_or(CalculatorEvalError::UnknownOperation)?;
    evaluate_operation(operation, arguments)
}

pub fn evaluate_operation(
    operation: CalculatorOperation,
    arguments: &[f64],
) -> Result<f64, CalculatorEvalError> {
    let expected = operation.spec().arity;
    if arguments.len() != expected as usize {
        return Err(CalculatorEvalError::WrongArgumentCount {
            expected,
            actual: arguments.len(),
        });
    }

    let a = arguments.first().copied().unwrap_or(0.0);
    let b = arguments.get(1).copied().unwrap_or(0.0);
    let c = arguments.get(2).copied().unwrap_or(0.0);
    let d = arguments.get(3).copied().unwrap_or(0.0);
    let result = match operation {
        CalculatorOperation::Add => a + b,
        CalculatorOperation::Subtract => a - b,
        CalculatorOperation::Multiply => a * b,
        CalculatorOperation::Divide => a / b,
        CalculatorOperation::Modulo => f64::modulo(a, b),
        CalculatorOperation::Power => f64::pow(a, b),
        CalculatorOperation::Minimum => f64::min(a, b),
        CalculatorOperation::Maximum => f64::max(a, b),
        CalculatorOperation::Clamp => f64::clamp(a, b, c),
        CalculatorOperation::Reciprocal => f64::reciprocal(a),
        CalculatorOperation::Square => square(a),
        CalculatorOperation::Cube => cube(a),
        CalculatorOperation::SquareRoot => f64::sqrt(a),
        CalculatorOperation::CubeRoot => f64::cbrt(a),
        CalculatorOperation::NthRoot => f64::nth_root(a, integer_u32_argument(arguments, 1)?),
        CalculatorOperation::Absolute => f64::abs(a),
        CalculatorOperation::Floor => f64::floor(a),
        CalculatorOperation::Ceiling => f64::ceil(a),
        CalculatorOperation::Round => f64::round(a),
        CalculatorOperation::Truncate => f64::trunc(a),
        CalculatorOperation::Fraction => f64::fract(a),
        CalculatorOperation::MultiplyAdd => f64::mul_add(a, b, c),
        CalculatorOperation::Sine => f64::sin(a),
        CalculatorOperation::Cosine => f64::cos(a),
        CalculatorOperation::Tangent => f64::tan(a),
        CalculatorOperation::ArcSine => f64::asin(a),
        CalculatorOperation::ArcCosine => f64::acos(a),
        CalculatorOperation::ArcTangent => f64::atan(a),
        CalculatorOperation::ArcTangent2 => f64::atan2(a, b),
        CalculatorOperation::HyperbolicSine => f64::sinh(a),
        CalculatorOperation::HyperbolicCosine => f64::cosh(a),
        CalculatorOperation::HyperbolicTangent => f64::tanh(a),
        CalculatorOperation::AreaHyperbolicSine => f64::asinh(a),
        CalculatorOperation::AreaHyperbolicCosine => f64::acosh(a),
        CalculatorOperation::AreaHyperbolicTangent => f64::atanh(a),
        CalculatorOperation::Secant => f64::sec(a),
        CalculatorOperation::Cosecant => f64::csc(a),
        CalculatorOperation::Cotangent => f64::cot(a),
        CalculatorOperation::Sinc => f64::sinc(a),
        CalculatorOperation::Hypotenuse => f64::hypot(a, b),
        CalculatorOperation::Hypotenuse3 => f64::hypot3(a, b, c),
        CalculatorOperation::ToRadians => f64::to_radians(a),
        CalculatorOperation::ToDegrees => f64::to_degrees(a),
        CalculatorOperation::NormalizeRadians => f64::normalize_radians(a),
        CalculatorOperation::NormalizeDegrees => f64::normalize_degrees(a),
        CalculatorOperation::Exponential => f64::exp(a),
        CalculatorOperation::Exponential2 => f64::exp2(a),
        CalculatorOperation::Exponential10 => f64::exp10(a),
        CalculatorOperation::ExponentialMinus1 => f64::exp_m1(a),
        CalculatorOperation::NaturalLog => f64::ln(a),
        CalculatorOperation::Log2 => f64::log2(a),
        CalculatorOperation::Log10 => f64::log10(a),
        CalculatorOperation::Logarithm => f64::log(a, b),
        CalculatorOperation::NaturalLog1Plus => f64::ln_1p(a),
        CalculatorOperation::ErrorFunction => f64::erf(a),
        CalculatorOperation::ComplementaryErrorFunction => f64::erfc(a),
        CalculatorOperation::Gamma => f64::gamma(a),
        CalculatorOperation::NaturalLogGamma => f64::ln_gamma(a),
        CalculatorOperation::Percent => f64::percent(a, b),
        CalculatorOperation::Percentage => f64::percentage(a, b),
        CalculatorOperation::PercentageChange => f64::percentage_change(a, b),
        CalculatorOperation::LinearInterpolation => f64::lerp(a, b, c),
        CalculatorOperation::InverseLinearInterpolation => f64::inverse_lerp(a, b, c),
        CalculatorOperation::MapRange => f64::map_range(a, b, c, d, arguments[4]),
        CalculatorOperation::Sigmoid => f64::sigmoid(a),
        CalculatorOperation::RoundToPlaces => {
            f64::round_to_places(a, integer_i32_argument(arguments, 1)?)
        }
        CalculatorOperation::CircleArea => geometry::circle_area(a),
        CalculatorOperation::CircleCircumference => geometry::circle_circumference(a),
        CalculatorOperation::RectangleArea => geometry::rectangle_area(a, b),
        CalculatorOperation::RectanglePerimeter => geometry::rectangle_perimeter(a, b),
        CalculatorOperation::TriangleArea => geometry::triangle_area(a, b),
        CalculatorOperation::TriangleAreaFromSides => {
            geometry::triangle_area_sides(a, b, c).unwrap_or(f64::NAN)
        }
        CalculatorOperation::Distance2d => geometry::distance_2d(a, b, c, d),
        CalculatorOperation::Distance3d => {
            geometry::distance_3d(a, b, c, d, arguments[4], arguments[5])
        }
        CalculatorOperation::SphereSurfaceArea => geometry::sphere_surface_area(a),
        CalculatorOperation::SphereVolume => geometry::sphere_volume(a),
        CalculatorOperation::CylinderVolume => geometry::cylinder_volume(a, b),
        CalculatorOperation::ConeVolume => geometry::cone_volume(a, b),
        CalculatorOperation::EvaluateLinear => algebra::evaluate_linear(a, b, c),
        CalculatorOperation::EvaluateQuadratic => algebra::evaluate_quadratic(a, b, c, d),
        CalculatorOperation::SimpleInterest => finance::simple_interest(a, b, c),
        CalculatorOperation::CompoundValue => {
            finance::compound_value(a, b, integer_u32_argument(arguments, 2)?, d)
                .unwrap_or(f64::NAN)
        }
        CalculatorOperation::PresentValue => finance::present_value(a, b, c),
        CalculatorOperation::FutureValue => finance::future_value(a, b, c),
        CalculatorOperation::LoanPayment => {
            finance::loan_payment(a, b, integer_u32_argument(arguments, 2)?, d).unwrap_or(f64::NAN)
        }
        CalculatorOperation::DiscountedPrice => finance::discounted_price(a, b),
        CalculatorOperation::PriceWithTax => finance::price_with_tax(a, b),
        CalculatorOperation::Profit => finance::profit(a, b),
        CalculatorOperation::ProfitMargin => finance::profit_margin(a, b),
        CalculatorOperation::ArithmeticSequenceTerm => {
            algebra::arithmetic_sequence_term(a, b, integer_u64_argument(arguments, 2)?)
        }
        CalculatorOperation::ArithmeticSequenceSum => {
            algebra::arithmetic_sequence_sum(a, b, integer_u64_argument(arguments, 2)?)
        }
        CalculatorOperation::GeometricSequenceTerm => {
            algebra::geometric_sequence_term(a, b, integer_i32_argument(arguments, 2)?)
        }
        CalculatorOperation::GeometricSequenceSum => {
            algebra::geometric_sequence_sum(a, b, integer_u64_argument(arguments, 2)?)
        }
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        CALCULATOR_FUNCTIONS, CALCULATOR_MAX_ARGUMENTS, CalculatorEvalError, CalculatorOperation,
        algebra, evaluate_operation, f32, f64, integer, statistics,
    };

    #[test]
    fn existing_float_functions_are_exposed_through_clean_names() {
        assert!(f32::approx_eq(f32::sin(f32::PI / 2.0), 1.0, 1e-6));
        assert!(f64::approx_eq(f64::log(8.0, 2.0), 3.0, 1e-12));
        assert_eq!(f64::nth_root(-27.0, 3), -3.0);
    }

    #[test]
    fn integer_functions_cover_common_number_theory() {
        assert_eq!(integer::gcd(54, 24), 6);
        assert_eq!(integer::lcm(21, 6), Some(42));
        assert_eq!(integer::factorial(10), Some(3_628_800));
        assert_eq!(integer::combinations(52, 5), Some(2_598_960));
        assert_eq!(
            integer::fibonacci(186),
            Some(332_825_110_087_067_562_321_196_029_789_634_457_848)
        );
        assert_eq!(integer::fibonacci(187), None);
        assert_eq!(integer::modular_power(4, 13, 497), Some(445));
        assert_eq!(integer::integer_sqrt(u128::MAX), u64::MAX as u128);
    }

    #[test]
    fn statistics_are_allocation_free() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(statistics::mean(&values), Some(2.5));
        assert_eq!(statistics::variance_population(&values), Some(1.25));
        assert_eq!(statistics::sum(&[1.0, f64::INFINITY]), f64::INFINITY);

        let mut unsorted = [4.0, 1.0, 3.0, 2.0];
        assert_eq!(statistics::median(&mut unsorted), Some(2.5));
    }

    #[test]
    fn polynomial_and_quadratic_helpers_work() {
        assert_eq!(algebra::evaluate_polynomial(&[2.0, 3.0, 4.0], 5.0), 69.0);
        assert_eq!(
            algebra::solve_quadratic(1.0, -3.0, 2.0),
            algebra::QuadraticRoots::Two(1.0, 2.0)
        );
    }

    #[test]
    fn dynamic_operation_registry_evaluates_checked_arguments() {
        assert_eq!(evaluate_operation(CalculatorOperation::Add, &[20.0, 22.0]), Ok(42.0));
        assert_eq!(evaluate_operation(CalculatorOperation::NthRoot, &[81.0, 4.0]), Ok(3.0));
        assert_eq!(
            evaluate_operation(CalculatorOperation::Sine, &[]),
            Err(CalculatorEvalError::WrongArgumentCount {
                expected: 1,
                actual: 0,
            })
        );
        assert_eq!(CALCULATOR_FUNCTIONS.len(), 93);

        let arguments = [2.0; CALCULATOR_MAX_ARGUMENTS];
        for spec in CALCULATOR_FUNCTIONS {
            assert!(
                evaluate_operation(spec.operation, &arguments[..spec.arity as usize]).is_ok(),
                "registry entry {} does not match its evaluator",
                spec.name
            );
        }
    }
}
