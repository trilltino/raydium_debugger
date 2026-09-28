//! Text formatter for the CLI and raw-report UI tab.
//!
//! The debugger is JSON-first, but terminal users and the frontend Raw tab need
//! a stable human-readable report. This file renders `TransactionDebugInfo`
//! without changing the underlying diagnosis or adding any extra heuristics.

use crate::failures::program_label;
use std::fmt::Write as _;

use super::types::TransactionDebugInfo;

/// Renders a compact terminal report from structured debug output.
pub fn format_debug_info(info: &TransactionDebugInfo) -> String {
    let mut out = String::new();
    write_header(&mut out, info);
    write_rpc(&mut out, info);
    write_experience(&mut out, info);
    write_failure_summary(&mut out, info);
    write_root_cause(&mut out, info);
    write_metadata(&mut out, info);
    write_rent(&mut out, info);
    write_programs(&mut out, info);
    write_logs(&mut out, info);
    out
}

fn write_header(out: &mut String, info: &TransactionDebugInfo) {
    let _ = writeln!(out, "Transaction: {}", info.signature);
    let _ = writeln!(out, "Slot: {}", info.slot);

    if let Some(ts) = info.timestamp {
        let datetime = chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "Unknown".to_string());
        let _ = writeln!(out, "Time: {datetime}");
    }

    let status = if info.success { "Success" } else { "Failed" };
    let _ = writeln!(out, "Status: {status}");
}

fn write_rpc(out: &mut String, info: &TransactionDebugInfo) {
    if !info.rpc.endpoint.is_empty() {
        let _ = writeln!(out, "\nRPC Endpoint: {}", info.rpc.endpoint);
        let _ = writeln!(out, "Fallback Used: {}", info.rpc.fallback_used);
    }
}

fn write_experience(out: &mut String, info: &TransactionDebugInfo) {
    let ux = &info.experience;
    let headline = info
        .failure
        .as_ref()
        .and_then(|failure| failure.plain_title.as_deref())
        .unwrap_or(&ux.headline);
    let message = info
        .failure
        .as_ref()
        .and_then(|failure| failure.plain_explanation.as_deref())
        .unwrap_or(&ux.message);
    let primary_action = info
        .failure
        .as_ref()
        .and_then(|failure| failure.primary_action.as_deref())
        .unwrap_or(&ux.next_step);
    let _ = writeln!(out, "\nSummary:\n  {headline}");
    let _ = writeln!(out, "  {message}");
    let _ = writeln!(out, "  Primary action: {primary_action}");
    if !ux.detail_badges.is_empty() {
        let _ = writeln!(out, "  Context: {}", ux.detail_badges.join(" | "));
    }
}

fn write_failure_summary(out: &mut String, info: &TransactionDebugInfo) {
    if let Some(ref error) = info.error {
        let _ = writeln!(out, "\nError:\n  {error}");
    }

    if let Some(ix) = &info.failing_instruction {
        let _ = writeln!(
            out,
            "\nFailing Instruction:\n  #{} {} ({})\n",
            ix.index, ix.program_label, ix.program_id
        );
    }

    if let Some(failure) = &info.failure {
        let title = failure.plain_title.as_deref().unwrap_or(&failure.title);
        let message = failure
            .plain_explanation
            .as_deref()
            .unwrap_or(&failure.user_message);
        let _ = writeln!(out, "\nFailure Message:\n  {title}");
        let _ = writeln!(out, "  {message}");
        if let Some(code_hex) = &failure.code_hex {
            let _ = write!(out, "  Code: {code_hex}");
            if let Some(code_decimal) = failure.code_decimal {
                let _ = write!(out, " ({code_decimal})");
            }
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "  Category: {} | Confidence: {}\n",
            failure.category, failure.confidence
        );
    }

    if let Some(product) = &info.raydium_product {
        let _ = writeln!(out, "\nRaydium Product: {:?}", product.product);
        if let Some(phase) = &product.phase {
            let _ = writeln!(out, "Raydium Phase: {phase:?}");
        }
    }
}

fn write_root_cause(out: &mut String, info: &TransactionDebugInfo) {
    let _ = writeln!(
        out,
        "\nRoot Cause:\n  {} - {}\n",
        info.root_cause.category, info.root_cause.summary
    );

    if let Some(cu) = info.compute_units_consumed {
        let _ = writeln!(out, "\nCompute Units: {cu}");
    }
}

fn write_metadata(out: &mut String, info: &TransactionDebugInfo) {
    let meta = &info.metadata;

    let _ = writeln!(out, "\nTransaction Version: {}", meta.transaction_version);
    let _ = writeln!(
        out,
        "RPC maxSupportedTransactionVersion: {}\n",
        meta.max_supported_transaction_version
    );
    if let Some(size) = meta.transaction_size_bytes {
        let _ = writeln!(out, "Transaction Size: {size} bytes");
    }
    let _ = writeln!(
        out,
        "Accounts: {} static, {} ALT writable, {} ALT readonly\n",
        meta.static_account_count,
        meta.loaded_writable_account_count,
        meta.loaded_readonly_account_count
    );
    if meta.transaction_version == "v1" {
        let _ = writeln!(
            out,
            "V1 Resource Limits: compute_unit_limit={}, loaded_accounts_data_size_limit={}\n",
            meta.v1_compute_unit_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "missing".to_string()),
            meta.v1_loaded_accounts_data_size_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "missing".to_string())
        );
    }
    for warning in &meta.fetch_warnings {
        let _ = writeln!(out, "Fetch Warning: {warning}");
    }

    let _ = writeln!(
        out,
        "Fee Paid: {} lamports ({} SOL)\n",
        info.fee_paid,
        info.fee_paid as f64 / 1_000_000_000.0
    );
}

fn write_rent(out: &mut String, info: &TransactionDebugInfo) {
    if !info.rent_evidence.is_empty() {
        out.push_str("\nRent Evidence:\n");
        for rent in info.rent_evidence.iter().take(20) {
            let min = rent
                .rent_exempt_minimum
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unavailable".to_string());
            let surplus = rent
                .reclaimable_surplus
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unavailable".to_string());
            let _ = writeln!(
                out,
                "  {}: lamports={} data_len={} rent_min={} surplus={}\n",
                rent.pubkey, rent.lamports, rent.data_len, min, surplus
            );
        }
    }
}

fn write_programs(out: &mut String, info: &TransactionDebugInfo) {
    if !info.program_ids.is_empty() {
        out.push_str("\nPrograms Invoked:\n");
        for (idx, program_id) in info.program_ids.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {}. {} ({})\n",
                idx + 1,
                program_label(program_id),
                program_id
            );
        }
    }
}

fn write_logs(out: &mut String, info: &TransactionDebugInfo) {
    if !info.logs.is_empty() {
        out.push_str("\nProgram Logs:\n");
        for log in &info.logs {
            let _ = writeln!(out, "  {log}");
        }
    }
}
