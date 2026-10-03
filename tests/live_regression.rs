use raydium_debugger::{run_debug_request_blocking, DebugRequest, RpcCluster};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LiveManifest {
    case: Vec<LiveCase>,
}

#[derive(Debug, Deserialize)]
struct LiveCase {
    name: String,
    signature: String,
    cluster: String,
    success: bool,
    program_label: Option<String>,
    required_code: Option<String>,
    required_text: Option<Vec<String>>,
}

#[test]
fn live_triton_regressions_from_manifest() {
    if std::env::var("RUN_LIVE_E2E").ok().as_deref() != Some("1") {
        eprintln!("skipping live Triton regressions; set RUN_LIVE_E2E=1");
        return;
    }

    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();

    let manifest: LiveManifest =
        toml::from_str(include_str!("live_signatures.toml")).expect("valid live manifest");
    for case in manifest.case {
        let response = run_debug_request_blocking(DebugRequest {
            signature: case.signature.clone(),
            cluster: Some(match case.cluster.as_str() {
                "devnet" => RpcCluster::Devnet,
                "mainnet" => RpcCluster::Mainnet,
                other => panic!("unsupported live cluster {other}"),
            }),
            ..DebugRequest::default()
        })
        .unwrap_or_else(|error| panic!("{} failed to debug: {error:#}", case.name));

        assert_eq!(response.info.success, case.success, "{}", case.name);
        if let Some(label) = case.program_label {
            assert!(
                response
                    .info
                    .failure
                    .as_ref()
                    .and_then(|failure| failure.program_label.as_deref())
                    == Some(label.as_str()),
                "{} expected program label {label}",
                case.name
            );
        }
        if let Some(code) = case.required_code {
            assert!(
                response
                    .info
                    .failure
                    .as_ref()
                    .and_then(|failure| failure.code_hex.as_deref())
                    == Some(code.as_str()),
                "{} expected code {code}",
                case.name
            );
        }
        for text in case.required_text.unwrap_or_default() {
            assert!(
                response.formatted_text.contains(&text)
                    || response.info.experience.headline.contains(&text)
                    || response.info.experience.message.contains(&text)
                    || response.info.failure.as_ref().is_some_and(|failure| {
                        failure
                            .plain_title
                            .as_deref()
                            .is_some_and(|value| value.contains(&text))
                            || failure
                                .plain_explanation
                                .as_deref()
                                .is_some_and(|value| value.contains(&text))
                    }),
                "{} missing required text {text}",
                case.name
            );
        }
    }
}
