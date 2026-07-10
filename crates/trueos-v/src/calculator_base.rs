//! Calculator facade and the small blueprint-to-kernel evaluation protocol.

pub use trueos_math::calculator_base::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorProtocolError {
    InvalidPointer,
    TooManyArguments,
    UnknownOperation,
    WrongArgumentCount,
    InvalidIntegerArgument,
    Kernel(i32),
}

/// Evaluates a dynamically selected operation.
///
/// TRUEOS and zkVM blueprints cross the kernel ABI. Host builds use the same
/// registry locally, which keeps blueprint demos testable on a development OS.
pub fn evaluate(
    operation: CalculatorOperation,
    arguments: &[f64],
) -> Result<f64, CalculatorProtocolError> {
    if arguments.len() > CALCULATOR_MAX_ARGUMENTS {
        return Err(CalculatorProtocolError::TooManyArguments);
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let mut value = 0.0;
        let status = unsafe {
            crate::bp_abi::trueos_cabi_calculator_evaluate(
                operation as u32,
                arguments.as_ptr(),
                arguments.len(),
                &mut value,
            )
        };
        match status {
            0 => Ok(value),
            -1 => Err(CalculatorProtocolError::InvalidPointer),
            -2 => Err(CalculatorProtocolError::TooManyArguments),
            -3 => Err(CalculatorProtocolError::UnknownOperation),
            -4 => Err(CalculatorProtocolError::WrongArgumentCount),
            -5 => Err(CalculatorProtocolError::InvalidIntegerArgument),
            other => Err(CalculatorProtocolError::Kernel(other)),
        }
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        evaluate_operation(operation, arguments).map_err(|error| match error {
            CalculatorEvalError::UnknownOperation => CalculatorProtocolError::UnknownOperation,
            CalculatorEvalError::WrongArgumentCount { .. } => {
                CalculatorProtocolError::WrongArgumentCount
            }
            CalculatorEvalError::InvalidIntegerArgument(_) => {
                CalculatorProtocolError::InvalidIntegerArgument
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_evaluator_uses_shared_registry() {
        assert_eq!(evaluate(CalculatorOperation::Add, &[2.0, 3.0]), Ok(5.0));
        assert_eq!(
            evaluate(CalculatorOperation::Sine, &[]),
            Err(CalculatorProtocolError::WrongArgumentCount)
        );
    }
}
