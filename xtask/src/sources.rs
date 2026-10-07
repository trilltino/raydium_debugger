const CLMM_IDL: &str = "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_clmm/raydium_clmm.json";
const CPMM_IDL: &str = "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_cpmm/raydium_cp_swap.json";
const LAUNCHLAB_IDL: &str = "https://raw.githubusercontent.com/raydium-io/raydium-idl/master/raydium_launchpad/raydium_launchpad.json";

pub(crate) const SOURCES: &[Source] = &[
    Source {
        product: "raydium_clmm",
        url: CLMM_IDL,
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_cpmm",
        url: CPMM_IDL,
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_launchpad",
        url: LAUNCHLAB_IDL,
        kind: SourceKind::Idl,
    },
    Source {
        product: "raydium_amm_v4",
        url: "https://raw.githubusercontent.com/raydium-io/raydium-amm/master/program/src/error.rs",
        kind: SourceKind::RustEnum,
    },
];

pub(crate) const INSTRUCTION_SOURCES: &[InstructionSource] = &[
    InstructionSource {
        protocol: "raydium_cpmm",
        url: CPMM_IDL,
        source: "https://github.com/raydium-io/raydium-idl/blob/master/raydium_cpmm/raydium_cp_swap.json",
    },
    InstructionSource {
        protocol: "raydium_clmm",
        url: CLMM_IDL,
        source: "https://github.com/raydium-io/raydium-idl/blob/master/raydium_clmm/raydium_clmm.json",
    },
    InstructionSource {
        protocol: "raydium_launchlab",
        url: LAUNCHLAB_IDL,
        source: "https://github.com/raydium-io/raydium-idl/blob/master/raydium_launchpad/raydium_launchpad.json",
    },
];

#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub(crate) product: &'static str,
    pub(crate) url: &'static str,
    pub(crate) kind: SourceKind,
}

#[derive(Clone, Copy)]
pub(crate) enum SourceKind {
    Idl,
    RustEnum,
}

#[derive(Clone, Copy)]
pub(crate) struct InstructionSource {
    pub(crate) protocol: &'static str,
    pub(crate) url: &'static str,
    pub(crate) source: &'static str,
}
