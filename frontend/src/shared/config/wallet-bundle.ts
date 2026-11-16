// The host side of banking's wallet bundle (banking, wallet/frontend/mfe/wallet/entry.tsx,
// apiVersion 3; the contract is banking docs/WALLET-INTENTS.md). One bundle defines both
// `<mfe-banking-wallet>` and `<mfe-banking-pay-invoice>`. Names must stay in step.

/** The one origin that may carry out a money action and report its result. */
const WALLET_ORIGIN = "https://evinvest.ltd";

/**
 * Where banking publishes the bundle (`/api/wallet/mfe/wallet.js`, as its registry.json says).
 * Not served by the panel on purpose: the elements talk to the origin they were loaded from
 * (balances, history, action pages), so a copy under the panel's origin would ask the panel for
 * `/api/wallet`.
 */
export const WALLET_SCRIPT = `${WALLET_ORIGIN}/api/wallet/mfe/wallet.js`;

/**
 * The bytes of `WALLET_SCRIPT` this panel runs (banking WALLET-INPAGE-SPEC control (g): the
 * in-page pay form runs in this page), as SRI. `.github/workflows/wallet-pin.yml` repins it by PR
 * from banking's source, never from what evinvest.ltd serves: its latest release and its main,
 * space-separated, so the next bundle is accepted before banking rolls it out (banking#601).
 */
export const WALLET_SCRIPT_INTEGRITY = "sha256-dTHZ9KONZftLYVv+aqUdkS0o8kCqt7O/d2MPeHxvQ0U=";

/** The lowest `static apiVersion` of the bundle's elements that speaks this contract. */
export const WALLET_API_VERSION = 2;

/** The scope host id the elements show (main plus this scope's compartments). */
export const WALLET_SCOPE = "sa";
