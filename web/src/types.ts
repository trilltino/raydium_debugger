export interface DebugRequest {
  signature: string;
  rpc_url?: string | null;
  no_fallback?: boolean;
  cluster?: 'devnet' | 'mainnet' | null;
  data_mode?: 'auto' | 'rpc_only' | 'rpc_plus_grpc' | null;
}

export interface DebugResponse {
  info: TransactionDebugInfo;
  formatted_text: string;
}

export interface DiagnosticResponse {
  observation: ObservationStatus;
  diagnosis: Diagnosis;
  transaction: TransactionDebugInfo | null;
  formatted_text: string;
}

export interface ObservationStatus {
  status: string;
  cluster: string | null;
  providers_queried: string[];
  evidence: string[];
  hypotheses: string[];
}

export interface Diagnosis {
  title: string;
  explanation: string;
  primary_action: string;
  evidence: string[];
  confidence: string;
  category: string;
  copy_markdown: string;
}

export interface ProviderStatus {
  name: string;
  triton: {
    devnet_rpc: string | null;
    mainnet_rpc: string | null;
    devnet_fallback_rpc: string | null;
    mainnet_fallback_rpc: string | null;
    devnet_configured: boolean;
    mainnet_configured: boolean;
    devnet_grpc_available: boolean;
    mainnet_grpc_available: boolean;
  };
}

export interface IntegratorRecord {
  id: string;
  name: string;
  slug: string;
  contact: string | null;
  notes: string | null;
  created_at: number;
  updated_at: number;
  signatures: SavedSignature[];
}

export interface CasebookRecord {
  id: string;
  integrator_id: string;
  name: string;
  description: string | null;
  tags: string[];
  owner_contact: string | null;
  created_at: number;
  updated_at: number;
  signatures: SavedSignature[];
}

export interface SavedSignature {
  id: string;
  signature: string;
  cluster: 'devnet' | 'mainnet';
  label: string | null;
  reason: string | null;
  outcome: string | null;
  product: string | null;
  failure_category: string | null;
  failure_code: string | null;
  tags: string[];
  notes: string | null;
  pinned: boolean;
  created_at: number;
  updated_at: number;
  last_debugged_at: number | null;
}

export interface CreateIntegratorRequest {
  name: string;
  contact?: string | null;
  notes?: string | null;
}

export interface SaveSignatureRequest {
  signature: string;
  cluster: 'devnet' | 'mainnet';
  label?: string | null;
  reason?: string | null;
  outcome?: string | null;
  product?: string | null;
  failure_category?: string | null;
  failure_code?: string | null;
  tags?: string[];
  notes?: string | null;
  pinned?: boolean | null;
}

export interface CreateCasebookRequest {
  name: string;
  description?: string | null;
  tags?: string[] | null;
  owner_contact?: string | null;
}

export interface AiAskRequest {
  info: TransactionDebugInfo;
  question: string;
  model?: string | null;
}

export interface AiResponse {
  model: string;
  answer: string;
}

export interface TransactionDebugInfo {
  signature: string;
  slot: number;
  slot_exact: string;
  timestamp: number | null;
  status: TransactionStatusSummary;
  success: boolean;
  error: string | null;
  metadata: TransactionMetadata;
  outer_instructions: InstructionDebugInfo[];
  failing_instruction: InstructionDebugInfo | null;
  cpi_tree: CpiFrame[];
  decoded_instructions: DecodedInstruction[];
  execution_tree: ExecutionNode[];
  accounts: AccountEvidence[];
  rent_evidence: RentEvidence[];
  logs: string[];
  account_changes: AccountChange[];
  compute_units_consumed: number | null;
  compute_units_consumed_exact: string | null;
  fee_paid: number;
  fee_paid_exact: string;
  program_ids: string[];
  program_context: TransactionProgramContext;
  rpc: RpcDebugInfo;
  provider: ProviderDebugInfo;
  raydium_product: RaydiumProductDebug | null;
  raydium_context: RaydiumContext | null;
  freshness: FreshnessInfo;
  compute_budget: ComputeBudgetInfo;
  compute_attribution: ComputeAttribution[];
  resource_usage: ResourceUsage;
  experience: ExperienceSummary;
  failure: StandardizedFailure | null;
  root_cause: RootCause;
  recommended_actions: string[];
}

export interface TransactionStatusSummary {
  landed: boolean;
  finalized: boolean;
  confirmation_status?: string | null;
  finalized_known?: boolean;
  err: string | null;
}

export interface TransactionMetadata {
  payer: string | null;
  recent_blockhash: string | null;
  required_signatures: number;
  readonly_signed_accounts: number;
  readonly_unsigned_accounts: number;
  transaction_version: string;
  max_supported_transaction_version: number;
  rpc_v1_fetch_supported: boolean;
  transaction_size_bytes: number | null;
  transaction_size_bytes_exact: string | null;
  uses_address_lookup_tables: boolean;
  static_account_count: number;
  resolved_account_count: number;
  loaded_writable_account_count: number;
  loaded_readonly_account_count: number;
  v1_compute_unit_limit: number | null;
  v1_compute_unit_limit_exact: string | null;
  v1_loaded_accounts_data_size_limit: number | null;
  v1_loaded_accounts_data_size_limit_exact: string | null;
  fetch_warnings: string[];
}

export interface InstructionDebugInfo {
  index: number;
  program_id: string;
  program_label: string;
  account_indexes: number[];
  accounts: InstructionAccountMeta[];
  data_base58: string;
  discriminator: string | null;
  error: string | null;
}

export interface TransactionProgramContext {
  invoked_programs: ProgramInvocationSummary[];
  token_programs: string[];
  system_programs: string[];
  raydium_programs: string[];
}

export interface ProgramInvocationSummary {
  program_id: string;
  program_label: string;
}

export interface DecodedInstruction {
  id: string;
  outer_instruction_index: number;
  inner_instruction_index: number | null;
  invocation_kind: string;
  program_id: string;
  program_label: string;
  accounts: string[];
  account_indexes: number[];
  raw_data_base58: string;
  discriminator: string | null;
  semantic_decode: InstructionSemanticDecode | null;
}

export interface InstructionSemanticDecode {
  protocol: string;
  instruction_name: string;
  source: string;
  confidence: string;
  arguments: DecodedArgument[];
  accounts: DecodedAccountRole[];
  remaining_accounts: string[];
}

export interface DecodedArgument {
  name: string;
  value: string;
}

export interface DecodedAccountRole {
  role: string;
  pubkey: string;
  account_index: number | null;
  source: string;
  confidence: string;
}

export interface ExecutionNode {
  id: string;
  parent_id: string | null;
  depth: number;
  outer_instruction_index: number | null;
  inner_instruction_index: number | null;
  decoded_instruction_id: string | null;
  program_id: string;
  program_label: string;
  status: string;
  failed: boolean;
  log_start: number;
  log_end: number;
  logs: string[];
  token_instruction: TokenInstructionDetails | null;
  compute: ComputeAttribution | null;
}

export interface InstructionAccountMeta {
  index: number;
  pubkey: string;
  signer: boolean;
  writable: boolean;
  owner: string | null;
  owner_label: string | null;
  raydium_role: string | null;
  raydium_role_confidence: string | null;
}

export interface AccountEvidence {
  index: number;
  pubkey: string;
  owner: string | null;
  owner_label: string | null;
  executable: boolean | null;
  lamports_pre: number | null;
  lamports_pre_exact: string | null;
  lamports_post: number | null;
  lamports_post_exact: string | null;
  lamports_change: number | null;
  lamports_change_exact: string | null;
  data_len: number | null;
  signer: boolean;
  writable: boolean;
}

export interface RentEvidence {
  pubkey: string;
  lamports: number;
  lamports_exact: string;
  data_len: number;
  rent_exempt_minimum: number | null;
  rent_exempt_minimum_exact: string | null;
  reclaimable_surplus: number | null;
  reclaimable_surplus_exact: string | null;
  below_rent_exempt: boolean | null;
}

export interface CpiFrame {
  depth: number;
  program_id: string;
  program_label: string;
  status: string;
  message: string;
  token_instruction: TokenInstructionDetails | null;
}

export interface TokenInstructionDetails {
  instruction_type: string;
  parameters: TokenInstructionParameter[];
}

export interface TokenInstructionParameter {
  name: string;
  value: string;
}

export interface AccountChange {
  pubkey: string;
  pre_balance: number;
  pre_balance_exact: string;
  post_balance: number;
  post_balance_exact: string;
  change: number;
  change_exact: string;
}

export interface RpcDebugInfo {
  endpoint: string;
  fallback_endpoint: string | null;
  fallback_used: boolean;
}

export interface ProviderDebugInfo {
  name: string;
  cluster: string | null;
  rpc_endpoint_redacted: string;
  fallback_endpoint_redacted: string | null;
  grpc_available: boolean;
  grpc_used: boolean;
  rate_limit: RateLimitDebugInfo | null;
  warnings: string[];
}

export interface RateLimitDebugInfo {
  limited: boolean;
  retry_after_seconds: number | null;
  details: string[];
}

export interface FreshnessInfo {
  execution_slot: number;
  execution_slot_exact: string;
  current_slot: number | null;
  current_slot_exact: string | null;
  slot_age: number | null;
  slot_age_exact: string | null;
  note: string;
}

export interface ComputeBudgetInfo {
  compute_unit_limit: number | null;
  compute_unit_limit_exact: string | null;
  compute_unit_price_micro_lamports: number | null;
  compute_unit_price_micro_lamports_exact: string | null;
  loaded_accounts_data_size_limit: number | null;
  loaded_accounts_data_size_limit_exact: string | null;
  heap_frame_bytes: number | null;
  heap_frame_bytes_exact: string | null;
  deprecated_request_units: DeprecatedRequestUnits | null;
}

export interface DeprecatedRequestUnits {
  units: number;
  units_exact: string;
  additional_fee_lamports: number;
  additional_fee_lamports_exact: string;
}

export interface ComputeAttribution {
  program_id: string;
  program_label: string;
  consumed: number;
  consumed_exact: string;
  limit: number;
  limit_exact: string;
  source_log: string;
}

export interface ResourceUsage {
  execution_compute: ExecutionComputeUsage | null;
  loaded_account_data: LoadedAccountDataUsage | null;
  transaction_size: TransactionSizeUsage | null;
}

export interface ExecutionComputeUsage {
  consumed: number | null;
  consumed_exact: string | null;
  limit: number | null;
  limit_exact: string | null;
  price_micro_lamports: number | null;
  price_micro_lamports_exact: string | null;
}

export interface LoadedAccountDataUsage {
  limit: number | null;
  limit_exact: string | null;
  observed_account_data_bytes: number | null;
  observed_account_data_bytes_exact: string | null;
}

export interface TransactionSizeUsage {
  serialized_size_bytes: number | null;
  serialized_size_bytes_exact: string | null;
  uses_address_lookup_tables: boolean;
  note: string;
}

export interface RaydiumProductDebug {
  product: string;
  phase: string | null;
  matched_program_ids: string[];
  evidence: string[];
}

export interface RaydiumContext {
  product: string | null;
  phase: string | null;
  decoded_instructions: DecodedInstruction[];
  instruction_roles: RaydiumInstructionRole[];
  account_roles: RaydiumAccountRole[];
  token_movements: TokenMovement[];
  swap_summary: RaydiumSwapSummary | null;
  warnings: string[];
}

export interface RaydiumInstructionRole {
  instruction_index: number;
  program_id: string;
  instruction_name: string;
  role_source: string;
  confidence: string;
}

export interface RaydiumAccountRole {
  instruction_index: number;
  account_index: number;
  pubkey: string;
  role: string;
  mint: string | null;
  owner: string | null;
  writable: boolean;
  signer: boolean;
  source: string;
  confidence: string;
}

export interface TokenMovement {
  account_index: number;
  account: string | null;
  mint: string;
  owner: string | null;
  program_id: string | null;
  pre_amount_raw: string;
  post_amount_raw: string;
  delta_raw: string;
  decimals: number;
  ui_pre_amount: string;
  ui_post_amount: string;
}

export interface RaydiumSwapSummary {
  route_kind: string;
  input_mint: string | null;
  output_mint: string | null;
  input_amount_raw: string | null;
  output_amount_raw: string | null;
  min_output_raw: string | null;
  max_input_raw: string | null;
  slippage_result: string | null;
  transfer_fee_notes: string[];
  route_leg_status: string[];
}

export interface ExperienceSummary {
  tone: string;
  status_label: string;
  headline: string;
  message: string;
  next_step: string;
  detail_badges: string[];
}

export interface StandardizedFailure {
  source: string;
  program_id: string | null;
  program_label: string | null;
  instruction_index: number | null;
  code_decimal: number | null;
  code_hex: string | null;
  name: string | null;
  title: string;
  user_message: string;
  technical_message: string;
  category: string;
  severity: string;
  confidence: string;
  evidence: string[];
  suggested_actions: string[];
  decode_status: string;
  decode_attempts: string[];
  missing_artifact: string | null;
  plain_title: string | null;
  plain_explanation: string | null;
  primary_action: string | null;
  action_checklist: string[];
  evidence_summary: string[];
  decode_explanation: string | null;
}

export interface RootCause {
  category: string;
  summary: string;
  evidence: string[];
}
