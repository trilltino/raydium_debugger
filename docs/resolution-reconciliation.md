# Resolution reconciliation

The private support database already contains imported messages and reply-linked candidate cases. Reconciliation reads those current cases; it does not reimport the HTML archive or approve guidance.

```powershell
cargo run -p xtask -- support-knowledge candidates reconcile .raydium-debugger/support-knowledge.sqlite
cargo run -p xtask -- support-knowledge candidates resolution-report .raydium-debugger/support-knowledge.sqlite > .raydium-debugger/resolution-report.tsv
cargo run -p xtask -- support-knowledge candidates resolution-show <case-id> .raydium-debugger/support-knowledge.sqlite
cargo run -p xtask -- support-knowledge candidates show <case-id> .raydium-debugger/support-knowledge.sqlite
```

The report assigns a *suggested* tier: `confirmed` for an apparent later reporter confirmation, `team_fixed` for an apparent team fix statement, `proposed` for a possible remedy without a detected outcome, and `unknown` when no rule matches. `not_scanned` means reconciliation has not run with the current rule version. These are review priorities, not verified facts. A `confirmed` thread may say only that the issue ended without explaining how; `unknown` does not mean it remained unresolved. The detail command lists the message revision IDs behind the suggestions; `show` displays the whole private conversation.

After checking the complete conversation and any relevant transaction evidence, a reviewer records the outcome and source revision IDs. Cite the action and later confirmation for `confirmed`; cite the team statement for `team_fixed`. If the thread only offers a suggestion, record `proposed` or `unknown` and do not approve it as a resolved incident.

```powershell
cargo run -p xtask -- support-knowledge candidates verify-resolution <case-id> <confirmed|team_fixed|proposed|unknown> <revision-id,revision-id> "review rationale" <reviewer> .raydium-debugger/support-knowledge.sqlite
cargo run -p xtask -- support-knowledge candidates annotate <case-id> <product> <domain> "sanitized problem summary" "sanitized historical resolution" .raydium-debugger/support-knowledge.sqlite
cargo run -p xtask -- support-knowledge candidates review <case-id> approve "evidence and wording checked" .raydium-debugger/support-knowledge.sqlite <reviewer>
cargo run -p xtask -- support-knowledge compile .raydium-debugger/support-knowledge.sqlite .raydium-debugger/knowledge/incidents.generated.json
```

Approval and compilation accept only reviewed `confirmed` or `team_fixed` outcomes with evidence still in the current case. A changed source revision or case membership makes a prior review stale; inspect and verify it again. Keep wording faithful to the evidence: a team statement alone does not prove the reporter succeeded, and a confirmation after multiple changes does not isolate one cause. The compiled artifact contains sanitized summaries and resolutions, never private message text, sender names, addresses, or source revision IDs.
