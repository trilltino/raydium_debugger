//! Vendored Raydium custom-error registries used at runtime.

use super::types::FailureCode;

pub(crate) const RAYDIUM_CLMM_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 6000,
        name: "NotApproved",
        message: "Not approved",
    },
    FailureCode {
        code: 6001,
        name: "InvalidUpdateConfigFlag",
        message: "invalid update amm config flag",
    },
    FailureCode {
        code: 6002,
        name: "AccountLack",
        message: "Account lack",
    },
    FailureCode {
        code: 6003,
        name: "ClosePositionErr",
        message:
            "Remove liquidity, collect fees owed and reward then you can close position account",
    },
    FailureCode {
        code: 6004,
        name: "InvalidTickIndex",
        message: "Tick out of range",
    },
    FailureCode {
        code: 6005,
        name: "TickInvalidOrder",
        message: "The lower tick must be below the upper tick",
    },
    FailureCode {
        code: 6014,
        name: "LiquidityInsufficient",
        message: "Liquidity insufficient",
    },
    FailureCode {
        code: 6015,
        name: "PriceSlippageCheck",
        message: "Price slippage check",
    },
    FailureCode {
        code: 6016,
        name: "TooLittleOutputReceived",
        message: "Too little output received",
    },
    FailureCode {
        code: 6017,
        name: "TooMuchInputPaid",
        message: "Too much input paid",
    },
    FailureCode {
        code: 6018,
        name: "ZeroAmountSpecified",
        message: "Swap special amount can not be zero",
    },
    FailureCode {
        code: 6019,
        name: "InvalidInputPoolVault",
        message: "Input pool vault is invalid",
    },
    FailureCode {
        code: 6020,
        name: "TooSmallInputOrOutputAmount",
        message: "Swap input or output amount is too small",
    },
    FailureCode {
        code: 6021,
        name: "NotEnoughTickArrayAccount",
        message: "Not enough tick array account",
    },
    FailureCode {
        code: 6040,
        name: "CalculateOverflow",
        message: "Calculate overflow",
    },
    FailureCode {
        code: 6041,
        name: "TransferFeeCalculateNotMatch",
        message: "TransferFee calculate not match",
    },
    FailureCode {
        code: 6049,
        name: "MissingTokenProgram2022",
        message: "Token-2022 program is required but not provided",
    },
];

pub(crate) const RAYDIUM_CPMM_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 6000,
        name: "NotApproved",
        message: "Not approved",
    },
    FailureCode {
        code: 6001,
        name: "InvalidOwner",
        message: "Input account owner is not the program address",
    },
    FailureCode {
        code: 6002,
        name: "EmptySupply",
        message: "Input token account empty",
    },
    FailureCode {
        code: 6003,
        name: "InvalidInput",
        message: "InvalidInput",
    },
    FailureCode {
        code: 6004,
        name: "IncorrectLpMint",
        message: "Address of the provided lp token mint is incorrect",
    },
    FailureCode {
        code: 6005,
        name: "ExceededSlippage",
        message: "Exceeds desired slippage limit",
    },
    FailureCode {
        code: 6006,
        name: "ZeroTradingTokens",
        message: "Given pool token amount results in zero trading tokens",
    },
    FailureCode {
        code: 6007,
        name: "NotSupportMint",
        message: "Not support token_2022 mint extension",
    },
    FailureCode {
        code: 6008,
        name: "InvalidVault",
        message: "invaild vault",
    },
    FailureCode {
        code: 6009,
        name: "InitLpAmountTooLess",
        message: "Init lp amount is too less",
    },
    FailureCode {
        code: 6010,
        name: "TransferFeeCalculateNotMatch",
        message: "TransferFee calculate not match",
    },
    FailureCode {
        code: 6011,
        name: "MathOverflow",
        message: "Math operation overflow",
    },
    FailureCode {
        code: 6012,
        name: "InsufficientLiquidity",
        message: "Insufficient liquidity",
    },
];

pub(crate) const RAYDIUM_LAUNCHPAD_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 6000,
        name: "NotApproved",
        message: "Not approved",
    },
    FailureCode {
        code: 6001,
        name: "InvalidOwner",
        message: "Input account owner is not the program address",
    },
    FailureCode {
        code: 6002,
        name: "InvalidInput",
        message: "InvalidInput",
    },
    FailureCode {
        code: 6003,
        name: "InputNotMatchCurveConfig",
        message: "The input params are not match with curve type in config",
    },
    FailureCode {
        code: 6004,
        name: "ExceededSlippage",
        message: "Exceeds desired slippage limit",
    },
    FailureCode {
        code: 6005,
        name: "InvalidPlatform",
        message: "Invalid platform account",
    },
    FailureCode {
        code: 6006,
        name: "InvalidPlatformFeeRate",
        message: "Invalid platform fee rate",
    },
    FailureCode {
        code: 6007,
        name: "InvalidCreatorFeeRate",
        message: "Invalid creator fee rate",
    },
    FailureCode {
        code: 6008,
        name: "InvalidMigrateType",
        message: "Invalid migrate type",
    },
    FailureCode {
        code: 6009,
        name: "MathOverflow",
        message: "Math operation overflow",
    },
    FailureCode {
        code: 6010,
        name: "InsufficientLiquidity",
        message: "Insufficient liquidity",
    },
    FailureCode {
        code: 6011,
        name: "TooLittleOutputReceived",
        message: "Too little output received",
    },
    FailureCode {
        code: 6012,
        name: "TooMuchInputPaid",
        message: "Too much input paid",
    },
    FailureCode {
        code: 6013,
        name: "PoolMigrated",
        message: "Pool migrated",
    },
];

pub(crate) const RAYDIUM_AMM_V4_ERRORS: &[FailureCode] = &[
    FailureCode {
        code: 0,
        name: "AlreadyInUse",
        message: "Already in use.",
    },
    FailureCode {
        code: 1,
        name: "InvalidProgramAddress",
        message: "Invalid program address.",
    },
    FailureCode {
        code: 2,
        name: "ExpectedMint",
        message: "Expected mint.",
    },
    FailureCode {
        code: 3,
        name: "ExpectedAccount",
        message: "Expected account.",
    },
    FailureCode {
        code: 4,
        name: "InvalidCoinVault",
        message: "Invalid coin vault.",
    },
    FailureCode {
        code: 5,
        name: "InvalidPCVault",
        message: "Invalid PC vault.",
    },
    FailureCode {
        code: 6,
        name: "InvalidTokenLP",
        message: "Invalid token LP.",
    },
    FailureCode {
        code: 7,
        name: "InvalidDestTokenCoin",
        message: "Invalid destination token coin.",
    },
    FailureCode {
        code: 8,
        name: "InvalidDestTokenPC",
        message: "Invalid destination token PC.",
    },
    FailureCode {
        code: 9,
        name: "InvalidPoolMint",
        message: "Invalid pool mint.",
    },
    FailureCode {
        code: 10,
        name: "InvalidOpenOrders",
        message: "Invalid open orders.",
    },
    FailureCode {
        code: 11,
        name: "InvalidMarket",
        message: "Invalid market.",
    },
    FailureCode {
        code: 12,
        name: "InvalidMarketProgram",
        message: "Invalid market program.",
    },
    FailureCode {
        code: 13,
        name: "InvalidTargetOrders",
        message: "Invalid target orders.",
    },
    FailureCode {
        code: 14,
        name: "AccountNeedWriteable",
        message: "Account must be writable.",
    },
    FailureCode {
        code: 15,
        name: "AccountNeedReadOnly",
        message: "Account must be read-only.",
    },
    FailureCode {
        code: 16,
        name: "InvalidCoinMint",
        message: "Invalid coin mint.",
    },
    FailureCode {
        code: 17,
        name: "InvalidPCMint",
        message: "Invalid PC mint.",
    },
    FailureCode {
        code: 18,
        name: "InvalidOwner",
        message: "Invalid owner.",
    },
    FailureCode {
        code: 19,
        name: "InvalidSupply",
        message: "Invalid supply.",
    },
    FailureCode {
        code: 20,
        name: "InvalidDelegate",
        message: "Invalid delegate.",
    },
    FailureCode {
        code: 21,
        name: "InvalidSignAccount",
        message: "Invalid sign account.",
    },
    FailureCode {
        code: 22,
        name: "InvalidStatus",
        message: "Invalid status.",
    },
    FailureCode {
        code: 23,
        name: "InvalidInstruction",
        message: "Invalid instruction.",
    },
    FailureCode {
        code: 24,
        name: "WrongAccountsNumber",
        message: "Wrong accounts number.",
    },
    FailureCode {
        code: 25,
        name: "InvalidTargetAccountOwner",
        message: "Invalid target account owner.",
    },
    FailureCode {
        code: 26,
        name: "InvalidTargetOwner",
        message: "Invalid target owner.",
    },
    FailureCode {
        code: 27,
        name: "InvalidAmmAccountOwner",
        message: "Invalid AMM account owner.",
    },
    FailureCode {
        code: 28,
        name: "InvalidParamsSet",
        message: "Invalid parameter set.",
    },
    FailureCode {
        code: 29,
        name: "InvalidInput",
        message: "Invalid input.",
    },
    FailureCode {
        code: 30,
        name: "ExceededSlippage",
        message: "Exceeded desired slippage limit.",
    },
    FailureCode {
        code: 31,
        name: "CalculationExRateFailure",
        message: "Calculation exchange rate failed.",
    },
    FailureCode {
        code: 32,
        name: "CheckedSubOverflow",
        message: "Checked subtraction overflow.",
    },
    FailureCode {
        code: 33,
        name: "CheckedAddOverflow",
        message: "Checked addition overflow.",
    },
    FailureCode {
        code: 34,
        name: "CheckedMulOverflow",
        message: "Checked multiplication overflow.",
    },
    FailureCode {
        code: 35,
        name: "CheckedDivOverflow",
        message: "Checked division overflow.",
    },
    FailureCode {
        code: 36,
        name: "CheckedEmptyFunds",
        message: "Empty funds.",
    },
    FailureCode {
        code: 37,
        name: "CalcPnlError",
        message: "P&L calculation error.",
    },
    FailureCode {
        code: 38,
        name: "InvalidSplTokenProgram",
        message: "Invalid SPL token program.",
    },
];
