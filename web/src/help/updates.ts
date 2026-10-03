export type Product = 'CLMM' | 'CPMM' | 'AMM v4' | 'Stable AMM' | 'LaunchLab';
export type Update = {
  id: string;
  title: string;
  products: Product[];
  announced: string;
  confirmed?: string;
  status: 'Confirmed deployed' | 'Announcement only';
  compatibility: string;
  summary: string;
  audience: string;
  action: string;
  technical: string[];
  check: string;
  messages: number[];
  changelogs: string[];
};

const changelog = (slug: string) => `https://docs.raydium.io/reference/changelog/${slug}`;

// Summarized from the public announcements export through 28 September 2026.
// Follow-up messages confirm only the upgrades they explicitly refer to.
export const updates: Update[] = [
  {
    id: 'clmm-anchor', title: 'CLMM: framework upgrade and excess SOL recovery', products: ['CLMM'],
    announced: '2026-09-28', status: 'Announcement only', compatibility: 'No transaction migration; CPI tooling changes',
    summary: 'A September 30 upgrade was announced. Ordinary swaps, liquidity management and pool readers keep the same accounts and calculations. Developers calling CLMM from their own Rust programs need to update their build tools.',
    audience: 'CLMM integrators, Rust CPI developers and IDL users.',
    action: 'Refresh the IDL and CPI dependencies. Confirm the deployed program before using the new instruction; this export does not confirm the September 30 deployment.',
    technical: [
      'The announcement says all instruction accounts, arguments, math and account layouts are unchanged. New IDL entries: admin-only CollectExcessLamports and error 6052 LamportsCalculateError. Codes 6000–6051 keep their meanings.',
      'Rust CPI guidance: anchor-lang and anchor-spl =1.0.2 on Agave 3.1.10. TypeScript guidance replaces @coral-xyz/anchor 0.32.1 with @anchor-lang/core 1.0.2. CPMM and CLMM pin the same Anchor version.',
      'Read owner and fund_owner from AmmConfig rather than hardcoding them. New configs receive fixed fee-owner keys; existing configs are untouched. Announcement tested branch chore/upgrade-anchor, head a72f9e1.',
    ],
    check: 'Check the transaction slot and deployed program version, then the IDL used to decode it. A missing new instruction in an older IDL is a decoder gap, not proof of a transaction failure. Verify fee owners from the actual config account.',
    messages: [38], changelogs: [changelog('2026-09-30-clmm-anchor-1-and-excess-lamports'), 'https://docs.raydium.io/sdk-api/rust-cpi'],
  },
  {
    id: 'september-frameworks', title: 'CPMM, AMM v4 and LaunchLab: maintenance upgrades', products: ['CPMM', 'AMM v4', 'LaunchLab'],
    announced: '2026-09-10', status: 'Announcement only', compatibility: 'Generally non-breaking; migration indexers need attention',
    summary: 'Three maintenance updates were announced, including newer program frameworks and ways to recover excess SOL held by accounts. LaunchLab indexers tracking migrations into AMM v4 need to adjust the account ordering they read.',
    audience: 'Program integrators and indexers tracking LaunchLab migrations.',
    action: 'Review the changelog for your product. Update LaunchLab migration account ordering; a planned September 11, 10:00 UTC upgrade is not a deployment confirmation.',
    technical: ['The announcement describes Anchor upgrades for CPMM/LaunchLab and a Solana 3 upgrade for AMM v4. It calls the changes primarily optimizations and non-code-breaking, but explicitly flags LaunchLab AMM v4 migration account ordering.', 'CPMM and AMM v4 were scheduled “shortly”; LaunchLab was scheduled for September 11 at 10:00 UTC. No follow-up deployment confirmation for these events appears in this snapshot.'],
    check: 'If an indexer reports the wrong migrated pool, compare its positional account mapping with the actual LaunchLab instruction and matching changelog before blaming on-chain execution.',
    messages: [37], changelogs: [changelog('2026-09-09-launchlab-anchor-1-and-excess-lamports'), changelog('2026-09-09-cpmm-anchor-1-and-excess-lamports'), changelog('2026-09-09-amm-v4-solana-3-and-excess-lamports')],
  },
  {
    id: 'launchlab-curve', title: 'LaunchLab: more flexible launch rules', products: ['LaunchLab'],
    announced: '2026-09-01', confirmed: '2026-09-02', status: 'Confirmed deployed', compatibility: 'Builder action required; config decoder may change',
    summary: 'Platforms can set ranges and combinations of rules for token launches instead of allowing only exact parameter matches. Launch builders must include the rules account every time, even when a platform has not enabled restrictions yet.',
    audience: 'InitializeV2 / InitializeWithToken2022 builders, platform operators and PlatformConfig readers.',
    action: 'Add the rule PDA to remaining_accounts unconditionally, including when it does not exist. Update decoders that expect the old trailing curve_params vector. Operators must deliberately enable their migrated rules.',
    technical: ['PlatformCurveRule replaces PlatformConfig.curve_params. Rules support Gte/Lte/Eq/Neq, ORed groups and derived rates. PDA seeds: ["platform_curve_rule", platform_config, global_config].', 'The September 1 announcement reported restrict_curve_param = 0 everywhere at that time. That was a snapshot, not a permanent guarantee: an admin may enable restrictions. Builders missing the PDA may then fail.', 'Readers stopping before padding remain unaffected; readers expecting the removed trailing vector can misparse. Old whitelist entries were translated, but restrictions stay off until restrict_curve_param = 1. Mainnet deployment was confirmed September 2.'],
    check: 'For a rejected launch, inspect remaining accounts, derive the rule PDA from the actual platform/global configs and read the restriction flag. For strange decoded config values, check the decoder layout.',
    messages: [35, 36], changelogs: [changelog('2026-08-31-launchlab-platform-curve-rules'), 'https://docs.raydium.io/products/launchlab/curve-rules#test-on-devnet-first'],
  },
  {
    id: 'withheld-authority', title: 'LaunchLab: platforms can claim transfer fees earlier', products: ['LaunchLab'],
    announced: '2026-08-27', status: 'Announcement only', compatibility: 'Authority timing changes',
    summary: 'An upgrade was announced to give platforms the authority to withdraw withheld Token-2022 transfer fees when a token is created, rather than waiting until its migration.',
    audience: 'Platforms creating Token-2022 assets with transfer fees.',
    action: 'Review authority handling at mint creation. The linked changelog is dated August 28, but the export contains no explicit deployment confirmation.',
    technical: ['The withdraw-withheld authority moves from migration time to mint creation time. This enables the platform to claim transfer fees from the creation event. The announcement does not specify a wider set of builder or layout changes.'],
    check: 'Read the mint extension and actual withdraw-withheld authority. Compare creation and migration transactions rather than assuming the platform gains authority only after migration.',
    messages: [34], changelogs: [changelog('2026-08-28-launchlab-token2022-withheld-authority')],
  },
  {
    id: 'fee-rate', title: 'LaunchLab: higher platform fee ceiling announced', products: ['LaunchLab'],
    announced: '2026-08-26', status: 'Announcement only', compatibility: 'Announced as non-breaking',
    summary: 'LaunchLab announced that platforms would be able to set a higher maximum fee on the bonding curve. The message does not give the new maximum or confirm deployment.',
    audience: 'Platform operators and applications displaying bonding-curve fees.',
    action: 'Read the active platform configuration and verify the deployed limits. Do not infer a new fee ceiling from this announcement alone.',
    technical: ['The announcement names fee_rate, says the maximum will increase and describes no other changes. There is no linked changelog, numeric maximum or completion message in this snapshot.'],
    check: 'Compare the configured fee rate and transaction amounts with the expected quote. A higher platform fee may change output without producing a program error.',
    messages: [33], changelogs: [],
  },
  {
    id: 'quote-mints', title: 'LaunchLab: Token-2022 quote assets', products: ['LaunchLab'],
    announced: '2026-08-24', confirmed: '2026-08-24', status: 'Confirmed deployed', compatibility: 'Additional supported quote assets',
    summary: 'Token-2022 assets were introduced as quote tokens—the asset used to price and pay for a launch—on bonding curves and after pool migration. A later message that day confirmed an additional upgrade to who can create and configure those quotes.',
    audience: 'LaunchLab quote-config readers, token launch builders and platform operators.',
    action: 'Discover quote configurations instead of assuming a fixed token list. Check the authority rules for creating and configuring Token-2022 quotes.',
    technical: ['The first August 24 message announced the Token-2022 quote feature. The 15:02 UTC follow-up explicitly says an additional upgrade was deployed addressing which accounts can create and configure new Token-2022 quote mints.', 'A public SDK demo shows how to fetch newly created quote configurations through gRPC. Token-2022 support still requires checking the relevant mint extensions and instruction path.'],
    check: 'Identify the actual quote mint, its token program, quote configuration and creating authority. Diagnose a token CPI using its own error and accounts rather than treating every Token-2022 asset alike.',
    messages: [31, 32], changelogs: [changelog('2026-08-24-launchlab-token2022-quote-mint'), 'https://github.com/raydium-io/raydium-sdk-V2-demo/blob/master/src/grpc/launchpadPoolInfo.ts'],
  },
  {
    id: 'cpmm-fees', title: 'CPMM: anyone can trigger creator-fee collection', products: ['CPMM'],
    announced: '2026-08-14', confirmed: '2026-08-17', status: 'Confirmed deployed', compatibility: 'Additive; existing collection still works',
    summary: 'Someone other than the pool creator can pay for a transaction that collects creator fees. The money still goes to the creator’s designated token accounts; the caller cannot redirect it.',
    audience: 'CPMM fee collectors, keeper services and delegated permission operators.',
    action: 'Existing creator-signed collection can stay as-is. Use the new permissionless instruction only when useful; account for the API updates that were still pending at deployment confirmation.',
    technical: ['CollectCreatorFeePermissionless sweeps both fee counters all-or-nothing into canonical associated token accounts of pool_state.pool_creator. CollectCreatorFee is unchanged.', 'CreatePermissionPda also accepts a dedicated delegated owner; ClosePermissionPda remains admin-only. August 14 follow-up gave integrators until August 17; completion was confirmed August 17 with API endpoint updates pending.'],
    check: 'Distinguish the transaction signer paying for the sweep from the recipient. Verify both fee counters and the creator’s canonical token accounts before labeling a third-party call suspicious.',
    messages: [28, 29, 30], changelogs: [changelog('2026-08-17-cpmm-permissionless-creator-fee-collection')],
  },
  {
    id: 'clmm-freezing', title: 'CLMM: restricted-asset position NFTs may be frozen', products: ['CLMM'],
    announced: '2026-08-14', confirmed: '2026-08-17', status: 'Confirmed deployed', compatibility: 'Conditional breaking change for closing frozen positions',
    summary: 'For certain restricted-issuer assets, a new position’s ownership token can be frozen to stop transfers. Liquidity management and fee collection still work. Closing a frozen position needs an extra pool account.',
    audience: 'Position builders and integrators supporting restricted-issuer CLMM pools.',
    action: 'For a frozen position NFT, pass personal_position.pool_id as remaining-accounts[0] to ClosePosition. Ordinary pools and existing positions are unaffected by the stated freeze trigger.',
    technical: ['All new position NFT mints name the pool as freeze authority, but accounts are unfrozen by default. Freezing triggers only through OpenPositionV2 or OpenPositionWithToken22Nft when a vault mint’s freeze authority matches the hardcoded restricted-issuer list (for example, Superstate). OpenPosition V1 does not trigger it.', 'Frozen NFTs block transfers/owner changes with native SPL AccountFrozen; liquidity and fee/reward collection remain available. Old close builders on frozen NFTs fail with AccountLack without the extra pool account.', 'Deployment was confirmed August 17 after the August 14 timing follow-up. API endpoint updates were pending at confirmation.'],
    check: 'Read the NFT token-account frozen state, opening instruction version and vault-mint freeze authorities. For AccountLack on close, inspect the first remaining account; do not treat every position as frozen.',
    messages: [28, 29, 30], changelogs: [changelog('2026-08-17-clmm-restricted-position-nft-freeze')],
  },
  {
    id: 'launchlab-cpmm', title: 'LaunchLab: new launches migrate to CPMM', products: ['LaunchLab'],
    announced: '2026-08-14', confirmed: '2026-08-17', status: 'Confirmed deployed', compatibility: 'Conditional breaking changes for launch/config builders',
    summary: 'New launches must target CPMM pools. Existing launches already targeting AMM v4 can still migrate there. Platform authorization and ownership of locked liquidity also changed, so platforms need to update their configuration builders.',
    audience: 'LaunchLab builders, platform config operators and IDL users.',
    action: 'Set migrate_type = 1 for new launches and creator_scale = 0 when creating/updating platformConfig. Refresh the IDL and replace old global-access authorization accounts.',
    technical: ['Initialize, InitializeV2 and InitializeWithToken2022 require migrate_type = 1; new AMM v4-bound launches return MigrateTypeNotMatch. Legacy migrate_type = 0 pools continue through MigrateToAmm.', 'platform_scale + creator_scale now combine into a platform-owned Fee Key; burn_scale still burns. Creator bonding-curve and CPMM fees are unaffected.', 'restrict_global_config + PlatformAllowConfig replace requires_platform_auth / PlatformGlobalAccess. Old global-access PDAs do not satisfy the new check and are not automatically migrated. Error 6022 becomes InvalidPlatformAllowConfig; 6023 is removed.', 'Dedicated authorities may create GlobalConfig quote-mint configurations via CreateConfig, not update them. The mint itself was not limited on-chain to SOL/USDC/RAY/USD1. Deployment confirmed August 17; API updates still pending.'],
    check: 'Check the launch’s creation date and migrate_type before applying new-launch rules to legacy pools. Match error codes to the correct LaunchLab IDL and inspect PlatformAllowConfig and LP ownership separately from creator fees.',
    messages: [28, 29, 30], changelogs: [changelog('2026-08-17-launchlab-cpmm-only-platform-config')],
  },
  {
    id: 'permissioned-pools', title: 'CLMM: multiple permissioned pools and frozen-account checks', products: ['CLMM'],
    announced: '2026-07-30', confirmed: '2026-07-30', status: 'Confirmed deployed', compatibility: 'Announced as backwards-compatible',
    summary: 'Whitelisted creators can make multiple pools for the same token pair. Limit-order creation also checks the receiving token account, so frozen accounts can be rejected before an order is opened.',
    audience: 'Permissioned pool creators and limit-order builders.',
    action: 'Use the correct seeded pool address and include the output-side accounts required by the updated OpenLimitOrder path.',
    technical: ['CreatePermissionedPool includes seed_index in the pool PDA, allowing multiple pools per pair for a whitelisted authority.', 'OpenLimitOrder takes output-side accounts to reject frozen input or output token accounts. The source describes both changes as backwards-compatible and says the update was deployed.'],
    check: 'When a pool lookup fails, verify seed_index rather than assuming the token pair gives one unique pool. For rejected orders, inspect both input and output token-account freeze states.',
    messages: [27], changelogs: [changelog('2026-07-30-clmm-permissioned-pools')],
  },
  {
    id: 'amm-openbook', title: 'AMM v4: dormant OpenBook plumbing removed', products: ['AMM v4'],
    announced: '2026-07-13', confirmed: '2026-07-22', status: 'Confirmed deployed', compatibility: 'Old swap flows stay; retired instructions revert',
    summary: 'AMM v4 removed unused OpenBook/Serum market-making machinery. Normal swaps, deposits and withdrawals keep working. New swap instructions need fewer accounts, while several old administrative and setup instructions are retired.',
    audience: 'AMM v4 routers, aggregators and older instruction builders.',
    action: 'Prefer SwapBaseInV2 / SwapBaseOutV2 for cheaper transactions. Use Initialize2 and stop calling the retired instructions.',
    technical: ['AMM v4 becomes a pure constant-product AMM, with no announced change to live swap behavior. Legacy Swap/Deposit/Withdraw account layouts continue executing.', 'V2 swaps use 8 accounts instead of 17+. Initialize, PreInitialize, MonitorStep, MigrateToOpenBook, WithdrawSrm, SimulateInfo and AdminCancelOrders are retired and revert.', 'July 22 follow-up confirms successful upgrade at Unix timestamp 1784726995.'],
    check: 'Decode the exact instruction name. A retired instruction reverting is different from an ordinary swap failing. Do not report absent OpenBook CPIs as missing execution after this upgrade.',
    messages: [25, 26], changelogs: [changelog('2026-07-22-amm-v4-openbook-removal')],
  },
  {
    id: 'stable-openbook', title: 'Stable AMM: fewer required accounts', products: ['Stable AMM'],
    announced: '2026-06-16', status: 'Announcement only', compatibility: 'WithdrawPnl has a hard breaking account change',
    summary: 'The stable-swap program announced removal of its OpenBook dependency. Existing swap, deposit and withdrawal layouts were said to work for now, but WithdrawPnl must use the new account list.',
    audience: 'Integrators using the Stable AMM program, particularly WithdrawPnl builders.',
    action: 'Update WithdrawPnl to the 10-account layout and migrate other flows to reduced layouts. The June 22, 12:00 UTC schedule has no explicit completion follow-up in this export.',
    technical: ['The source is about Stable AMM, a separate product from AMM v4 and CPMM. It recommends reduced-account layouts for Swap/Deposit/Withdraw while maintaining the old versions for now.', 'WithdrawPnl is explicitly a hard breaking change to 10 accounts. The announcement scheduled the update for Monday June 22 at 12:00 UTC.'],
    check: 'Identify the actual program before applying AMM v4 advice. Compare WithdrawPnl accounts and ordering against the matching Stable AMM changelog.',
    messages: [24], changelogs: [changelog('2026-06-22-stable-amm-openbook-cleanup')],
  },
  {
    id: 'clmm-features', title: 'CLMM: limit orders and new fee choices', products: ['CLMM'],
    announced: '2026-05-08', confirmed: '2026-05-18', status: 'Confirmed deployed', compatibility: 'Pool features opt-in; old volume/fee readers must migrate',
    summary: 'New pools can offer limit orders, fees that react to rapid price changes, and fees always collected in a chosen token. Existing pools and positions keep working, but old lifetime volume and fee counters stop updating.',
    audience: 'CLMM builders, limit-order services, analytics readers and SDK deep-import users.',
    action: 'Move analytics off the old PoolState counters to the Observation ring or API. Choose new features through CreateCustomizablePool; classic CreatePool still supports default-fee pools.',
    technical: ['Limit orders use single ticks and FIFO fills. Lifecycle: Open → Filled → Settled → Closed. LimitOrderState uses pool/owner/tick/nonce; LimitOrderNonce supplies incrementing PDA seeds. The keeper can only settle/close, with output to the owner’s associated token account; it cannot open orders or mutate pool fields.', 'DynamicFeeConfig holds per-tier filter/decay/reduction/cap settings. SwapV2 applies decay, accumulation and cap at each step, adding a surcharge to the base fee. PoolState embeds dynamic_fee_info. fee_on selects input-side legacy fees (0), token0 (1) or token1 (2), fixed at creation.', 'Account size stays the same, but swap_in/out_amount_token_* become padding5 [u128; 4], and total_fees*_token_* become padding6 [u64; 4]. Existing stored values remain stale. TickState replaces padding with order_phase, orders_amount, part_filled_orders_remaining and unfilled_ratio_x64; tick-array size and seeds stay the same.', 'SDK adds customizable-pool and order create/increase/decrease/settle/close methods, plus API config helpers. utils/ moves to libraries/, affecting deep imports. APIs include /main/clmm-dynamic-config, /main/clmm-limit-order-config and /clmm/limit-orders/{open,filled,closed}. New errors 6045–6050 cover dynamic fee, fee-side and order-phase/amount validation.', 'May 8 announcement and documentation follow-up were confirmed live on mainnet May 18. Program branch: feat_limitorder_dynamicfee; SDK branch: clmm-dynamic-fee-and-limit-order.'],
    check: 'For unexpected fees, read the pool’s fee_on and dynamic configuration. Distinguish filled orders awaiting settlement from closed orders. For flat analytics after May 18, check whether the reader still uses the retired counters.',
    messages: [21, 22, 23], changelogs: ['https://docs.raydium.io/reference/changelog#unreleased-%E2%80%94-clmm-limit-orders-single-sided-fee-dynamic-fee', 'https://github.com/raydium-io/raydium-clmm/tree/feat_limitorder_dynamicfee', 'https://github.com/raydium-io/raydium-docs/tree/master/audit/Sec3%20Q2%202026'],
  },
];

export const glossary = [
  ['Pool', 'An on-chain account and token reserves that people trade against. CLMM concentrates liquidity in price ranges; CPMM and AMM v4 use constant-product pools.'],
  ['CPI', 'Cross-program invocation: one Solana program calls another, such as Raydium asking the token program to transfer tokens. The called program may be the actual failure point.'],
  ['PDA', 'Program-derived address: an account address calculated from seeds and a program ID. Changing a seed changes the expected account.'],
  ['IDL', 'Interface description language: the schema a client uses to build or decode program instructions and accounts. An old IDL can misread new data.'],
  ['Token-2022', 'A Solana token program supporting extra features such as transfer fees and restrictions. The mint’s enabled extensions determine its behavior.'],
  ['Lamports', 'The smallest unit of SOL: one SOL is one billion lamports. “Excess lamports” means SOL held beyond the amount required by an account.'],
  ['ATA', 'Associated token account: the standard token account for a particular owner and token mint.'],
  ['Mint / quote mint', 'A mint identifies a token. A quote mint is the token used to price or pay for the asset being launched or traded.'],
] as const;
