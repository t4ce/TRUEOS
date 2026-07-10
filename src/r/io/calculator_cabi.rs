//! Compact calculator protocol used by blueprints through the portal ABI.

use trueos_math::calculator_base::{
    CALCULATOR_MAX_ARGUMENTS, CalculatorEvalError, evaluate_operation_id,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_calculator_evaluate(
    operation: u32,
    arguments: *const f64,
    argument_count: usize,
    out_value: *mut f64,
) -> i32 {
    if out_value.is_null() || (argument_count != 0 && arguments.is_null()) {
        return -1;
    }
    if argument_count > CALCULATOR_MAX_ARGUMENTS {
        return -2;
    }

    let arguments = if argument_count == 0 {
        &[]
    } else {
        // SAFETY: the portal caller supplied a non-null pointer and the slice is
        // bounded by CALCULATOR_MAX_ARGUMENTS before it is constructed.
        unsafe { core::slice::from_raw_parts(arguments, argument_count) }
    };

    match evaluate_operation_id(operation, arguments) {
        Ok(value) => {
            // SAFETY: out_value was checked for null above.
            unsafe { out_value.write(value) };
            0
        }
        Err(CalculatorEvalError::UnknownOperation) => -3,
        Err(CalculatorEvalError::WrongArgumentCount { .. }) => -4,
        Err(CalculatorEvalError::InvalidIntegerArgument(_)) => -5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos_math::calculator_base::CalculatorOperation;

    #[test]
    fn abi_evaluates_and_rejects_bad_arity() {
        let arguments = [20.0, 22.0];
        let mut value = 0.0;
        assert_eq!(
            unsafe {
                trueos_cabi_calculator_evaluate(
                    CalculatorOperation::Add as u32,
                    arguments.as_ptr(),
                    arguments.len(),
                    &mut value,
                )
            },
            0
        );
        assert_eq!(value, 42.0);
        assert_eq!(
            unsafe {
                trueos_cabi_calculator_evaluate(
                    CalculatorOperation::Sine as u32,
                    arguments.as_ptr(),
                    arguments.len(),
                    &mut value,
                )
            },
            -4
        );
    }
}
