//! Native Solana and associated-token failure registries.

use super::types::FailureCode;

pub(crate) const SYSTEM_PROGRAM_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 0,
        name: "AccountAlreadyInUse",
        message: "Tried to create an account that already exists",
    },
    FailureCode {
        code: 1,
        name: "ResultWithNegativeLamports",
        message: "A transfer would overdraw the source account",
    },
    FailureCode {
        code: 2,
        name: "InvalidProgramId",
        message: "Assigned ownership to an invalid program",
    },
    FailureCode {
        code: 3,
        name: "InvalidAccountDataLength",
        message: "Created account data length is invalid",
    },
    FailureCode {
        code: 4,
        name: "MaxSeedLengthExceeded",
        message: "A PDA seed exceeds the maximum seed length",
    },
    FailureCode {
        code: 5,
        name: "AddressWithSeedMismatch",
        message: "Address, seed, and owner do not derive the provided account",
    },
];

pub(crate) const ASSOCIATED_TOKEN_ACCOUNT_ERRORS: &[FailureCode] = &[FailureCode {
    code: 0,
    name: "AccountAlreadyInUse",
    message: "Associated token account already exists",
}];
