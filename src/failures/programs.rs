//! Program IDs and display labels used while decoding failures.

/// System program ID.
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
pub const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYFnAFkL5MP8gy1pjxP5M";
pub const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
pub const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
pub const RAYDIUM_CPMM_LEGACY_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVN5hM6KcpCwxo1zqyA3CcuF1";
pub const RAYDIUM_LAUNCHLAB_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

pub fn program_label(program_id: &str) -> &'static str {
    match program_id {
        SYSTEM_PROGRAM_ID => "System Program",
        COMPUTE_BUDGET_PROGRAM_ID => "Compute Budget",
        MEMO_PROGRAM_ID => "Memo",
        SPL_TOKEN_PROGRAM_ID => "SPL Token",
        TOKEN_2022_PROGRAM_ID => "Token-2022",
        ASSOCIATED_TOKEN_PROGRAM_ID => "Associated Token",
        RAYDIUM_AMM_V4_PROGRAM_ID | RAYDIUM_AMM_V4_LEGACY_PROGRAM_ID => "Raydium AMM v4",
        RAYDIUM_CLMM_PROGRAM_ID => "Raydium CLMM",
        RAYDIUM_CPMM_PROGRAM_ID | RAYDIUM_CPMM_LEGACY_PROGRAM_ID => "Raydium CPMM",
        RAYDIUM_LAUNCHLAB_PROGRAM_ID => "Raydium LaunchLab",
        _ => "Unknown Program",
    }
}
