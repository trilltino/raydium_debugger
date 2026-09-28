//! Anchor framework error registry.

use super::types::FailureCode;

pub(crate) const ANCHOR_FRAMEWORK_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 100,
        name: "InstructionMissing",
        message: "No matching instruction discriminator was found",
    },
    FailureCode {
        code: 101,
        name: "InstructionFallbackNotFound",
        message: "Instruction discriminator did not match and no fallback exists",
    },
    FailureCode {
        code: 102,
        name: "InstructionDidNotDeserialize",
        message: "Instruction arguments could not be deserialized",
    },
    FailureCode {
        code: 2000,
        name: "ConstraintMut",
        message: "Account needed to be writable but was passed read-only",
    },
    FailureCode {
        code: 2001,
        name: "ConstraintHasOne",
        message: "A has_one account constraint was violated",
    },
    FailureCode {
        code: 2002,
        name: "ConstraintSigner",
        message: "Account was required to sign but did not",
    },
    FailureCode {
        code: 2003,
        name: "ConstraintRaw",
        message: "A custom account constraint evaluated false",
    },
    FailureCode {
        code: 2004,
        name: "ConstraintOwner",
        message: "Account owner is not the expected program",
    },
    FailureCode {
        code: 2006,
        name: "ConstraintSeeds",
        message: "PDA seeds or bump do not derive the provided address",
    },
    FailureCode {
        code: 2012,
        name: "ConstraintAddress",
        message: "Account address does not match the required address",
    },
    FailureCode {
        code: 2014,
        name: "ConstraintTokenMint",
        message: "Token account mint does not match the expected mint",
    },
    FailureCode {
        code: 2015,
        name: "ConstraintTokenOwner",
        message: "Token account owner does not match the expected owner",
    },
    FailureCode {
        code: 2019,
        name: "ConstraintSpace",
        message: "Initialized account space does not match the expected size",
    },
    FailureCode {
        code: 2500,
        name: "RequireViolated",
        message: "An Anchor require condition was false",
    },
    FailureCode {
        code: 2501,
        name: "RequireEqViolated",
        message: "An Anchor require_eq comparison failed",
    },
    FailureCode {
        code: 2502,
        name: "RequireKeysEqViolated",
        message: "An Anchor require_keys_eq comparison failed",
    },
    FailureCode {
        code: 2503,
        name: "RequireNeqViolated",
        message: "An Anchor require_neq comparison failed",
    },
    FailureCode {
        code: 2505,
        name: "RequireGtViolated",
        message: "An Anchor require_gt comparison failed",
    },
    FailureCode {
        code: 2506,
        name: "RequireGteViolated",
        message: "An Anchor require_gte comparison failed",
    },
    FailureCode {
        code: 3001,
        name: "AccountDiscriminatorNotFound",
        message: "Account has no Anchor discriminator",
    },
    FailureCode {
        code: 3002,
        name: "AccountDiscriminatorMismatch",
        message: "Account discriminator does not match the expected account type",
    },
    FailureCode {
        code: 3003,
        name: "AccountDidNotDeserialize",
        message: "Account data could not be deserialized",
    },
    FailureCode {
        code: 3007,
        name: "AccountOwnedByWrongProgram",
        message: "Account is owned by the wrong program",
    },
    FailureCode {
        code: 3012,
        name: "AccountNotInitialized",
        message: "Account does not exist or was not initialized",
    },
    FailureCode {
        code: 3014,
        name: "AccountNotAssociatedTokenAccount",
        message: "Account is not the expected associated token account",
    },
    FailureCode {
        code: 4100,
        name: "DeclaredProgramIdMismatch",
        message: "The declared program id does not match the deployed program id",
    },
];

// Vendored from Raydium public sources on 2026-09-26. These tables cover
// the most diagnostic swap/liquidity/account errors and keep the decoder
// deterministic at runtime. Additional upstream variants can be appended here
// without touching the decoding flow.
