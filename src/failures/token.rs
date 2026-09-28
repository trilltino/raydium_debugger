//! SPL Token and Token-2022 custom-error registry.

use super::types::FailureCode;

pub(crate) const SPL_TOKEN_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 0,
        name: "NotRentExempt",
        message: "Lamport balance below rent-exempt threshold",
    },
    FailureCode {
        code: 1,
        name: "InsufficientFunds",
        message: "Insufficient funds",
    },
    FailureCode {
        code: 2,
        name: "InvalidMint",
        message: "Invalid mint",
    },
    FailureCode {
        code: 3,
        name: "MintMismatch",
        message: "Account not associated with this mint",
    },
    FailureCode {
        code: 4,
        name: "OwnerMismatch",
        message: "Owner does not match",
    },
    FailureCode {
        code: 5,
        name: "FixedSupply",
        message: "Fixed supply",
    },
    FailureCode {
        code: 6,
        name: "AlreadyInUse",
        message: "Already in use",
    },
    FailureCode {
        code: 7,
        name: "InvalidNumberOfProvidedSigners",
        message: "Invalid number of provided signers",
    },
    FailureCode {
        code: 8,
        name: "InvalidNumberOfRequiredSigners",
        message: "Invalid number of required signers",
    },
    FailureCode {
        code: 9,
        name: "UninitializedState",
        message: "State is uninitialized",
    },
    FailureCode {
        code: 10,
        name: "NativeNotSupported",
        message: "Instruction does not support native tokens",
    },
    FailureCode {
        code: 11,
        name: "NonNativeHasBalance",
        message: "Non-native account can only be closed if its balance is zero",
    },
    FailureCode {
        code: 12,
        name: "InvalidInstruction",
        message: "Invalid instruction",
    },
    FailureCode {
        code: 13,
        name: "InvalidState",
        message: "State is invalid for requested operation",
    },
    FailureCode {
        code: 14,
        name: "Overflow",
        message: "Operation overflowed",
    },
    FailureCode {
        code: 15,
        name: "AuthorityTypeNotSupported",
        message: "Authority type is not supported",
    },
    FailureCode {
        code: 16,
        name: "MintCannotFreeze",
        message: "Mint cannot freeze accounts",
    },
    FailureCode {
        code: 17,
        name: "AccountFrozen",
        message: "Account is frozen",
    },
    FailureCode {
        code: 18,
        name: "MintDecimalsMismatch",
        message: "Mint decimals mismatch",
    },
    FailureCode {
        code: 19,
        name: "NonNativeNotSupported",
        message: "Instruction does not support non-native tokens",
    },
];
