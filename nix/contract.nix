# The conductor's deploy contract, beside what `container.implement` records; a file of its own for crates/server/tests/it/contract.rs.
{ pname }:
{
  # What the deploy must provide, beyond what `container.implement` records: plain data on the contract,
  # read by the devops generator. crates/server/tests/it/contract.rs holds it to the binary's `--print-*`.
  deploy = {
    # Settings::required_var_names("production"): the pod exits 78 without any of them.
    # PANEL_DB_PATH and PANEL_HISTORY_DB_PATH are the container's own env (the flake's); the rest the deploy supplies.
    requiredEnv = [ "PANEL_DB_PATH" "PANEL_HISTORY_DB_PATH" "PANEL_DATA_KEY" "PANEL_PUBLIC_ORIGIN" "CONCIERGE_PUBLIC_ORIGIN" "CONCIERGE_GRPC_ADDR" "RP_CLIENT_SECRET_SA" "PANEL_ASSERTION_KEY" "PANEL_REVIEW_ARCHIVE_URL" "PANEL_PLAYBOOK_URL" "PANEL_SITES_URL" "TOKENS_START_CAP_MULTIPLE" ];
    # from the sops-backed Secret, never literal env
    secretEnv = [
      "PANEL_DATA_KEY"
      "SENTRY_DSN"
      "RP_CLIENT_SECRET_SA"
      "PANEL_ASSERTION_KEY"
      "TELEGRAM_BOT_TOKEN"
      "GOLOGIN_TOKEN"
      "MULTILOGIN_TOKEN"
      "CLAUDE_CODE_OAUTH_TOKEN"
    ];
    # TELEGRAM_BOT_TOKEN needs the forward (it sends sites' notes).
    optionalEnv = [
      "SENTRY_DSN"
      "TELEGRAM_BOT_TOKEN"
      "TELEGRAM_BOT_USERNAME"
      "TELEGRAM_LOCALE"
      # 15 and 500 by default
      "TOKENS_DAILY"
      "TOKENS_CAP"
      # the gmails' browsers: the runner (sa_browsers, below) with a provider's token or both; unset,
      # their routes answer 503, and a provider without its token too
      "GOLOGIN_TOKEN"
      "MULTILOGIN_TOKEN"
      "PANEL_BROWSERS_URL"
      # the in-page assistant's model (a Claude subscription); unset, asking it answers 503
      "CLAUDE_CODE_OAUTH_TOKEN"
    ];
    ingress = {
      # in-cluster only, by service DNS: the landings' and bots' ingest, the landings' review kit
      # (docs/ARCHITECTURE.md, Deploy requirements)
      excludePathPrefixes = [ "/api/ingest" "/api/kit" ];
      # per client IP at the edge; the conductor bounds concurrency, not who calls. /api/hooks/booking:
      # the booking providers' webhooks, forwarded to sites (public, signature-checked there).
      # /api/queries_cache: open to everyone, so this is its only guard. /mcp/register: open too.
      rateLimitPathPrefixes = [ "/auth" "/api/hooks/booking" "/api/queries_cache" "/mcp/register" "/mcp/authorize" "/mcp/token" ];
    };
    egress = {
      # concierge's gRPC, at the address it names
      grpcEnv = [ "CONCIERGE_GRPC_ADDR" ];
      # the Bot API; the Places API (the accounts' Places keys); GoLogin's (GOLOGIN_TOKEN) and
      # Multilogin's (MULTILOGIN_TOKEN) APIs; the Claude API (CLAUDE_CODE_OAUTH_TOKEN)
      # the meters' baseline: each account's billing export, its service account's token
      hosts = [ "api.telegram.org:443" "places.googleapis.com:443" "geocode.googleapis.com:443" "api.gologin.com:443" "api.multilogin.com:443" "bigquery.googleapis.com:443" "oauth2.googleapis.com:443" "api.anthropic.com:443" ];
    };
  };

  # What the conductor serves to its peers, by endpoint: devops builds a consumer's URL from it.
  provides = {
    ingest = "/api/ingest/v1/events";
    # the landings' review kit pages (docs/ARCHITECTURE.md, Review kit)
    kit = "/api/kit/v1";
    # the queries cache (docs/ARCHITECTURE.md, Queries cache): review_archive's and placement's
    queries_cache = "/api/queries_cache";
  };
  # The services the forward reaches (docs/ARCHITECTURE.md, Forward), by contract name: devops
  # sets each variable to the peer's in-cluster origin and opens the egress to it.
  consumes = {
    PANEL_SITES_URL = "sites_panel";
    PANEL_REVIEW_ARCHIVE_URL = "review_archive";
    PANEL_PLAYBOOK_URL = "playbook_web";
  };
}
