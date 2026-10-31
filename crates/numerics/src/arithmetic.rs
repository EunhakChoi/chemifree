//! Exact rational operations followed by one explicit profile-specific store.

use std::cmp::Ordering;

use num_bigint::BigUint;

use crate::{Binary32, Binary64, Exact, Extended80, NumericError, Real48, Sign};

#[derive(Clone, Copy)]
enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

fn opposite(sign: Sign) -> Sign {
    match sign {
        Sign::Positive => Sign::Negative,
        Sign::Negative => Sign::Positive,
    }
}

fn product_sign(left: Sign, right: Sign) -> Sign {
    if left == right {
        Sign::Positive
    } else {
        Sign::Negative
    }
}

fn add(left: &Exact, right: &Exact, right_sign: Sign) -> Result<Exact, NumericError> {
    let left_magnitude = &left.numerator * &right.denominator;
    let right_magnitude = &right.numerator * &left.denominator;
    let denominator = &left.denominator * &right.denominator;
    let (sign, numerator) = if left.sign == right_sign {
        (left.sign, left_magnitude + right_magnitude)
    } else {
        match left_magnitude.cmp(&right_magnitude) {
            Ordering::Greater => (left.sign, left_magnitude - right_magnitude),
            Ordering::Less => (right_sign, right_magnitude - left_magnitude),
            // Under the selected nearest rounding, exact cancellation is positive zero.
            Ordering::Equal => (Sign::Positive, BigUint::from(0_u8)),
        }
    };
    Exact::from_ratio(sign, numerator, denominator)
}

fn calculate(left: &Exact, right: &Exact, operation: Operation) -> Result<Exact, NumericError> {
    match operation {
        Operation::Add => add(left, right, right.sign),
        Operation::Subtract => add(left, right, opposite(right.sign)),
        Operation::Multiply => Exact::from_ratio(
            product_sign(left.sign, right.sign),
            &left.numerator * &right.numerator,
            &left.denominator * &right.denominator,
        ),
        Operation::Divide => {
            if right.numerator == BigUint::from(0_u8) {
                return Err(NumericError::DivisionByZero);
            }
            Exact::from_ratio(
                product_sign(left.sign, right.sign),
                &left.numerator * &right.denominator,
                &left.denominator * &right.numerator,
            )
        }
    }
}

macro_rules! arithmetic {
    ($number:ty) => {
        impl $number {
            /// Adds finite operands and stores one result in this type's rounding profile.
            ///
            /// # Errors
            /// Rejects nonfinite or unsupported inputs, overflow and resource limits.
            pub fn checked_add(self, rhs: Self) -> Result<Self, NumericError> {
                Self::from_exact(&calculate(&self.exact()?, &rhs.exact()?, Operation::Add)?)
            }

            /// Subtracts finite operands and stores one result in this type's rounding profile.
            ///
            /// # Errors
            /// Rejects nonfinite or unsupported inputs, overflow and resource limits.
            pub fn checked_sub(self, rhs: Self) -> Result<Self, NumericError> {
                Self::from_exact(&calculate(
                    &self.exact()?,
                    &rhs.exact()?,
                    Operation::Subtract,
                )?)
            }

            /// Multiplies finite operands and stores one result in this type's rounding profile.
            ///
            /// # Errors
            /// Rejects nonfinite or unsupported inputs, overflow and resource limits.
            pub fn checked_mul(self, rhs: Self) -> Result<Self, NumericError> {
                Self::from_exact(&calculate(
                    &self.exact()?,
                    &rhs.exact()?,
                    Operation::Multiply,
                )?)
            }

            /// Divides finite operands and stores one result in this type's rounding profile.
            ///
            /// # Errors
            /// Rejects a zero divisor, nonfinite or unsupported input, overflow and resource limits.
            pub fn checked_div(self, rhs: Self) -> Result<Self, NumericError> {
                Self::from_exact(&calculate(
                    &self.exact()?,
                    &rhs.exact()?,
                    Operation::Divide,
                )?)
            }
        }
    };
}

arithmetic!(Binary32);
arithmetic!(Binary64);
arithmetic!(Real48);
arithmetic!(Extended80);
